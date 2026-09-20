//! The Gamend backend as a Balaur plugin: `gamend.*` for scripts.
//!
//! Built on the [`client`] wire layer, with the same delivery contract as the net
//! plugin: all I/O runs on worker threads, completions cross a channel and
//! enter the simulation once per tick at [`Stage::First`] — recorded in
//! [`GamendSnapshot`], dispatched to handler methods, and waking await
//! tokens, in arrival order. Handlers and resumptions never run from an I/O
//! thread, so a recorded snapshot replays a whole online session.
//!
//! The sequential shape reads like this:
//!
//! ```rune
//! pub async fn init(this) {
//!     gamend::configure(()); // [gamend] url, gamend.org by default
//!     task::wait(gamend::login((), #{ device_id: engine::device_id() })).await;
//!     this.socket = gamend::connect(this.node);
//! }
//!
//! pub async fn on_gamend_event(this, e) {
//!     if e["kind"] == "open" {
//!         task::wait(gamend::call_hook(this.socket, "arena", "start", #{})).await;
//!     }
//! }
//! ```
//!
//! One-shot operations (login, rest, call_hook, join, push, leave) wake
//! their returned id and, when a node is given, also dispatch to its
//! handler. Socket events are a stream, so they only dispatch. Every event
//! map carries a `kind` — `login`, `rest`, `reply`, `error`, `open`,
//! `message`, `reconnecting`, `reopened`, `closed` — so one handler can take
//! them all.

use std::sync::mpsc::{Sender, channel};

use anyhow::{Result, anyhow};
use balaur_core::engine_api::from_json;
use balaur_core::replay::ExternalIo;
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};
use serde_json::Value as Json;

mod activity;
#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod browser;
#[cfg(not(target_os = "emscripten"))]
mod channels;
pub mod client;
mod inspect;
mod target;
#[cfg(not(target_family = "wasm"))]
mod worker;

#[cfg(not(target_family = "wasm"))]
mod backend {
    pub(crate) use crate::worker::{SharedClient, spawn_login, spawn_rest, spawn_socket};

    /// Nothing to pump: the worker threads deliver on their own.
    pub(crate) fn pump() {}
}

#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod backend {
    pub(crate) use crate::browser::{SharedClient, pump, spawn_login, spawn_rest, spawn_socket};
}

/// The emscripten stub: no networking stack compiles there, so every
/// operation resolves to an error event and scripts keep running.
#[cfg(all(target_family = "wasm", target_os = "emscripten"))]
mod backend {
    use std::sync::mpsc::{Receiver, Sender};

    use crate::{GamendEvent, SocketCommand};

    #[derive(Clone, Default)]
    pub(crate) struct SharedClient;

    impl SharedClient {
        pub(crate) fn new(_base_url: &str) -> Self {
            Self
        }

        pub(crate) fn session(&self) -> Option<crate::client::Session> {
            None
        }

        pub(crate) fn set_session(&self, _session: Option<crate::client::Session>) {}
    }

    pub(crate) fn pump() {}

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

/// Login input, mirrored from [`client::auth::Credentials`] so the wasm
/// stub compiles without the wire layer.
pub enum LoginCredentials {
    EmailPassword {
        email: String,
        password: String,
    },
    Device {
        device_id: String,
    },
    Register {
        email: String,
        password: String,
        username: Option<String>,
    },
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
    /// Cut the connection as a network failure would, so it reconnects.
    Interrupt,
}

/// A completion crossing from a worker thread back to the frame loop.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
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
    /// The connection dropped; try `attempt` starts in `wait` seconds.
    SocketReconnecting {
        socket: u64,
        attempt: u32,
        reason: String,
        wait: f64,
    },
    /// The connection is back and its topics joined again, but for `lost`,
    /// which the server refused.
    SocketReopened {
        socket: u64,
        lost: Vec<String>,
    },
}

/// How many times a dropped socket tries to come back before it gives up.
pub(crate) const RECONNECT_TRIES: u32 = 8;

/// Seconds before try `attempt`, counted from 1: 1, 2, 4, 8, 16, then 30.
pub(crate) fn backoff(attempt: u32) -> f64 {
    f64::from(1_u32 << attempt.saturating_sub(1).min(5)).min(30.0)
}

