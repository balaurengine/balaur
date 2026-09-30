//! The worker threads driving [`crate::client`]'s REST half.
//!
//! A login, a call or a token renewal gets a short-lived thread each; the
//! realtime socket is `crate::realtime`, on the engine thread. The shared
//! client holds the session, so a login on one thread authenticates every
//! call after it.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use crate::client::{Client, Credentials, auth};
use serde_json::Value as Json;

use crate::{GamendEvent, LoginCredentials};

/// One client, shared by every worker: `Mutex` because REST threads and the
/// socket thread all borrow the session.
#[derive(Clone)]
pub(crate) struct SharedClient(Arc<Mutex<Client>>);

impl SharedClient {
    pub(crate) fn new(base_url: &str) -> Self {
        Self(Arc::new(Mutex::new(Client::new(base_url))))
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Client> {
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

    pub(crate) fn base_url(&self) -> String {
        self.lock().base_url().to_string()
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
        balaur_core::replay::report(&events, GamendEvent::logged_in(request, outcome));
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
        balaur_core::replay::report(&events, GamendEvent::rest_done(request, outcome));
    });
}

/// Trade the refresh token for a new session, answering on `done`.
pub(crate) fn spawn_renew(client: &SharedClient, done: Sender<Result<(), String>>) {
    let client = client.clone();
    std::thread::spawn(move || {
        let outcome = client.lock().renew().map_err(|err| err.to_string());
        balaur_core::replay::report(&done, outcome);
    });
}
