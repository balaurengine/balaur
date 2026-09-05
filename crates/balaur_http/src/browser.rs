//! The browser HTTP backend outside emscripten: the Fetch API through web-sys.
//!
//! Same delivery contract as the emscripten backend next door, and for the
//! same reason: browser I/O is already asynchronous on the main thread, so
//! there is no worker thread. A promise settles between frames, feeds the
//! channel the native worker feeds, and the pump drains it at `Stage::First`
//! — which is what keeps a response landing on a tick boundary and inside
//! the recording.
//!
//! The browser owns the transport, so TLS, redirects and HTTP/2 or /3
//! negotiation are its problem, not this crate's.

use std::sync::mpsc::Sender;

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{AbortSignal, Headers, Request, RequestInit, Response};

use crate::{HttpCall, HttpEvent};

/// The default when a call names no timeout, matching `HttpConfig`.
const DEFAULT_TIMEOUT: f64 = 10.0;

pub(crate) fn spawn_request(call: HttpCall, events: Sender<HttpEvent>) {
    let request = call.id;
    spawn_local(async move {
        let event = match send(call, &events).await {
            Ok(event) => event,
            Err(message) => HttpEvent::Error { request, message },
        };
        let _ = events.send(event);
    });
}

/// Nothing to pump: the browser calls us, not the other way round.
pub(crate) fn pump() {}

async fn send(call: HttpCall, events: &Sender<HttpEvent>) -> Result<HttpEvent, String> {
    let init = RequestInit::new();
    init.set_method(&call.method);
    // Fetch has no timeout of its own; an abort signal is how one is spelled.
    let seconds = call.timeout.unwrap_or(DEFAULT_TIMEOUT);
    init.set_signal(Some(&AbortSignal::timeout_with_f64(seconds * 1000.0)));
    if let Some(body) = &call.body {
        init.set_body(&JsValue::from_str(body));
    }
    let headers = Headers::new().map_err(describe)?;
    for (name, value) in &call.headers {
        headers.append(name, value).map_err(describe)?;
    }
    init.set_headers(&headers);

    let request = Request::new_with_str_and_init(&call.url, &init).map_err(describe)?;
    let window = web_sys::window().ok_or_else(|| String::from("no window to fetch from"))?;
    let response: Response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(describe)?
        .dyn_into()
        .map_err(|_| String::from("fetch resolved to something that is not a Response"))?;

    let status = response.status();
    let headers = read_headers(&response.headers());
    // The body is a second promise, and a failure reading it is still a
    // failure of the request as the script asked for it.
    if let Some(path) = call
        .save_to
        .as_ref()
        .filter(|_| (200..300).contains(&status))
    {
        let buffer = JsFuture::from(response.array_buffer().map_err(describe)?)
            .await
            .map_err(describe)?;
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        // The page's filesystem is whatever backend the host installed, and
        // it takes the whole body at once: a browser has no disk to stream to.
        balaur_core::files::default_backend()
            .write(path, &bytes)
            .map_err(|err| err.to_string())?;
        let _ = events.send(HttpEvent::Progress {
            request: call.id,
            received: bytes.len() as u64,
            total: Some(bytes.len() as u64),
        });
        return Ok(HttpEvent::Response {
            request: call.id,
            status,
            headers,
            body: String::new(),
            saved: Some(path.display().to_string()),
        });
    }
    let body = JsFuture::from(response.text().map_err(describe)?)
        .await
        .map_err(describe)?
        .as_string()
        .unwrap_or_default();
    Ok(HttpEvent::Response {
        request: call.id,
        status,
        headers,
        body,
        saved: None,
    })
}

/// `Headers` is a JS iterable of `[name, value]` pairs; anything that does not
/// come back in that shape is skipped rather than failing the response.
fn read_headers(headers: &Headers) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(Some(entries)) = js_sys::try_iter(headers) else {
        return out;
    };
    for entry in entries.flatten() {
        let pair = js_sys::Array::from(&entry);
        if let (Some(name), Some(value)) = (pair.get(0).as_string(), pair.get(1).as_string()) {
            out.push((name, value));
        }
    }
    out
}

/// A thrown JS value is not always an `Error`; say something either way.
fn describe(error: JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| String::from("the request failed"))
}