impl GamendEvent {
    /// A login's outcome, as the frame loop hears it.
    pub(crate) fn logged_in(request: u64, outcome: Result<client::Session, String>) -> Self {
        match outcome {
            Ok(session) => Self::LoggedIn {
                request,
                user_id: session.user_id,
                username: session.username,
                display_name: session.display_name,
            },
            Err(message) => Self::Failed { request, message },
        }
    }

    /// A REST call's outcome, as the frame loop hears it.
    pub(crate) fn rest_done(request: u64, outcome: Result<client::rest::Reply, String>) -> Self {
        match outcome {
            Ok(reply) => Self::RestDone {
                request,
                status: reply.status,
                body: reply.body,
            },
            Err(message) => Self::Failed { request, message },
        }
    }
}

impl From<LoginCredentials> for client::Credentials {
    fn from(credentials: LoginCredentials) -> Self {
        match credentials {
            LoginCredentials::EmailPassword { email, password } => {
                Self::EmailPassword { email, password }
            }
            LoginCredentials::Device { device_id } => Self::Device { device_id },
            LoginCredentials::Register {
                email,
                password,
                username,
            } => Self::Register {
                email,
                password,
                username,
            },
        }
    }
}

/// Where results go: a method on one node's script.
#[derive(Clone)]
pub struct Handler {
    pub node: NodeId,
    pub method: String,
}

/// Handle tables, the shared client, and the channel workers report into.
#[derive(Default)]
pub struct GamendState {
    client: Option<backend::SharedClient>,
    /// The worker channel, this tick's arrivals, and the rule that a replay
    /// never reaches the server — all three live in here.
    io: ExternalIo<GamendEvent>,
    sockets: DetHashMap<u64, Sender<SocketCommand>>,
    request_handlers: DetHashMap<u64, Handler>,
    socket_handlers: DetHashMap<u64, Handler>,
    /// Recent calls and sockets, for a dock to show.
    activity: activity::Activity,
}

impl GamendState {
    /// Point the plugin at a server. Must happen before anything else; a
    /// second call replaces the client (and forgets the session).
    pub fn configure(&mut self, base_url: &str) {
        self.client = Some(backend::SharedClient::new(base_url));
        self.activity.configured(base_url);
    }

    fn client(&self) -> Result<backend::SharedClient> {
        self.client
            .clone()
            .ok_or_else(|| anyhow!("gamend.configure(url) must be called first"))
    }

    pub fn login(
        &mut self,
        eng: &Engine,
        request: u64,
        credentials: LoginCredentials,
        handler: Option<Handler>,
    ) -> Result<()> {
        let client = self.client()?;
        if let Some(handler) = handler {
            self.request_handlers.insert(request, handler);
        }
        let (kind, who) = match &credentials {
            LoginCredentials::Device { .. } => ("login", String::from("device")),
            LoginCredentials::EmailPassword { email, .. } => ("login", email.clone()),
            LoginCredentials::Register { email, .. } => ("register", email.clone()),
        };
        self.activity.started(request, None, kind, who, None);
        self.io.start(eng, |report| {
            backend::spawn_login(&client, request, credentials, report);
        });
        Ok(())
    }

    pub fn rest(
        &mut self,
        eng: &Engine,
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
        self.activity.started(
            request,
            None,
            "rest",
            format!("{method} {path}"),
            body.as_ref(),
        );
        self.io.start(eng, |report| {
            backend::spawn_rest(&client, request, method, path, body, report);
        });
        Ok(())
    }

