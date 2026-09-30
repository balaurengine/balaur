//! The realtime socket: Phoenix Channels over a `balaur_websocket`
//! connection, stepped once per tick on the engine thread, native and
//! browser alike.
//!
//! A socket outlives a connection: a drop reconnects, backing off, and joins
//! again every topic the game had joined. Nothing runs here between ticks; a
//! heartbeat or a back-off that falls due while the loop sleeps wakes it
//! through `balaur_core::wake::at`.

use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::time::Instant;
use balaur_websocket::connection::{Arrival, Connection};
use balaur_websocket::{SocketOptions, WebsocketConfig};
use serde_json::{Value as Json, json};

use crate::backend::{SharedClient, spawn_renew};
use crate::channels::{Rejoin, Rejoins};
use crate::client::{Protocol, SocketEvent};
use crate::{GamendEvent, SocketCommand};

/// Seconds before its expiry that a token counts as stale: the server reads
/// the token only as a socket opens.
const RENEW_MARGIN: i64 = 30;

/// Seconds on the socket's clock: the heartbeat's rhythm, never the tick's.
#[allow(
    clippy::disallowed_methods,
    reason = "connection keep-alive and back-off, never a simulation input"
)]
fn now() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Wake the loop when the socket's clock reads `at`.
#[allow(
    clippy::disallowed_methods,
    reason = "a deadline to wake the loop at, never a simulation input"
)]
fn wake_at(at: f64) {
    let left = Duration::from_secs_f64((at - now()).max(0.0));
    balaur_core::wake::at(Instant::now() + left);
}

