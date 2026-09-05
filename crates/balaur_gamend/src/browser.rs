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

use anyhow::{anyhow, Result};
use serde_json::{json, Value as Json};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    AbortSignal, BinaryType, CloseEvent, ErrorEvent, Headers, MessageEvent, Request, RequestInit,
    Response, WebSocket,
};

use crate::client::auth::{login_request, refresh_request, session_of, Credentials};
use crate::client::rest::{reply_of, Prepared, Reply};
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
    if matches!(prepared.method.as_str(), "POST" | "PUT" | "PATCH") {
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
/// does, so an expired access token heals invisibly here too.
async fn call(
    client: &SharedClient,
    method: &str,
    path: &str,
    body: Option<&Json>,
) -> Result<Reply, String> {
    let prepared = client.0.borrow().prepare(method, path, body, true);
    let reply = send(prepared).await?;
    let token = client.0.borrow().refresh_token();
    if reply.status != 401 || token.is_empty() {
        return Ok(reply);
    }
    let (refresh_path, refresh_body) = refresh_request(&token);
    let prepared = client
        .0
        .borrow()
        .prepare("POST", refresh_path, Some(&refresh_body), false);
    let refreshed = send(prepared).await?;
    let session =
        session_of(&refreshed.body, refreshed.status, "refresh").map_err(|err| err.to_string())?;
    client.0.borrow_mut().set_session(Some(session));
    let prepared = client.0.borrow().prepare(method, path, body, true);
    send(prepared).await
}

pub(crate) fn spawn_login(
    client: &SharedClient,
    request: u64,
    credentials: LoginCredentials,
    events: &Sender<GamendEvent>,
) {
    let client = client.clone();
    let events = events.clone();
    let credentials = match credentials {
        LoginCredentials::EmailPassword { email, password } => {
            Credentials::EmailPassword { email, password }
        }
        LoginCredentials::Device { device_id } => Credentials::Device { device_id },
    };
    spawn_local(async move {
        let (path, body) = login_request(&credentials);
        let prepared = client.0.borrow().prepare("POST", path, Some(&body), false);
        let outcome = send(prepared)
            .await
            .map_err(anyhow::Error::msg)
            .and_then(|reply| session_of(&reply.body, reply.status, "login"));
        let event = match outcome {
            Ok(session) => {
                client.0.borrow_mut().set_session(Some(session.clone()));
                GamendEvent::LoggedIn {
                    request,
                    user_id: session.user_id,
                    username: session.username,
                    display_name: session.display_name,
                }
            }
            Err(err) => GamendEvent::Failed {
                request,
                message: err.to_string(),
            },
        };
        let _ = events.send(event);
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
        let event = match call(&client, &method, &path, body.as_ref()).await {
            Ok(reply) => GamendEvent::RestDone {
                request,
                status: reply.status,
                body: reply.body,
            },
            Err(message) => GamendEvent::Failed { request, message },
        };
        let _ = events.send(event);
    });
}

/// What a socket callback queued for the pump to read.
enum Arrival {
    Text(String),
    Closed(String),
    Failed(String),
}

/// One browser connection the engine still holds, stepped once per tick.
struct LiveSocket {
    ws: WebSocket,
    socket: u64,
    protocol: Protocol,
    arrivals: Rc<RefCell<VecDeque<Arrival>>>,
    opened: Rc<Cell<bool>>,
    commands: Receiver<SocketCommand>,
    events: Sender<GamendEvent>,
    user_topic: String,
    /// The own-user join's ref, sent on open; the connection reports `open`
    /// only once its reply says ok, so `open` implies "ready for call_hook".
    join_ref: Option<String>,
    joined: bool,
    /// ref → the request id whose reply it will carry.
    pending: Vec<(String, u64)>,
    /// The callbacks stay owned here: dropping one unregisters it.
    _callbacks: Vec<Closure<dyn FnMut(JsValue)>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
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
        "{}/socket/websocket?token={}&vsn=2.0.0",
        client.base_url().replacen("http", "ws", 1),
        session.access_token
    );
    Ok((url, format!("user:{}", session.user_id)))
}

/// Seconds, from the page's clock: the heartbeat's rhythm, never the tick's.
fn now() -> f64 {
    js_sys::Date::now() / 1000.0
}

pub(crate) fn spawn_socket(
    client: &SharedClient,
    socket: u64,
    commands: Receiver<SocketCommand>,
    events: &Sender<GamendEvent>,
) {
    let (url, user_topic) = match socket_url(client) {
        Ok(parts) => parts,
        Err(err) => {
            let _ = events.send(GamendEvent::SocketError {
                socket,
                reason: err.to_string(),
            });
            return;
        }
    };
    let ws = match WebSocket::new(&url) {
        Ok(ws) => ws,
        Err(error) => {
            let _ = events.send(GamendEvent::SocketError {
                socket,
                reason: describe(error),
            });
            return;
        }
    };
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
                .map(|e| e.reason())
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
                .map(|e| e.message())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| String::from("the connection failed"));
            arrivals.borrow_mut().push_back(Arrival::Failed(reason));
        }) as Box<dyn FnMut(JsValue)>)
    };
    ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    LIVE.with(|live| {
        live.borrow_mut().push(LiveSocket {
            ws,
            socket,
            protocol: Protocol::new(now()),
            arrivals,
            opened,
            commands,
            events: events.clone(),
            user_topic,
            join_ref: None,
            joined: false,
            pending: Vec::new(),
            _callbacks: vec![on_open, on_close, on_error],
            _on_message: on_message,
        });
    });
}

