//! WebTransport over QUIC, behind [`Transport`].
//!
//! What the websocket transport pretends to be, this one is. A datagram here
//! is a real unreliable datagram: losing one costs a misprediction the
//! rollback session already knows how to repair, where over TCP it stalls
//! every input behind it until the retransmit lands. The reliable channel is
//! one bidirectional stream, for the handshake and the digests.
//!
//! **Async underneath, polled on top.** `web-transport-quinn` is written for
//! tokio and the engine is not: nothing may block or await on the frame loop.
//! So a link owns a worker thread running a current-thread runtime, and talks
//! to the engine over the same two channels the websocket worker uses —
//! commands out, events in. The runtime never leaves that thread.
//!
//! **QUIC is always TLS**, with no plaintext mode to fall back to. A server
//! generates a self-signed certificate by default, so two engines on one
//! machine need no setup, and the client accepts it by hash. Anything shipped
//! passes a real certificate and key instead. A browser will accept a
//! self-signed certificate by hash too, but only while it is valid for at
//! most two weeks.

#[cfg(not(target_family = "wasm"))]
use std::net::SocketAddr;
use std::sync::mpsc::{channel, Receiver, Sender};

#[cfg(not(target_family = "wasm"))]
use anyhow::Context;
use anyhow::{bail, Result};
use balaur_core::replay;
use balaur_core::transport::{Delivery, LinkState, Received, Transport};
use balaur_core::Engine;

#[cfg(target_family = "wasm")]
mod browser;
#[cfg(not(target_family = "wasm"))]
mod link;
#[cfg(not(target_family = "wasm"))]
mod tls;

#[cfg(not(target_family = "wasm"))]
pub use tls::Certificate;

/// What a client is willing to trust in a server.
///
/// Target-independent on purpose: both arms mean something in a browser too,
/// where `Hashes` is the WebTransport API's `serverCertificateHashes` and
/// `SystemRoots` is what it does by default.
#[derive(Clone, Debug)]
pub enum Accept {
    /// Pin exactly these certificate hashes. What a self-signed server needs,
    /// and what a browser accepts for one.
    Hashes(Vec<Vec<u8>>),
    /// Trust anything a public authority signed, as a browser does for a
    /// website. What a shipped server uses.
    SystemRoots,
}

/// What a worker thread reports back to the frame loop.
#[cfg_attr(
    target_family = "wasm",
    allow(dead_code, reason = "the browser backend reports only Closed until it is built")
)]
enum LinkEvent {
    Open,
    /// A payload and the promise it was sent under.
    Payload(Received),
    Closed(String),
}

/// What the frame loop asks a worker thread to do.
#[cfg_attr(
    target_family = "wasm",
    allow(dead_code, reason = "nothing in a browser drains these until the backend does")
)]
enum LinkCommand {
    Send(Delivery, Vec<u8>),
    Close,
}

/// One QUIC link to one peer.
///
/// The same type on both ends: [`WebTransportLink::connect`] dials out and a
/// [`WebTransportServer`] hands back the other side, so a session cannot tell
/// which end it is holding.
pub struct WebTransportLink {
    events: Receiver<LinkEvent>,
    commands: Option<Sender<LinkCommand>>,
    state: LinkState,
    /// Reported by the peer's QUIC stack once the link is up; the conservative
    /// floor every QUIC implementation accepts until then.
    max_datagram: usize,
}

/// The floor QUIC guarantees before a path has been measured. Real links
/// report more, and the worker raises this once the session is up.
const MIN_DATAGRAM: usize = 1200;

impl WebTransportLink {
    /// Dial `url`, which must be `https://host:port/path`.
    ///
    /// Returns immediately; the handshake runs on the worker thread and
    /// [`Transport::state`] answers `Connecting` until it lands. A replay or a
    /// re-simulated tick opens nothing at all, and the link stays
    /// `Connecting` — the intended outcome, since neither should be talking
    /// to anyone.
    ///
    /// `accept` names what to trust: the hashes of a self-signed server's
    /// certificate, or the system roots for a real one.
    ///
    /// # Errors
    /// When the url or the trust settings are unusable. A failure to reach
    /// the peer is not an error here — it arrives as a `Closed` state.
    pub fn connect(eng: &Engine, url: &str, accept: Accept) -> Result<Self> {
        let (commands, command_rx) = channel();
        let (event_tx, events) = channel();
        if replay::suppressed(eng) {
            return Ok(Self::idle(events));
        }
        let url = url.to_string();
        #[cfg(not(target_family = "wasm"))]
        std::thread::spawn(move || link::dial(&url, accept, command_rx, event_tx));
        #[cfg(target_family = "wasm")]
        browser::dial(&url, accept, command_rx, &event_tx);
        Ok(Self {
            events,
            commands: Some(commands),
            state: LinkState::Connecting,
            max_datagram: MIN_DATAGRAM,
        })
    }

