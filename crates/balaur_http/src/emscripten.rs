//! The browser HTTP backend: emscripten fetch, through the C shim in
//! `shim/emscripten_http.c`.
//!
//! No threads — browser I/O is already asynchronous on the main thread. A
//! completion lands as a C callback between frames, feeds the same channel
//! the native worker thread does, and the pump drains it at `Stage::First`.
//! The browser owns the transport, so TLS, proxies and HTTP/2 or /3
//! negotiation are its problem.
//!
//! The final web binary links with `-sFETCH`, from `.cargo/config.toml`; see
//! `build.rs`.

use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::mpsc::Sender;

use crate::{HttpCall, HttpEvent};

extern "C" {
    fn balaur_fetch(
        method: *const c_char,
        url: *const c_char,
        headers_joined: *const c_char,
        body: *const c_char,
        body_len: c_int,
        timeout_ms: c_int,
        user: *mut c_void,
        callback: extern "C" fn(*mut c_void, c_int, *const c_char, c_int, *const c_char),
    );
}

struct FetchState {
    request: u64,
    events: Sender<HttpEvent>,
    save_to: Option<std::path::PathBuf>,
}

extern "C" fn fetch_settled(
    user: *mut c_void,
    status: c_int,
    body: *const c_char,
    body_len: c_int,
    error: *const c_char,
) {
    // Reclaims the box `spawn_request` leaked; the shim calls exactly once.
    let state = unsafe { Box::from_raw(user.cast::<FetchState>()) };
    let event = if status > 0 {
        let bytes = if body.is_null() || body_len <= 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(body.cast::<u8>(), body_len as usize) }
        };
        let saved = state
            .save_to
            .as_ref()
            .filter(|_| (200..300).contains(&status))
            .and_then(|path| {
                // The whole body at once: the shim hands it over settled.
                balaur_core::files::default_backend()
                    .write(path, bytes)
                    .ok()?;
                let _ = state.events.send(HttpEvent::Progress {
                    request: state.request,
                    received: bytes.len() as u64,
                    total: Some(bytes.len() as u64),
                });
                Some(path.display().to_string())
            });
        HttpEvent::Response {
            request: state.request,
            status: status as u16,
            // Response headers are not surfaced by the shim; nothing in the
            // engine reads them yet, and the shape stays the native one.
            headers: Vec::new(),
            body: if saved.is_some() {
                String::new()
            } else {
                String::from_utf8_lossy(bytes).into_owned()
            },
            saved,
        }
    } else {
        HttpEvent::Error {
            request: state.request,
            message: text_at(error, "the request failed"),
        }
    };
    let _ = state.events.send(event);
}

fn text_at(pointer: *const c_char, fallback: &str) -> String {
    if pointer.is_null() {
        return fallback.to_string();
    }
    unsafe { std::ffi::CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn spawn_request(call: HttpCall, events: Sender<HttpEvent>) {
    let request = call.id;
    let refuse = |message: String| {
        let _ = events.send(HttpEvent::Error { request, message });
    };
    let (Ok(method), Ok(url)) = (CString::new(call.method), CString::new(call.url)) else {
        return refuse("the method or url holds a NUL byte".into());
    };
    let joined = call
        .headers
        .iter()
        .flat_map(|(k, v)| [k.as_str(), v.as_str()])
        .collect::<Vec<_>>()
        .join("\n");
    let Ok(headers) = CString::new(joined) else {
        return refuse("a header holds a NUL byte".into());
    };
    let body = call.body.unwrap_or_default();
    let timeout_ms = (call.timeout.unwrap_or(10.0).max(0.0) * 1000.0) as c_int;
    let state = Box::into_raw(Box::new(FetchState {
        request,
        events: events.clone(),
        save_to: call.save_to,
    }));
    unsafe {
        balaur_fetch(
            method.as_ptr(),
            url.as_ptr(),
            headers.as_ptr(),
            body.as_ptr().cast::<c_char>(),
            body.len() as c_int,
            timeout_ms,
            state.cast::<c_void>(),
            fetch_settled,
        );
    }
}
