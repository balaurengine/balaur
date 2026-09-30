//! Accepting connections, which is the half the engine never had.
//!
//! Every socket until now was outbound, so two engines could not meet without
//! a server between them. A [`WebsocketListener`] binds a port, does the
//! server side of the upgrade on a worker thread, and hands back a
//! [`WebsocketTransport`] per peer — the same type the client side produces,
//! so a session cannot tell which end it is on.
//!
//! The accepting thread sleeps in a `mio::Poll` until a peer knocks or the
//! listener is dropped; each peer's upgrade runs on that peer's own thread,
//! so a slow client holds up nobody else.
//!
//! No TLS and no `permessage-deflate` here. A listener is what a game's own
//! host process or a loopback test runs; anything public belongs behind a
//! proxy that already terminates both.

use std::io::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use anyhow::{Context, Result, bail};
use balaur_core::Engine;
use balaur_core::replay;
use balaur_core::wake::{Commands, Worker};
use mio::{Events, Interest, Token};
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::frame::FrameSocket;
use tungstenite::stream::MaybeTlsStream;

use crate::frames;
use crate::transport::WebsocketTransport;
use crate::{SocketCommand, SocketEvent};

/// A bound port, accepting peers until it is dropped.
pub struct WebsocketListener {
    addr: SocketAddr,
    arrivals: Receiver<Accepted>,
    /// Dropping this ends the accepting thread: its queue disconnects.
    _stop: Commands<()>,
}

/// One peer that finished the upgrade, as its two channel ends.
pub(crate) struct Accepted {
    pub commands: Commands<SocketCommand>,
    pub events: Receiver<SocketEvent>,
}

/// The listening socket, among the accepting thread's poll sources.
const LISTENING: Token = Token(0);

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
        listener.set_nonblocking(true)?;
        let mut listener = mio::net::TcpListener::from_std(listener);
        let (stop, worker) = balaur_core::wake::worker()?;
        worker
            .poll
            .registry()
            .register(&mut listener, LISTENING, Interest::READABLE)?;
        let (sender, arrivals) = channel();
        std::thread::spawn(move || accept_until_dropped(&listener, worker, &sender));
        Ok(Self {
            addr: bound,
            arrivals,
            _stop: stop,
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

/// Hand every knocking peer its own thread, sleeping between knocks, until
/// the listener's queue disconnects.
fn accept_until_dropped(
    listener: &mio::net::TcpListener,
    mut worker: Worker<()>,
    arrivals: &Sender<Accepted>,
) {
    let mut ready = Events::with_capacity(4);
    loop {
        if matches!(worker.commands.try_recv(), Err(TryRecvError::Disconnected)) {
            return;
        }
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let arrivals = arrivals.clone();
                    std::thread::spawn(move || serve(stream.into(), &arrivals));
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                // A peer gone before its accept, or no descriptor left: the
                // next knock tries again.
                Err(err) => {
                    tracing::warn!(error = %err, "accepting a peer failed");
                    break;
                }
            }
        }
        if let Err(err) = worker.poll.poll(&mut ready, None)
            && err.kind() != std::io::ErrorKind::Interrupted
        {
            tracing::warn!(error = %err, "the listener stopped waiting for peers");
            return;
        }
    }
}

/// Upgrade one peer, then run the same frame loop the client side runs on
/// this thread.
fn serve(stream: TcpStream, arrivals: &Sender<Accepted>) {
    match upgrade(stream) {
        Ok((connection, worker, accepted, event_tx)) => {
            if arrivals.send(accepted).is_err() {
                return;
            }
            balaur_core::replay::report(&event_tx, SocketEvent::Open { socket: 0 });
            let event = frames::run(0, connection, None, worker, &event_tx);
            balaur_core::replay::report(&event_tx, event);
        }
        Err(err) => tracing::warn!(error = %format!("{err:#}"), "a peer failed to upgrade"),
    }
}

/// What a finished upgrade leaves: the frames, the thread's end of the queue,
/// the engine's two ends, and where events go.
type Upgraded = (
    FrameSocket<MaybeTlsStream<mio::net::TcpStream>>,
    Worker<SocketCommand>,
    Accepted,
    Sender<SocketEvent>,
);

/// The server half of the upgrade, as a blocking exchange; the socket turns
/// non-blocking once it is done.
fn upgrade(stream: TcpStream) -> Result<Upgraded> {
    stream.set_nonblocking(false)?;
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

    let connection = FrameSocket::from_partially_read(frames::nonblocking(stream)?, tail);
    let (commands, worker) = balaur_core::wake::worker()?;
    let (event_tx, events) = channel();
    Ok((connection, worker, Accepted { commands, events }, event_tx))
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
