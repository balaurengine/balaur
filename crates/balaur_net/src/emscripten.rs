//! The browser backend: emscripten fetch for `http`, browser websockets for
//! `websocket`, through the C shim in `shim/emscripten_net.c`.
//!
//! No threads — browser I/O is already asynchronous on the main thread.
//! Completions land as C callbacks between frames, feed the same channel the
//! native worker threads do, and the pump drains them at `Stage::First`;
//! outbound sends flush from [`pump`] once per tick. The browser owns the
//! transport, so TLS, proxies and HTTP/2 or /3 negotiation are its problem.
//!
//! The final web binary links with `-sFETCH -lwebsocket.js`, from
//! `.cargo/config.toml`; see `build.rs`.

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use crate::{HttpCall, NetEvent, SocketCommand, SocketOptions};

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
    fn balaur_ws_connect(
        url: *const c_char,
        user: *mut c_void,
        on_open: extern "C" fn(*mut c_void),
        on_message: extern "C" fn(*mut c_void, *const c_char, c_int, c_int),
        on_close: extern "C" fn(*mut c_void, c_int, *const c_char),
        on_error: extern "C" fn(*mut c_void),
    ) -> c_int;
    fn balaur_ws_send_text(socket: c_int, text: *const c_char);
    fn balaur_ws_send_binary(socket: c_int, data: *const c_void, len: c_int);
    fn balaur_ws_close(socket: c_int, code: c_int, reason: *const c_char);
}

struct FetchState {
    request: u64,
    events: Sender<NetEvent>,
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
        NetEvent::HttpResponse {
            request: state.request,
            status: status as u16,
            // Response headers are not surfaced by the shim; nothing in the
            // engine reads them yet, and the shape stays the native one.
            headers: Vec::new(),
            body: String::from_utf8_lossy(bytes).into_owned(),
        }
    } else {
        NetEvent::HttpError {
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

pub(crate) fn spawn_request(call: HttpCall, events: Sender<NetEvent>) {
    let request = call.id;
    let refuse = |message: String| {
        let _ = events.send(NetEvent::HttpError { request, message });
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

/// One browser websocket the engine still holds: its outbound command queue
/// and the shared state the C callbacks write into.
struct LiveSocket {
    handle: c_int,
    commands: Receiver<SocketCommand>,
    state: *mut SocketState,
}

struct SocketState {
    socket: u64,
    events: Sender<NetEvent>,
    /// Set by the terminal callbacks; the pump then drops the entry.
    finished: Cell<bool>,
}

thread_local! {
    static LIVE: RefCell<Vec<LiveSocket>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn ws_opened(user: *mut c_void) {
    let state = unsafe { &*user.cast::<SocketState>() };
    let _ = state.events.send(NetEvent::SocketOpen {
        socket: state.socket,
    });
}

extern "C" fn ws_received(user: *mut c_void, data: *const c_char, len: c_int, is_text: c_int) {
    let state = unsafe { &*user.cast::<SocketState>() };
    let bytes = if data.is_null() || len <= 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len as usize) }
    };
    let event = if is_text == 0 {
        NetEvent::SocketBinary {
            socket: state.socket,
            bytes: bytes.to_vec(),
        }
    } else {
        NetEvent::SocketMessage {
            socket: state.socket,
            text: String::from_utf8_lossy(bytes).into_owned(),
        }
    };
    let _ = state.events.send(event);
}

extern "C" fn ws_closed(user: *mut c_void, _code: c_int, reason: *const c_char) {
    let state = unsafe { &*user.cast::<SocketState>() };
    let _ = state.events.send(NetEvent::SocketClosed {
        socket: state.socket,
        reason: text_at(reason, ""),
    });
    state.finished.set(true);
}

extern "C" fn ws_failed(user: *mut c_void) {
    // Reports only: the browser follows an error with a close, and `finished`
    // must not be set until no callback can touch this state again.
    let state = unsafe { &*user.cast::<SocketState>() };
    let _ = state.events.send(NetEvent::SocketError {
        socket: state.socket,
        reason: "the connection failed".into(),
    });
}

pub(crate) fn spawn_socket(
    socket: u64,
    url: String,
    // The browser negotiates compression and headers itself.
    _options: SocketOptions,
    commands: Receiver<SocketCommand>,
    events: &Sender<NetEvent>,
) {
    let refuse = |reason: String| {
        let _ = events.send(NetEvent::SocketError { socket, reason });
    };
    let Ok(url) = CString::new(url) else {
        return refuse("the url holds a NUL byte".into());
    };
    let state = Box::into_raw(Box::new(SocketState {
        socket,
        events: events.clone(),
        finished: Cell::new(false),
    }));
    let handle = unsafe {
        balaur_ws_connect(
            url.as_ptr(),
            state.cast::<c_void>(),
            ws_opened,
            ws_received,
            ws_closed,
            ws_failed,
        )
    };
    if handle <= 0 {
        drop(unsafe { Box::from_raw(state) });
        return refuse("websockets are unavailable here".into());
    }
    LIVE.with(|live| {
        live.borrow_mut().push(LiveSocket {
            handle,
            commands,
            state,
        });
    });
}

/// Flush queued sends and drop finished connections; called once per tick
/// from the net pump. Send latency is therefore one frame at most.
pub(crate) fn pump() {
    LIVE.with(|live| {
        live.borrow_mut().retain_mut(|entry| {
            let finished = unsafe { &*entry.state }.finished.get();
            if finished {
                // The shim freed its own state on close; ours goes here, and
                // no callback can fire for this handle again.
                drop(unsafe { Box::from_raw(entry.state) });
                return false;
            }
            loop {
                match entry.commands.try_recv() {
                    Ok(SocketCommand::SendText(text)) => {
                        if let Ok(text) = CString::new(text) {
                            unsafe { balaur_ws_send_text(entry.handle, text.as_ptr()) };
                        }
                    }
                    Ok(SocketCommand::SendBytes(bytes)) => unsafe {
                        balaur_ws_send_binary(
                            entry.handle,
                            bytes.as_ptr().cast::<c_void>(),
                            c_int::try_from(bytes.len()).unwrap_or(c_int::MAX),
                        );
                    },
                    Ok(SocketCommand::Close) | Err(TryRecvError::Disconnected) => {
                        let reason = CString::new("bye").expect("a literal without NUL bytes");
                        unsafe { balaur_ws_close(entry.handle, 1000, reason.as_ptr()) };
                        // Kept until the close event marks it finished.
                        return true;
                    }
                    Err(TryRecvError::Empty) => return true,
                }
            }
        });
    });
}
