//! The worker threads driving [`crate::client`].
//!
//! REST operations get a short-lived thread each; the realtime connection
//! gets one long-lived thread that alternates between the engine's commands
//! and the socket. The shared client holds the session, so a login on one
//! thread authenticates every call after it.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use crate::client::{auth, Client, Credentials, Socket, SocketEvent};
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

/// Connect, join the own-user topic, then serve until the connection ends.
/// The returned event is the connection's final word.
fn open(
    client: &SharedClient,
    socket: u64,
    commands: &Receiver<SocketCommand>,
    events: &Sender<GamendEvent>,
) -> anyhow::Result<GamendEvent> {
    let (url, user_topic) = {
        let client = client.lock();
        let session = client
            .session()
            .ok_or_else(|| anyhow::anyhow!("connect needs a logged-in session"))?;
        let ws = format!(
            "{}/socket/websocket?token={}&vsn=2.0.0",
            client.base_url().replacen("http", "ws", 1),
            session.access_token
        );
        (ws, format!("user:{}", session.user_id))
    };
    let mut connection = Socket::connect(&url)?;

    // The own-user channel carries hooks, notifications and profile pushes;
    // joining it first means `open` implies "ready for call_hook".
    let join_ref = connection.join(&user_topic, &serde_json::json!({}))?;
    wait_join(&mut connection, &join_ref, socket, events)?;
    let _ = events.send(GamendEvent::SocketOpen { socket });

    // ref → the request id whose reply it will carry.
    let mut pending: Vec<(String, u64)> = Vec::new();
    loop {
        match run_commands(&mut connection, commands, &user_topic, &mut pending) {
            Ok(true) => {}
            Ok(false) => {
                fail_pending(&mut pending, events);
                return Ok(GamendEvent::SocketClosed {
                    socket,
                    reason: "closed by the game".into(),
                });
            }
            Err(err) => {
                fail_pending(&mut pending, events);
                return Ok(GamendEvent::SocketError {
                    socket,
                    reason: err.to_string(),
                });
            }
        }
        for event in connection.poll()? {
            match event {
                SocketEvent::Reply {
                    reference,
                    status,
                    response,
                    ..
                } => {
                    let Some(at) = pending.iter().position(|(r, _)| *r == reference) else {
                        continue;
                    };
                    let (_, request) = pending.remove(at);
                    let _ = events.send(GamendEvent::Replied {
                        request,
                        status,
                        response,
                    });
                }
                SocketEvent::Message {
                    topic,
                    event,
                    payload,
                } => forward_message(events, socket, topic, event, payload),
                SocketEvent::Closed { reason } => {
                    fail_pending(&mut pending, events);
                    return Ok(GamendEvent::SocketClosed { socket, reason });
                }
            }
        }
    }
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

/// A reply that will never come is an error the caller must see, not a task
/// suspended forever.
fn fail_pending(pending: &mut Vec<(String, u64)>, events: &Sender<GamendEvent>) {
    for (_, request) in pending.drain(..) {
        let _ = events.send(GamendEvent::Failed {
            request,
            message: "the connection ended before the reply".into(),
        });
    }
}

/// Pump the socket until the join's own reply lands, forwarding whatever
/// else arrives meanwhile.
fn wait_join(
    connection: &mut Socket,
    join_ref: &str,
    socket: u64,
    events: &Sender<GamendEvent>,
) -> anyhow::Result<()> {
    for _ in 0..400 {
        for event in connection.poll()? {
            match event {
                SocketEvent::Reply {
                    reference, status, ..
                } if reference == join_ref => {
                    if status == "ok" {
                        return Ok(());
                    }
                    anyhow::bail!("joining the user channel was refused: {status}");
                }
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
    anyhow::bail!("the user channel join never got a reply")
}

/// Apply queued commands. `Ok(false)` means the game asked to close.
fn run_commands(
    connection: &mut Socket,
    commands: &Receiver<SocketCommand>,
    user_topic: &str,
    pending: &mut Vec<(String, u64)>,
) -> anyhow::Result<bool> {
    loop {
        let command = match commands.try_recv() {
            Ok(command) => command,
            Err(TryRecvError::Empty) => return Ok(true),
            Err(TryRecvError::Disconnected) => return Ok(false),
        };
        match command {
            SocketCommand::Join {
                request,
                topic,
                payload,
            } => pending.push((connection.join(&topic, &payload)?, request)),
            SocketCommand::Push {
                request,
                topic,
                event,
                payload,
            } => pending.push((connection.push(&topic, &event, &payload)?, request)),
            SocketCommand::Leave { request, topic } => {
                pending.push((connection.leave(&topic)?, request));
            }
            SocketCommand::CallHook {
                request,
                plugin,
                function,
                args,
            } => {
                let payload = serde_json::json!({ "plugin": plugin, "fn": function, "args": args });
                pending.push((connection.push(user_topic, "call_hook", &payload)?, request));
            }
            SocketCommand::Close => return Ok(false),
        }
    }
}
