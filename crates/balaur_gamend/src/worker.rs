//! The worker threads driving [`crate::client`].
//!
//! REST operations get a short-lived thread each; the realtime connection
//! gets one long-lived thread that alternates between the engine's commands
//! and the socket. The shared client holds the session, so a login on one
//! thread authenticates every call after it.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use crate::channels::{Rejoin, Rejoins};
use crate::client::{Client, Credentials, Socket, SocketEvent, auth};
use serde_json::Value as Json;

use crate::{GamendEvent, LoginCredentials, SocketCommand};

/// One client, shared by every worker: `Mutex` because REST threads and the
/// socket thread all borrow the session.
#[derive(Clone)]
pub(crate) struct SharedClient(Arc<Mutex<Client>>);

impl SharedClient {
    pub(crate) fn new(base_url: &str) -> Self {
        Self(Arc::new(Mutex::new(Client::new(base_url))))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Client> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn session(&self) -> Option<crate::client::Session> {
        self.lock().session().cloned()
    }

    pub(crate) fn set_session(&self, session: Option<crate::client::Session>) {
        self.lock().set_session(session);
    }
}

pub(crate) fn spawn_login(
    client: &SharedClient,
    request: u64,
    credentials: LoginCredentials,
    events: &Sender<GamendEvent>,
) {
    let client = client.clone();
    let events = events.clone();
    let credentials = Credentials::from(credentials);
    std::thread::spawn(move || {
        let outcome = auth::login(&mut client.lock(), &credentials).map_err(|err| err.to_string());
        let _ = events.send(GamendEvent::logged_in(request, outcome));
    });
}

pub(crate) fn spawn_rest(
    client: &SharedClient,
    request: u64,
    method: String,
    path: String,
    body: Option<Json>,
    events: &Sender<GamendEvent>,
) {
    let client = client.clone();
    let events = events.clone();
    std::thread::spawn(move || {
        let outcome = client
            .lock()
            .call(&method, &path, body.as_ref())
            .map_err(|err| err.to_string());
        let _ = events.send(GamendEvent::rest_done(request, outcome));
    });
}

pub(crate) fn spawn_socket(
    client: &SharedClient,
    socket: u64,
    commands: Receiver<SocketCommand>,
    events: &Sender<GamendEvent>,
) {
    let client = client.clone();
    let events = events.clone();
    std::thread::spawn(move || {
        let event = match open(&client, socket, &commands, &events) {
            Ok(event) => event,
            Err(err) => GamendEvent::SocketError {
                socket,
                reason: err.to_string(),
            },
        };
        let _ = events.send(event);
    });
}

/// Seconds before its expiry that a token counts as stale: a connection
/// takes a moment to open.
const RENEW_MARGIN: i64 = 30;

/// The socket's url and the own-user topic, for the session in hand.
fn socket_url(client: &SharedClient) -> anyhow::Result<(String, String)> {
    let client = client.lock();
    let session = client
        .session()
        .ok_or_else(|| anyhow::anyhow!("connect needs a logged-in session"))?;
    let url = format!(
        "{}/socket/websocket?token={}&client_session={}&vsn=2.0.0",
        client.base_url().replacen("http", "ws", 1),
        session.access_token,
        crate::client::run_id()
    );
    Ok((url, format!("user:{}", session.user_id)))
}

/// Whether the handshake failed because the server refused the token.
fn refused(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<tungstenite::Error>(),
        Some(tungstenite::Error::Http(response)) if matches!(response.status().as_u16(), 401 | 403)
    )
}

#[allow(
    clippy::disallowed_methods,
    reason = "a token's expiry is wall-clock time, not simulation"
)]
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// How a served connection ended.
enum Served {
    /// The game closed the socket.
    Closed,
    /// The connection dropped, for this reason.
    Dropped(String),
}

