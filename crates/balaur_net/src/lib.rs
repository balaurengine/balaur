//! Networking as a Balaur plugin: `http.*` and `websocket.*` for scripts.
//!
//! All I/O runs on background threads; completions cross back over a channel
//! and enter the simulation once per tick, at [`Stage::First`], recorded in
//! [`NetSnapshot`] — the same model as input. Recording the snapshot per tick
//! is all a replay needs.
//!
//! Scripts never poll. A request or connection names the node that handles
//! its results, and the pump dispatches them as method calls — the same
//! signal shape a widget's `on_click` or an animation key uses:
//!
//! ```lua
//! function S:init()
//!     self.request = http.request(self.node, "https://example.com")
//! end
//! function S:on_response(r) print(r.status, r.body) end
//! ```
//!
//! Or, sequentially: an http request made without a node is a token the
//! script suspends on until the pump wakes it:
//! `let r = task::wait(http::request(url)).await`.
//!
//! Nothing here blocks the frame: `http.request` and `websocket.connect`
//! return an id immediately, and handlers and resumptions run at
//! [`Stage::First`] of a later tick, in arrival order, never from an I/O
//! thread.

use std::sync::mpsc::{channel, Sender};

use anyhow::{anyhow, bail, Result};
use balaur_core::replay::ExternalIo;
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

#[cfg(not(target_family = "wasm"))]
mod http;
#[cfg(not(target_family = "wasm"))]
mod websocket;

/// The native backend: a thread per request and per connection.
#[cfg(not(target_family = "wasm"))]
mod backend {
    pub(crate) use crate::http::spawn_request;
    pub(crate) use crate::websocket::spawn_socket;

    /// Threads deliver on their own; nothing to flush per tick.
    pub(crate) fn pump() {}
}

#[cfg(all(target_family = "wasm", target_os = "emscripten"))]
mod emscripten;

/// The browser backend: emscripten fetch and websockets, no threads.
#[cfg(all(target_family = "wasm", target_os = "emscripten"))]
mod backend {
    pub(crate) use crate::emscripten::{pump, spawn_request, spawn_socket};
}

/// The stub for wasm outside emscripten: no networking stack exists there
/// yet, so every request and connect resolves to an error event and scripts
/// keep running.
#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod backend {
    use std::sync::mpsc::{Receiver, Sender};

    use crate::{HttpCall, NetEvent, SocketCommand};

    pub(crate) fn spawn_request(call: HttpCall, events: Sender<NetEvent>) {
        let _ = events.send(NetEvent::HttpError {
            request: call.id,
            message: "no network backend compiles for wasm".into(),
        });
    }

    pub(crate) fn spawn_socket(
        socket: u64,
        _url: String,
        _options: SocketOptions,
        _commands: Receiver<SocketCommand>,
        events: &Sender<NetEvent>,
    ) {
        let _ = events.send(NetEvent::SocketError {
            socket,
            reason: "no network backend compiles for wasm".into(),
        });
    }

    pub(crate) fn pump() {}
}

/// One `http.request`, on its way to a worker thread.
pub struct HttpCall {
    pub id: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// Seconds for the whole request; `None` takes the 10 second default.
    pub timeout: Option<f64>,
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

/// Project-wide defaults for the net module, the `[net]` table of
/// `project.toml`:
///
/// ```toml
/// [net]
/// websocket_compression = true   # offer permessage-deflate on every connection
/// http_timeout = 10.0            # seconds, when a request names none
/// ```
///
/// A call's own options override these.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetConfig {
    pub websocket_compression: bool,
    pub http_timeout: f64,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            websocket_compression: true,
            http_timeout: 10.0,
        }
    }
}

