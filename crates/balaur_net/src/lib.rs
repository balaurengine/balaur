//! Networking as a Balaur plugin: `http.*` and `websocket.*` for scripts.
//!
//! All I/O runs on background threads; completions cross back over a channel
//! and enter the simulation once per tick, at [`Stage::First`], as the frame's
//! [`NetSnapshot`] — the same model as input. Scripts see one stable view for
//! the whole tick, so recording the snapshot per tick is all a replay needs.
//!
//! Nothing here blocks the frame: `http.request` and `websocket.connect`
//! return an id immediately and results arrive in a later tick's snapshot,
//! read back with `http.responses()` and `websocket.events()`.

use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{anyhow, Result};
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};

#[cfg(not(target_family = "wasm"))]
mod http;
#[cfg(not(target_family = "wasm"))]
mod websocket;

/// The real backend: a thread per request and per connection.
#[cfg(not(target_family = "wasm"))]
mod backend {
    pub(crate) use crate::http::spawn_request;
    pub(crate) use crate::websocket::spawn_socket;
}

/// The wasm stub: no networking stack compiles there, so every request and
/// connect resolves to an error event and scripts keep running.
#[cfg(target_family = "wasm")]
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
        _commands: Receiver<SocketCommand>,
        events: &Sender<NetEvent>,
    ) {
        let _ = events.send(NetEvent::SocketError {
            socket,
            reason: "no network backend compiles for wasm".into(),
        });
    }
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

/// What the engine thread asks a connection's worker thread to do.
pub(crate) enum SocketCommand {
    SendText(String),
    Close,
}

/// A completion crossing from a worker thread back to the frame loop.
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
    SocketClosed {
        socket: u64,
        reason: String,
    },
    SocketError {
        socket: u64,
        reason: String,
    },
}

/// Handle tables and the channel every worker thread reports into.
pub struct NetState {
    next_id: u64,
    events: Receiver<NetEvent>,
    report: Sender<NetEvent>,
    sockets: DetHashMap<u64, Sender<SocketCommand>>,
}

impl Default for NetState {
    fn default() -> Self {
        let (report, events) = channel();
        Self {
            next_id: 1,
            events,
            report,
            sockets: DetHashMap::default(),
        }
    }
}

impl NetState {
    /// Start an HTTP request; the response (or error) arrives in a later
    /// tick's [`NetSnapshot`] carrying this id.
    pub fn request(&mut self, mut call: HttpCall) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        call.id = id;
        backend::spawn_request(call, self.report.clone());
        id
    }

    /// Open a websocket connection; the handshake happens off-thread, and an
    /// `open` (or `error`) event with this id lands in a later snapshot.
    pub fn connect(&mut self, url: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let (commands, receiver) = channel();
        backend::spawn_socket(id, url.to_string(), receiver, &self.report);
        self.sockets.insert(id, commands);
        id
    }

    /// Queue a text frame. False when the connection is gone — a script
    /// racing a close should not take the frame down.
    pub fn send_text(&mut self, socket: u64, text: &str) -> bool {
        self.sockets
            .get(&socket)
            .is_some_and(|commands| commands.send(SocketCommand::SendText(text.into())).is_ok())
    }

    /// Ask the connection to close. The `closed` event still arrives through
    /// the snapshot once the handshake finishes.
    pub fn close(&mut self, socket: u64) -> bool {
        self.sockets
            .get(&socket)
            .is_some_and(|commands| commands.send(SocketCommand::Close).is_ok())
    }
}

/// This tick's completions, as the neutral values scripts read. Cleared and
/// refilled by `pump_net_system` at [`Stage::First`], so the view is stable
/// for the whole tick.
#[derive(Default)]
pub struct NetSnapshot {
    pub http: Vec<Value>,
    pub socket: Vec<Value>,
}

