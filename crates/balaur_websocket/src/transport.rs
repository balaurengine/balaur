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

use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{bail, Result};
use balaur_core::replay;
use balaur_core::transport::{Delivery, LinkState, Received, Transport};
use balaur_core::Engine;

use crate::{backend, SocketCommand, SocketEvent, SocketOptions};

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
/// The same type on both ends: [`WebsocketTransport::connect`] dials out and
/// a [`crate::listener::WebsocketListener`] hands back the other side, so a
/// session cannot tell which end it is holding.
pub struct WebsocketTransport {
    events: Receiver<SocketEvent>,
    commands: Option<Sender<SocketCommand>>,
    state: LinkState,
}

impl WebsocketTransport {
    /// Open a link to `url`.
    ///
    /// Returns immediately; the handshake runs on a worker thread and
    /// [`Transport::state`] answers `Connecting` until it lands. Goes through
    /// [`ExternalIo::start`](balaur_core::replay::ExternalIo::start), so a
    /// replay or a re-simulated tick opens no
    /// socket at all and the link stays `Connecting` forever — which is the
    /// intended outcome, since neither should be talking to anyone.
    #[must_use]
    pub fn connect(eng: &Engine, url: &str, options: SocketOptions) -> Self {
        let (commands, command_rx) = channel();
        let (event_tx, events) = channel();
        // Same rule `ExternalIo::start` enforces, asked directly because a
        // transport owns its channel rather than borrowing one.
        let started = !replay::suppressed(eng);
        if started {
            // The worker's socket id routes events inside `WebsocketState`; a
            // transport owns its channel, so there is nothing to route.
            backend::spawn_socket(0, url.to_string(), options, command_rx, &event_tx);
        }
        Self {
            events,
            commands: started.then_some(commands),
            state: LinkState::Connecting,
        }
    }

    /// The peer side of a link a listener accepted.
    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn from_accepted(accepted: crate::listener::Accepted) -> Self {
        Self {
            events: accepted.events,
            commands: Some(accepted.commands),
            state: LinkState::Connecting,
        }
    }

    fn send_tagged(&mut self, tag: u8, bytes: &[u8]) -> Result<()> {
        if self.state != LinkState::Open {
            bail!("the link is {:?}, not open", self.state);
        }
        let Some(commands) = &self.commands else {
            bail!("the link has no worker");
        };
        let mut framed = Vec::with_capacity(bytes.len() + 1);
        framed.push(tag);
        framed.extend_from_slice(bytes);
        if commands.send(SocketCommand::SendBytes(framed)).is_err() {
            self.state = LinkState::Closed(String::from("the worker is gone"));
            bail!("the link closed while sending");
        }
        Ok(())
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
        let mut arrivals = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            arrivals.push(event);
        }
        for event in arrivals {
            match event {
                SocketEvent::Open { .. } => self.state = LinkState::Open,
                SocketEvent::Binary { bytes, .. } => {
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
                SocketEvent::Message { .. } => {
                    tracing::warn!("a text frame arrived on a transport link");
                }
                SocketEvent::Closed { reason, .. } | SocketEvent::Failed { reason, .. } => {
                    self.state = LinkState::Closed(reason);
                }
            }
        }
        out
    }

    fn max_datagram(&self) -> usize {
        MAX_DATAGRAM
    }

    fn state(&self) -> LinkState {
        self.state.clone()
    }

    fn close(&mut self) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(SocketCommand::Close);
        }
    }
}