    /// Open the realtime connection. The worker joins the session's own
    /// `user:<id>` topic before reporting `open`, so hooks work immediately.
    pub fn connect(&mut self, eng: &Engine, socket: u64, handler: Option<Handler>) -> Result<()> {
        let client = self.client()?;
        if let Some(handler) = handler {
            self.socket_handlers.insert(socket, handler);
        }
        self.activity
            .started(socket, None, "connect", String::from("realtime"), None);
        let (commands, receiver) = channel();
        let started = self.io.start(eng, |report| {
            backend::spawn_socket(&client, socket, receiver, report);
        });
        if started {
            self.sockets.insert(socket, commands);
        }
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
        self.activity
            .started(request, Some(socket), "join", topic.clone(), Some(&payload));
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
        self.activity.started(
            request,
            Some(socket),
            "push",
            format!("{topic} {event}"),
            Some(&payload),
        );
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
        self.activity
            .started(request, Some(socket), "leave", topic.clone(), None);
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
        self.activity.started(
            request,
            Some(socket),
            "hook",
            format!("{plugin}.{function}"),
            Some(&args),
        );
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

    pub fn interrupt(&mut self, socket: u64) -> bool {
        self.sockets
            .get(&socket)
            .is_some_and(|commands| commands.send(SocketCommand::Interrupt).is_ok())
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
/// This tick's arrivals, raw. The pump has already stashed them.
fn capture_gamend(eng: &Engine) -> serde_json::Value {
    eng.resource::<GamendState>().borrow().io.capture()
}

/// Back down the same channel the worker threads use, so the pump dispatches
/// them exactly as it did when they came from the server.
fn restore_gamend(eng: &Engine, value: &serde_json::Value) {
    eng.resource::<GamendState>().borrow().io.restore(value);
}

fn pump_gamend_system(eng: &Engine, _: f32) {
    // A backend with no delivery threads (the browser) drives its sockets
    // here; the native one is a no-op.
    backend::pump();
    let mut dispatches: Vec<(Option<Handler>, Option<u64>, Value)> = Vec::new();
    {
        let state = eng.resource::<GamendState>();
        let snapshot = eng.resource::<GamendSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        snapshot.events.clear();
        for event in state.io.drain() {
            state.activity.heard(&event);
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
                GamendEvent::SocketOpen { socket }
                | GamendEvent::SocketMessage { socket, .. }
                | GamendEvent::SocketReconnecting { socket, .. }
                | GamendEvent::SocketReopened { socket, .. } => {
                    (state.socket_handlers.get(socket).cloned(), None)
                }
            };
            let value = event_value(event);
            if let Some(request) = wake {
                state.activity.keep_reply(request, value.clone());
            }
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
            ("user_id".into(), Value::Str(user_id.into())),
            ("username".into(), Value::Str(username.into())),
            ("display_name".into(), Value::Str(display_name.into())),
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
            ("error".into(), Value::Str(message.into())),
        ],
        GamendEvent::Replied {
            request,
            status,
            response,
        } => vec![
            ("request".into(), int(request)),
            ("kind".into(), Value::Str("reply".into())),
            ("status".into(), Value::Str(status.into())),
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
            ("topic".into(), Value::Str(topic.into())),
            ("event".into(), Value::Str(event.into())),
            ("payload".into(), json_or_nil(&payload)),
        ],
        GamendEvent::SocketClosed { socket, reason } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("closed".into())),
            ("reason".into(), Value::Str(reason.into())),
        ],
        GamendEvent::SocketError { socket, reason } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("error".into())),
            ("reason".into(), Value::Str(reason.into())),
        ],
        GamendEvent::SocketReconnecting {
            socket,
            attempt,
            reason,
            wait,
        } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("reconnecting".into())),
            ("attempt".into(), Value::Int(i64::from(attempt))),
            ("reason".into(), Value::Str(reason.into())),
            ("wait".into(), Value::Num(wait)),
        ],
        GamendEvent::SocketReopened { socket, lost } => vec![
            ("socket".into(), int(socket)),
            ("kind".into(), Value::Str("reopened".into())),
            (
                "lost".into(),
                Value::List(lost.into_iter().map(Value::text).collect()),
            ),
        ],
    };
    Value::Map(pairs)
}

pub(crate) fn int(id: u64) -> Value {
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
        reg.add_replay_source("gamend", capture_gamend, restore_gamend);
        target::declare(reg.engine());
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
        None => default_method.to_string().into(),
    };
    Ok(Some(Handler {
        node,
        method: method.to_string(),
    }))
}

fn credentials_of(spec: &Value) -> Result<LoginCredentials> {
    let field = |key: &str| match opt(Some(spec), key) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    if let Some(device_id) = field("device_id") {
        return Ok(LoginCredentials::Device {
            device_id: device_id.to_string(),
        });
    }
    match (field("email"), field("password")) {
        (Some(email), Some(password)) => Ok(LoginCredentials::EmailPassword {
            email: email.to_string(),
            password: password.to_string(),
        }),
        _ => Err(anyhow!(
            "credentials need `device_id`, or `email` and `password`"
        )),
    }
}

