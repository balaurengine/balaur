//! The websocket worker: one thread per connection.
//!
//! The thread alternates between draining the engine's outbound commands and
//! a `read` with a short timeout, so a single blocking socket serves both
//! directions without an async runtime. Worst-case send latency is one read
//! timeout, well under a frame's budget for game traffic.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::{NetEvent, SocketCommand};

type Socket = WebSocket<MaybeTlsStream<std::net::TcpStream>>;

pub(crate) fn spawn_socket(
    socket: u64,
    url: String,
    commands: Receiver<SocketCommand>,
    events: &Sender<NetEvent>,
) {
    let events = events.clone();
    std::thread::spawn(move || {
        // The handshake blocks — DNS, TCP, TLS — which is exactly why it
        // happens here and not in the `websocket.connect` binding.
        let connection = match tungstenite::connect(&url) {
            Ok((connection, _)) => connection,
            Err(err) => {
                let _ = events.send(NetEvent::SocketError {
                    socket,
                    reason: err.to_string(),
                });
                return;
            }
        };
        let _ = events.send(NetEvent::SocketOpen { socket });
        let event = run(socket, connection, &commands, &events);
        let _ = events.send(event);
    });
}

/// Serve one open connection until it ends, returning the closing event.
fn run(
    socket: u64,
    mut connection: Socket,
    commands: &Receiver<SocketCommand>,
    events: &Sender<NetEvent>,
) -> NetEvent {
    if let Err(err) = read_timeout(&mut connection) {
        return NetEvent::SocketError {
            socket,
            reason: format!("no read timeout: {err}"),
        };
    }
    loop {
        match drain_commands(&mut connection, commands) {
            Ok(()) => {}
            Err(err) => {
                return NetEvent::SocketError {
                    socket,
                    reason: err.to_string(),
                }
            }
        }
        match connection.read() {
            Ok(Message::Text(text)) => {
                let _ = events.send(NetEvent::SocketMessage {
                    socket,
                    text: text.as_str().to_string(),
                });
            }
            // The value model has no bytes; dropping loudly beats a silent
            // stall when a server switches to a binary protocol.
            Ok(Message::Binary(_)) => {
                tracing::warn!("websocket {socket}: binary frame dropped (text only)");
            }
            Ok(Message::Close(frame)) => {
                return NetEvent::SocketClosed {
                    socket,
                    reason: frame.map(|f| f.reason.to_string()).unwrap_or_default(),
                }
            }
            // Ping and pong are answered inside tungstenite's read.
            Ok(_) => {}
            Err(tungstenite::Error::Io(err)) if idle(&err) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return NetEvent::SocketClosed {
                    socket,
                    reason: String::new(),
                }
            }
            Err(err) => {
                return NetEvent::SocketError {
                    socket,
                    reason: err.to_string(),
                }
            }
        }
    }
}

/// A read timeout on the raw stream is what turns the blocking read into the
/// poll half of the loop.
fn read_timeout(connection: &mut Socket) -> std::io::Result<()> {
    let timeout = Some(Duration::from_millis(30));
    match connection.get_ref() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
        MaybeTlsStream::Rustls(stream) => stream.get_ref().set_read_timeout(timeout),
        _ => Ok(()),
    }
}

fn drain_commands(
    connection: &mut Socket,
    commands: &Receiver<SocketCommand>,
) -> tungstenite::Result<()> {
    loop {
        match commands.try_recv() {
            Ok(SocketCommand::SendText(text)) => connection.send(Message::Text(text.into()))?,
            // Closing twice (a script's close racing shutdown) is a no-op,
            // not a failure worth reporting.
            Ok(SocketCommand::Close) => {
                let _ = connection.close(None);
            }
            // The engine dropping its sender means shutdown; the close
            // handshake still runs so the server sees a clean goodbye.
            Err(TryRecvError::Disconnected) => {
                let _ = connection.close(None);
                return Ok(());
            }
            Err(TryRecvError::Empty) => return Ok(()),
        }
    }
}

/// A timed-out read is the loop breathing, not a failure. Which error kind
/// the OS reports for `SO_RCVTIMEO` differs by platform.
fn idle(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}