impl NetConfig {
    /// The `[net]` table of the project's manifest, or the defaults when the
    /// file or the table is missing. A table that does not parse is reported
    /// and ignored rather than failing the boot over a networking setting.
    #[must_use]
    pub fn load(files: &balaur_core::project::ProjectFiles) -> Self {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            net: NetConfig,
        }
        let Ok(bytes) = files.read("project.toml") else {
            return Self::default();
        };
        match toml::from_str::<Manifest>(&String::from_utf8_lossy(&bytes)) {
            Ok(manifest) => manifest.net,
            Err(err) => {
                tracing::warn!("project.toml [net]: {err}; using the defaults");
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

/// A completion crossing from a worker thread back to the frame loop.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum NetEvent {
    HttpResponse {
        request: u64,
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    },
    HttpError {
        request: u64,
        message: String,
    },
    SocketOpen {
        socket: u64,
    },
    SocketMessage {
        socket: u64,
        text: String,
    },
    /// A binary frame. Its own variant rather than a payload enum inside
    /// `SocketMessage` so recordings made before binary frames still decode.
    SocketBinary {
        socket: u64,
        bytes: Vec<u8>,
    },
    SocketClosed {
        socket: u64,
        reason: String,
    },
    SocketError {
        socket: u64,
        reason: String,
    },
}

/// Where one request's or connection's results go: a method on one node's
/// script, dispatched through `ScriptHost::call_on`.
///
/// A method name rather than a function value on purpose: `Value::Callback`
/// is valid only during the binding call that received it, so a handler that
/// outlives the call is named, not held.
#[derive(Clone)]
pub struct Handler {
    pub node: NodeId,
    pub method: String,
}

/// Handle tables and the channel every worker thread reports into.
#[derive(Default)]
pub struct NetState {
    /// The worker channel, this tick's arrivals, and the rule that a replay
    /// never reaches the network — all three live in here.
    io: ExternalIo<NetEvent>,
    sockets: DetHashMap<u64, Sender<SocketCommand>>,
    request_handlers: DetHashMap<u64, Handler>,
    socket_handlers: DetHashMap<u64, Handler>,
}

impl NetState {
    /// Start an HTTP request under `id` — an [`Engine::next_token`] value, so
    /// awaiting it can never collide with another subsystem's ids. The
    /// response (or error) reaches `handler` on a later tick, and is recorded
    /// in that tick's [`NetSnapshot`] either way.
    pub fn request(&mut self, eng: &Engine, id: u64, mut call: HttpCall, handler: Option<Handler>) {
        call.id = id;
        // The handler is wired up either way: on a replay the recorded reply
        // still has to find its way home.
        if let Some(handler) = handler {
            self.request_handlers.insert(id, handler);
        }
        // The outbound side, which no source records: a reply is captured
        // with the id it answers, and nothing else says the request went out.
        balaur_core::replay::event(
            eng,
            "net.request",
            format!("{} {}", call.method, call.url),
            Some(serde_json::json!({ "id": id, "method": call.method, "url": call.url })),
        );
        self.io
            .start(eng, |report| backend::spawn_request(call, report.clone()));
    }

    /// Open a websocket connection under `id` (an [`Engine::next_token`]
    /// value); the handshake happens off-thread, and every event — `open`
    /// first, `closed` or `error` last — reaches `handler` on a later tick.
    pub fn connect(
        &mut self,
        eng: &Engine,
        id: u64,
        url: &str,
        options: SocketOptions,
        handler: Option<Handler>,
    ) {
        if let Some(handler) = handler {
            self.socket_handlers.insert(id, handler);
        }
        balaur_core::replay::event(
            eng,
            "net.request",
            format!("connect {url}"),
            Some(serde_json::json!({ "id": id, "method": "connect", "url": url })),
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

/// This tick's completions, as the neutral values the handlers received.
/// Cleared and refilled by `pump_net_system` at [`Stage::First`]. Scripts
/// never read it — it exists so Rust code can observe traffic, and so a
/// recorder has one place to tap for replay.
#[derive(Default)]
pub struct NetSnapshot {
    pub http: Vec<Value>,
    pub socket: Vec<Value>,
}

/// Drain the worker threads' reports, record them in the frame's snapshot,
/// then dispatch each to its handler — in arrival order throughout. Arrival
/// order is an input to the simulation, like a key press: not reproducible
/// across runs, but stable for everyone this tick.
///
/// Dispatch happens after the borrows are released, so a handler may itself
/// request, connect, send or close.
fn pump_net_system(eng: &Engine, _: f32) {
    // Backends with no delivery threads (the browser) flush their queues
    // here; the native one is a no-op.
    backend::pump();
    let mut dispatches: Vec<(Option<Handler>, Option<u64>, Value)> = Vec::new();
    {
        let state = eng.resource::<NetState>();
        let snapshot = eng.resource::<NetSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        snapshot.http.clear();
        snapshot.socket.clear();
        for event in state.io.drain() {
            // shift_remove: keeps the remaining entries in insertion order,
            // so iteration stays deterministic.
            let handler = match &event {
                NetEvent::HttpResponse { request, .. } | NetEvent::HttpError { request, .. } => {
                    state.request_handlers.shift_remove(request)
                }
                NetEvent::SocketClosed { socket, .. } | NetEvent::SocketError { socket, .. } => {
                    state.sockets.shift_remove(socket);
                    state.socket_handlers.shift_remove(socket)
                }
                NetEvent::SocketOpen { socket }
                | NetEvent::SocketMessage { socket, .. }
                | NetEvent::SocketBinary { socket, .. } => {
                    state.socket_handlers.get(socket).cloned()
                }
            };
            // An http completion also wakes its request id, so a script that
            // chose `await` over a handler resumes here.
            let wake = match &event {
                NetEvent::HttpResponse { request, .. } | NetEvent::HttpError { request, .. } => {
                    Some(*request)
                }
                _ => None,
            };
            let value = event_value(event);
            if handler.is_some() || wake.is_some() {
                dispatches.push((handler, wake, value.clone()));
            }
            if wake.is_some() {
                snapshot.http.push(value);
            } else {
                snapshot.socket.push(value);
            }
        }
    }
    if let Some(host) = eng.script_host() {
        for (handler, wake, value) in dispatches {
            if let Some(handler) = handler {
                host.call_on(handler.node, &handler.method, std::slice::from_ref(&value));
            }
            if let Some(token) = wake {
                host.wake(token, &value);
            }
        }
    }
}

fn event_value(event: NetEvent) -> Value {
    let pairs = match event {
        NetEvent::HttpResponse {
            request,
            status,
            headers,
            body,
        } => vec![
            ("request".into(), int(request)),
            ("status".into(), Value::Int(i64::from(status))),
            (
                "headers".into(),
                Value::Map(
                    headers
                        .into_iter()
                        .map(|(k, v)| (k, Value::Str(v)))
                        .collect(),
                ),
            ),
            ("body".into(), Value::Str(body)),
        ],
        NetEvent::HttpError { request, message } => vec![
            ("request".into(), int(request)),
            ("error".into(), Value::Str(message)),
        ],
        NetEvent::SocketOpen { socket } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("open".into())),
        ],
        NetEvent::SocketMessage { socket, text } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("message".into())),
            ("text".into(), Value::Str(text)),
        ],
        NetEvent::SocketBinary { socket, bytes } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("binary".into())),
            ("bytes".into(), Value::Bytes(bytes)),
        ],
        NetEvent::SocketClosed { socket, reason } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("closed".into())),
            ("reason".into(), Value::Str(reason)),
        ],
        NetEvent::SocketError { socket, reason } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("error".into())),
            ("reason".into(), Value::Str(reason)),
        ],
    };
    Value::Map(pairs)
}

