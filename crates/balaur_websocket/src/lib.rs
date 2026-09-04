//! Websockets as a Balaur plugin: `websocket.*` for scripts, and a
//! `balaur_core::transport::Transport` for sessions.
//!
//! Connections run on background threads; events cross back over a channel
//! and enter the simulation once per tick, at [`Stage::First`], recorded in
//! [`WebsocketSnapshot`] — the same model as input. Recording the snapshot
//! per tick is all a replay needs.
//!
//! Scripts never poll. A connection names the node that handles its events,
//! and the pump dispatches them as method calls — the same signal shape a
//! widget's `on_click` or an animation key uses.
//!
//! Nothing here blocks the frame: `websocket.connect` returns an id
//! immediately, and handlers run at [`Stage::First`] of a later tick, in
//! arrival order, never from an I/O thread.

use std::sync::mpsc::{channel, Sender};

use anyhow::{anyhow, bail, Result};
use balaur_core::handler::{handler_of, headers_of, id_value, opt, Handler};
use balaur_core::replay::ExternalIo;
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};

#[cfg(not(target_family = "wasm"))]
mod frames;
#[cfg(not(target_family = "wasm"))]
pub mod listener;
pub mod transport;

/// The native backend: a thread per connection.
#[cfg(not(target_family = "wasm"))]
mod backend {
    pub(crate) use crate::frames::spawn_socket;

    /// Threads deliver on their own; nothing to flush per tick.
    pub(crate) fn pump() {}
}

#[cfg(all(target_family = "wasm", target_os = "emscripten"))]
mod emscripten;

/// The browser backend: emscripten websockets, no threads.
#[cfg(all(target_family = "wasm", target_os = "emscripten"))]
mod backend {
    pub(crate) use crate::emscripten::{pump, spawn_socket};
}

/// The browser outside emscripten: the WebSocket API through web-sys.
#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod browser;

#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod backend {
    pub(crate) use crate::browser::{pump, spawn_socket};
}

/// How one `websocket.connect` opens its connection.
#[derive(Clone, Debug)]
pub struct SocketOptions {
    /// Offer `permessage-deflate`; the server decides whether frames are
    /// compressed. Off, frames go as they are.
    pub compression: bool,
    /// Extra headers on the upgrade request — an `Authorization`, a cookie.
    pub headers: Vec<(String, String)>,
}

impl Default for SocketOptions {
    fn default() -> Self {
        Self {
            compression: true,
            headers: Vec::new(),
        }
    }
}

/// Project-wide defaults, the `[websocket]` table of `project.toml`:
///
/// ```toml
/// [websocket]
/// compression = true   # offer permessage-deflate on every connection
/// ```
///
/// A call's own options override these.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebsocketConfig {
    pub compression: bool,
}

impl Default for WebsocketConfig {
    fn default() -> Self {
        Self { compression: true }
    }
}

impl WebsocketConfig {
    /// The `[websocket]` table of the project's manifest, or the defaults when
    /// the file or the table is missing. A table that does not parse is
    /// reported and ignored rather than failing the boot over a networking
    /// setting.
    #[must_use]
    pub fn load(files: &balaur_core::project::ProjectFiles) -> Self {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            websocket: WebsocketConfig,
        }
        let Ok(bytes) = files.read("project.toml") else {
            return Self::default();
        };
        match toml::from_str::<Manifest>(&String::from_utf8_lossy(&bytes)) {
            Ok(manifest) => manifest.websocket,
            Err(err) => {
                tracing::warn!("project.toml [websocket]: {err}; using the defaults");
                Self::default()
            }
        }
    }
}

/// What the engine thread asks a connection's worker thread to do.
pub(crate) enum SocketCommand {
    SendText(String),
    SendBytes(Vec<u8>),
    Close,
}

/// One connection event crossing from a worker thread back to the frame loop.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum SocketEvent {
    Open {
        socket: u64,
    },
    Message {
        socket: u64,
        text: String,
    },
    /// A binary frame. Its own variant rather than a payload enum inside
    /// `Message`, so the two never have to be told apart by inspection.
    Binary {
        socket: u64,
        bytes: Vec<u8>,
    },
    Closed {
        socket: u64,
        reason: String,
    },
    Failed {
        socket: u64,
        reason: String,
    },
}

/// Handle tables and the channel every worker thread reports into.
#[derive(Default)]
pub struct WebsocketState {
    /// The worker channel, this tick's arrivals, and the rule that a replay
    /// never reaches the network — all three live in here.
    io: ExternalIo<SocketEvent>,
    sockets: DetHashMap<u64, Sender<SocketCommand>>,
    handlers: DetHashMap<u64, Handler>,
}

