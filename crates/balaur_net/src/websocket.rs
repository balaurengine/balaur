//! The websocket worker: one thread per connection, over raw frames.
//!
//! tungstenite does the upgrade request and the frame codec; the protocol
//! loop lives here because `permessage-deflate` (RFC 7692) sets a reserved
//! bit that tungstenite's own `WebSocket` refuses to read. The thread
//! alternates between draining the engine's outbound commands and a `read`
//! with a short timeout, so a single blocking socket serves both directions
//! without an async runtime. Worst-case send latency is one read timeout,
//! well under a frame's budget for game traffic.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};
use tungstenite::client::{uri_mode, IntoClientRequest};
use tungstenite::handshake::client::generate_request;
use tungstenite::handshake::derive_accept_key;
use tungstenite::http::header::{HeaderName, HeaderValue};
use tungstenite::protocol::frame::coding::{Control, Data, OpCode};
use tungstenite::protocol::frame::{Frame, FrameHeader, FrameSocket};
use tungstenite::stream::{MaybeTlsStream, Mode};

use crate::{NetEvent, SocketCommand, SocketOptions};

type Socket = FrameSocket<MaybeTlsStream<TcpStream>>;

/// A message past this many bytes is a protocol failure, not game traffic.
const MAX_MESSAGE: usize = 64 * 1024 * 1024;
/// The empty block a sync flush ends on; stripped when sending, restored
/// when receiving (RFC 7692 §7.2.1).
const DEFLATE_TAIL: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

pub(crate) fn spawn_socket(
    socket: u64,
    url: String,
    options: SocketOptions,
    commands: Receiver<SocketCommand>,
    events: &Sender<NetEvent>,
) {
    let events = events.clone();
    std::thread::spawn(move || {
        // The handshake blocks — DNS, TCP, TLS — which is exactly why it
        // happens here and not in the `websocket.connect` binding.
        let (connection, deflate) = match open(&url, &options) {
            Ok(opened) => opened,
            Err(err) => {
                let _ = events.send(NetEvent::SocketError {
                    socket,
                    reason: format!("{err:#}"),
                });
                return;
            }
        };
        let _ = events.send(NetEvent::SocketOpen { socket });
        let event = run(socket, connection, deflate, &commands, &events);
        let _ = events.send(event);
    });
}

/// Connect, upgrade, and negotiate compression; the frames start after.
fn open(url: &str, options: &SocketOptions) -> Result<(Socket, Option<Deflate>)> {
    let mut request = url
        .into_client_request()
        .with_context(|| format!("websocket url '{url}'"))?;
    for (name, value) in &options.headers {
        request.headers_mut().insert(
            HeaderName::from_bytes(name.as_bytes()).with_context(|| format!("header `{name}`"))?,
            HeaderValue::from_str(value).with_context(|| format!("header `{name}`"))?,
        );
    }
    if options.compression {
        // No `client_max_window_bits`: the default backend has one window
        // size, so the server must not be invited to pick a smaller one.
        request.headers_mut().insert(
            "Sec-WebSocket-Extensions",
            HeaderValue::from_static("permessage-deflate"),
        );
    }
    let host = request
        .uri()
        .host()
        .ok_or_else(|| anyhow!("no host in '{url}'"))?
        .to_string();
    let mode = uri_mode(request.uri())?;
    let port = request
        .uri()
        .port_u16()
        .unwrap_or(if matches!(mode, Mode::Tls) { 443 } else { 80 });
    let tcp = TcpStream::connect((host.as_str(), port))
        .with_context(|| format!("connecting to {host}:{port}"))?;
    tcp.set_nodelay(true)?;
    let mut stream = match mode {
        Mode::Tls => MaybeTlsStream::Rustls(tls(tcp, &host)?),
        Mode::Plain => MaybeTlsStream::Plain(tcp),
    };
    let (bytes, key) = generate_request(request)?;
    stream.write_all(&bytes)?;
    let (head, tail) = read_head(&mut stream)?;
    let negotiated = verify(&head, &key, options.compression)?;
    Ok((
        FrameSocket::from_partially_read(stream, tail),
        negotiated.map(Deflate::new),
    ))
}