/// Connect, join the own-user topic, then serve until the game closes the
/// socket. A drop reconnects, backing off, and joins again every topic the
/// game had joined. The returned event is the connection's final word.
fn open(
    client: &SharedClient,
    socket: u64,
    commands: &Receiver<SocketCommand>,
    events: &Sender<GamendEvent>,
) -> anyhow::Result<GamendEvent> {
    let (mut connection, user_topic) = dial(client)?;
    // The own-user channel carries hooks, notifications and profile pushes;
    // joining it first means `open` implies "ready for call_hook".
    if !join(
        &mut connection,
        &user_topic,
        &Json::Object(serde_json::Map::new()),
        socket,
        events,
    )? {
        anyhow::bail!("joining the user channel was refused");
    }
    let _ = events.send(GamendEvent::SocketOpen { socket });
    // What the game joined, with its payload: what a reconnect joins again.
    let mut topics: Vec<(String, Json)> = Vec::new();
    loop {
        let reason = match serve(
            &mut connection,
            socket,
            commands,
            events,
            &user_topic,
            &mut topics,
        ) {
            Served::Closed => return Ok(closed_by_game(socket)),
            Served::Dropped(reason) => reason,
        };
        match reconnect(client, socket, commands, events, &mut topics, reason) {
            Ok(Some(fresh)) => connection = fresh,
            Ok(None) => return Ok(closed_by_game(socket)),
            Err(gave_up) => {
                return Ok(GamendEvent::SocketError {
                    socket,
                    reason: gave_up.to_string(),
                });
            }
        }
    }
}

fn closed_by_game(socket: u64) -> GamendEvent {
    GamendEvent::SocketClosed {
        socket,
        reason: "closed by the game".into(),
    }
}

/// A connection the server takes, and the own-user topic. The server reads
/// the token only here: a stale one is renewed first, a refused one once
/// more.
fn dial(client: &SharedClient) -> anyhow::Result<(Socket, String)> {
    let stale = client
        .lock()
        .session()
        .is_some_and(|session| session.stale(unix_now(), RENEW_MARGIN));
    if stale {
        client.lock().renew()?;
    }
    let (url, user_topic) = socket_url(client)?;
    let connection = match Socket::connect(&url) {
        Err(err) if refused(&err) => {
            client.lock().renew()?;
            Socket::connect(&socket_url(client)?.0)?
        }
        other => other?,
    };
    Ok((connection, user_topic))
}

/// Run the game's commands and the server's frames until the game closes the
/// socket or the connection drops.
fn serve(
    connection: &mut Socket,
    socket: u64,
    commands: &Receiver<SocketCommand>,
    events: &Sender<GamendEvent>,
    user_topic: &str,
    topics: &mut Vec<(String, Json)>,
) -> Served {
    let mut calls = Calls::default();
    let mut rejoins = Rejoins::default();
    let clock = Clock::start();
    let reason = loop {
        match run_commands(connection, commands, user_topic, &mut calls, topics) {
            Ok(true) => {}
            Ok(false) => {
                calls.fail(events);
                return Served::Closed;
            }
            Err(err) => break err.to_string(),
        }
        if let Err(err) = send_rejoins(connection, &mut rejoins, &mut calls, clock.now()) {
            break err.to_string();
        }
        let arrived = match connection.poll() {
            Ok(arrived) => arrived,
            Err(err) => break err.to_string(),
        };
        let mut ended = None;
        for event in arrived {
            match event {
                SocketEvent::Reply {
                    reference,
                    status,
                    response,
                    ..
                } => match calls.take_rejoin(&reference) {
                    Some(rejoin) => rejoins.answered(rejoin, status == "ok", topics, clock.now()),
                    None => calls.replied(events, topics, &reference, status, response),
                },
                SocketEvent::Message {
                    topic,
                    event,
                    payload,
                } => {
                    rejoins.heard(&event, &topic, user_topic, topics, clock.now());
                    forward_message(events, socket, topic, event, payload);
                }
                SocketEvent::Closed { reason } if reason.is_empty() => {
                    ended = Some(String::from("the server closed the connection"));
                }
                SocketEvent::Closed { reason } => ended = Some(reason),
            }
        }
        if let Some(reason) = ended {
            break reason;
        }
    };
    calls.fail(events);
    Served::Dropped(reason)
}

/// Join again the channels whose time has come.
fn send_rejoins(
    connection: &mut Socket,
    rejoins: &mut Rejoins,
    calls: &mut Calls,
    now: f64,
) -> anyhow::Result<()> {
    for rejoin in rejoins.take_due(now) {
        let reference = connection.join(&rejoin.topic, &rejoin.payload)?;
        calls.rejoining.push((reference, rejoin));
    }
    Ok(())
}