fn account_of(spec: &Value) -> Result<LoginCredentials> {
    let field = |key: &str| match opt(Some(spec), key) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    match (field("email"), field("password")) {
        (Some(email), Some(password)) => Ok(LoginCredentials::Register {
            email: email.to_string(),
            password: password.to_string(),
            username: field("username").map(|name| name.to_string()),
        }),
        _ => Err(anyhow!("an account needs an `email` and a `password`")),
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
    m.module_doc(
        "The Gamend backend: session, REST API and realtime socket. Each call returns an id to await; the result also reaches the node's `on_gamend_event` (or `on_event`) as a `kind` map.",
    );
    m.describe(&[
        ("configure", &[], "(url: string?)", "Point the plugin at a server and answer its url; with none, the one `[gamend]` names for this run (see `target`). Every other call errors until this one runs."),
        ("login", &[], "", "Open a session from a `device_id`, or an `email` and `password`, and return the id its `login` result answers."),
        ("register", &[], "(node: node?, account: map)", "Make an account from an `email` and a `password` (and a `username`, generated when left out) and open its session, as `login` does; its result is a `login` one. The server mails the address its confirmation link."),
        ("rest", &[], "", "Call a path on the configured server over HTTP; the result carries the `status` and the decoded `body`."),
        ("connect", &[], "", "Open the realtime socket and return the id `join`, `push`, `leave`, `call_hook` and `close` take. A dropped connection comes back on its own: the handler hears `reconnecting` before each try, then `reopened` once its topics are joined again, or `error` when it gives up."),
    ]);
    // `gamend.configure(url)` — where the server lives. Everything else
    // errors until this is called.
    m.function("configure", |eng: &Engine, url: Option<String>| {
        let url = url
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| target::url(eng));
        eng.resource::<GamendState>().borrow_mut().configure(&url);
        Ok(Value::Str(url.into()))
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
                .login(eng, id, credentials, handler)?;
            Ok(int(id))
        },
    );
    m.function(
        "register",
        |eng: &Engine, (node, spec, opts): (Value, Option<Value>, Option<Value>)| {
            let (node, spec) = normalize_target(node, spec);
            let account = account_of(
                spec.as_ref()
                    .ok_or_else(|| anyhow!("register needs an account table"))?,
            )?;
            let handler = handler_of(&node, opts.as_ref(), "on_gamend_event")?;
            let id = eng.next_token();
            eng.resource::<GamendState>()
                .borrow_mut()
                .login(eng, id, account, handler)?;
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
                eng,
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
                .connect(eng, id, handler)?;
            Ok(int(id))
        },
    );
    install_gamend_socket_api(m);
    inspect::install(m);
}

/// The per-connection half of `gamend.*`: operations on an open socket.
fn install_gamend_socket_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("join", &[], "", "Subscribe the socket to a topic and return the id the server's `reply` answers."),
        ("push", &[], "", "Send an event and its payload to a topic on the socket, returning the id the `reply` answers."),
        ("leave", &[], "", "Unsubscribe the socket from a topic, returning the id the `reply` answers."),
        ("call_hook", &[], "", "Call a server plugin's function over the socket; the reply's `response` holds `data` or `error`."),
        ("close", &[], "", "Shut the socket down; false when the connection was already gone."),
        ("interrupt", &[], "(socket: int)", "Cut the connection as a network failure would, to try a game's reconnect path: the socket reports `reconnecting`, then `reopened`. False when it is already gone."),
    ]);
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
    m.function("interrupt", |eng: &Engine, socket: i64| {
        let cut = u64::try_from(socket)
            .is_ok_and(|id| eng.resource::<GamendState>().borrow_mut().interrupt(id));
        Ok(Value::Bool(cut))
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

#[cfg(test)]
mod tests {
    #[test]
    fn a_reconnect_waits_twice_as_long_each_try_up_to_thirty_seconds() {
        let waits: Vec<f64> = (1..=crate::RECONNECT_TRIES).map(crate::backoff).collect();
        assert_eq!(waits, [1.0, 2.0, 4.0, 8.0, 16.0, 30.0, 30.0, 30.0]);
    }
}
