//! The HTTP worker: one thread per request, reporting back over the channel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Result, anyhow};

use crate::{HttpCall, HttpEvent};

/// Everything a request needs travels in `call`, so the thread owns its work
/// outright and the frame loop never waits on it.
pub(crate) fn spawn_request(call: HttpCall, events: Sender<HttpEvent>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let request = call.id;
        let event = match perform(&call, &events, &cancel) {
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
        balaur_core::replay::report(&events, event);
    });
}

/// How much of a download lands between two progress events.
const PROGRESS_STEP: u64 = 256 * 1024;

fn agent_for(call: &HttpCall) -> ureq::Agent {
    let timeout = Duration::from_secs_f64(call.timeout.unwrap_or(10.0).max(0.0));
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        // A 4xx or 5xx is a response the script must see, not a transfer
        // failure.
        .http_status_as_error(false)
        .build()
        .into()
}

/// Status, headers and body — the three parts of a response a script sees —
/// and where the body went instead when the call asked for a file.
type Response = (u16, Vec<(String, String)>, String, Option<String>);

fn perform(call: &HttpCall, events: &Sender<HttpEvent>, cancel: &AtomicBool) -> Result<Response> {
    let agent = agent_for(call);
    let mut response = match call.method.as_str() {
        "GET" => with_headers(agent.get(&call.url), call).call()?,
        // ureq sends a DELETE body only when forced to.
        "DELETE" => match call.body.as_deref() {
            Some(_) => send(
                with_headers(agent.delete(&call.url).force_send_body(), call),
                call,
                events,
            )?,
            None => with_headers(agent.delete(&call.url), call).call()?,
        },
        "HEAD" => with_headers(agent.head(&call.url), call).call()?,
        "POST" => send(with_headers(agent.post(&call.url), call), call, events)?,
        "PUT" => send(with_headers(agent.put(&call.url), call), call, events)?,
        "PATCH" => send(with_headers(agent.patch(&call.url), call), call, events)?,
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
                &events,
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
        &events,
        HttpEvent::Progress {
            request,
            received,
            total: Some(total.unwrap_or(received)),
        },
    );
    Ok(())
}

fn with_headers<B>(
    mut builder: ureq::RequestBuilder<B>,
    call: &HttpCall,
) -> ureq::RequestBuilder<B> {
    for (name, value) in &call.headers {
        builder = builder.header(name, value);
    }
    builder
}
