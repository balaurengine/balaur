//! The browser backend: Fetch for REST and the WebSocket API for realtime,
//! both through web-sys.
//!
//! No threads. A REST call is a promise that settles between frames and
//! feeds the same channel the native workers feed. A socket's callbacks only
//! queue what arrived; [`pump`] hands the queue to the [`Protocol`] once per
//! tick, so every arrival still lands on a tick boundary and in the
//! recording, exactly as on native.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use anyhow::{Result, anyhow};
use serde_json::{Value as Json, json};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    AbortSignal, BinaryType, CloseEvent, ErrorEvent, Headers, MessageEvent, Request, RequestInit,
    Response, WebSocket,
};

use crate::channels::{Rejoin, Rejoins};
use crate::client::auth::{Credentials, login_request, refresh_request, session_of};
use crate::client::rest::{Prepared, Reply, reply_of};
use crate::client::{Client, Protocol, SocketEvent};
use crate::{GamendEvent, LoginCredentials, SocketCommand};

/// The whole request, as the native client's agent is configured.
const TIMEOUT_SECONDS: f64 = 10.0;

/// One client shared by every in-flight call: `RefCell` rather than a
/// mutex, because the page has one thread.
#[derive(Clone)]
pub(crate) struct SharedClient(Rc<RefCell<Client>>);

impl SharedClient {
    pub(crate) fn new(base_url: &str) -> Self {
        Self(Rc::new(RefCell::new(Client::new(base_url))))
    }

    pub(crate) fn session(&self) -> Option<crate::client::Session> {
        self.0.borrow().session().cloned()
    }

    pub(crate) fn set_session(&self, session: Option<crate::client::Session>) {
        self.0.borrow_mut().set_session(session);
    }
}

async fn send(prepared: Prepared) -> Result<Reply, String> {
    let init = RequestInit::new();
    init.set_method(&prepared.method);
    init.set_signal(Some(&AbortSignal::timeout_with_f64(
        TIMEOUT_SECONDS * 1000.0,
    )));
    let headers = Headers::new().map_err(describe)?;
    if let Some(bearer) = &prepared.bearer {
        headers.append("authorization", bearer).map_err(describe)?;
    }
    headers
        .append(crate::client::RUN_HEADER, crate::client::run_id())
        .map_err(describe)?;
    let deleting_with_body = prepared.method == "DELETE" && prepared.body.is_some();
    if matches!(prepared.method.as_str(), "POST" | "PUT" | "PATCH") || deleting_with_body {
        headers
            .append("content-type", "application/json")
            .map_err(describe)?;
        init.set_body(&JsValue::from_str(prepared.body.as_deref().unwrap_or("{}")));
    }
    init.set_headers(&headers);
    let request = Request::new_with_str_and_init(&prepared.url, &init).map_err(describe)?;
    let window = web_sys::window().ok_or_else(|| String::from("no window to fetch from"))?;
    let response: Response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(describe)?
        .dyn_into()
        .map_err(|_| String::from("fetch resolved to something that is not a Response"))?;
    let status = response.status();
    let text = JsFuture::from(response.text().map_err(describe)?)
        .await
        .map_err(describe)?
        .as_string()
        .unwrap_or_default();
    Ok(reply_of(status, &text))
}

/// One authenticated call, refreshing once on a 401 as the native client
/// does, so an expired access token heals invisibly here too. A refused
/// refresh answers the 401 itself.
async fn call(
    client: &SharedClient,
    method: &str,
    path: &str,
    body: Option<&Json>,
) -> Result<Reply, String> {
    let prepared = client.0.borrow().prepare(method, path, body, true);
    let reply = send(prepared).await?;
    let token = client.0.borrow().refresh_token();
    if reply.status != 401 || token.is_empty() || renew(client).await.is_err() {
        return Ok(reply);
    }
    let prepared = client.0.borrow().prepare(method, path, body, true);
    send(prepared).await
}

