//! The worker driving [`crate::client`]'s REST half.
//!
//! One thread per client takes its logins, calls and token renewals off a
//! queue, in the order they were made, and sleeps while the queue is empty.
//! The realtime socket is `crate::realtime`, on the engine thread. The client
//! holds the session, so a login authenticates every call queued after it.

use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::client::{Client, Credentials, auth};
use serde_json::Value as Json;

use crate::{GamendEvent, LoginCredentials};

/// Work for the client's thread.
type Job = Box<dyn FnOnce(&mut Client) + Send>;

/// One client, shared by its worker and the engine thread: `Mutex` because
/// the realtime socket reads the session the worker's logins write.
#[derive(Clone)]
pub(crate) struct SharedClient {
    client: Arc<Mutex<Client>>,
    /// The worker's queue; the thread ends once every clone is dropped.
    jobs: Sender<Job>,
}

impl SharedClient {
    pub(crate) fn new(base_url: &str) -> Self {
        let client = Arc::new(Mutex::new(Client::new(base_url)));
        let (jobs, queue) = channel::<Job>();
        let theirs = Arc::clone(&client);
        std::thread::spawn(move || {
            while let Ok(job) = queue.recv() {
                job(&mut theirs.lock().unwrap_or_else(PoisonError::into_inner));
            }
        });
        Self { client, jobs }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Client> {
        self.client.lock().unwrap_or_else(PoisonError::into_inner)
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

    /// Queue `job` for the worker, behind everything queued before it.
    fn run(&self, job: impl FnOnce(&mut Client) + Send + 'static) {
        let _ = self.jobs.send(Box::new(job));
    }
}

pub(crate) fn spawn_login(
    client: &SharedClient,
    request: u64,
    credentials: LoginCredentials,
    events: &Sender<GamendEvent>,
) {
    let events = events.clone();
    let credentials = Credentials::from(credentials);
    client.run(move |client| {
        let outcome = auth::login(client, &credentials).map_err(|err| err.to_string());
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
    let events = events.clone();
    client.run(move |client| {
        let outcome = client
            .call(&method, &path, body.as_ref())
            .map_err(|err| err.to_string());
        balaur_core::replay::report(&events, GamendEvent::rest_done(request, outcome));
    });
}

/// Trade the refresh token for a new session, answering on `done`.
pub(crate) fn spawn_renew(client: &SharedClient, done: Sender<Result<(), String>>) {
    client.run(move |client| {
        let outcome = client.renew().map_err(|err| err.to_string());
        balaur_core::replay::report(&done, outcome);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clients_work_runs_on_one_thread_in_the_order_it_was_queued() {
        let client = SharedClient::new("http://127.0.0.1:1");
        let (done, heard) = channel();
        for step in 0..20 {
            let done = done.clone();
            client.run(move |_| {
                let _ = done.send((step, std::thread::current().id()));
            });
        }
        let ran: Vec<_> = (0..20).map(|_| heard.recv().unwrap()).collect();
        assert_eq!(
            ran.iter().map(|(step, _)| *step).collect::<Vec<_>>(),
            (0..20).collect::<Vec<_>>(),
            "in the order queued"
        );
        assert!(
            ran.iter().all(|(_, thread)| *thread == ran[0].1),
            "all on the client's one thread"
        );
        assert_ne!(ran[0].1, std::thread::current().id(), "and not this one");
    }

    #[test]
    #[allow(clippy::disallowed_methods, reason = "a test's deadline")]
    fn a_dropped_client_ends_its_worker() {
        let client = SharedClient::new("http://127.0.0.1:1");
        let held = Arc::clone(&client.client);
        drop(client);
        // The worker holds the other reference, so one left means it ended.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while Arc::strong_count(&held) > 1 {
            assert!(
                std::time::Instant::now() < deadline,
                "the worker outlived its client"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
