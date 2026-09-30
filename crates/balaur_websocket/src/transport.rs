//! A [`Transport`] over a websocket.
//!
//! The transport games get today, and the one the loopback tests run against
//! until QUIC lands. It is honest about what it is: websockets run over TCP,
//! so a lost packet stalls every frame behind it, and a "datagram" here is a
//! reliable frame wearing a label. That is the right trade for turn-based
//! play and for a lockstep session at a low tick rate, and the wrong one for
//! anything twitchy — which is why `PLAN-networking.md` puts WebTransport
//! under the same trait next.
//!
//! Both deliveries share one websocket, so each frame carries a one-byte tag
//! saying which it was. A peer that is not this transport will not understand
//! them, which is fine: both ends of a session run this code.

use anyhow::{Result, bail};
use balaur_core::Engine;
use balaur_core::transport::{Delivery, LinkState, Received, Transport};

use crate::SocketOptions;
use crate::connection::{Arrival, Connection};

/// The first byte of every frame, saying which promise the payload was sent
/// under. A tag rather than two websockets, because one connection is one
/// ordering domain and splitting it would reorder the reliable half.
const TAG_RELIABLE: u8 = 0;
const TAG_DATAGRAM: u8 = 1;

/// A websocket's largest frame here. Well under what a server will accept,
/// and far over a tick of inputs, which is all a datagram carries.
const MAX_DATAGRAM: usize = 60 * 1024;

/// One link to one peer, over one websocket.
///
/// The same type on both ends: [`WebsocketTransport::connect`] dials out, and
/// the connection a [`crate::listener::WebsocketListener`] accepts becomes one
/// through `From`, so a session cannot tell which end it is holding.
pub struct WebsocketTransport {
    connection: Connection,
}

impl WebsocketTransport {
    /// Open a link to `url`.
    ///
    /// Returns immediately; the handshake runs on a worker thread and
    /// [`Transport::state`] answers `Connecting` until it lands. A replay or
    /// a re-simulated tick opens no socket at all and the link stays
    /// `Connecting` forever — which is the intended outcome, since neither
    /// should be talking to anyone.
    #[must_use]
    pub fn connect(eng: &Engine, url: &str, options: SocketOptions) -> Self {
        Self {
            connection: Connection::connect(eng, url, options),
        }
    }

    fn send_tagged(&mut self, tag: u8, bytes: &[u8]) -> Result<()> {
        let mut framed = Vec::with_capacity(bytes.len() + 1);
        framed.push(tag);
        framed.extend_from_slice(bytes);
        self.connection.send_bytes(framed)
    }
}

/// A session's link over a connection either end made: a dialled one, or one
/// a [`crate::listener::WebsocketListener`] accepted.
impl From<Connection> for WebsocketTransport {
    fn from(connection: Connection) -> Self {
        Self { connection }
    }
}

impl Transport for WebsocketTransport {
    fn send_reliable(&mut self, bytes: &[u8]) -> Result<()> {
        self.send_tagged(TAG_RELIABLE, bytes)
    }

    fn send_datagram(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_DATAGRAM {
            bail!(
                "{} bytes is over the {MAX_DATAGRAM} datagram limit",
                bytes.len()
            );
        }
        self.send_tagged(TAG_DATAGRAM, bytes)
    }

    fn receive(&mut self) -> Vec<Received> {
        let mut out = Vec::new();
        for arrival in self.connection.receive() {
            match arrival {
                Arrival::Binary(bytes) => {
                    // A frame with no tag is a peer that is not this
                    // transport; dropping it beats guessing which half it is.
                    let Some((&tag, payload)) = bytes.split_first() else {
                        continue;
                    };
                    let delivery = match tag {
                        TAG_RELIABLE => Delivery::Reliable,
                        TAG_DATAGRAM => Delivery::Datagram,
                        other => {
                            tracing::warn!(tag = other, "a frame arrived with an unknown tag");
                            continue;
                        }
                    };
                    out.push(Received {
                        delivery,
                        bytes: payload.to_vec(),
                    });
                }
                // Text frames are what a hand-written server or a browser
                // console sends; this protocol is binary.
                Arrival::Text(_) => tracing::warn!("a text frame arrived on a transport link"),
                Arrival::Opened | Arrival::Closed { .. } | Arrival::Failed { .. } => {}
            }
        }
        out
    }

    fn max_datagram(&self) -> usize {
        MAX_DATAGRAM
    }

    fn state(&self) -> LinkState {
        self.connection.state()
    }

    fn close(&mut self) {
        self.connection.close();
    }
}