/// Seconds since a connection opened, for the rejoin waits.
struct Clock(std::time::Instant);

impl Clock {
    #[allow(
        clippy::disallowed_methods,
        reason = "a network back-off, not simulation"
    )]
    fn start() -> Self {
        Self(std::time::Instant::now())
    }

    fn now(&self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
}

/// The calls waiting on a reply over one connection.
#[derive(Default)]
struct Calls {
    /// ref → the request id whose reply it will carry.
    pending: Vec<(String, u64)>,
    /// ref → the topic a join asked for, forgotten if the server refuses it.
    joining: Vec<(String, String)>,
    /// ref → a crashed channel's join, sent by the socket itself.
    rejoining: Vec<(String, Rejoin)>,
}

impl Calls {
    fn take_rejoin(&mut self, reference: &str) -> Option<Rejoin> {
        let at = self.rejoining.iter().position(|(r, _)| r == reference)?;
        Some(self.rejoining.remove(at).1)
    }

    fn replied(
        &mut self,
        events: &Sender<GamendEvent>,
        topics: &mut Vec<(String, Json)>,
        reference: &str,
        status: String,
        response: Json,
    ) {
        if let Some(at) = self.joining.iter().position(|(r, _)| r == reference) {
            let (_, topic) = self.joining.remove(at);
            if status != "ok" {
                topics.retain(|(joined, _)| *joined != topic);
            }
        }
        let Some(at) = self.pending.iter().position(|(r, _)| r == reference) else {
            return;
        };
        let (_, request) = self.pending.remove(at);
        let _ = events.send(GamendEvent::Replied {
            request,
            status,
            response,
        });
    }

    /// A reply that will never come is an error the caller must see, not a
    /// task suspended forever.
    fn fail(&mut self, events: &Sender<GamendEvent>) {
        self.joining.clear();
        self.rejoining.clear();
        for (_, request) in self.pending.drain(..) {
            let _ = events.send(GamendEvent::Failed {
                request,
                message: "the connection ended before the reply".into(),
            });
        }
    }
}

/// Come back after a drop, telling the game before each try. `Ok(None)` when
/// the game closed the socket meanwhile; an error once it gives up.
fn reconnect(
    client: &SharedClient,
    socket: u64,
    commands: &Receiver<SocketCommand>,
    events: &Sender<GamendEvent>,
    topics: &mut Vec<(String, Json)>,
    mut reason: String,
) -> anyhow::Result<Option<Socket>> {
    for attempt in 1..=crate::RECONNECT_TRIES {
        let wait = crate::backoff(attempt);
        let _ = events.send(GamendEvent::SocketReconnecting {
            socket,
            attempt,
            reason: reason.clone(),
            wait,
        });
        if !wait_out(wait, commands, events, topics) {
            return Ok(None);
        }
        match reopen(client, socket, events, topics) {
            Ok(connection) => return Ok(Some(connection)),
            Err(err) => reason = err.to_string(),
        }
    }
    anyhow::bail!("gave up after {} tries: {reason}", crate::RECONNECT_TRIES)
}

