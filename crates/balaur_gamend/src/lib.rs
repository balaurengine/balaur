//! The Gamend backend as a Balaur plugin: `gamend.*` for scripts.
//!
//! Built on [`gamend_client`], with the same delivery contract as the net
//! plugin: all I/O runs on worker threads, completions cross a channel and
//! enter the simulation once per tick at [`Stage::First`] — recorded in
//! [`GamendSnapshot`], dispatched to handler methods, and waking await
//! tokens, in arrival order. Handlers and resumptions never run from an I/O
//! thread, so a recorded snapshot replays a whole online session.
//!
//! The sequential shape reads like this:
//!
//! ```lua
//! function S:init()
//!     gamend.configure("http://localhost:4000")
//!     local login = await(gamend.login({ device_id = "player-1" }))
//!     self.socket = gamend.connect(self.node)
//!     local hook = await(gamend.call_hook(self.socket, "arena", "start", {}))
//! end
//! function S:on_gamend_event(e) ... end  -- socket events: open/message/closed/error
//! ```
//!
//! One-shot operations (login, rest, call_hook, join, push, leave) wake
//! their returned id and, when a node is given, also dispatch to its
//! handler. Socket events are a stream, so they only dispatch. Every event
//! map carries a `kind` — `login`, `rest`, `reply`, `error`, `open`,
//! `message`, `closed` — so one handler can take them all.

use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{anyhow, Result};
use balaur_core::engine_api::from_json;
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};
use serde_json::Value as Json;

#[cfg(not(target_family = "wasm"))]
mod worker;

#[cfg(not(target_family = "wasm"))]
mod backend {
    pub(crate) use crate::worker::{spawn_login, spawn_rest, spawn_socket, SharedClient};
}

/// The wasm stub: no networking stack compiles there, so every operation
/// resolves to an error event and scripts keep running.
#[cfg(target_family = "wasm")]
mod backend {
    use std::sync::mpsc::{Receiver, Sender};

    use crate::{GamendEvent, SocketCommand};

    #[derive(Clone, Default)]
    pub(crate) struct SharedClient;

    impl SharedClient {
        pub(crate) fn new(_base_url: &str) -> Self {
            Self
        }
    }

    fn refuse(events: &Sender<GamendEvent>, request: u64) {
        let _ = events.send(GamendEvent::Failed {
            request,
            message: "no network backend compiles for wasm".into(),
        });
    }

    pub(crate) fn spawn_login(
        _client: &SharedClient,
        request: u64,
        _credentials: crate::LoginCredentials,
        events: &Sender<GamendEvent>,
    ) {
        refuse(events, request);
    }

    pub(crate) fn spawn_rest(
        _client: &SharedClient,
        request: u64,
        _method: String,
        _path: String,
        _body: Option<serde_json::Value>,
        events: &Sender<GamendEvent>,
    ) {
        refuse(events, request);
    }

    pub(crate) fn spawn_socket(
        _client: &SharedClient,
        socket: u64,
        _commands: Receiver<SocketCommand>,
        events: &Sender<GamendEvent>,
    ) {
        let _ = events.send(GamendEvent::SocketError {
            socket,
            reason: "no network backend compiles for wasm".into(),
        });
    }
}

/// Login input, mirrored from [`gamend_client::Credentials`] so the wasm
/// stub compiles without the client crate.
pub enum LoginCredentials {
    EmailPassword { email: String, password: String },
    Device { device_id: String },
}

/// What the engine thread asks a socket worker to do.
pub(crate) enum SocketCommand {
    Join {
        request: u64,
        topic: String,
        payload: Json,
    },
    Push {
        request: u64,
        topic: String,
        event: String,
        payload: Json,
    },
    Leave {
        request: u64,
        topic: String,
    },
    CallHook {
        request: u64,
        plugin: String,
        function: String,
        args: Json,
    },
    Close,
}

