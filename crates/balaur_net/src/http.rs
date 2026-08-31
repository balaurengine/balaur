//! The HTTP worker: one thread per request, reporting back over the channel.

use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::{HttpCall, NetEvent};

/// Everything a request needs travels in `call`, so the thread owns its work
/// outright and the frame loop never waits on it.
pub(crate) fn spawn_request(call: HttpCall, events: Sender<NetEvent>) {
    std::thread::spawn(move || {
        let request = call.id;
        let event = match perform(&call) {
            Ok((status, headers, body)) => NetEvent::HttpResponse {
                request,
                status,
                headers,
                body,
            },
            Err(err) => NetEvent::HttpError {
                request,
                message: err.to_string(),
            },
        };
        // The engine shutting down mid-flight drops the receiver; nothing to
        // report to, nothing to do.
        let _ = events.send(event);
    });
}

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

/// Status, headers and body — the three parts of a response a script sees.
type Response = (u16, Vec<(String, String)>, String);

fn perform(call: &HttpCall) -> Result<Response> {
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
    let body = response.body_mut().read_to_string()?;
    Ok((status, headers, body))
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