impl WebsocketState {
    /// Open a connection under `id` (an [`Engine::next_token`] value); the
    /// handshake happens off-thread, and every event — `open` first, `closed`
    /// or `error` last — reaches `handler` on a later tick.
    pub fn connect(
        &mut self,
        eng: &Engine,
        id: u64,
        url: &str,
        options: SocketOptions,
        handler: Option<Handler>,
    ) {
        if let Some(handler) = handler {
            self.handlers.insert(id, handler);
        }
        balaur_core::replay::event(
            eng,
            "websocket.connect",
            format!("connect {url}"),
            Some(serde_json::json!({ "id": id, "url": url })),
        );
        let (commands, receiver) = channel();
        let started = self.io.start(eng, |report| {
            backend::spawn_socket(id, url.to_string(), options, receiver, report);
        });
        if started {
            self.sockets.insert(id, commands);
        }
    }

    /// Queue a text frame. False when the connection is gone — a script
    /// racing a close should not take the frame down.
    pub fn send_text(&mut self, socket: u64, text: &str) -> bool {
        self.send_command(socket, SocketCommand::SendText(text.into()))
    }

    /// Queue a binary frame, on the same terms as [`Self::send_text`].
    pub fn send_bytes(&mut self, socket: u64, bytes: Vec<u8>) -> bool {
        self.send_command(socket, SocketCommand::SendBytes(bytes))
    }

    fn send_command(&mut self, socket: u64, command: SocketCommand) -> bool {
        self.sockets
            .get(&socket)
            .is_some_and(|commands| commands.send(command).is_ok())
    }

    /// Ask the connection to close. The `closed` event still arrives through
    /// the snapshot once the handshake finishes.
    pub fn close(&mut self, socket: u64) -> bool {
        self.sockets
            .get(&socket)
            .is_some_and(|commands| commands.send(SocketCommand::Close).is_ok())
    }
}

/// This tick's events, as the neutral values the handlers received. Cleared
/// and refilled by `pump_websocket_system` at [`Stage::First`]. Scripts never
/// read it — it exists so Rust code can observe traffic, and so a recorder
/// has one place to tap for replay.
#[derive(Default)]
pub struct WebsocketSnapshot {
    pub events: Vec<Value>,
}

/// Drain the worker threads' reports, record them in the frame's snapshot,
/// then dispatch each to its handler — in arrival order throughout. Arrival
/// order is an input to the simulation, like a key press: not reproducible
/// across runs, but stable for everyone this tick.
///
/// Dispatch happens after the borrows are released, so a handler may itself
/// connect, send or close.
fn pump_websocket_system(eng: &Engine, _: f32) {
    // Backends with no delivery threads (the browser) flush their queues
    // here; the native one is a no-op.
    backend::pump();
    let mut dispatches: Vec<(Handler, Value)> = Vec::new();
    {
        let state = eng.resource::<WebsocketState>();
        let snapshot = eng.resource::<WebsocketSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        snapshot.events.clear();
        for event in state.io.drain() {
            // shift_remove: keeps the remaining entries in insertion order,
            // so iteration stays deterministic.
            let handler = match &event {
                SocketEvent::Closed { socket, .. } | SocketEvent::Failed { socket, .. } => {
                    state.sockets.shift_remove(socket);
                    state.handlers.shift_remove(socket)
                }
                SocketEvent::Open { socket }
                | SocketEvent::Message { socket, .. }
                | SocketEvent::Binary { socket, .. } => state.handlers.get(socket).cloned(),
            };
            let value = event_value(event);
            snapshot.events.push(value.clone());
            if let Some(handler) = handler {
                dispatches.push((handler, value));
            }
        }
    }
    if let Some(host) = eng.script_host() {
        for (handler, value) in dispatches {
            host.call_on(handler.node, &handler.method, std::slice::from_ref(&value));
        }
    }
}

fn event_value(event: SocketEvent) -> Value {
    let pairs = match event {
        SocketEvent::Open { socket } => vec![
            ("socket".into(), id_value(socket)),
            ("kind".into(), Value::Str("open".into())),
        ],
        SocketEvent::Message { socket, text } => vec![
            ("socket".into(), id_value(socket)),
            ("kind".into(), Value::Str("message".into())),
            ("text".into(), Value::Str(text)),
        ],
        SocketEvent::Binary { socket, bytes } => vec![
            ("socket".into(), id_value(socket)),
            ("kind".into(), Value::Str("binary".into())),
            ("bytes".into(), Value::Bytes(bytes)),
        ],
        SocketEvent::Closed { socket, reason } => vec![
            ("socket".into(), id_value(socket)),
            ("kind".into(), Value::Str("closed".into())),
            ("reason".into(), Value::Str(reason)),
        ],
        SocketEvent::Failed { socket, reason } => vec![
            ("socket".into(), id_value(socket)),
            ("kind".into(), Value::Str("error".into())),
            ("reason".into(), Value::Str(reason)),
        ],
    };
    Value::Map(pairs)
}