fn tls(
    tcp: TcpStream,
    host: &str,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .with_context(|| format!("'{host}' is not a valid server name"))?;
    let connection = rustls::ClientConnection::new(Arc::new(config), name)?;
    Ok(rustls::StreamOwned::new(connection, tcp))
}

/// The upgrade response up to its blank line, and whatever the server sent
/// after it — an eager server's first frame, which must not be lost.
fn read_head(stream: &mut impl Read) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            let tail = buffer.split_off(end + 4);
            return Ok((buffer, tail));
        }
        if buffer.len() > 64 * 1024 {
            bail!("the handshake response never ended");
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            bail!("the server closed the connection during the handshake");
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

/// Check the upgrade and read back what the server agreed to.
fn verify(head: &[u8], key: &str, offered_deflate: bool) -> Result<Option<DeflateParams>> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    response
        .parse(head)
        .context("parsing the handshake response")?;
    let code = response.code.unwrap_or(0);
    if code != 101 {
        bail!("the server answered {code} instead of switching protocols");
    }
    let header = |name: &str| -> Vec<String> {
        response
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| String::from_utf8_lossy(h.value).into_owned())
            .collect()
    };
    let expected = derive_accept_key(key.as_bytes());
    if header("sec-websocket-accept").first().map(|v| v.trim()) != Some(expected.as_str()) {
        bail!("the server's Sec-WebSocket-Accept does not match");
    }
    deflate_params(
        &header("sec-websocket-extensions").join(","),
        offered_deflate,
    )
}

/// What `permessage-deflate` was accepted with, if it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeflateParams {
    /// Whether this side keeps its compression window between messages.
    pub client_takeover: bool,
    pub server_takeover: bool,
}

/// Parse a `Sec-WebSocket-Extensions` response value (RFC 7692 §7.1).
pub(crate) fn deflate_params(value: &str, offered: bool) -> Result<Option<DeflateParams>> {
    let mut params = None;
    for extension in value.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let mut parts = extension.split(';').map(str::trim);
        let name = parts.next().unwrap_or_default();
        if name != "permessage-deflate" {
            bail!("the server accepted an extension that was not offered: {name}");
        }
        if !offered {
            bail!("the server accepted permessage-deflate, which was not offered");
        }
        let mut accepted = DeflateParams {
            client_takeover: true,
            server_takeover: true,
        };
        for param in parts {
            let (key, _value) = param.split_once('=').unwrap_or((param, ""));
            match key.trim() {
                "server_no_context_takeover" => accepted.server_takeover = false,
                "client_no_context_takeover" => accepted.client_takeover = false,
                // Any window the server keeps fits inside ours.
                "server_max_window_bits" => {}
                other => bail!("permessage-deflate parameter `{other}` was not offered"),
            }
        }
        params = Some(accepted);
    }
    Ok(params)
}

/// One connection's compression state: a window in each direction, kept
/// across messages unless the negotiation said otherwise.
pub(crate) struct Deflate {
    compress: Compress,
    decompress: Decompress,
    params: DeflateParams,
}

impl Deflate {
    pub(crate) fn new(params: DeflateParams) -> Self {
        Self {
            compress: Compress::new(Compression::default(), false),
            decompress: Decompress::new(false),
            params,
        }
    }

