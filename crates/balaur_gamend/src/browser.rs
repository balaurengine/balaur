//! The browser backend for REST: Fetch through web-sys.
//!
//! No threads. A call is a promise that settles between frames and feeds the
//! same channel the native workers feed; the realtime socket is
//! `crate::realtime`, over `balaur_websocket`'s browser backend.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Sender;

use anyhow::Result;
use serde_json::Value as Json;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{AbortSignal, Headers, Request, RequestInit, Response};

use crate::client::Client;
use crate::client::auth::{Credentials, login_request, refresh_request, session_of};
use crate::client::rest::{Prepared, Reply, reply_of};
use crate::{GamendEvent, LoginCredentials};

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

    pub(crate) fn base_url(&self) -> String {
        self.0.borrow().base_url().to_string()
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

/// Trade the refresh token for a new session, answering on `done`.
pub(crate) fn spawn_renew(client: &SharedClient, done: Sender<Result<(), String>>) {
    let client = client.clone();
    spawn_local(async move {
        let _ = done.send(renew(&client).await);
    });
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