fn int(id: u64) -> Value {
    Value::Int(i64::try_from(id).unwrap_or(i64::MAX))
}

pub struct NetPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for NetPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("net", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for NetPlugin {
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
            NetConfig::load(&files)
        };
        reg.insert_resource(config);
        reg.insert_resource(NetState::default());
        reg.insert_resource(NetSnapshot::default());
        reg.add_system(Stage::First, pump_net_system);
        reg.add_replay_source("net", capture_net, restore_net);
        let mut m = reg.script_module("http")?;
        install_http_api(&mut *m);
        let mut m = reg.script_module("websocket")?;
        install_websocket_api(&mut *m);
        Ok(())
    }
}

/// This tick's arrivals, raw. The pump has already stashed them.
fn capture_net(eng: &Engine) -> serde_json::Value {
    eng.resource::<NetState>().borrow().io.capture()
}

/// Push recorded arrivals back down the same channel the worker threads use,
/// so the pump dispatches them exactly as it did when they were real.
fn restore_net(eng: &Engine, value: &serde_json::Value) {
    eng.resource::<NetState>().borrow().io.restore(value);
}

/// One key out of a script options table, or `None` if the table, the key or
/// its type is missing.
fn opt<'a>(opts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    match opts? {
        Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn call_of(url: &str, opts: Option<&Value>) -> Result<HttpCall> {
    let method = match opt(opts, "method") {
        Some(Value::Str(m)) => m.to_uppercase(),
        Some(other) => return Err(anyhow!("method should be a string, got {other:?}")),
        None => "GET".into(),
    };
    let body = match opt(opts, "body") {
        Some(Value::Str(b)) => Some(b.clone()),
        Some(other) => return Err(anyhow!("body should be a string, got {other:?}")),
        None => None,
    };
    let headers = headers_of(opts)?;
    let timeout = match opt(opts, "timeout") {
        Some(Value::Num(n)) => Some(*n),
        Some(Value::Int(n)) => Some(*n as f64),
        _ => None,
    };
    Ok(HttpCall {
        id: 0,
        method,
        url: url.to_string(),
        headers,
        body,
        timeout,
    })
}

