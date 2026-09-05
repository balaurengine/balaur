//! The HTTP worker: one thread per request, reporting back over the channel.

use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Result, anyhow};

use crate::{HttpCall, HttpEvent};

/// Everything a request needs travels in `call`, so the thread owns its work
/// outright and the frame loop never waits on it.
pub(crate) fn spawn_request(call: HttpCall, events: Sender<HttpEvent>) {
    std::thread::spawn(move || {
        let request = call.id;
        let event = match perform(&call, &events) {
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
        let _ = events.send(event);
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

fn perform(call: &HttpCall, events: &Sender<HttpEvent>) -> Result<Response> {
    let agent = agent_for(call);
    let mut response =
        match call.method.as_str() {
            "GET" => with_headers(agent.get(&call.url), call).call()?,
            "DELETE" => with_headers(agent.delete(&call.url), call).call()?,
            "HEAD" => with_headers(agent.head(&call.url), call).call()?,
            "POST" => with_headers(agent.post(&call.url), call)
                .send(call.body.as_deref().unwrap_or(""))?,
            "PUT" => {
                with_headers(agent.put(&call.url), call).send(call.body.as_deref().unwrap_or(""))?
            }
            "PATCH" => with_headers(agent.patch(&call.url), call)
                .send(call.body.as_deref().unwrap_or(""))?,
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
        stream_to_file(call.id, &mut reader, path, total, events)?;
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

/// Copy a body to disk as it arrives, reporting every few hundred kilobytes
/// and once more at the end.
fn stream_to_file(
    request: u64,
    reader: &mut impl std::io::Read,
    path: &std::path::Path,
    total: Option<u64>,
    events: &Sender<HttpEvent>,
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
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        received += read as u64;
        if received - reported >= PROGRESS_STEP {
            reported = received;
            let _ = events.send(HttpEvent::Progress {
                request,
                received,
                total,
            });
        }
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&partial, path)?;
    let _ = events.send(HttpEvent::Progress {
        request,
        received,
        total: Some(total.unwrap_or(received)),
    });
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
