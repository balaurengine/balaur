//! The HTTP workers: a pool of threads taking requests off one queue, over
//! one agent that keeps connections alive, reporting back over the channel.
//!
//! A project runs `[http] max_parallel` requests at once and the rest wait
//! their turn, first in first out. An idle worker sleeps until a request is
//! queued; the pool's threads end with the engine that owns it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::{Result, anyhow};

use crate::{HttpCall, HttpEvent};

/// One request waiting for a worker: everything it needs travels with it, so
/// the worker owns its work outright and the frame loop never waits on it.
struct Job {
    call: HttpCall,
    events: Sender<HttpEvent>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<Job>,
    workers: usize,
    idle: usize,
    closed: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    /// Signalled when a request is queued, and when the pool closes.
    queued: Condvar,
    /// One agent for every request, so a connection is reused while alive.
    agent: ureq::Agent,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The engine's requests and the threads that run them.
pub(crate) struct Pool {
    shared: Arc<Shared>,
}

impl Pool {
    pub(crate) fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            // A 4xx or 5xx is a response the script must see, not a transfer
            // failure.
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                queued: Condvar::new(),
                agent,
            }),
        }
    }

    /// Queue a request. A worker takes it at once while fewer than `parallel`
    /// are busy; otherwise it waits behind the ones queued before it.
    pub(crate) fn submit(
        &self,
        call: HttpCall,
        events: Sender<HttpEvent>,
        cancel: Arc<AtomicBool>,
        parallel: usize,
    ) {
        let spawn = {
            let mut queue = self.shared.lock();
            queue.waiting.push_back(Job {
                call,
                events,
                cancel,
            });
            let spawn = queue.idle == 0 && queue.workers < parallel.max(1);
            if spawn {
                queue.workers += 1;
            }
            spawn
        };
        if spawn {
            let shared = Arc::clone(&self.shared);
            std::thread::spawn(move || work(&shared));
        }
        self.shared.queued.notify_one();
    }
}

/// The engine is gone: what still waits is dropped, and each worker ends
/// once its current request does.
impl Drop for Pool {
    fn drop(&mut self) {
        let mut queue = self.shared.lock();
        queue.closed = true;
        queue.waiting.clear();
        drop(queue);
        self.shared.queued.notify_all();
    }
}

/// One worker: take the next request, run it, and sleep when there is none.
fn work(shared: &Shared) {
    let mut queue = shared.lock();
    loop {
        if queue.closed {
            queue.workers -= 1;
            return;
        }
        if let Some(job) = queue.waiting.pop_front() {
            drop(queue);
            run(&shared.agent, &job);
            queue = shared.lock();
            continue;
        }
        queue.idle += 1;
        queue = shared
            .queued
            .wait(queue)
            .unwrap_or_else(PoisonError::into_inner);
        queue.idle -= 1;
    }
}

fn run(agent: &ureq::Agent, job: &Job) {
    let request = job.call.id;
    // Cancelled while it waited: it never goes out, and saying so clears the
    // engine's note of it.
    let outcome = if job.cancel.load(Ordering::Relaxed) {
        Err(anyhow!("cancelled"))
    } else {
        perform(agent, &job.call, &job.events, &job.cancel)
    };
    let event = match outcome {
        Ok((status, headers, body, saved)) => HttpEvent::Response {
            request,
            status,
            headers,
            body,
            saved,
        },
        Err(err) => HttpEvent::Error {
            request,
            message: err.to_string(),
        },
    };
    // The engine shutting down mid-flight drops the receiver; nothing to
    // report to, nothing to do.
    balaur_core::replay::report(&job.events, event);
}

/// How much of a download lands between two progress events.
const PROGRESS_STEP: u64 = 256 * 1024;

/// The call's own deadline for the whole request.
fn timeout_of(call: &HttpCall) -> Result<Duration> {
    let seconds = call.timeout.unwrap_or(crate::HttpConfig::default().timeout);
    Duration::try_from_secs_f64(seconds)
        .map_err(|_| anyhow!("a timeout of {seconds} seconds is not a duration"))
}

/// A request builder with the call's headers and its deadline.
fn prepared<B>(
    builder: ureq::RequestBuilder<B>,
    call: &HttpCall,
) -> Result<ureq::RequestBuilder<B>> {
    let builder = call.headers.iter().fold(builder, |builder, (name, value)| {
        builder.header(name, value)
    });
    Ok(builder
        .config()
        .timeout_global(Some(timeout_of(call)?))
        .build())
}