/// Trade the refresh token for a new session, kept for the calls after.
async fn renew(client: &SharedClient) -> Result<(), String> {
    let token = client.0.borrow().refresh_token();
    let (refresh_path, refresh_body) = refresh_request(&token);
    let prepared = client
        .0
        .borrow()
        .prepare("POST", refresh_path, Some(&refresh_body), false);
    let refreshed = send(prepared).await?;
    let session =
        session_of(&refreshed.body, refreshed.status, "refresh").map_err(|err| err.to_string())?;
    client.0.borrow_mut().set_session(Some(session));
    Ok(())
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
    spawn_local(async move {
        let (path, body) = login_request(&credentials);
        let prepared = client.0.borrow().prepare("POST", path, Some(&body), false);
        let outcome = send(prepared)
            .await
            .map_err(anyhow::Error::msg)
            .and_then(|reply| session_of(&reply.body, reply.status, "login"))
            .map_err(|err| err.to_string());
        if let Ok(session) = &outcome {
            client.0.borrow_mut().set_session(Some(session.clone()));
        }
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
    spawn_local(async move {
        let outcome = call(&client, &method, &path, body.as_ref()).await;
        let _ = events.send(GamendEvent::rest_done(request, outcome));
    });
}

/// What a socket callback queued for the pump to read.
enum Arrival {
    Text(String),
    Closed(String),
    Failed(String),
}

/// One WebSocket and what its callbacks queued.
struct Link {
    ws: WebSocket,
    protocol: Protocol,
    arrivals: Rc<RefCell<VecDeque<Arrival>>>,
    opened: Rc<Cell<bool>>,
    /// The own-user join's ref, sent on open; the connection reports `open`
    /// only once its reply says ok, so `open` implies "ready for call_hook".
    join_ref: Option<String>,
    joined: bool,
    /// After a reconnect, ref → the topic joined again, until its reply.
    rejoining: Vec<(String, String)>,
    /// The callbacks stay owned here: dropping one unregisters it.
    _callbacks: Vec<Closure<dyn FnMut(JsValue)>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

/// The browser still fires a closed socket's `onclose` after this is
/// dropped, and a dropped closure throws when called: detach them first.
impl Drop for Link {
    fn drop(&mut self) {
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onclose(None);
        self.ws.set_onerror(None);
        let _ = self.ws.close();
    }
}

/// Where a socket is between one connection and the next.
enum Phase {
    Live(Link),
    /// Sitting out a back-off until this page time, in seconds.
    Waiting(f64),
    /// Renewing a stale token; the promise settles into the cell.
    Renewing(Rc<RefCell<Option<Result<(), String>>>>),
}

/// What the rest of a tick does after a live step.
enum Next {
    Stay,
    Drop(String),
    ClosedByGame,
}

/// One socket the engine still holds, stepped once per tick. It outlives a
/// connection: a drop reconnects, backing off, and joins again every topic
/// the game had joined.
struct LiveSocket {
    socket: u64,
    client: SharedClient,
    commands: Receiver<SocketCommand>,
    events: Sender<GamendEvent>,
    user_topic: String,
    phase: Phase,
    /// A connection that never opened is an error, not a drop.
    opened_once: bool,
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

thread_local! {
    static LIVE: RefCell<Vec<LiveSocket>> = const { RefCell::new(Vec::new()) };
}

fn socket_url(client: &SharedClient) -> Result<(String, String)> {
    let client = client.0.borrow();
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

/// Seconds, from the page's clock: the heartbeat's rhythm, never the tick's.
fn now() -> f64 {
    js_sys::Date::now() / 1000.0
}

/// Seconds before its expiry that a token counts as stale.
const RENEW_MARGIN: i64 = 30;

/// Whether the session's token is past its expiry, or near it. The server
/// reads the token only as a socket opens, and a browser hides why a
/// handshake failed, so a stale one is renewed first.
fn stale(client: &SharedClient) -> bool {
    #[allow(clippy::cast_possible_truncation, reason = "seconds since 1970")]
    let now = now() as i64;
    client
        .0
        .borrow()
        .session()
        .is_some_and(|session| session.stale(now, RENEW_MARGIN))
}

pub(crate) fn spawn_socket(
    client: &SharedClient,
    socket: u64,
    commands: Receiver<SocketCommand>,
    events: &Sender<GamendEvent>,
) {
    let user_topic = match socket_url(client) {
        Ok((_, topic)) => topic,
        Err(err) => {
            let _ = events.send(GamendEvent::SocketError {
                socket,
                reason: err.to_string(),
            });
            return;
        }
    };
    // Due at once: the first step renews a stale token and dials.
    LIVE.with(|live| {
        live.borrow_mut().push(LiveSocket {
            socket,
            client: client.clone(),
            commands,
            events: events.clone(),
            user_topic,
            phase: Phase::Waiting(0.0),
            opened_once: false,
            tries: 0,
            pending: Vec::new(),
            joining: Vec::new(),
            topics: Vec::new(),
            lost: Vec::new(),
            rejoins: Rejoins::default(),
            rejoining: Vec::new(),
        });
    });
}

impl Link {
    fn open(url: &str) -> Result<Self, String> {
        let ws = WebSocket::new(url).map_err(describe)?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        let arrivals: Rc<RefCell<VecDeque<Arrival>>> = Rc::new(RefCell::new(VecDeque::new()));
        let opened = Rc::new(Cell::new(false));

        let on_open = {
            let opened = Rc::clone(&opened);
            Closure::wrap(Box::new(move |_: JsValue| opened.set(true)) as Box<dyn FnMut(JsValue)>)
        };
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        let on_message = {
            let arrivals = Rc::clone(&arrivals);
            Closure::wrap(Box::new(move |event: MessageEvent| {
                if let Some(text) = event.data().as_string() {
                    arrivals.borrow_mut().push_back(Arrival::Text(text));
                } else {
                    tracing::warn!("binary frame on a JSON connection; dropped");
                }
            }) as Box<dyn FnMut(MessageEvent)>)
        };
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let on_close = {
            let arrivals = Rc::clone(&arrivals);
            Closure::wrap(Box::new(move |event: JsValue| {
                let reason = event
                    .dyn_ref::<CloseEvent>()
                    .map(CloseEvent::reason)
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| String::from("closed"));
                arrivals.borrow_mut().push_back(Arrival::Closed(reason));
            }) as Box<dyn FnMut(JsValue)>)
        };
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let on_error = {
            let arrivals = Rc::clone(&arrivals);
            Closure::wrap(Box::new(move |event: JsValue| {
                // The browser deliberately withholds why a socket failed.
                let reason = event
                    .dyn_ref::<ErrorEvent>()
                    .map(ErrorEvent::message)
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| String::from("the connection failed"));
                arrivals.borrow_mut().push_back(Arrival::Failed(reason));
            }) as Box<dyn FnMut(JsValue)>)
        };
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        Ok(Self {
            ws,
            protocol: Protocol::new(now()),
            arrivals,
            opened,
            join_ref: None,
            joined: false,
            rejoining: Vec::new(),
            _callbacks: vec![on_open, on_close, on_error],
            _on_message: on_message,
        })
    }

    fn send(&self, frame: &str) -> Result<(), String> {
        self.ws
            .send_with_str(frame)
            .map_err(|err| format!("websocket send: {}", describe(err)))
    }
}

/// Step every live connection once; called once per tick.
pub(crate) fn pump() {
    LIVE.with(|live| live.borrow_mut().retain_mut(LiveSocket::step));
}

impl LiveSocket {
    /// One tick of the socket. Answers whether it is still alive.
    fn step(&mut self) -> bool {
        match &self.phase {
            Phase::Live(_) => match self.live() {
                Next::Stay => true,
                Next::Drop(reason) => self.dropped(reason),
                Next::ClosedByGame => self.closed_by_game(),
            },
            Phase::Waiting(at) => {
                let at = *at;
                if !self.wait_commands() {
                    return self.closed_by_game();
                }
                if now() < at {
                    return true;
                }
                if !stale(&self.client) {
                    return self.dial();
                }
                let settled = Rc::new(RefCell::new(None));
                let into = Rc::clone(&settled);
                let client = self.client.clone();
                spawn_local(async move {
                    *into.borrow_mut() = Some(renew(&client).await);
                });
                self.phase = Phase::Renewing(settled);
                true
            }
            Phase::Renewing(settled) => {
                let outcome = settled.borrow_mut().take();
                if !self.wait_commands() {
                    return self.closed_by_game();
                }
                match outcome {
                    None => true,
                    Some(Ok(())) => self.dial(),
                    Some(Err(reason)) => self.dropped(reason),
                }
            }
        }
    }

    fn dial(&mut self) -> bool {
        let link = socket_url(&self.client)
            .map_err(|err| err.to_string())
            .and_then(|(url, _)| Link::open(&url));
        match link {
            Ok(link) => {
                self.phase = Phase::Live(link);
                true
            }
            Err(reason) => self.dropped(reason),
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
        if let Phase::Live(link) = &self.phase {
            let _ = link.ws.close();
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
                self.phase = Phase::Waiting(now() + wait);
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
        if let Phase::Live(link) = &self.phase {
            let _ = link.ws.close_with_code_and_reason(1000, "bye");
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

    /// One tick of an open connection.
    fn live(&mut self) -> Next {
        let Phase::Live(link) = &mut self.phase else {
            return Next::Stay;
        };
        if link.opened.get() && link.join_ref.is_none() {
            let (reference, frame) = link.protocol.join(&self.user_topic, &json!({}));
            if let Err(reason) = link.send(&frame) {
                return Next::Drop(reason);
            }
            link.join_ref = Some(reference);
        }
        // Decoded before any is delivered: delivering needs the whole socket.
        let arrivals: Vec<Arrival> = link.arrivals.borrow_mut().drain(..).collect();
        let mut decoded = Vec::new();
        let mut ended = None;
        for arrival in arrivals {
            match arrival {
                Arrival::Text(text) => match link.protocol.decode(&text) {
                    Ok(Some(event)) => decoded.push(event),
                    Ok(None) => {}
                    Err(err) => {
                        ended = Some(err.to_string());
                        break;
                    }
                },
                Arrival::Closed(reason) | Arrival::Failed(reason) => {
                    ended = Some(reason);
                    break;
                }
            }
        }
        for event in decoded {
            if let Next::Drop(reason) = self.deliver(event) {
                return Next::Drop(reason);
            }
        }
        if let Some(reason) = ended {
            return Next::Drop(reason);
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

/// A thrown JS value is not always an `Error`; say something either way.
#[allow(
    clippy::needless_pass_by_value,
    reason = "the argument of `map_err`, which hands the error over"
)]
fn describe(error: JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| String::from("the request failed"))
}