/// Step every live connection once; called once per tick.
pub(crate) fn pump() {
    LIVE.with(|live| live.borrow_mut().retain_mut(LiveSocket::step));
}

impl LiveSocket {
    /// One tick of the connection. Answers whether it is still alive.
    fn step(&mut self) -> bool {
        if self.opened.get() && self.join_ref.is_none() {
            let (reference, frame) = self.protocol.join(&self.user_topic, &json!({}));
            if self.ws.send_with_str(&frame).is_err() {
                return self.fail("the user channel join could not be sent");
            }
            self.join_ref = Some(reference);
        }
        let arrivals: Vec<Arrival> = self.arrivals.borrow_mut().drain(..).collect();
        for arrival in arrivals {
            match arrival {
                Arrival::Text(text) => match self.protocol.decode(&text) {
                    Ok(Some(event)) => {
                        if !self.deliver(event) {
                            return false;
                        }
                    }
                    Ok(None) => {}
                    Err(err) => return self.fail(&err.to_string()),
                },
                Arrival::Closed(reason) => return self.close(reason),
                Arrival::Failed(reason) => return self.fail(&reason),
            }
        }
        if !self.joined {
            return true;
        }
        loop {
            match self.commands.try_recv() {
                Ok(command) => match self.run(command) {
                    Ok(true) => {}
                    Ok(false) => {
                        let _ = self.ws.close_with_code_and_reason(1000, "bye");
                        return self.close("closed by the game".into());
                    }
                    Err(err) => return self.fail(&err.to_string()),
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = self.ws.close_with_code_and_reason(1000, "bye");
                    return self.close("closed by the game".into());
                }
            }
        }
        match self.protocol.heartbeat(now()) {
            Ok(Some(frame)) => {
                if self.ws.send_with_str(&frame).is_err() {
                    return self.fail("the heartbeat could not be sent");
                }
            }
            Ok(None) => {}
            Err(err) => return self.fail(&err.to_string()),
        }
        true
    }

    fn deliver(&mut self, event: SocketEvent) -> bool {
        match event {
            SocketEvent::Reply {
                reference,
                status,
                response,
                ..
            } => {
                if !self.joined && self.join_ref.as_deref() == Some(reference.as_str()) {
                    if status != "ok" {
                        return self
                            .fail(&format!("joining the user channel was refused: {status}"));
                    }
                    self.joined = true;
                    let _ = self.events.send(GamendEvent::SocketOpen {
                        socket: self.socket,
                    });
                    return true;
                }
                if let Some(at) = self.pending.iter().position(|(r, _)| *r == reference) {
                    let (_, request) = self.pending.remove(at);
                    let _ = self.events.send(GamendEvent::Replied {
                        request,
                        status,
                        response,
                    });
                }
                true
            }
            SocketEvent::Message {
                topic,
                event,
                payload,
            } => {
                let _ = self.events.send(GamendEvent::SocketMessage {
                    socket: self.socket,
                    topic,
                    event,
                    payload,
                });
                true
            }
            SocketEvent::Closed { reason } => self.close(reason),
        }
    }

    /// Apply one command. `Ok(false)` means the game asked to close.
    fn run(&mut self, command: SocketCommand) -> Result<bool> {
        let (reference, frame, request) = match command {
            SocketCommand::Join {
                request,
                topic,
                payload,
            } => {
                let (reference, frame) = self.protocol.join(&topic, &payload);
                (reference, frame, request)
            }
            SocketCommand::Push {
                request,
                topic,
                event,
                payload,
            } => {
                let (reference, frame) = self.protocol.push(&topic, &event, &payload)?;
                (reference, frame, request)
            }
            SocketCommand::Leave { request, topic } => {
                let (reference, frame) = self.protocol.leave(&topic)?;
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
                    self.protocol
                        .push(&self.user_topic, "call_hook", &payload)?;
                (reference, frame, request)
            }
            SocketCommand::Close => return Ok(false),
        };
        self.ws
            .send_with_str(&frame)
            .map_err(|err| anyhow!("websocket send: {}", describe(err)))?;
        self.pending.push((reference, request));
        Ok(true)
    }

    /// A reply that will never come is an error the caller must see, not a
    /// task suspended forever.
    fn fail_pending(&mut self) {
        for (_, request) in self.pending.drain(..) {
            let _ = self.events.send(GamendEvent::Failed {
                request,
                message: "the connection ended before the reply".into(),
            });
        }
    }

    fn fail(&mut self, reason: &str) -> bool {
        self.fail_pending();
        let _ = self.ws.close();
        let _ = self.events.send(GamendEvent::SocketError {
            socket: self.socket,
            reason: reason.to_string(),
        });
        false
    }

    fn close(&mut self, reason: String) -> bool {
        self.fail_pending();
        let _ = self.events.send(GamendEvent::SocketClosed {
            socket: self.socket,
            reason,
        });
        false
    }
}

/// A thrown JS value is not always an `Error`; say something either way.
fn describe(error: JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| String::from("the request failed"))
}