    /// One message's payload, compressed as a raw deflate stream with its
    /// sync-flush tail removed.
    pub(crate) fn deflate(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(input.len() / 2 + 64);
        let mut consumed = 0;
        loop {
            if out.len() == out.capacity() {
                out.reserve(out.len().max(256));
            }
            let before = self.compress.total_in();
            self.compress
                .compress_vec(&input[consumed..], &mut out, FlushCompress::Sync)?;
            consumed += usize::try_from(self.compress.total_in() - before)?;
            // A sync flush is complete once it stops filling the buffer.
            if consumed == input.len() && out.len() < out.capacity() {
                break;
            }
        }
        if out.ends_with(&DEFLATE_TAIL) {
            out.truncate(out.len() - DEFLATE_TAIL.len());
        }
        if !self.params.client_takeover {
            self.compress.reset();
        }
        Ok(out)
    }

    /// One received payload, inflated; the stripped tail goes back on first.
    pub(crate) fn inflate(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut input = Vec::with_capacity(payload.len() + DEFLATE_TAIL.len());
        input.extend_from_slice(payload);
        input.extend_from_slice(&DEFLATE_TAIL);
        let mut out = Vec::with_capacity(input.len() * 2 + 64);
        let mut consumed = 0;
        loop {
            if out.len() == out.capacity() {
                if out.len() >= MAX_MESSAGE {
                    bail!("a compressed message inflated past {MAX_MESSAGE} bytes");
                }
                out.reserve(out.len().max(256));
            }
            let before = self.decompress.total_in();
            self.decompress
                .decompress_vec(&input[consumed..], &mut out, FlushDecompress::Sync)?;
            consumed += usize::try_from(self.decompress.total_in() - before)?;
            if consumed == input.len() && out.len() < out.capacity() {
                break;
            }
        }
        if !self.params.server_takeover {
            self.decompress.reset(false);
        }
        Ok(out)
    }
}

/// A message being assembled from its frames.
#[derive(Default)]
struct Inbox {
    open: bool,
    text: bool,
    compressed: bool,
    bytes: Vec<u8>,
}

/// One complete message: whether it was text, whether it came compressed,
/// and its bytes as they arrived.
struct Incoming {
    text: bool,
    compressed: bool,
    bytes: Vec<u8>,
}

impl Inbox {
    fn push(
        &mut self,
        kind: Data,
        header: &FrameHeader,
        payload: &[u8],
    ) -> Result<Option<Incoming>> {
        match kind {
            Data::Continue => {
                if !self.open {
                    bail!("a continuation frame arrived with no message open");
                }
                if header.rsv1 {
                    bail!("a continuation frame carried the compression bit");
                }
            }
            Data::Text | Data::Binary => {
                if self.open {
                    bail!("a new message started before the previous one ended");
                }
                self.open = true;
                self.text = kind == Data::Text;
                self.compressed = header.rsv1;
                self.bytes.clear();
            }
            Data::Reserved(code) => bail!("reserved data opcode {code}"),
        }
        if header.rsv2 || header.rsv3 {
            bail!("a frame set reserved bits no extension defines");
        }
        if self.bytes.len() + payload.len() > MAX_MESSAGE {
            bail!("a message ran past {MAX_MESSAGE} bytes");
        }
        self.bytes.extend_from_slice(payload);
        if !header.is_final {
            return Ok(None);
        }
        self.open = false;
        Ok(Some(Incoming {
            text: self.text,
            compressed: self.compressed,
            bytes: std::mem::take(&mut self.bytes),
        }))
    }
}