/// The handler a binding's node-and-options arguments name, or `None` for a
/// nil node — fire and forget.
fn handler_of(
    node: &Value,
    opts: Option<&Value>,
    key: &str,
    default_method: &str,
) -> Result<Option<Handler>> {
    let node = match node {
        Value::Node(id) => NodeId(*id),
        Value::Nil => return Ok(None),
        other => return Err(anyhow!("argument 0 should be a node or nil, got {other:?}")),
    };
    let method = match opt(opts, key) {
        Some(Value::Str(name)) => name.clone(),
        Some(other) => return Err(anyhow!("`{key}` should be a method name, got {other:?}")),
        None => default_method.to_string(),
    };
    Ok(Some(Handler { node, method }))
}

/// `http.*`. Declared against the neutral seam, so it works on any backend.
fn install_http_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "HTTP calls, off the frame: the reply arrives on a later tick as a map \
         with `status`, `headers` and `body`, or with `error`, both to the \
         node's `on_response` method and to whoever awaits the returned id. \
         Options are `method`, `headers`, `body` and a `timeout` in seconds, \
         which falls back to the project's `[net] http_timeout`.",
    );
    m.describe(&[
        ("request", &[], "", "Start an HTTP request and return the id its reply carries, to await or to match inside the handler."),
    ]);
    // An HTTP error status is a response, not an error. With a nil node the
    // returned id is a token to suspend on (`await` / `task::wait`).
    m.function(
        "request",
        |eng: &Engine, (first, second, third): (Value, Option<Value>, Option<Value>)| {
            let (node, url, opts) = match &first {
                Value::Str(url) => (Value::Nil, url.clone(), second),
                _ => match second {
                    Some(Value::Str(url)) => (first, url, third),
                    other => return Err(anyhow!("argument 1 should be a url, got {other:?}")),
                },
            };
            let handler = handler_of(&node, opts.as_ref(), "on_response", "on_response")?;
            let mut call = call_of(&url, opts.as_ref())?;
            if call.timeout.is_none() {
                call.timeout = Some(eng.resource::<NetConfig>().borrow().http_timeout);
            }
            let id = eng.next_token();
            let state = eng.resource::<NetState>();
            state.borrow_mut().request(eng, id, call, handler);
            Ok(int(id))
        },
    );
}

/// The headers table of an options value, as string pairs.
fn headers_of(opts: Option<&Value>) -> Result<Vec<(String, String)>> {
    match opt(opts, "headers") {
        Some(Value::Map(pairs)) => pairs
            .iter()
            .map(|(k, v)| match v {
                Value::Str(s) => Ok((k.clone(), s.clone())),
                other => Err(anyhow!("header `{k}` should be a string, got {other:?}")),
            })
            .collect(),
        Some(other) => Err(anyhow!("headers should be a table, got {other:?}")),
        None => Ok(Vec::new()),
    }
}

/// A connection's options: `compression` and `headers`, over the project's
/// `[net]` defaults.
fn socket_options_of(opts: Option<&Value>, config: &NetConfig) -> Result<SocketOptions> {
    let compression = match opt(opts, "compression") {
        Some(Value::Bool(on)) => *on,
        Some(other) => {
            return Err(anyhow!(
                "compression should be true or false, got {other:?}"
            ))
        }
        None => config.websocket_compression,
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
    // Events fire as `{ socket, kind, ... }` with kind `open`, `message`,
    // `closed` or `error`; a nil node discards them. Options: `on_event`,
    // `compression`, `headers`.
    m.function(
        "connect",
        |eng: &Engine, (node, url, opts): (Value, String, Option<Value>)| {
            let handler = handler_of(&node, opts.as_ref(), "on_event", "on_websocket_event")?;
            let options = socket_options_of(opts.as_ref(), &eng.resource::<NetConfig>().borrow())?;
            let id = eng.next_token();
            let state = eng.resource::<NetState>();
            state.borrow_mut().connect(eng, id, &url, options, handler);
            Ok(int(id))
        },
    );
    m.function("send", |eng: &Engine, (socket, payload): (i64, Value)| {
        let state = eng.resource::<NetState>();
        let Ok(id) = u64::try_from(socket) else {
            return Ok(Value::Bool(false));
        };
        let sent = match payload {
            Value::Str(text) => state.borrow_mut().send_text(id, &text),
            Value::Bytes(bytes) => state.borrow_mut().send_bytes(id, bytes),
            other => bail!("websocket.send takes a string or bytes, got {}", other.type_name()),
        };
        Ok(Value::Bool(sent))
    });
    m.function("close", |eng: &Engine, socket: i64| {
        let state = eng.resource::<NetState>();
        let closed = u64::try_from(socket).is_ok_and(|id| state.borrow_mut().close(id));
        Ok(Value::Bool(closed))
    });
}