/// Status, headers and body — the three parts of a response a script sees —
/// and where the body went instead when the call asked for a file.
type Response = (u16, Vec<(String, String)>, String, Option<String>);

fn perform(
    agent: &ureq::Agent,
    call: &HttpCall,
    events: &Sender<HttpEvent>,
    cancel: &AtomicBool,
) -> Result<Response> {
    let mut response = match call.method.as_str() {
        "GET" => prepared(agent.get(&call.url), call)?.call()?,
        // ureq sends a DELETE body only when forced to.
        "DELETE" => match call.body.as_deref() {
            Some(_) => send(
                prepared(agent.delete(&call.url).force_send_body(), call)?,
                call,
                events,
            )?,
            None => prepared(agent.delete(&call.url), call)?.call()?,
        },
        "HEAD" => prepared(agent.head(&call.url), call)?.call()?,
        "POST" => send(prepared(agent.post(&call.url), call)?, call, events)?,
        "PUT" => send(prepared(agent.put(&call.url), call)?, call, events)?,
        "PATCH" => send(prepared(agent.patch(&call.url), call)?, call, events)?,
        other => return Err(anyhow!("unsupported method `{other}`")),
    };
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    // A miss is handed back as text: a 404 page saved as the pack it was
    // asked for would be worse than no file.
    if let Some(path) = call
        .save_to
        .as_ref()
        .filter(|_| (200..300).contains(&status))
    {
        let total = response.body().content_length();
        let mut reader = response.body_mut().as_reader();
        stream_to_file(call.id, &mut reader, path, total, events, cancel)?;
        return Ok((
            status,
            headers,
            String::new(),
            Some(path.display().to_string()),
        ));
    }
    let body = response.body_mut().read_to_string()?;
    Ok((status, headers, body, None))
}

/// Send the call's body. One long enough to take a while goes out through a
/// reader that reports every few hundred kilobytes, with its length stated
/// so it is not sent chunked.
fn send(
    builder: ureq::RequestBuilder<ureq::typestate::WithBody>,
    call: &HttpCall,
    events: &Sender<HttpEvent>,
) -> Result<ureq::http::Response<ureq::Body>> {
    let body = call.body.as_deref().unwrap_or("");
    if (body.len() as u64) < PROGRESS_STEP {
        return Ok(builder.send(body)?);
    }
    let reader = Outgoing {
        request: call.id,
        body: body.as_bytes().to_vec(),
        at: 0,
        reported: 0,
        events: events.clone(),
    };
    let sized = builder.header("content-length", body.len().to_string());
    Ok(sized.send(ureq::SendBody::from_owned_reader(reader))?)
}

/// A body on its way out, telling the frame loop how much has gone.
struct Outgoing {
    request: u64,
    body: Vec<u8>,
    at: usize,
    reported: usize,
    events: Sender<HttpEvent>,
}

impl std::io::Read for Outgoing {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = (&self.body[self.at..]).read(buf)?;
        self.at += n;
        let done = self.at == self.body.len() && self.reported != self.at;
        if done || (self.at - self.reported) as u64 >= PROGRESS_STEP {
            self.reported = self.at;
            let _ = self.events.send(HttpEvent::Sent {
                request: self.request,
                sent: self.at as u64,
                total: self.body.len() as u64,
            });
        }
        Ok(n)
    }
}

/// Copy a body to disk as it arrives, reporting every few hundred kilobytes
/// and once more at the end.
fn stream_to_file(
    request: u64,
    reader: &mut impl std::io::Read,
    path: &std::path::Path,
    total: Option<u64>,
    events: &Sender<HttpEvent>,
    cancel: &AtomicBool,
) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Written beside the target and moved over it at the end, so a download
    // cut short never leaves half a file under the name a script trusts.
    let partial = path.with_extension("part");
    let mut file = std::fs::File::create(&partial)?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    let mut reported = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err(anyhow!("cancelled"));
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        received += read as u64;
        if received - reported >= PROGRESS_STEP {
            reported = received;
            balaur_core::replay::report(
                events,
                HttpEvent::Progress {
                    request,
                    received,
                    total,
                },
            );
        }
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&partial, path)?;
    balaur_core::replay::report(
        events,
        HttpEvent::Progress {
            request,
            received,
            total: Some(total.unwrap_or(received)),
        },
    );
    Ok(())
}
