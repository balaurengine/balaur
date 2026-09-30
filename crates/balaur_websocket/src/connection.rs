//! A websocket for Rust callers: text and binary messages over the worker a
//! script's socket runs on. [`crate::transport::WebsocketTransport`] and
//! Gamend's realtime socket both stand on it.

use std::sync::mpsc::{Receiver, channel};

use anyhow::{Result, bail};
use balaur_core::Engine;
use balaur_core::replay;
use balaur_core::transport::LinkState;
use balaur_core::wake::Commands;

use crate::{SocketCommand, SocketEvent, SocketOptions, backend};

/// What reached a connection since the last [`Connection::receive`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arrival {
    Opened,
    Text(String),
    Binary(Vec<u8>),
    /// The close frame's status (RFC 6455 §7.4), and its reason.
    Closed {
        code: u16,
        reason: String,
    },
    /// `status` is the HTTP answer that refused the upgrade, where the
    /// platform says; a browser never does.
    Failed {
        reason: String,
        status: Option<u16>,
    },
}

/// One websocket, owned by the engine thread.
pub struct Connection {
    events: Receiver<SocketEvent>,
    commands: Option<Commands<SocketCommand>>,
    state: LinkState,
}

impl Connection {
    /// Open a connection to `url`.
    ///
    /// Returns at once; the handshake runs on the worker and the connection
    /// answers `Connecting` until [`Arrival::Opened`]. A replay or a
    /// re-simulated tick opens no socket, so the connection stays
    /// `Connecting`: neither may talk to anyone.
    #[must_use]
    pub fn connect(eng: &Engine, url: &str, options: SocketOptions) -> Self {
        let (event_tx, events) = channel();
        let mut connection = Self {
            events,
            commands: None,
            state: LinkState::Connecting,
        };
        if replay::suppressed(eng) {
            return connection;
        }
        match backend::queue() {
            Ok((commands, worker)) => {
                // The socket id routes events inside `WebsocketState`; a
                // connection owns its channel, so there is nothing to route.
                backend::spawn_socket(0, url.to_string(), options, worker, &event_tx);
                connection.commands = Some(commands);
            }
            Err(err) => {
                connection.state = LinkState::Closed(format!("no worker for the link: {err}"));
            }
        }
        connection
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

    /// Queue a text message.
    ///
    /// # Errors
    /// When the connection is not open.
    pub fn send_text(&mut self, text: &str) -> Result<()> {
        self.send(SocketCommand::SendText(text.to_string()))
    }

    /// Queue a binary message.
    ///
    /// # Errors
    /// When the connection is not open.
    pub fn send_bytes(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.send(SocketCommand::SendBytes(bytes))
    }

    fn send(&mut self, command: SocketCommand) -> Result<()> {
        if self.state != LinkState::Open {
            bail!("the link is {:?}, not open", self.state);
        }
        let Some(commands) = &self.commands else {
            bail!("the link has no worker");
        };
        if !commands.send(command) {
            self.state = LinkState::Closed(String::from("the worker is gone"));
            bail!("the link closed while sending");
        }
        Ok(())
    }

    /// Everything that arrived since the last call, in order.
    pub fn receive(&mut self) -> Vec<Arrival> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            let arrival = match event {
                SocketEvent::Open { .. } => {
                    self.state = LinkState::Open;
                    Arrival::Opened
                }
                SocketEvent::Message { text, .. } => Arrival::Text(text),
                SocketEvent::Binary { bytes, .. } => Arrival::Binary(bytes),
                SocketEvent::Closed { code, reason, .. } => {
                    self.state = LinkState::Closed(reason.clone());
                    Arrival::Closed { code, reason }
                }
                SocketEvent::Failed { reason, status, .. } => {
                    self.state = LinkState::Closed(reason.clone());
                    Arrival::Failed { reason, status }
                }
            };
            out.push(arrival);
        }
        out
    }

    /// Where the link stands, as of the last [`Self::receive`].
    #[must_use]
    pub fn state(&self) -> LinkState {
        self.state.clone()
    }

    /// Ask the peer to close; [`Arrival::Closed`] follows.
    pub fn close(&mut self) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(SocketCommand::Close);
        }
    }
}

/// Send what the browser's connections queued; a native worker sends on its
/// own. A caller stepping connections outside the websocket plugin's tick
/// calls this after its sends.
pub fn flush() {
    backend::pump();
}
