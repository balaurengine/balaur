//! The browser backend: WebTransport through the browser's own API.
//!
//! QUIC in a browser is not reachable as QUIC — there is no UDP socket, and
//! `web-transport-quinn` cannot run here at all. What a page gets instead is
//! the `WebTransport` constructor, which speaks the same protocol and hands
//! back the same two things this crate needs: one bidirectional stream and
//! unreliable datagrams.
//!
//! No worker thread either. Browser I/O is already asynchronous on the main
//! thread, so a completion lands as a promise callback between frames and
//! feeds the same channel the native worker does — the shape
//! `balaur_http`'s emscripten backend already uses.
//!
//! Not built yet. Until it is, a link reports itself closed rather than
//! hanging in `Connecting` forever, so a script sees a reason instead of a
//! silence.

use std::sync::mpsc::{Receiver, Sender};

use crate::{Accept, LinkCommand, LinkEvent};

/// What the real backend maps onto, kept here so the shape is not re-derived:
///
/// - `Accept::Hashes` → the `serverCertificateHashes` option, which the
///   browser accepts only for a certificate valid at most two weeks — which
///   is why `Certificate::self_signed` is short-lived.
/// - `Accept::SystemRoots` → the default, no option set.
/// - the session's one bidirectional stream → `WebTransport.createBidirectionalStream`
/// - datagrams → `WebTransport.datagrams`, reader and writer.
pub(crate) fn dial(
    url: &str,
    _accept: Accept,
    _commands: Receiver<LinkCommand>,
    events: &Sender<LinkEvent>,
) {
    tracing::warn!("no WebTransport backend for the browser yet; {url} stays closed");
    let _ = events.send(LinkEvent::Closed(String::from(
        "WebTransport in a browser needs the web-sys backend, which is not built yet",
    )));
}