/// Serve one open connection until it ends, returning the closing event.
fn run(
    socket: u64,
    mut connection: Socket,
    mut deflate: Option<Deflate>,
    commands: &Receiver<SocketCommand>,
    events: &Sender<NetEvent>,
) -> NetEvent {
    let failed = |reason: String| NetEvent::SocketError { socket, reason };
    if let Err(err) = read_timeout(&connection) {
        return failed(format!("no read timeout: {err}"));
    }
    let mut inbox = Inbox::default();
    let mut closing = false;
    loop {
        if let Err(err) = drain_commands(&mut connection, &mut deflate, &mut closing, commands) {
            return failed(format!("{err:#}"));
        }
        match connection.read(Some(MAX_MESSAGE)) {
            Ok(Some(frame)) => match handle(
                socket,
                &mut connection,
                frame,
                &mut inbox,
                &mut deflate,
                closing,
                events,
            ) {
                Ok(None) => {}
                Ok(Some(event)) => return event,
                Err(err) => return failed(format!("{err:#}")),
            },
            // End of stream: a server that hung up after (or without) a
            // close frame.
            Ok(None) => {
                return NetEvent::SocketClosed {
                    socket,
                    reason: String::new(),
                }
            }
            Err(tungstenite::Error::Io(err)) if idle(&err) => {}
            Err(err) => return failed(err.to_string()),
        }
    }
}

/// One frame from the server, answered or assembled; `Some` when the
/// connection is over.
fn handle(
    socket: u64,
    connection: &mut Socket,
    frame: Frame,
    inbox: &mut Inbox,
    deflate: &mut Option<Deflate>,
    closing: bool,
    events: &Sender<NetEvent>,
) -> Result<Option<NetEvent>> {
    let header = frame.header().clone();
    match header.opcode {
        OpCode::Control(Control::Ping) => send(connection, Frame::pong(frame.into_payload()))?,
        OpCode::Control(Control::Pong) => {}
        OpCode::Control(Control::Close) => {
            let reason = close_reason(frame.payload());
            if !closing {
                let _ = send(connection, Frame::close(None));
            }
            return Ok(Some(NetEvent::SocketClosed { socket, reason }));
        }
        OpCode::Control(Control::Reserved(code)) => bail!("reserved control opcode {code}"),
        OpCode::Data(kind) => {
            if let Some(message) = inbox.push(kind, &header, frame.payload())? {
                deliver(socket, message, deflate, events)?;
            }
        }
    }
    Ok(None)
}

/// A finished message to the engine thread, inflated if it came compressed.
fn deliver(
    socket: u64,
    message: Incoming,
    deflate: &mut Option<Deflate>,
    events: &Sender<NetEvent>,
) -> Result<()> {
    let bytes = if message.compressed {
        match deflate {
            Some(deflate) => deflate.inflate(&message.bytes)?,
            None => bail!("a compressed frame arrived on a connection without compression"),
        }
    } else {
        message.bytes
    };
    let event = if message.text {
        let text = String::from_utf8(bytes).context("a text frame was not UTF-8")?;
        NetEvent::SocketMessage { socket, text }
    } else {
        NetEvent::SocketBinary { socket, bytes }
    };
    let _ = events.send(event);
    Ok(())
}

/// The reason in a close frame: a two-byte code, then optional UTF-8.
fn close_reason(payload: &[u8]) -> String {
    payload
        .get(2..)
        .map(|reason| String::from_utf8_lossy(reason).into_owned())
        .unwrap_or_default()
}

/// A read timeout on the raw stream is what turns the blocking read into the
/// poll half of the loop.
fn read_timeout(connection: &Socket) -> std::io::Result<()> {
    let timeout = Some(Duration::from_millis(30));
    match connection.get_ref() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
        MaybeTlsStream::Rustls(stream) => stream.get_ref().set_read_timeout(timeout),
        _ => Ok(()),
    }
}

fn drain_commands(
    connection: &mut Socket,
    deflate: &mut Option<Deflate>,
    closing: &mut bool,
    commands: &Receiver<SocketCommand>,
) -> Result<()> {
    loop {
        match commands.try_recv() {
            Ok(SocketCommand::SendText(text)) => {
                send_message(connection, deflate, text.into_bytes(), Data::Text)?;
            }
            Ok(SocketCommand::SendBytes(bytes)) => {
                send_message(connection, deflate, bytes, Data::Binary)?;
            }
            // Closing twice (a script's close racing shutdown) is a no-op,
            // not a failure worth reporting.
            Ok(SocketCommand::Close) => request_close(connection, closing),
            // The engine dropping its sender means shutdown; the close
            // handshake still runs so the server sees a clean goodbye.
            Err(TryRecvError::Disconnected) => {
                request_close(connection, closing);
                return Ok(());
            }
            Err(TryRecvError::Empty) => return Ok(()),
        }
    }
}