    /// A link that never connects, for a tick that must not reach the wire.
    fn idle(events: Receiver<LinkEvent>) -> Self {
        Self {
            events,
            commands: None,
            state: LinkState::Connecting,
            max_datagram: MIN_DATAGRAM,
        }
    }

    #[cfg(not(target_family = "wasm"))]
    fn from_accepted(events: Receiver<LinkEvent>, commands: Sender<LinkCommand>) -> Self {
        Self {
            events,
            commands: Some(commands),
            state: LinkState::Connecting,
            max_datagram: MIN_DATAGRAM,
        }
    }

    fn send(&mut self, delivery: Delivery, bytes: &[u8]) -> Result<()> {
        if self.state != LinkState::Open {
            bail!("the link is {:?}, not open", self.state);
        }
        let Some(commands) = &self.commands else {
            bail!("the link has no worker");
        };
        if commands
            .send(LinkCommand::Send(delivery, bytes.to_vec()))
            .is_err()
        {
            self.state = LinkState::Closed(String::from("the worker is gone"));
            bail!("the link closed while sending");
        }
        Ok(())
    }
}

impl Transport for WebTransportLink {
    fn send_reliable(&mut self, bytes: &[u8]) -> Result<()> {
        self.send(Delivery::Reliable, bytes)
    }

    fn send_datagram(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.max_datagram {
            bail!(
                "{} bytes is over this path's {} byte datagram limit",
                bytes.len(),
                self.max_datagram
            );
        }
        self.send(Delivery::Datagram, bytes)
    }

    fn receive(&mut self) -> Vec<Received> {
        let mut out = Vec::new();
        let mut arrivals = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            arrivals.push(event);
        }
        for event in arrivals {
            match event {
                LinkEvent::Open => self.state = LinkState::Open,
                LinkEvent::Payload(received) => out.push(received),
                LinkEvent::Closed(reason) => self.state = LinkState::Closed(reason),
            }
        }
        out
    }

    fn max_datagram(&self) -> usize {
        self.max_datagram
    }

    fn state(&self) -> LinkState {
        self.state.clone()
    }

    fn close(&mut self) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(LinkCommand::Close);
        }
    }
}

/// A bound UDP port accepting QUIC sessions.
///
/// Native-only, and permanently so rather than pending a backend: a browser
/// has no listening socket.
#[cfg(not(target_family = "wasm"))]
pub struct WebTransportServer {
    addr: SocketAddr,
    arrivals: Receiver<(Receiver<LinkEvent>, Sender<LinkCommand>)>,
    certificate: Certificate,
}

#[cfg(not(target_family = "wasm"))]
impl WebTransportServer {
    /// Bind and start accepting, with a freshly generated self-signed
    /// certificate.
    ///
    /// `addr` takes a port of 0 to let the OS choose, which is what a test
    /// wants; [`WebTransportServer::url`] reports where it landed.
    ///
    /// # Errors
    /// When the port cannot be bound, the certificate cannot be generated, or
    /// a recording is playing — which does not accept connections.
    pub fn bind(eng: &Engine, addr: &str) -> Result<Self> {
        Self::bind_with(eng, addr, Certificate::self_signed(&["localhost"])?)
    }

    /// Bind with a certificate of your own: what a shipped server does.
    ///
    /// # Errors
    /// As [`WebTransportServer::bind`].
    pub fn bind_with(eng: &Engine, addr: &str, certificate: Certificate) -> Result<Self> {
        if replay::suppressed(eng) {
            bail!("a replayed or re-simulated tick does not accept connections");
        }
        let addr: SocketAddr = addr
            .parse()
            .with_context(|| format!("'{addr}' is not a socket address"))?;
        let (sender, arrivals) = channel();
        let bound = link::listen(addr, &certificate, sender)?;
        Ok(Self {
            addr: bound,
            arrivals,
            certificate,
        })
    }

    /// The address actually bound.
    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// A url a [`WebTransportLink`] can dial.
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://{}/", self.addr)
    }

    /// What a client has to trust to reach this server.
    #[must_use]
    pub fn certificate(&self) -> &Certificate {
        &self.certificate
    }

    /// Every peer whose session finished since the last call.
    ///
    /// Polled like everything else, so an accept lands between ticks rather
    /// than in the middle of one.
    pub fn accept(&mut self) -> Vec<WebTransportLink> {
        let mut out = Vec::new();
        while let Ok((events, commands)) = self.arrivals.try_recv() {
            out.push(WebTransportLink::from_accepted(events, commands));
        }
        out
    }
}

/// Turn a failure into the reason a link reports, rather than losing it.
#[cfg(not(target_family = "wasm"))]
fn reason(error: &anyhow::Error) -> String {
    format!("{error:#}")
}