/// A completion crossing from a worker thread back to the frame loop.
pub(crate) enum GamendEvent {
    LoggedIn {
        request: u64,
        user_id: String,
        username: String,
        display_name: String,
    },
    RestDone {
        request: u64,
        status: u16,
        body: Json,
    },
    /// A one-shot operation that could not produce a result at all.
    Failed {
        request: u64,
        message: String,
    },
    /// The reply to a join, push, leave or call_hook.
    Replied {
        request: u64,
        status: String,
        response: Json,
    },
    SocketOpen {
        socket: u64,
    },
    SocketMessage {
        socket: u64,
        topic: String,
        event: String,
        payload: Json,
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

/// Where results go: a method on one node's script.
#[derive(Clone)]
pub struct Handler {
    pub node: NodeId,
    pub method: String,
}

/// Handle tables, the shared client, and the channel workers report into.
pub struct GamendState {
    client: Option<backend::SharedClient>,
    events: Receiver<GamendEvent>,
    report: Sender<GamendEvent>,
    sockets: DetHashMap<u64, Sender<SocketCommand>>,
    request_handlers: DetHashMap<u64, Handler>,
    socket_handlers: DetHashMap<u64, Handler>,
}

impl Default for GamendState {
    fn default() -> Self {
        let (report, events) = channel();
        Self {
            client: None,
            events,
            report,
            sockets: DetHashMap::default(),
            request_handlers: DetHashMap::default(),
            socket_handlers: DetHashMap::default(),
        }
    }
}

impl GamendState {
    /// Point the plugin at a server. Must happen before anything else; a
    /// second call replaces the client (and forgets the session).
    pub fn configure(&mut self, base_url: &str) {
        self.client = Some(backend::SharedClient::new(base_url));
    }

    fn client(&self) -> Result<backend::SharedClient> {
        self.client
            .clone()
            .ok_or_else(|| anyhow!("gamend.configure(url) must be called first"))
    }

    pub fn login(
        &mut self,
        request: u64,
        credentials: LoginCredentials,
        handler: Option<Handler>,
    ) -> Result<()> {
        let client = self.client()?;
        if let Some(handler) = handler {
            self.request_handlers.insert(request, handler);
        }
        backend::spawn_login(&client, request, credentials, &self.report);
        Ok(())
    }

    pub fn rest(
        &mut self,
        request: u64,
        method: String,
        path: String,
        body: Option<Json>,
        handler: Option<Handler>,
    ) -> Result<()> {
        let client = self.client()?;
        if let Some(handler) = handler {
            self.request_handlers.insert(request, handler);
        }
        backend::spawn_rest(&client, request, method, path, body, &self.report);
        Ok(())
    }

    /// Open the realtime connection. The worker joins the session's own
    /// `user:<id>` topic before reporting `open`, so hooks work immediately.
    pub fn connect(&mut self, socket: u64, handler: Option<Handler>) -> Result<()> {
        let client = self.client()?;
        if let Some(handler) = handler {
            self.socket_handlers.insert(socket, handler);
        }
        let (commands, receiver) = channel();
        backend::spawn_socket(&client, socket, receiver, &self.report);
        self.sockets.insert(socket, commands);
        Ok(())
    }

    fn command(&mut self, socket: u64, request: u64, command: SocketCommand) -> Result<()> {
        // The one-shot inherits the socket's handler, so replies reach the
        // same method its stream events do.
        if let Some(handler) = self.socket_handlers.get(&socket).cloned() {
            self.request_handlers.insert(request, handler);
        }
        self.sockets
            .get(&socket)
            .ok_or_else(|| anyhow!("no such gamend socket"))?
            .send(command)
            .map_err(|_| anyhow!("the gamend connection is gone"))
    }

    pub fn join(&mut self, socket: u64, request: u64, topic: String, payload: Json) -> Result<()> {
        self.command(
            socket,
            request,
            SocketCommand::Join {
                request,
                topic,
                payload,
            },
        )
    }

    pub fn push(
        &mut self,
        socket: u64,
        request: u64,
        topic: String,
        event: String,
        payload: Json,
    ) -> Result<()> {
        self.command(
            socket,
            request,
            SocketCommand::Push {
                request,
                topic,
                event,
                payload,
            },
        )
    }

    pub fn leave(&mut self, socket: u64, request: u64, topic: String) -> Result<()> {
        self.command(socket, request, SocketCommand::Leave { request, topic })
    }

    pub fn call_hook(
        &mut self,
        socket: u64,
        request: u64,
        plugin: String,
        function: String,
        args: Json,
    ) -> Result<()> {
        self.command(
            socket,
            request,
            SocketCommand::CallHook {
                request,
                plugin,
                function,
                args,
            },
        )
    }

    pub fn close(&mut self, socket: u64) -> bool {
        self.sockets
            .get(&socket)
            .is_some_and(|commands| commands.send(SocketCommand::Close).is_ok())
    }
}

/// This tick's events, as the neutral values handlers received. Scripts
/// never read it — it is the Rust-side view and the replay tap.
#[derive(Default)]
pub struct GamendSnapshot {
    pub events: Vec<Value>,
}

/// Drain worker reports, record them, then dispatch and wake — in arrival
/// order, after the borrows are released so a handler may call back in.
fn pump_gamend_system(eng: &Engine, _: f32) {
    let mut dispatches: Vec<(Option<Handler>, Option<u64>, Value)> = Vec::new();
    {
        let state = eng.resource::<GamendState>();
        let snapshot = eng.resource::<GamendSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        snapshot.events.clear();
        while let Ok(event) = state.events.try_recv() {
            let (handler, wake) = match &event {
                GamendEvent::LoggedIn { request, .. }
                | GamendEvent::RestDone { request, .. }
                | GamendEvent::Failed { request, .. }
                | GamendEvent::Replied { request, .. } => {
                    (state.request_handlers.shift_remove(request), Some(*request))
                }
                GamendEvent::SocketClosed { socket, .. }
                | GamendEvent::SocketError { socket, .. } => {
                    state.sockets.shift_remove(socket);
                    (state.socket_handlers.shift_remove(socket), None)
                }
                GamendEvent::SocketOpen { socket } | GamendEvent::SocketMessage { socket, .. } => {
                    (state.socket_handlers.get(socket).cloned(), None)
                }
            };
            let value = event_value(event);
            if handler.is_some() || wake.is_some() {
                dispatches.push((handler, wake, value.clone()));
            }
            snapshot.events.push(value);
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

fn event_value(event: GamendEvent) -> Value {
    let json_or_nil = |v: &Json| from_json(v).unwrap_or(Value::Nil);
    let pairs = match event {
        GamendEvent::LoggedIn {
            request,
            user_id,
            username,
            display_name,
        } => vec![
            ("request".into(), int(request)),
            ("kind".into(), Value::Str("login".into())),
            ("user_id".into(), Value::Str(user_id)),
            ("username".into(), Value::Str(username)),
            ("display_name".into(), Value::Str(display_name)),
        ],
        GamendEvent::RestDone {
            request,
            status,
            body,
        } => vec![
            ("request".into(), int(request)),
            ("kind".into(), Value::Str("rest".into())),
            ("status".into(), Value::Int(i64::from(status))),
            ("body".into(), json_or_nil(&body)),
        ],
        GamendEvent::Failed { request, message } => vec![
            ("request".into(), int(request)),
            ("kind".into(), Value::Str("error".into())),
            ("error".into(), Value::Str(message)),
        ],
        GamendEvent::Replied {
            request,
            status,
            response,
        } => vec![
            ("request".into(), int(request)),
            ("kind".into(), Value::Str("reply".into())),
            ("status".into(), Value::Str(status)),
            ("response".into(), json_or_nil(&response)),
        ],
        GamendEvent::SocketOpen { socket } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("open".into())),
        ],
        GamendEvent::SocketMessage {
            socket,
            topic,
            event,
            payload,
        } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("message".into())),
            ("topic".into(), Value::Str(topic)),
            ("event".into(), Value::Str(event)),
            ("payload".into(), json_or_nil(&payload)),
        ],
        GamendEvent::SocketClosed { socket, reason } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("closed".into())),
            ("reason".into(), Value::Str(reason)),
        ],
        GamendEvent::SocketError { socket, reason } => vec![
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

pub struct GamendPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for GamendPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("gamend", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for GamendPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(GamendState::default());
        reg.insert_resource(GamendSnapshot::default());
        reg.add_system(Stage::First, pump_gamend_system);
        let mut m = reg.script_module("gamend")?;
        install_gamend_api(&mut *m);
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

/// The handler a node-and-options pair names; a nil node relies on await.
fn handler_of(node: &Value, opts: Option<&Value>, default_method: &str) -> Result<Option<Handler>> {
    let node = match node {
        Value::Node(id) => NodeId(*id),
        Value::Nil => return Ok(None),
        other => return Err(anyhow!("argument 0 should be a node or nil, got {other:?}")),
    };
    let method = match opt(opts, "on_event") {
        Some(Value::Str(name)) => name.clone(),
        Some(other) => return Err(anyhow!("`on_event` should be a method name, got {other:?}")),
        None => default_method.to_string(),
    };
    Ok(Some(Handler { node, method }))
}

fn credentials_of(spec: &Value) -> Result<LoginCredentials> {
    let field = |key: &str| match opt(Some(spec), key) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    if let Some(device_id) = field("device_id") {
        return Ok(LoginCredentials::Device { device_id });
    }
    match (field("email"), field("password")) {
        (Some(email), Some(password)) => Ok(LoginCredentials::EmailPassword { email, password }),
        _ => Err(anyhow!(
            "credentials need `device_id`, or `email` and `password`"
        )),
    }
}

fn json_of(value: Option<&Value>) -> Result<Json> {
    match value {
        None | Some(Value::Nil) => Ok(Json::Object(serde_json::Map::new())),
        Some(v) => balaur_core::engine_api::to_json(v),
    }
}

/// `gamend.*`. Declared against the neutral seam, so it works on any
/// backend. One-shots return an awaitable id; socket events stream to the
/// connect call's handler method (default `on_gamend_event`).
fn install_gamend_api(m: &mut dyn Bindings<Engine>) {
    // `gamend.configure(url)` — where the server lives. Everything else
    // errors until this is called.
    m.function("configure", |eng: &Engine, url: String| {
        eng.resource::<GamendState>().borrow_mut().configure(&url);
        Ok(())
    });
    // `gamend.login(node|nil, { device_id = ... } or { email = ..,
    // password = .. })` -> id. Completion: `{ request, user_id, username,
    // display_name }` or `{ request, error }`.
    m.function(
        "login",
        |eng: &Engine, (node, spec, opts): (Value, Option<Value>, Option<Value>)| {
            let (node, spec) = normalize_target(node, spec);
            let credentials = credentials_of(
                spec.as_ref()
                    .ok_or_else(|| anyhow!("login needs a credentials table"))?,
            )?;
            let handler = handler_of(&node, opts.as_ref(), "on_gamend_event")?;
            let id = eng.next_token();
            eng.resource::<GamendState>()
                .borrow_mut()
                .login(id, credentials, handler)?;
            Ok(int(id))
        },
    );
    // `gamend.rest(node|nil, method, path, body?)` -> id. Completion:
    // `{ request, status, body }` or `{ request, error }`.
    m.function(
        "rest",
        |eng: &Engine, (node, method, path, body): (Value, String, String, Option<Value>)| {
            let handler = handler_of(&node, None, "on_gamend_event")?;
            let body = match body {
                None | Some(Value::Nil) => None,
                Some(v) => Some(balaur_core::engine_api::to_json(&v)?),
            };
            let id = eng.next_token();
            eng.resource::<GamendState>().borrow_mut().rest(
                id,
                method.to_uppercase(),
                path,
                body,
                handler,
            )?;
            Ok(int(id))
        },
    );
    // `gamend.connect(node|nil, { on_event = "on_gamend_event" })` -> socket
    // id. The worker logs in to the realtime endpoint with the current
    // session and joins the own-user topic before reporting `open`.
    m.function(
        "connect",
        |eng: &Engine, (node, opts): (Value, Option<Value>)| {
            let handler = handler_of(&node, opts.as_ref(), "on_gamend_event")?;
            let id = eng.next_token();
            eng.resource::<GamendState>()
                .borrow_mut()
                .connect(id, handler)?;
            Ok(int(id))
        },
    );
    install_gamend_socket_api(m);
}

/// The per-connection half of `gamend.*`: operations on an open socket.
fn install_gamend_socket_api(m: &mut dyn Bindings<Engine>) {
    // `gamend.join(socket, topic, payload?)` -> id; reply arrives as
    // `{ request, kind = "reply", status, response }`.
    m.function(
        "join",
        |eng: &Engine, (socket, topic, payload): (i64, String, Option<Value>)| {
            let id = eng.next_token();
            eng.resource::<GamendState>().borrow_mut().join(
                token_of(socket)?,
                id,
                topic,
                json_of(payload.as_ref())?,
            )?;
            Ok(int(id))
        },
    );
    // `gamend.push(socket, topic, event, payload?)` -> id; reply as join's.
    m.function(
        "push",
        |eng: &Engine, (socket, topic, event, payload): (i64, String, String, Option<Value>)| {
            let id = eng.next_token();
            eng.resource::<GamendState>().borrow_mut().push(
                token_of(socket)?,
                id,
                topic,
                event,
                json_of(payload.as_ref())?,
            )?;
            Ok(int(id))
        },
    );
    // `gamend.leave(socket, topic)` -> id; reply as join's.
    m.function("leave", |eng: &Engine, (socket, topic): (i64, String)| {
        let id = eng.next_token();
        eng.resource::<GamendState>()
            .borrow_mut()
            .leave(token_of(socket)?, id, topic)?;
        Ok(int(id))
    });
    // `gamend.call_hook(socket, plugin, name, args?)` -> id. The server
    // hook's reply arrives as `{ request, status, response }`, where an ok
    // response is `{ data = ... }` and an error one `{ error = "..." }`.
    m.function(
        "call_hook",
        |eng: &Engine, (socket, plugin, name, args): (i64, String, String, Option<Value>)| {
            let args = match args {
                None | Some(Value::Nil) => Json::Array(Vec::new()),
                Some(v) => balaur_core::engine_api::to_json(&v)?,
            };
            let id = eng.next_token();
            eng.resource::<GamendState>().borrow_mut().call_hook(
                token_of(socket)?,
                id,
                plugin,
                name,
                args,
            )?;
            Ok(int(id))
        },
    );
    // `gamend.close(socket)` — false when the connection is already gone.
    m.function("close", |eng: &Engine, socket: i64| {
        let closed = u64::try_from(socket)
            .is_ok_and(|id| eng.resource::<GamendState>().borrow_mut().close(id));
        Ok(Value::Bool(closed))
    });
}

/// `login` may be called with or without a leading node, like
/// `http.request`: a credentials map in position 0 means no handler node.
fn normalize_target(node: Value, spec: Option<Value>) -> (Value, Option<Value>) {
    match &node {
        Value::Map(_) => (Value::Nil, Some(node)),
        _ => (node, spec),
    }
}

fn token_of(id: i64) -> Result<u64> {
    u64::try_from(id).map_err(|_| anyhow!("not a gamend handle: {id}"))
}