/// Drain the worker threads' reports into the frame's snapshot, in arrival
/// order. Arrival order is an input to the simulation, like a key press: not
/// reproducible across runs, but stable for everyone reading this tick.
fn pump_net_system(eng: &Engine, _: f32) {
    let state = eng.resource::<NetState>();
    let snapshot = eng.resource::<NetSnapshot>();
    let mut state = state.borrow_mut();
    let mut snapshot = snapshot.borrow_mut();
    snapshot.http.clear();
    snapshot.socket.clear();
    while let Ok(event) = state.events.try_recv() {
        match &event {
            NetEvent::SocketClosed { socket, .. } | NetEvent::SocketError { socket, .. } => {
                // shift_remove: keeps the remaining entries in insertion
                // order, so iteration stays deterministic.
                state.sockets.shift_remove(socket);
            }
            _ => {}
        }
        match event {
            NetEvent::HttpResponse { .. } | NetEvent::HttpError { .. } => {
                snapshot.http.push(event_value(event));
            }
            _ => snapshot.socket.push(event_value(event)),
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
        reg.insert_resource(NetState::default());
        reg.insert_resource(NetSnapshot::default());
        reg.add_system(Stage::First, pump_net_system);
        let mut m = reg.script_module("http")?;
        install_http_api(&mut *m);
        let mut m = reg.script_module("websocket")?;
        install_websocket_api(&mut *m);
        Ok(())
    }
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
    let headers = match opt(opts, "headers") {
        Some(Value::Map(pairs)) => pairs
            .iter()
            .map(|(k, v)| match v {
                Value::Str(s) => Ok((k.clone(), s.clone())),
                other => Err(anyhow!("header `{k}` should be a string, got {other:?}")),
            })
            .collect::<Result<_>>()?,
        _ => Vec::new(),
    };
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

/// `http.*`. Declared against the neutral seam, so it works on any backend.
fn install_http_api(m: &mut dyn Bindings<Engine>) {
    // `http.request(url, { method = "POST", body = "...", headers = {...},
    // timeout = 5 })` -> id. The verb lives in the options table rather than
    // in the function name (N9); the default is GET.
    m.function(
        "request",
        |eng: &Engine, (url, opts): (String, Option<Value>)| {
            let call = call_of(&url, opts.as_ref())?;
            let state = eng.resource::<NetState>();
            let id = state.borrow_mut().request(call);
            Ok(int(id))
        },
    );
    // This tick's completed requests: `{ request, status, headers, body }`,
    // or `{ request, error }` when the transfer itself failed. An HTTP error
    // status is a response, not an error.
    m.function("responses", |eng: &Engine, ()| {
        Ok(Value::List(
            eng.resource::<NetSnapshot>().borrow().http.clone(),
        ))
    });
}

/// `websocket.*`. Text frames only: the value model has no bytes, and a
/// binary frame is logged and dropped in the reader thread.
fn install_websocket_api(m: &mut dyn Bindings<Engine>) {
    m.function("connect", |eng: &Engine, url: String| {
        let state = eng.resource::<NetState>();
        let id = state.borrow_mut().connect(&url);
        Ok(int(id))
    });
    m.function("send", |eng: &Engine, (socket, text): (i64, String)| {
        let state = eng.resource::<NetState>();
        let sent = u64::try_from(socket).is_ok_and(|id| state.borrow_mut().send_text(id, &text));
        Ok(Value::Bool(sent))
    });
    m.function("close", |eng: &Engine, socket: i64| {
        let state = eng.resource::<NetState>();
        let closed = u64::try_from(socket).is_ok_and(|id| state.borrow_mut().close(id));
        Ok(Value::Bool(closed))
    });
    // This tick's connection events, each `{ socket, kind, ... }` where kind
    // is `open`, `message` (with `text`), `closed` or `error` (with `reason`).
    m.function("events", |eng: &Engine, ()| {
        Ok(Value::List(
            eng.resource::<NetSnapshot>().borrow().socket.clone(),
        ))
    });
}