fn request_close(connection: &mut Socket, closing: &mut bool) {
    if !*closing {
        *closing = true;
        let _ = send(connection, Frame::close(None));
    }
}

/// One message, compressed when the connection negotiated it.
fn send_message(
    connection: &mut Socket,
    deflate: &mut Option<Deflate>,
    bytes: Vec<u8>,
    kind: Data,
) -> Result<()> {
    let (payload, compressed) = match deflate {
        Some(deflate) => (deflate.deflate(&bytes)?, true),
        None => (bytes, false),
    };
    let mut frame = Frame::message(payload, OpCode::Data(kind), true);
    frame.header_mut().rsv1 = compressed;
    send(connection, frame)?;
    Ok(())
}

/// Every client frame is masked (RFC 6455 §5.3); the codec applies it.
#[allow(
    clippy::disallowed_methods,
    reason = "the mask must be unpredictable per RFC 6455; it never reaches simulation"
)]
fn send(connection: &mut Socket, mut frame: Frame) -> tungstenite::Result<()> {
    frame.header_mut().mask = Some(rand::random());
    connection.send(frame)
}

/// A timed-out read is the loop breathing, not a failure. Which error kind
/// the OS reports for `SO_RCVTIMEO` differs by platform.
fn idle(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_round_trips_through_one_side_and_back() {
        let params = DeflateParams {
            client_takeover: true,
            server_takeover: true,
        };
        let mut a = Deflate::new(params);
        let mut b = Deflate::new(params);
        let text = "hello ".repeat(200);
        let packed = a.deflate(text.as_bytes()).unwrap();
        assert!(packed.len() < text.len() / 4, "{} bytes", packed.len());
        assert!(!packed.ends_with(&DEFLATE_TAIL));
        // The other side inflates with its own window; a second message
        // rides the first one's context.
        assert_eq!(b.inflate(&packed).unwrap(), text.as_bytes());
        let again = a.deflate(text.as_bytes()).unwrap();
        assert!(again.len() < packed.len());
        assert_eq!(b.inflate(&again).unwrap(), text.as_bytes());
    }

    #[test]
    fn no_context_takeover_resets_the_window_each_message() {
        let params = DeflateParams {
            client_takeover: false,
            server_takeover: false,
        };
        let mut a = Deflate::new(params);
        let mut b = Deflate::new(params);
        let text = "abc ".repeat(100);
        let first = a.deflate(text.as_bytes()).unwrap();
        let second = a.deflate(text.as_bytes()).unwrap();
        assert_eq!(
            first, second,
            "a fresh window compresses the same bytes the same way"
        );
        assert_eq!(b.inflate(&first).unwrap(), text.as_bytes());
        assert_eq!(b.inflate(&second).unwrap(), text.as_bytes());
    }

    #[test]
    fn the_extension_header_is_read_as_the_rfc_says() {
        assert_eq!(deflate_params("", true).unwrap(), None);
        let plain = deflate_params("permessage-deflate", true).unwrap().unwrap();
        assert!(plain.client_takeover && plain.server_takeover);
        let strict = deflate_params(
            "permessage-deflate; server_no_context_takeover; client_no_context_takeover; server_max_window_bits=10",
            true,
        )
        .unwrap()
        .unwrap();
        assert!(!strict.client_takeover && !strict.server_takeover);
        assert!(deflate_params("permessage-deflate", false).is_err());
        assert!(deflate_params("permessage-deflate; client_max_window_bits=8", true).is_err());
        assert!(deflate_params("x-webkit-deflate-frame", true).is_err());
    }
}