/// Sit out a back-off. A call made meanwhile fails at once rather than wait
/// on a connection that may not come back; a leave drops its topic from what
/// is joined again. False when the game closed the socket.
#[allow(
    clippy::disallowed_methods,
    reason = "a network back-off, not simulation"
)]
fn wait_out(
    seconds: f64,
    commands: &Receiver<SocketCommand>,
    events: &Sender<GamendEvent>,
    topics: &mut Vec<(String, Json)>,
) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds);
    while std::time::Instant::now() < until {
        loop {
            let command = match commands.try_recv() {
                Ok(command) => command,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return false,
            };
            match command {
                SocketCommand::Close => return false,
                SocketCommand::Interrupt => {}
                SocketCommand::Leave { request, topic } => {
                    topics.retain(|(joined, _)| *joined != topic);
                    let _ = events.send(GamendEvent::Replied {
                        request,
                        status: "ok".into(),
                        response: Json::Null,
                    });
                }
                SocketCommand::Join { request, .. }
                | SocketCommand::Push { request, .. }
                | SocketCommand::CallHook { request, .. } => {
                    let _ = events.send(GamendEvent::Failed {
                        request,
                        message: "the socket is reconnecting".into(),
                    });
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    true
}

/// A new connection with the own-user topic and every topic the game joined
/// joined again. One the server refuses now is dropped and reported.
fn reopen(
    client: &SharedClient,
    socket: u64,
    events: &Sender<GamendEvent>,
    topics: &mut Vec<(String, Json)>,
) -> anyhow::Result<Socket> {
    let (mut connection, user_topic) = dial(client)?;
    if !join(
        &mut connection,
        &user_topic,
        &Json::Object(serde_json::Map::new()),
        socket,
        events,
    )? {
        anyhow::bail!("joining the user channel was refused");
    }
    let mut lost = Vec::new();
    for (topic, payload) in topics.clone() {
        if !join(&mut connection, &topic, &payload, socket, events)? {
            lost.push(topic);
        }
    }
    topics.retain(|(topic, _)| !lost.contains(topic));
    let _ = events.send(GamendEvent::SocketReopened { socket, lost });
    Ok(connection)
}

/// A channel message, handed to the frame loop as it arrived.
fn forward_message(
    events: &Sender<GamendEvent>,
    socket: u64,
    topic: String,
    event: String,
    payload: Json,
) {
    let _ = events.send(GamendEvent::SocketMessage {
        socket,
        topic,
        event,
        payload,
    });
}

/// Join a topic and pump the socket until its reply lands, forwarding
/// whatever else arrives meanwhile. False when the server refused it; an
/// error when the connection failed first.
fn join(
    connection: &mut Socket,
    topic: &str,
    payload: &Json,
    socket: u64,
    events: &Sender<GamendEvent>,
) -> anyhow::Result<bool> {
    let wanted = connection.join(topic, payload)?;
    for _ in 0..400 {
        for event in connection.poll()? {
            match event {
                SocketEvent::Reply {
                    reference, status, ..
                } if reference == wanted => return Ok(status == "ok"),
                SocketEvent::Message {
                    topic,
                    event,
                    payload,
                } => forward_message(events, socket, topic, event, payload),
                SocketEvent::Closed { reason } => {
                    anyhow::bail!("connection closed during join: {reason}")
                }
                SocketEvent::Reply { .. } => {}
            }
        }
    }
    anyhow::bail!("joining {topic} never got a reply")
}

/// Apply queued commands. `Ok(false)` means the game asked to close.
fn run_commands(
    connection: &mut Socket,
    commands: &Receiver<SocketCommand>,
    user_topic: &str,
    calls: &mut Calls,
    topics: &mut Vec<(String, Json)>,
) -> anyhow::Result<bool> {
    loop {
        let command = match commands.try_recv() {
            Ok(command) => command,
            Err(TryRecvError::Empty) => return Ok(true),
            Err(TryRecvError::Disconnected) => return Ok(false),
        };
        let (reference, request) = match command {
            SocketCommand::Join {
                request,
                topic,
                payload,
            } => {
                let reference = connection.join(&topic, &payload)?;
                calls.joining.push((reference.clone(), topic.clone()));
                topics.retain(|(joined, _)| *joined != topic);
                topics.push((topic, payload));
                (reference, request)
            }
            SocketCommand::Push {
                request,
                topic,
                event,
                payload,
            } => (connection.push(&topic, &event, &payload)?, request),
            SocketCommand::Leave { request, topic } => {
                topics.retain(|(joined, _)| *joined != topic);
                (connection.leave(&topic)?, request)
            }
            SocketCommand::CallHook {
                request,
                plugin,
                function,
                args,
            } => {
                let payload = serde_json::json!({ "plugin": plugin, "fn": function, "args": args });
                (connection.push(user_topic, "call_hook", &payload)?, request)
            }
            SocketCommand::Close => return Ok(false),
            SocketCommand::Interrupt => anyhow::bail!("interrupted by the game"),
        };
        calls.pending.push((reference, request));
    }
}