/// Seconds since 1970, which a token's expiry is written in.
#[allow(
    clippy::disallowed_methods,
    reason = "a token's expiry is wall-clock time, not simulation"
)]
fn unix_now() -> i64 {
    balaur_core::time::SystemTime::now()
        .duration_since(balaur_core::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The socket's url and the own-user topic, for the session in hand.
fn socket_url(client: &SharedClient) -> Result<(String, String)> {
    let session = client
        .session()
        .ok_or_else(|| anyhow!("connect needs a logged-in session"))?;
    let url = format!(
        "{}/socket/websocket?token={}&client_session={}&vsn=2.0.0",
        client.base_url().replacen("http", "ws", 1),
        session.access_token,
        crate::client::run_id()
    );
    Ok((url, format!("user:{}", session.user_id)))
}

/// One connection and the protocol state that lives as long as it does.
struct Link {
    connection: Connection,
    protocol: Protocol,
    opened: bool,
    /// The own-user join's ref, sent on open; the socket reports `open` only
    /// once its reply says ok, so `open` implies "ready for call_hook".
    join_ref: Option<String>,
    joined: bool,
    /// After a reconnect, ref → the topic joined again, until its reply.
    rejoining: Vec<(String, String)>,
}

impl Link {
    fn send(&mut self, frame: &str) -> Result<(), String> {
        self.connection
            .send_text(frame)
            .map_err(|err| format!("websocket send: {err}"))
    }
}

/// Where a socket is between one connection and the next.
enum Phase {
    Live(Link),
    /// Sitting out a back-off until this time on the socket's clock.
    Waiting(f64),
    /// Renewing the session's token; the answer arrives here.
    Renewing(Receiver<Result<(), String>>),
}

/// What the rest of a tick does after a live step.
enum Next {
    Stay,
    Drop(String),
    /// The server refused the token as the connection opened.
    Refused(String),
    ClosedByGame,
}

/// One socket the engine still holds.
pub(crate) struct LiveSocket {
    socket: u64,
    client: SharedClient,
    commands: Receiver<SocketCommand>,
    events: Sender<GamendEvent>,
    user_topic: String,
    phase: Phase,
    /// A connection that never opened is an error, not a drop.
    opened_once: bool,
    /// A refused token is renewed once per dial, not forever.
    renewed_after_refusal: bool,
    tries: u32,
    /// ref → the request id whose reply it will carry.
    pending: Vec<(String, u64)>,
    /// ref → the topic a join asked for, forgotten if the server refuses it.
    joining: Vec<(String, String)>,
    /// What the game joined, with its payload: what a reconnect joins again.
    topics: Vec<(String, Json)>,
    /// Topics the server refused on the last reconnect.
    lost: Vec<String>,
    /// Channels the server crashed, waiting to be joined again, and the
    /// joins sent for them, by ref.
    rejoins: Rejoins,
    rejoining: Vec<(String, Rejoin)>,
}

impl LiveSocket {
    /// A socket that dials on its first step.
    ///
    /// # Errors
    /// When there is no session to connect with.
    pub(crate) fn new(
        client: &SharedClient,
        socket: u64,
        commands: Receiver<SocketCommand>,
        events: &Sender<GamendEvent>,
    ) -> Result<Self> {
        let (_, user_topic) = socket_url(client)?;
        Ok(Self {
            socket,
            client: client.clone(),
            commands,
            events: events.clone(),
            user_topic,
            phase: Phase::Waiting(0.0),
            opened_once: false,
            renewed_after_refusal: false,
            tries: 0,
            pending: Vec::new(),
            joining: Vec::new(),
            topics: Vec::new(),
            lost: Vec::new(),
            rejoins: Rejoins::default(),
            rejoining: Vec::new(),
        })
    }

    /// One tick of the socket. Answers whether it is still alive.
    pub(crate) fn step(&mut self, eng: &Engine) -> bool {
        match &self.phase {
            Phase::Live(_) => match self.live() {
                Next::Stay => {
                    self.schedule();
                    true
                }
                Next::Drop(reason) => self.dropped(reason),
                Next::Refused(reason) => {
                    if self.renewed_after_refusal {
                        return self.dropped(reason);
                    }
                    self.renewed_after_refusal = true;
                    self.renew()
                }
                Next::ClosedByGame => self.closed_by_game(),
            },
            Phase::Waiting(at) => {
                let at = *at;
                if !self.wait_commands() {
                    return self.closed_by_game();
                }
                if now() < at {
                    wake_at(at);
                    return true;
                }
                let stale = self
                    .client
                    .session()
                    .is_some_and(|session| session.stale(unix_now(), RENEW_MARGIN));
                if stale { self.renew() } else { self.dial(eng) }
            }
            Phase::Renewing(answer) => {
                let outcome = match answer.try_recv() {
                    Ok(outcome) => Some(outcome),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        Some(Err(String::from("the renewal ended without an answer")))
                    }
                };
                if !self.wait_commands() {
                    return self.closed_by_game();
                }
                match outcome {
                    None => true,
                    Some(Ok(())) => self.dial(eng),
                    Some(Err(reason)) => self.dropped(reason),
                }
            }
        }
    }

    /// Trade the refresh token for a fresh session before the next dial.
    fn renew(&mut self) -> bool {
        let (done, answer) = channel();
        spawn_renew(&self.client, done);
        self.phase = Phase::Renewing(answer);
        true
    }

    fn dial(&mut self, eng: &Engine) -> bool {
        let url = match socket_url(&self.client) {
            Ok((url, _)) => url,
            Err(err) => return self.dropped(err.to_string()),
        };
        let options = SocketOptions {
            compression: WebsocketConfig::from_settings(eng).compression,
            headers: Vec::new(),
        };
        self.phase = Phase::Live(Link {
            connection: Connection::connect(eng, &url, options),
            protocol: Protocol::new(now()),
            opened: false,
            join_ref: None,
            joined: false,
            rejoining: Vec::new(),
        });
        true
    }

    /// Wake the loop for the next thing due on a live connection: the
    /// heartbeat, or a crashed channel's rejoin.
    fn schedule(&self) {
        let Phase::Live(link) = &self.phase else {
            return;
        };
        wake_at(link.protocol.heartbeat_due());
        if let Some(at) = self.rejoins.next_due() {
            wake_at(at);
        }
    }

    /// The connection is gone. A socket that was open backs off and tries
    /// again, telling the game; one that never opened, or ran out of
    /// tries, reports an error and ends.
    fn dropped(&mut self, reason: String) -> bool {
        self.fail_pending();
        // A reconnect joins every topic again anyway.
        self.rejoins = Rejoins::default();
        self.rejoining.clear();
        if let Phase::Live(link) = &mut self.phase {
            link.connection.close();
        }
        let reason = if self.opened_once {
            self.tries += 1;
            if self.tries <= crate::RECONNECT_TRIES {
                let wait = crate::backoff(self.tries);
                let _ = self.events.send(GamendEvent::SocketReconnecting {
                    socket: self.socket,
                    attempt: self.tries,
                    reason,
                    wait,
                });
                let at = now() + wait;
                self.phase = Phase::Waiting(at);
                wake_at(at);
                return true;
            }
            format!("gave up after {} tries: {reason}", crate::RECONNECT_TRIES)
        } else {
            reason
        };
        let _ = self.events.send(GamendEvent::SocketError {
            socket: self.socket,
            reason,
        });
        false
    }

    fn closed_by_game(&mut self) -> bool {
        if let Phase::Live(link) = &mut self.phase {
            link.connection.close();
        }
        self.fail_pending();
        let _ = self.events.send(GamendEvent::SocketClosed {
            socket: self.socket,
            reason: "closed by the game".into(),
        });
        false
    }

    /// Commands that arrive with no connection to carry them. A call fails
    /// at once rather than wait on a connection that may not come back; a
    /// leave drops its topic from what is joined again. False when the game
    /// closed the socket.
    fn wait_commands(&mut self) -> bool {
        loop {
            let command = match self.commands.try_recv() {
                Ok(command) => command,
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            };
            match command {
                SocketCommand::Close => return false,
                SocketCommand::Interrupt => {}
                SocketCommand::Leave { request, topic } => {
                    self.topics.retain(|(joined, _)| *joined != topic);
                    let _ = self.events.send(GamendEvent::Replied {
                        request,
                        status: "ok".into(),
                        response: Json::Null,
                    });
                }
                SocketCommand::Join { request, .. }
                | SocketCommand::Push { request, .. }
                | SocketCommand::CallHook { request, .. } => {
                    let _ = self.events.send(GamendEvent::Failed {
                        request,
                        message: "the socket is reconnecting".into(),
                    });
                }
            }
        }
    }

    /// Frames off the connection, decoded; `Err` when it ended.
    fn arrivals(link: &mut Link) -> (Vec<SocketEvent>, Option<Next>) {
        let mut decoded = Vec::new();
        for arrival in link.connection.receive() {
            match arrival {
                Arrival::Opened => link.opened = true,
                Arrival::Text(text) => match link.protocol.decode(&text) {
                    Ok(Some(event)) => decoded.push(event),
                    Ok(None) => {}
                    Err(err) => return (decoded, Some(Next::Drop(err.to_string()))),
                },
                Arrival::Binary(_) => tracing::warn!("binary frame on a JSON connection; dropped"),
                Arrival::Failed {
                    reason,
                    status: Some(401 | 403),
                } if !link.opened => return (decoded, Some(Next::Refused(reason))),
                Arrival::Closed { reason, .. } | Arrival::Failed { reason, .. } => {
                    let reason = if reason.is_empty() {
                        String::from("closed")
                    } else {
                        reason
                    };
                    return (decoded, Some(Next::Drop(reason)));
                }
            }
        }
        (decoded, None)
    }

    /// One tick of an open connection.
    fn live(&mut self) -> Next {
        let Phase::Live(link) = &mut self.phase else {
            return Next::Stay;
        };
        let (decoded, ended) = Self::arrivals(link);
        if link.opened && link.join_ref.is_none() {
            let (reference, frame) = link.protocol.join(&self.user_topic, &json!({}));
            if let Err(reason) = link.send(&frame) {
                return Next::Drop(reason);
            }
            link.join_ref = Some(reference);
        }
        for event in decoded {
            if let Next::Drop(reason) = self.deliver(event) {
                return Next::Drop(reason);
            }
        }
        if let Some(ended) = ended {
            return ended;
        }
        let Phase::Live(link) = &mut self.phase else {
            return Next::Stay;
        };
        if !link.joined {
            return Next::Stay;
        }
        loop {
            let command = match self.commands.try_recv() {
                Ok(command) => command,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Next::ClosedByGame,
            };
            match self.run(command) {
                Ok(true) => {}
                Ok(false) => return Next::ClosedByGame,
                Err(err) => return Next::Drop(err.to_string()),
            }
        }
        let Phase::Live(link) = &mut self.phase else {
            return Next::Stay;
        };
        for rejoin in self.rejoins.take_due(now()) {
            let (reference, frame) = link.protocol.join(&rejoin.topic, &rejoin.payload);
            if let Err(reason) = link.send(&frame) {
                return Next::Drop(reason);
            }
            self.rejoining.push((reference, rejoin));
        }
        match link.protocol.heartbeat(now()) {
            Ok(Some(frame)) => match link.send(&frame) {
                Ok(()) => Next::Stay,
                Err(reason) => Next::Drop(reason),
            },
            Ok(None) => Next::Stay,
            Err(err) => Next::Drop(err.to_string()),
        }
    }

    fn deliver(&mut self, event: SocketEvent) -> Next {
        let Phase::Live(link) = &mut self.phase else {
            return Next::Stay;
        };
        match event {
            SocketEvent::Reply {
                reference,
                status,
                response,
                ..
            } => {
                if !link.joined && link.join_ref.as_deref() == Some(reference.as_str()) {
                    if status != "ok" {
                        return Next::Drop(format!(
                            "joining the user channel was refused: {status}"
                        ));
                    }
                    link.joined = true;
                    self.renewed_after_refusal = false;
                    if self.opened_once {
                        self.rejoin();
                    } else {
                        self.opened_once = true;
                        let _ = self.events.send(GamendEvent::SocketOpen {
                            socket: self.socket,
                        });
                    }
                    return Next::Stay;
                }
                if let Some(at) = link.rejoining.iter().position(|(r, _)| *r == reference) {
                    let (_, topic) = link.rejoining.remove(at);
                    if status != "ok" {
                        self.topics.retain(|(joined, _)| *joined != topic);
                        self.lost.push(topic);
                    }
                    if link.rejoining.is_empty() {
                        self.reopened();
                    }
                    return Next::Stay;
                }
                if let Some(at) = self.rejoining.iter().position(|(r, _)| *r == reference) {
                    let (_, rejoin) = self.rejoining.remove(at);
                    self.rejoins
                        .answered(rejoin, status == "ok", &mut self.topics, now());
                    return Next::Stay;
                }
                if let Some(at) = self.joining.iter().position(|(r, _)| *r == reference) {
                    let (_, topic) = self.joining.remove(at);
                    if status != "ok" {
                        self.topics.retain(|(joined, _)| *joined != topic);
                    }
                }
                if let Some(at) = self.pending.iter().position(|(r, _)| *r == reference) {
                    let (_, request) = self.pending.remove(at);
                    let _ = self.events.send(GamendEvent::Replied {
                        request,
                        status,
                        response,
                    });
                }
                Next::Stay
            }
            SocketEvent::Message {
                topic,
                event,
                payload,
            } => {
                self.rejoins
                    .heard(&event, &topic, &self.user_topic, &mut self.topics, now());
                let _ = self.events.send(GamendEvent::SocketMessage {
                    socket: self.socket,
                    topic,
                    event,
                    payload,
                });
                Next::Stay
            }
            SocketEvent::Closed { reason } => Next::Drop(reason),
        }
    }

    /// Join again every topic the game had, after a reconnect.
    fn rejoin(&mut self) {
        let Phase::Live(link) = &mut self.phase else {
            return;
        };
        for (topic, payload) in &self.topics {
            let (reference, frame) = link.protocol.join(topic, payload);
            if link.send(&frame).is_ok() {
                link.rejoining.push((reference, topic.clone()));
            }
        }
        if link.rejoining.is_empty() {
            self.reopened();
        }
    }

    fn reopened(&mut self) {
        self.tries = 0;
        let _ = self.events.send(GamendEvent::SocketReopened {
            socket: self.socket,
            lost: std::mem::take(&mut self.lost),
        });
    }

    /// Apply one command. `Ok(false)` means the game asked to close.
    fn run(&mut self, command: SocketCommand) -> Result<bool> {
        let Phase::Live(link) = &mut self.phase else {
            return Ok(true);
        };
        let (reference, frame, request) = match command {
            SocketCommand::Join {
                request,
                topic,
                payload,
            } => {
                let (reference, frame) = link.protocol.join(&topic, &payload);
                self.joining.push((reference.clone(), topic.clone()));
                self.topics.retain(|(joined, _)| *joined != topic);
                self.topics.push((topic, payload));
                (reference, frame, request)
            }
            SocketCommand::Push {
                request,
                topic,
                event,
                payload,
            } => {
                let (reference, frame) = link.protocol.push(&topic, &event, &payload)?;
                (reference, frame, request)
            }
            SocketCommand::Leave { request, topic } => {
                self.topics.retain(|(joined, _)| *joined != topic);
                let (reference, frame) = link.protocol.leave(&topic)?;
                (reference, frame, request)
            }
            SocketCommand::CallHook {
                request,
                plugin,
                function,
                args,
            } => {
                let payload = json!({ "plugin": plugin, "fn": function, "args": args });
                let (reference, frame) =
                    link.protocol
                        .push(&self.user_topic, "call_hook", &payload)?;
                (reference, frame, request)
            }
            SocketCommand::Close => return Ok(false),
            SocketCommand::Interrupt => return Err(anyhow!("interrupted by the game")),
        };
        link.send(&frame).map_err(anyhow::Error::msg)?;
        self.pending.push((reference, request));
        Ok(true)
    }

    /// A reply that will never come is an error the caller must see, not a
    /// task suspended forever.
    fn fail_pending(&mut self) {
        self.joining.clear();
        for (_, request) in self.pending.drain(..) {
            let _ = self.events.send(GamendEvent::Failed {
                request,
                message: "the connection ended before the reply".into(),
            });
        }
    }
}

/// Step every socket once, then send what they queued. A replay or a
/// re-simulated tick reaches no server, so it steps nothing.
pub(crate) fn pump(eng: &Engine, live: &mut Vec<LiveSocket>) {
    if balaur_core::replay::suppressed(eng) {
        return;
    }
    live.retain_mut(|socket| socket.step(eng));
    balaur_websocket::connection::flush();
}

// The native half only: the tests sign in over the blocking REST client.
#[cfg(test)]
#[cfg(not(target_family = "wasm"))]
mod tests {
    use super::*;
    use crate::client::{Credentials, Session, auth};

    #[test]
    fn a_socket_needs_a_session_to_connect_with() {
        let client = SharedClient::new("http://127.0.0.1:1");
        let (_commands, receiver) = channel();
        let (events, _heard) = channel();
        let made = LiveSocket::new(&client, 1, receiver, &events);
        assert!(made.is_err_and(|err| err.to_string().contains("logged-in session")));
    }

    /// A device account on `server`, signed in, waiting out the server's limit
    /// on sign-ins; `None` without a server to test against.
    #[allow(clippy::disallowed_methods, reason = "names a throwaway test account")]
    fn signed_in(server: &str) -> (SharedClient, Session) {
        let client = SharedClient::new(server);
        let device = format!(
            "balaur-realtime-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let credentials = Credentials::Device { device_id: device };
        for _ in 0..6 {
            let outcome = auth::login(&mut client.lock(), &credentials);
            match outcome {
                Err(err) if err.to_string().contains("(429)") => {
                    std::thread::sleep(Duration::from_secs(15));
                }
                outcome => {
                    let session = outcome.unwrap();
                    client.set_session(Some(session.clone()));
                    return (client, session);
                }
            }
        }
        panic!("the server kept refusing sign-ins");
    }

    /// The same token with its signature broken: its claims still say it is
    /// fresh, so only the server can tell.
    fn forged(token: &str) -> String {
        let (claims, _) = token.rsplit_once('.').unwrap();
        format!("{claims}.bm90LXRoZS1zaWduYXR1cmU")
    }

    /// Step the socket until it opens or ends, or twenty seconds pass.
    #[allow(clippy::disallowed_methods, reason = "a test's deadline")]
    fn settle(socket: &mut LiveSocket, heard: &Receiver<GamendEvent>) -> GamendEvent {
        let app = balaur_core::App::new(balaur_core::AppConfig::bare(".")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            let alive = socket.step(&app.engine);
            balaur_websocket::connection::flush();
            while let Ok(event) = heard.try_recv() {
                if matches!(
                    event,
                    GamendEvent::SocketOpen { .. } | GamendEvent::SocketError { .. }
                ) {
                    return event;
                }
            }
            assert!(alive, "the socket ended without saying why");
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the socket neither opened nor failed");
    }

    fn delete(client: &SharedClient, session: Session) {
        client.set_session(Some(session));
        let gone = client.lock().call("DELETE", "/api/v1/me", None).unwrap();
        assert_eq!(gone.status, 200, "{}", gone.body);
    }

    #[test]
    fn a_refused_token_is_renewed_once_and_the_socket_opens() {
        let Some(server) = balaur_testkit::gamend_url() else {
            return;
        };
        let (client, session) = signed_in(&server);
        let mut spoiled = session.clone();
        spoiled.access_token = forged(&session.access_token);
        assert!(
            !spoiled.stale(unix_now(), RENEW_MARGIN),
            "the forged token looks fresh, so only the server's refusal renews it"
        );
        client.set_session(Some(spoiled.clone()));
        let (_commands, receiver) = channel();
        let (events, heard) = channel();
        let mut socket = LiveSocket::new(&client, 1, receiver, &events).unwrap();
        let event = settle(&mut socket, &heard);
        assert!(
            matches!(event, GamendEvent::SocketOpen { .. }),
            "the socket opened"
        );
        let renewed = client.session().unwrap();
        assert_ne!(
            renewed.access_token, spoiled.access_token,
            "on a renewed token"
        );
        delete(&client, renewed);
    }

    #[test]
    fn a_refused_token_that_cannot_be_renewed_is_an_error() {
        let Some(server) = balaur_testkit::gamend_url() else {
            return;
        };
        let (client, session) = signed_in(&server);
        let mut spoiled = session.clone();
        spoiled.access_token = forged(&session.access_token);
        spoiled.refresh_token = String::from("not-a-refresh-token");
        client.set_session(Some(spoiled));
        let (_commands, receiver) = channel();
        let (events, heard) = channel();
        let mut socket = LiveSocket::new(&client, 1, receiver, &events).unwrap();
        let event = settle(&mut socket, &heard);
        assert!(
            matches!(event, GamendEvent::SocketError { .. }),
            "a socket that never opened reports an error rather than retrying"
        );
        delete(&client, session);
    }
}
