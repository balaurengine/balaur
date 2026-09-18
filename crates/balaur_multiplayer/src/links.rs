//! Opening links over whichever transports this build has: a listener for a
//! host, a dialled link for a joiner, chosen by the URL's scheme.

use anyhow::{Result, bail};
use balaur_core::Engine;
use balaur_core::transport::Transport;

use crate::vocabulary::TransportKind;

/// Something that hands back a link per peer that arrived.
trait Accepting {
    fn accept(&mut self) -> Vec<Box<dyn Transport>>;
}

/// A bound port, and what a joiner needs to reach it.
pub(crate) struct Listener {
    inner: Box<dyn Accepting>,
    pub url: String,
    /// Hex SHA-256 of a self-signed certificate, which a joiner pins.
    pub cert_hash: Option<String>,
}

impl Listener {
    /// Every peer that finished connecting since the last call.
    pub(crate) fn accept(&mut self) -> Vec<Box<dyn Transport>> {
        self.inner.accept()
    }
}

/// Listen on `address` over `kind`.
///
/// # Errors
/// When this build has no such transport, the address will not bind, or a
/// replay is playing.
pub(crate) fn bind(eng: &Engine, kind: TransportKind, address: &str) -> Result<Listener> {
    match kind {
        TransportKind::Websocket => bind_websocket(eng, address),
        TransportKind::Webtransport => bind_webtransport(eng, address),
    }
}

/// Dial `url`: `ws://` and `wss://` by websocket, `https://` by
/// WebTransport, pinning `cert_hash` when there is one.
///
/// # Errors
/// When the scheme names a transport this build has not got.
pub(crate) fn connect(
    eng: &Engine,
    url: &str,
    cert_hash: Option<&str>,
) -> Result<Box<dyn Transport>> {
    if url.starts_with("ws://") || url.starts_with("wss://") {
        return connect_websocket(eng, url);
    }
    if url.starts_with("https://") {
        return connect_webtransport(eng, url, cert_hash);
    }
    bail!("`{url}` is not a ws://, wss:// or https:// address")
}

/// An address a peer on this machine can dial, for a listener bound to
/// every interface.
#[cfg(all(
    any(feature = "websocket", feature = "webtransport"),
    not(target_family = "wasm")
))]
fn reachable(url: &str) -> String {
    url.replace("0.0.0.0", "127.0.0.1").replace("[::]", "[::1]")
}

#[cfg(all(feature = "websocket", not(target_family = "wasm")))]
impl Accepting for balaur_websocket::listener::WebsocketListener {
    fn accept(&mut self) -> Vec<Box<dyn Transport>> {
        Self::accept(self)
            .into_iter()
            .map(|link| Box::new(link) as Box<dyn Transport>)
            .collect()
    }
}

#[cfg(all(feature = "websocket", not(target_family = "wasm")))]
fn bind_websocket(eng: &Engine, address: &str) -> Result<Listener> {
    let listener = balaur_websocket::listener::WebsocketListener::bind(eng, address)?;
    Ok(Listener {
        url: reachable(&listener.url()),
        cert_hash: None,
        inner: Box::new(listener),
    })
}

#[cfg(not(all(feature = "websocket", not(target_family = "wasm"))))]
fn bind_websocket(_eng: &Engine, _address: &str) -> Result<Listener> {
    bail!("this build cannot listen for websocket peers")
}

#[cfg(feature = "websocket")]
#[allow(
    clippy::unnecessary_wraps,
    reason = "a build without websocket refuses here"
)]
fn connect_websocket(eng: &Engine, url: &str) -> Result<Box<dyn Transport>> {
    Ok(Box::new(
        balaur_websocket::transport::WebsocketTransport::connect(
            eng,
            url,
            balaur_websocket::SocketOptions::default(),
        ),
    ))
}

#[cfg(not(feature = "websocket"))]
fn connect_websocket(_eng: &Engine, url: &str) -> Result<Box<dyn Transport>> {
    bail!("this build has no websocket transport to reach {url}")
}

#[cfg(all(feature = "webtransport", not(target_family = "wasm")))]
impl Accepting for balaur_webtransport::WebTransportServer {
    fn accept(&mut self) -> Vec<Box<dyn Transport>> {
        Self::accept(self)
            .into_iter()
            .map(|link| Box::new(link) as Box<dyn Transport>)
            .collect()
    }
}

#[cfg(all(feature = "webtransport", not(target_family = "wasm")))]
fn bind_webtransport(eng: &Engine, address: &str) -> Result<Listener> {
    let server = balaur_webtransport::WebTransportServer::bind(eng, address)?;
    let cert_hash = server.certificate().hashes().first().map(|hash| hex(hash));
    Ok(Listener {
        url: reachable(&server.url()),
        cert_hash,
        inner: Box::new(server),
    })
}

#[cfg(not(all(feature = "webtransport", not(target_family = "wasm"))))]
fn bind_webtransport(_eng: &Engine, _address: &str) -> Result<Listener> {
    bail!("this build cannot listen for WebTransport peers")
}

#[cfg(feature = "webtransport")]
fn connect_webtransport(
    eng: &Engine,
    url: &str,
    cert_hash: Option<&str>,
) -> Result<Box<dyn Transport>> {
    use balaur_webtransport::Accept;
    let accept = match cert_hash {
        Some(hash) => Accept::Hashes(vec![unhex(hash)?]),
        None => Accept::SystemRoots,
    };
    Ok(Box::new(balaur_webtransport::WebTransportLink::connect(
        eng, url, accept,
    )?))
}

#[cfg(not(feature = "webtransport"))]
fn connect_webtransport(
    _eng: &Engine,
    url: &str,
    _cert_hash: Option<&str>,
) -> Result<Box<dyn Transport>> {
    bail!("this build has no WebTransport to reach {url}")
}

/// Lower-case hex, two digits a byte.
#[cfg(feature = "webtransport")]
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[must_use]
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The bytes a [`hex`] string spells.
///
/// # Errors
/// On an odd length or a character that is not a hex digit.
#[cfg(feature = "webtransport")]
pub(crate) fn unhex(text: &str) -> Result<Vec<u8>> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        bail!("a certificate hash needs an even number of hex digits");
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
                .ok_or_else(|| anyhow::anyhow!("`{text}` is not a hex certificate hash"))
        })
        .collect()
}