pub struct WebsocketPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for WebsocketPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("websocket", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for WebsocketPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        let config = {
            let files = reg
                .app()
                .engine
                .resource::<balaur_core::project::ProjectFiles>();
            let files = files.borrow();
            WebsocketConfig::load(&files)
        };
        reg.insert_resource(config);
        reg.insert_resource(WebsocketState::default());
        reg.insert_resource(WebsocketSnapshot::default());
        reg.add_system(Stage::First, pump_websocket_system);
        reg.add_replay_source("websocket", capture, restore);
        balaur_core::settings::register(
            &reg.app().engine,
            balaur_core::settings::SettingsPage {
                category: String::from("WebSocket"),
                table: String::from("websocket"),
                scope: balaur_core::settings::Scope::Project,
                schema: balaur_core::ComponentDef::parse_schema(
                    "settings.websocket",
                    r#"
compression = { type = "bool", default = true, help = "Offer permessage-deflate on every connection; the server decides whether frames are compressed." }
"#,
                ),
            },
        );
        let mut m = reg.script_module("websocket")?;
        install_websocket_api(&mut *m);
        Ok(())
    }
}

/// This tick's arrivals, raw. The pump has already stashed them.
fn capture(eng: &Engine) -> serde_json::Value {
    eng.resource::<WebsocketState>().borrow().io.capture()
}

/// Push recorded arrivals back down the same channel the worker threads use,
/// so the pump dispatches them exactly as it did when they were real.
fn restore(eng: &Engine, value: &serde_json::Value) {
    eng.resource::<WebsocketState>().borrow().io.restore(value);
}

/// A connection's options: `compression` and `headers`, over the project's
/// `[websocket]` defaults.
fn socket_options_of(opts: Option<&Value>, config: &WebsocketConfig) -> Result<SocketOptions> {
    let compression = match opt(opts, "compression") {
        Some(Value::Bool(on)) => *on,
        Some(other) => {
            return Err(anyhow!(
                "compression should be true or false, got {other:?}"
            ))
        }
        None => config.compression,
    };
    Ok(SocketOptions {
        compression,
        headers: headers_of(opts)?,
    })
}

/// `websocket.*`. Text and binary frames both cross as themselves: a text
/// frame arrives as `Value::Str`, a binary one as `Value::Bytes`.
fn install_websocket_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "A long-lived connection carrying text or binary frames. Its events \
         are a stream, not a result: each one reaches the connecting node's \
         handler method (`on_websocket_event` unless `on_event` names \
         another) as a map `{ socket, kind, .. }` with kind `open`, \
         `message` (with `text`), `binary` (with `bytes`), `closed` or \
         `error`, and nothing awaits a socket id.",
    );
    m.describe(&[
        ("connect", &[], "", "Open a connection and return the id `send` and `close` take; options are `on_event`, `compression` and `headers`."),
        ("send", &[], "", "Queue a frame on the connection, text for a string and binary for bytes; false when it is already gone."),
        ("close", &[], "", "Ask the connection to close, which still delivers a `closed` event; false when it was already gone."),
    ]);
    m.function(
        "connect",
        |eng: &Engine, (node, url, opts): (Value, String, Option<Value>)| {
            let handler = handler_of(&node, opts.as_ref(), "on_event", "on_websocket_event")?;
            let options =
                socket_options_of(opts.as_ref(), &eng.resource::<WebsocketConfig>().borrow())?;
            let id = eng.next_token();
            let state = eng.resource::<WebsocketState>();
            state.borrow_mut().connect(eng, id, &url, options, handler);
            Ok(id_value(id))
        },
    );
    m.function("send", |eng: &Engine, (socket, payload): (i64, Value)| {
        let state = eng.resource::<WebsocketState>();
        let Ok(id) = u64::try_from(socket) else {
            return Ok(Value::Bool(false));
        };
        let sent = match payload {
            Value::Str(text) => state.borrow_mut().send_text(id, &text),
            Value::Bytes(bytes) => state.borrow_mut().send_bytes(id, bytes),
            other => bail!(
                "websocket.send takes a string or bytes, got {}",
                other.type_name()
            ),
        };
        Ok(Value::Bool(sent))
    });
    m.function("close", |eng: &Engine, socket: i64| {
        let state = eng.resource::<WebsocketState>();
        let closed = u64::try_from(socket).is_ok_and(|id| state.borrow_mut().close(id));
        Ok(Value::Bool(closed))
    });
}
