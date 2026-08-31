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
//! script suspends on until the pump wakes it —
//! `local r = await(http.request(url))` in Luau,
//! `let r = task::wait(http::request(url)).await` in Rune.
//!
//! Nothing here blocks the frame: `http.request` and `websocket.connect`
//! return an id immediately, and handlers and resumptions run at
//! [`Stage::First`] of a later tick, in arrival order, never from an I/O
//! thread.

use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{anyhow, Result};
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

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
pub struct NetState {
    events: Receiver<NetEvent>,
    report: Sender<NetEvent>,
    sockets: DetHashMap<u64, Sender<SocketCommand>>,
    request_handlers: DetHashMap<u64, Handler>,
    socket_handlers: DetHashMap<u64, Handler>,
}

impl Default for NetState {
    fn default() -> Self {
        let (report, events) = channel();
        Self {
            events,
            report,
            sockets: DetHashMap::default(),
            request_handlers: DetHashMap::default(),
            socket_handlers: DetHashMap::default(),
        }
    }
}

impl NetState {
    /// Start an HTTP request under `id` — an [`Engine::next_token`] value, so
    /// awaiting it can never collide with another subsystem's ids. The
    /// response (or error) reaches `handler` on a later tick, and is recorded
    /// in that tick's [`NetSnapshot`] either way.
    pub fn request(&mut self, id: u64, mut call: HttpCall, handler: Option<Handler>) {
        call.id = id;
        if let Some(handler) = handler {
            self.request_handlers.insert(id, handler);
        }
        backend::spawn_request(call, self.report.clone());
    }

    /// Open a websocket connection under `id` (an [`Engine::next_token`]
    /// value); the handshake happens off-thread, and every event — `open`
    /// first, `closed` or `error` last — reaches `handler` on a later tick.
    pub fn connect(&mut self, id: u64, url: &str, handler: Option<Handler>) {
        if let Some(handler) = handler {
            self.socket_handlers.insert(id, handler);
        }
        let (commands, receiver) = channel();
        backend::spawn_socket(id, url.to_string(), receiver, &self.report);
        self.sockets.insert(id, commands);
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
    let mut dispatches: Vec<(Option<Handler>, Option<u64>, Value)> = Vec::new();
    {
        let state = eng.resource::<NetState>();
        let snapshot = eng.resource::<NetSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        snapshot.http.clear();
        snapshot.socket.clear();
        while let Ok(event) = state.events.try_recv() {
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
                NetEvent::SocketOpen { socket } | NetEvent::SocketMessage { socket, .. } => {
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
    // `http.request(self.node, url, { method = "POST", body = "...",
    // headers = {...}, timeout = 5, on_response = "on_login" })` -> id.
    // The response fires `on_response(response)` on that node's script —
    // `{ request, status, headers, body }`, or `{ request, error }` when the
    // transfer itself failed; an HTTP error status is a response, not an
    // error. The verb lives in the options table rather than in the function
    // name (N9); the default is GET.
    //
    // Without a node — `http.request(url, opts)` — nothing is dispatched and
    // the id is a token to suspend on: `await(http.request(url))` in Luau,
    // `task::wait(http::request(url)).await` in Rune.
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
            let call = call_of(&url, opts.as_ref())?;
            let id = eng.next_token();
            let state = eng.resource::<NetState>();
            state.borrow_mut().request(id, call, handler);
            Ok(int(id))
        },
    );
}

/// `websocket.*`. Text frames only: the value model has no bytes, and a
/// binary frame is logged and dropped in the reader thread.
fn install_websocket_api(m: &mut dyn Bindings<Engine>) {
    // `websocket.connect(self.node, url, { on_event = "on_websocket_event" })`
    // -> id. Every connection event fires that method with one argument,
    // `{ socket, kind, ... }` where kind is `open`, `message` (with `text`),
    // `closed` or `error` (with `reason`). A nil node discards the events.
    m.function(
        "connect",
        |eng: &Engine, (node, url, opts): (Value, String, Option<Value>)| {
            let handler = handler_of(&node, opts.as_ref(), "on_event", "on_websocket_event")?;
            let id = eng.next_token();
            let state = eng.resource::<NetState>();
            state.borrow_mut().connect(id, &url, handler);
            Ok(int(id))
        },
    );
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
}
