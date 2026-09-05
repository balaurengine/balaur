//! The browser backend: WebTransport through the browser's own API.
//!
//! QUIC is not reachable as QUIC in a page — there is no UDP socket, and
//! `web-transport-quinn` cannot run here at all. What a page gets instead is
//! the `WebTransport` constructor, which speaks the same protocol and offers
//! the same two things this crate needs: one bidirectional stream, and
//! unreliable datagrams.
//!
//! **Why a hand-written shim.** `web_sys::WebTransport` exists but sits
//! behind `#[cfg(web_sys_unstable_apis)]`, which is a global rustflag over
//! the whole build and an API that may change without a semver bump. So the
//! binding is written here instead — the same thing
//! `balaur_websocket/shim/emscripten_websocket.c` does for emscripten, in
//! the language this target speaks.
//!
//! **No worker thread.** Browser I/O is already asynchronous on the main
//! thread: the read loops run as promises, push into the channel a native
//! worker would push into, and [`pump`] flushes queued sends once per tick
//! from `receive`. Send latency is one frame at most, as everywhere else.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use balaur_core::transport::{Delivery, Received};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::{Accept, LinkCommand, LinkEvent};

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function bt_open(url, hashes, on_open, on_reliable, on_datagram, on_closed) {
  const options = {};
  // A self-signed server is pinned by hash, which is the only form a browser
  // accepts for one — and only while the certificate is under two weeks old.
  if (hashes && hashes.length) {
    options.serverCertificateHashes = hashes.map(
      (value) => ({ algorithm: "sha-256", value })
    );
  }
  let wt;
  try {
    wt = new WebTransport(url, options);
  } catch (e) {
    on_closed(String(e));
    return null;
  }
  const handle = { wt, reliable: null, datagrams: null, closed: false };
  const die = (e) => { handle.closed = true; on_closed(String(e)); };
  wt.closed.then(() => die("the peer closed the session")).catch(die);
  wt.ready.then(async () => {
    const stream = await wt.createBidirectionalStream();
    handle.reliable = stream.writable.getWriter();
    handle.datagrams = wt.datagrams.writable.getWriter();
    on_open();
    pipe(stream.readable.getReader(), on_reliable, die);
    pipe(wt.datagrams.readable.getReader(), on_datagram, die);
  }).catch(die);
  return handle;
}

// One read loop per channel. A reader that ends is the session ending, which
// `wt.closed` reports too, so this only has to stop.
async function pipe(reader, deliver, die) {
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      if (value) deliver(value);
    }
  } catch (e) { die(e); }
}

export function bt_send(handle, bytes, reliable) {
  if (!handle || handle.closed) return;
  const writer = reliable ? handle.reliable : handle.datagrams;
  // Before `ready` settles there is no writer; the engine refuses sends until
  // the link reports Open, so this is belt and braces.
  if (writer) writer.write(bytes).catch(() => {});
}

export function bt_close(handle) {
  if (!handle || handle.closed) return;
  handle.closed = true;
  try { handle.wt.close(); } catch (e) { /* already gone */ }
}
"#)]
unsafe extern "C" {
    fn bt_open(
        url: &str,
        hashes: JsValue,
        on_open: &js_sys::Function,
        on_reliable: &js_sys::Function,
        on_datagram: &js_sys::Function,
        on_closed: &js_sys::Function,
    ) -> JsValue;
    fn bt_send(handle: &JsValue, bytes: &[u8], reliable: bool);
    fn bt_close(handle: &JsValue);
}

/// One browser session the engine still holds.
struct LiveLink {
    handle: JsValue,
    commands: Receiver<LinkCommand>,
    /// Set by the closing callback; the pump then drops the entry.
    finished: Rc<Cell<bool>>,
    /// The callbacks stay owned here: dropping one unregisters it, and a
    /// session whose handlers vanished is a session that goes quiet.
    _opened: Closure<dyn FnMut()>,
    _payloads: Vec<Closure<dyn FnMut(JsValue)>>,
}

thread_local! {
    static LIVE: RefCell<Vec<LiveLink>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn dial(
    url: &str,
    accept: Accept,
    commands: Receiver<LinkCommand>,
    events: &Sender<LinkEvent>,
) {
    let finished = Rc::new(Cell::new(false));

    let opened = {
        let events = events.clone();
        Closure::wrap(Box::new(move || {
            let _ = events.send(LinkEvent::Open);
        }) as Box<dyn FnMut()>)
    };
    let on_reliable = payload_callback(events.clone(), Delivery::Reliable);
    let on_datagram = payload_callback(events.clone(), Delivery::Datagram);
    let on_closed = {
        let events = events.clone();
        let finished = Rc::clone(&finished);
        Closure::wrap(Box::new(move |reason: JsValue| {
            finished.set(true);
            let _ = events.send(LinkEvent::Closed(
                reason
                    .as_string()
                    .unwrap_or_else(|| String::from("the session closed")),
            ));
        }) as Box<dyn FnMut(JsValue)>)
    };

    let handle = bt_open(
        url,
        hashes_to_js(&accept),
        opened.as_ref().unchecked_ref(),
        on_reliable.as_ref().unchecked_ref(),
        on_datagram.as_ref().unchecked_ref(),
        on_closed.as_ref().unchecked_ref(),
    );
    // A constructor that threw already reported itself through `on_closed`.
    if handle.is_null() {
        return;
    }
    LIVE.with(|live| {
        live.borrow_mut().push(LiveLink {
            handle,
            commands,
            finished,
            _opened: opened,
            _payloads: vec![on_reliable, on_datagram, on_closed],
        });
    });
}

/// Flush queued sends and drop finished sessions; called once per tick from
/// `Transport::receive`, which is this backend's only per-frame hook.
pub(crate) fn pump() {
    LIVE.with(|live| {
        live.borrow_mut().retain_mut(|entry| {
            if entry.finished.get() {
                return false;
            }
            loop {
                match entry.commands.try_recv() {
                    Ok(LinkCommand::Send(delivery, bytes)) => {
                        bt_send(&entry.handle, &bytes, delivery == Delivery::Reliable);
                    }
                    Ok(LinkCommand::Close) | Err(TryRecvError::Disconnected) => {
                        bt_close(&entry.handle);
                        // Kept until the closed callback marks it finished.
                        return true;
                    }
                    Err(TryRecvError::Empty) => return true,
                }
            }
        });
    });
}

/// Each arriving chunk is a `Uint8Array` the JS side handed over.
fn payload_callback(events: Sender<LinkEvent>, delivery: Delivery) -> Closure<dyn FnMut(JsValue)> {
    Closure::wrap(Box::new(move |chunk: JsValue| {
        let bytes = js_sys::Uint8Array::new(&chunk).to_vec();
        let _ = events.send(LinkEvent::Payload(Received { delivery, bytes }));
    }) as Box<dyn FnMut(JsValue)>)
}

/// `SystemRoots` is the browser's default, so it passes no hashes at all.
fn hashes_to_js(accept: &Accept) -> JsValue {
    match accept {
        Accept::Hashes(hashes) => {
            let out = js_sys::Array::new();
            for hash in hashes {
                out.push(&js_sys::Uint8Array::from(hash.as_slice()));
            }
            out.into()
        }
        Accept::SystemRoots => JsValue::NULL,
    }
}
