//! Accepting connections, which is the half the engine never had.
//!
//! Every socket until now was outbound, so two engines could not meet without
//! a server between them. A [`WebsocketListener`] binds a port, does the
//! server side of the upgrade on a worker thread, and hands back a
//! [`WebsocketTransport`] per peer — the same type the client side produces,
//! so a session cannot tell which end it is on.
//!
//! No TLS and no `permessage-deflate` here. A listener is what a game's own
//! host process or a loopback test runs; anything public belongs behind a
//! proxy that already terminates both.

use std::io::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

use anyhow::{bail, Context, Result};
use balaur_core::replay;
use balaur_core::Engine;
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::frame::FrameSocket;
use tungstenite::stream::MaybeTlsStream;

use crate::transport::WebsocketTransport;
use crate::websocket;
use crate::{NetEvent, SocketCommand};

/// A bound port, accepting peers.
pub struct WebsocketListener {
    addr: SocketAddr,
    arrivals: Receiver<Accepted>,
}

/// One peer that finished the upgrade, as its two channel ends.
pub(crate) struct Accepted {
    pub commands: Sender<SocketCommand>,
    pub events: Receiver<NetEvent>,
}

impl WebsocketListener {
    /// Bind and start accepting.
    ///
    /// `addr` takes a port of 0 to let the OS choose, which is what a test
    /// wants; [`WebsocketListener::addr`] reports what it got.
    ///
    /// # Errors
    /// When the port cannot be bound, or while a recording is playing or a
    /// tick is being re-simulated — neither of which may reach a network.
    pub fn bind(eng: &Engine, addr: &str) -> Result<Self> {
        if replay::suppressed(eng) {
            bail!("a replayed or re-simulated tick does not accept connections");
        }
        let listener = TcpListener::bind(addr).with_context(|| format!("binding {addr}"))?;
        let bound = listener.local_addr()?;
        let (sender, arrivals) = channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                match serve(stream) {
                    Ok(accepted) => {
                        if sender.send(accepted).is_err() {
                            // The engine dropped the listener; stop accepting.
                            return;
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %format!("{err:#}"), "a peer failed to upgrade");
                    }
                }
            }
        });
        Ok(Self {
            addr: bound,
            arrivals,
        })
    }

    /// The address actually bound.
    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// A url a [`WebsocketTransport`] can connect to.
    #[must_use]
    pub fn url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    /// Every peer that finished its upgrade since the last call.
    ///
    /// Polled like everything else, so an accept lands between ticks rather
    /// than in the middle of one.
    pub fn accept(&mut self) -> Vec<WebsocketTransport> {
        let mut out = Vec::new();
        loop {
            match self.arrivals.try_recv() {
                Ok(accepted) => out.push(WebsocketTransport::from_accepted(accepted)),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
            }
        }
    }
}

/// Do the server half of the upgrade, then run the same frame loop the
/// client side runs.
fn serve(stream: TcpStream) -> Result<Accepted> {
    stream.set_nodelay(true)?;
    let mut stream = MaybeTlsStream::Plain(stream);
    let (head, tail) = read_request(&mut stream)?;
    let key = request_key(&head)?;
    // No extensions echoed back, so the client's own negotiation resolves to
    // no compression and both ends send plain frames.
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        derive_accept_key(key.as_bytes())
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;

    let connection = FrameSocket::from_partially_read(stream, tail);
    let (commands, command_rx) = channel();
    let (event_tx, events) = channel();
    std::thread::spawn(move || {
        let _ = event_tx.send(NetEvent::SocketOpen { socket: 0 });
        let event = websocket::run(0, connection, None, &command_rx, &event_tx);
        let _ = event_tx.send(event);
    });
    Ok(Accepted { commands, events })
}

/// The upgrade request up to its blank line, and whatever came after it — an
/// eager client's first frame, which must not be lost.
fn read_request(stream: &mut impl std::io::Read) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            let tail = buffer.split_off(end + 4);
            return Ok((buffer, tail));
        }
        if buffer.len() > 64 * 1024 {
            bail!("the handshake request never ended");
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            bail!("the peer closed the connection during the handshake");
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn request_key(head: &[u8]) -> Result<String> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    request.parse(head).context("parsing the upgrade request")?;
    request
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("sec-websocket-key"))
        .map(|h| String::from_utf8_lossy(h.value).trim().to_string())
        .ok_or_else(|| anyhow::anyhow!("the request carried no Sec-WebSocket-Key"))
}
