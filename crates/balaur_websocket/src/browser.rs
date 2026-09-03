//! The browser websocket backend outside emscripten: the `WebSocket` API
//! through web-sys.
//!
//! The same shape as the emscripten backend next door — a registry of live
//! sockets, callbacks that push events, and a `pump` that flushes queued
//! sends once per tick — but with closures instead of C callbacks, so no
//! raw pointer outlives anything and there is no `unsafe` here at all.
//!
//! **Two options a browser will not honour.** `SocketOptions::headers` cannot
//! be set: the WebSocket constructor takes a url and subprotocols, and the
//! browser writes the upgrade request itself. `compression` is likewise not a
//! choice — the browser offers `permessage-deflate` on its own. Both are
//! accepted and ignored rather than refused, because a script written for
//! desktop should still run here; what it must not do is believe a header
//! was sent.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

use crate::{SocketCommand, SocketEvent, SocketOptions};

/// One browser websocket the engine still holds.
struct LiveSocket {
    ws: WebSocket,
    commands: Receiver<SocketCommand>,
    /// Set by the terminal callbacks; the pump then drops the entry.
    finished: Rc<Cell<bool>>,
    /// The callbacks stay owned here: dropping one unregisters it, and a
    /// socket whose handlers vanished is a socket that goes quiet.
    _callbacks: Vec<Closure<dyn FnMut(JsValue)>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

thread_local! {
    static LIVE: RefCell<Vec<LiveSocket>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn spawn_socket(
    socket: u64,
    url: String,
    _options: SocketOptions,
    commands: Receiver<SocketCommand>,
    events: &Sender<SocketEvent>,
) {
    let ws = match WebSocket::new(&url) {
        Ok(ws) => ws,
        Err(error) => {
            let _ = events.send(SocketEvent::Failed {
                socket,
                reason: describe(&error),
            });
            return;
        }
    };
    // Binary frames arrive as ArrayBuffer rather than Blob, so a message is
    // readable synchronously in the callback instead of behind another promise.
    ws.set_binary_type(BinaryType::Arraybuffer);

    let finished = Rc::new(Cell::new(false));

    let opened = {
        let events = events.clone();
        Closure::wrap(Box::new(move |_: JsValue| {
            let _ = events.send(SocketEvent::Open { socket });
        }) as Box<dyn FnMut(JsValue)>)
    };
    ws.set_onopen(Some(opened.as_ref().unchecked_ref()));

    let on_message = {
        let events = events.clone();
        Closure::wrap(Box::new(move |event: MessageEvent| {
            let data = event.data();
            let sent = if let Some(text) = data.as_string() {
                events.send(SocketEvent::Message { socket, text })
            } else if let Ok(buffer) = data.dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                events.send(SocketEvent::Binary { socket, bytes })
            } else {
                // A Blob, if the binary type did not take. Dropping it is
                // wrong, so say so rather than going silent.
                events.send(SocketEvent::Failed {
                    socket,
                    reason: String::from("a frame arrived in a form this backend cannot read"),
                })
            };
            let _ = sent;
        }) as Box<dyn FnMut(MessageEvent)>)
    };
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let closed = {
        let events = events.clone();
        let finished = Rc::clone(&finished);
        Closure::wrap(Box::new(move |event: JsValue| {
            let reason = event
                .dyn_ref::<CloseEvent>()
                .map(|e| e.reason())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| String::from("closed"));
            finished.set(true);
            let _ = events.send(SocketEvent::Closed { socket, reason });
        }) as Box<dyn FnMut(JsValue)>)
    };
    ws.set_onclose(Some(closed.as_ref().unchecked_ref()));

    let failed = {
        let events = events.clone();
        let finished = Rc::clone(&finished);
        Closure::wrap(Box::new(move |event: JsValue| {
            // The browser deliberately withholds why a socket failed, so this
            // is as specific as it can honestly be.
            let reason = event
                .dyn_ref::<ErrorEvent>()
                .map(|e| e.message())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| String::from("the connection failed"));
            finished.set(true);
            let _ = events.send(SocketEvent::Failed { socket, reason });
        }) as Box<dyn FnMut(JsValue)>)
    };
    ws.set_onerror(Some(failed.as_ref().unchecked_ref()));

    LIVE.with(|live| {
        live.borrow_mut().push(LiveSocket {
            ws,
            commands,
            finished,
            _callbacks: vec![opened, closed, failed],
            _on_message: on_message,
        });
    });
}

/// Flush queued sends and drop finished connections; called once per tick.
/// Send latency is therefore one frame at most, as on every other backend.
pub(crate) fn pump() {
    LIVE.with(|live| {
        live.borrow_mut().retain_mut(|entry| {
            if entry.finished.get() {
                return false;
            }
            loop {
                match entry.commands.try_recv() {
                    Ok(SocketCommand::SendText(text)) => {
                        let _ = entry.ws.send_with_str(&text);
                    }
                    Ok(SocketCommand::SendBytes(bytes)) => {
                        let _ = entry.ws.send_with_u8_array(&bytes);
                    }
                    Ok(SocketCommand::Close) | Err(TryRecvError::Disconnected) => {
                        let _ = entry.ws.close_with_code_and_reason(1000, "bye");
                        // Kept until the close event marks it finished.
                        return true;
                    }
                    Err(TryRecvError::Empty) => return true,
                }
            }
        });
    });
}

/// A thrown JS value is not always an `Error`; say something either way.
fn describe(error: &JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| String::from("the socket could not be opened"))
}
