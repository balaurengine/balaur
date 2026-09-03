//! The worker thread: a current-thread tokio runtime bridging the async QUIC
//! session to the engine's two channels.
//!
//! One thread per link, one runtime per thread, and the runtime never leaves
//! it. The engine side sees only `Sender<LinkCommand>` and
//! `Receiver<LinkEvent>`, exactly as it does for a websocket.
//!
//! The reliable channel is one bidirectional stream, and a stream is bytes
//! rather than messages, so every reliable payload goes out behind a four
//! byte big-endian length. Datagrams need no framing: QUIC already delivers
//! them whole or not at all, which is the entire point of using them.

use std::net::SocketAddr;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::Duration;

use anyhow::{Context, Result};
use balaur_core::transport::{Delivery, Received};
use web_transport_quinn::{ClientBuilder, RecvStream, SendStream, ServerBuilder, Session};

use crate::tls::Certificate;
use crate::{reason, Accept, LinkCommand, LinkEvent};

/// How often the worker looks for outbound commands. The engine queues at
/// most one datagram a tick, so anything under a frame is invisible; this is
/// well under.
const COMMAND_POLL: Duration = Duration::from_millis(1);

/// The largest reliable payload the worker will assemble. A peer claiming
/// more is misbehaving, and the link fails rather than allocating it.
const MAX_RELIABLE: u32 = 16 * 1024 * 1024;

/// Dial a server and run the link until it closes.
pub(crate) fn dial(
    url: &str,
    accept: Accept,
    commands: Receiver<LinkCommand>,
    events: Sender<LinkEvent>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            let _ = events.send(LinkEvent::Closed(format!("no runtime: {e}")));
            return;
        }
    };
    // A `LocalSet`: the pump's tasks hold channel ends that are not `Send`.
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        let session = match connect(url, accept).await {
            Ok(session) => session,
            Err(e) => {
                let _ = events.send(LinkEvent::Closed(reason(&e)));
                return;
            }
        };
        // The dialling side opens the reliable stream; the accepting side
        // waits for it, so exactly one exists and both agree which.
        let stream = session.open_bi().await;
        match stream {
            Ok((send, recv)) => {
                let _ = events.send(LinkEvent::Open);
                let ended = pump(session, send, recv, commands, events.clone()).await;
                let _ = events.send(LinkEvent::Closed(ended));
            }
            Err(e) => {
                let _ = events.send(LinkEvent::Closed(format!("no reliable stream: {e}")));
            }
        }
    });
}

async fn connect(url: &str, accept: Accept) -> Result<Session> {
    let client = match accept {
        Accept::Hashes(hashes) => ClientBuilder::new()
            .with_server_certificate_hashes(hashes)
            .context("pinning the server's certificate hashes")?,
        Accept::SystemRoots => ClientBuilder::new()
            .with_system_roots()
            .context("loading the system root certificates")?,
    };
    let url: url::Url = url.parse().with_context(|| format!("the url '{url}'"))?;
    client
        .connect(url)
        .await
        .with_context(|| String::from("connecting"))
}

/// Bind a server and start accepting, returning the address it landed on.
///
/// Binding happens inside the worker's runtime, because quinn needs one to
/// open its socket, and the address travels back so the caller can report a
/// real port after asking for zero.
pub(crate) fn listen(
    addr: SocketAddr,
    certificate: &Certificate,
    peers: Sender<(Receiver<LinkEvent>, Sender<LinkCommand>)>,
) -> Result<SocketAddr> {
    let chain = certificate.chain.clone();
    let key = certificate.key();
    let (bound_tx, bound_rx) = channel();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(e) => {
                let _ = bound_tx.send(Err(format!("no runtime: {e}")));
                return;
            }
        };
        // A `LocalSet`: a peer's task holds channel ends that are not `Send`,
        // and `spawn_local` outside a set panics rather than failing to build.
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async move {
            let server = ServerBuilder::new()
                .with_addr(addr)
                .with_certificate(chain, key);
            let mut server = match server {
                Ok(server) => server,
                Err(e) => {
                    let _ = bound_tx.send(Err(format!("binding: {e}")));
                    return;
                }
            };
            let local = match server.local_addr() {
                Ok(local) => local,
                Err(e) => {
                    let _ = bound_tx.send(Err(format!("reading the bound address: {e}")));
                    return;
                }
            };
            if bound_tx.send(Ok(local)).is_err() {
                return;
            }
            while let Some(request) = server.accept().await {
                let session = match request.ok().await {
                    Ok(session) => session,
                    Err(e) => {
                        tracing::warn!(error = %e, "a peer failed to open a session");
                        continue;
                    }
                };
                let (commands, command_rx) = channel();
                let (event_tx, events) = channel();
                if peers.send((events, commands)).is_err() {
                    // The engine dropped the server; stop accepting.
                    return;
                }
                // One task per peer, all on this thread's runtime: a session
                // is IO-bound and there is no work to spread.
                tokio::task::spawn_local(async move {
                    serve(session, command_rx, event_tx).await;
                });
            }
        });
    });
    match bound_rx.recv() {
        Ok(Ok(addr)) => Ok(addr),
        Ok(Err(e)) => anyhow::bail!("{e}"),
        Err(_) => anyhow::bail!("the server thread stopped before binding"),
    }
}

/// The accepting side: wait for the dialler's reliable stream, then pump.
async fn serve(session: Session, commands: Receiver<LinkCommand>, events: Sender<LinkEvent>) {
    match session.accept_bi().await {
        Ok((send, recv)) => {
            let _ = events.send(LinkEvent::Open);
            let ended = pump(session, send, recv, commands, events.clone()).await;
            let _ = events.send(LinkEvent::Closed(ended));
        }
        Err(e) => {
            let _ = events.send(LinkEvent::Closed(format!("no reliable stream: {e}")));
        }
    }
}

/// Move payloads both ways until something ends the link, and say what did.
///
/// Three tasks rather than one `select!`, because `read_exact` is not
/// cancel-safe: a `select!` that drops it half way through a message loses
/// the bytes it had already taken off the stream, and every message after it
/// is framed against the wrong offset. Loopback hides this — a small message
/// arrives whole — and a fragmented one on a real link would not.
async fn pump(
    session: Session,
    send: SendStream,
    recv: RecvStream,
    commands: Receiver<LinkCommand>,
    events: Sender<LinkEvent>,
) -> String {
    // Whichever task ends first says why; the rest are then torn down.
    let (done, mut ended) = tokio::sync::mpsc::channel::<String>(3);

    let datagrams = {
        let session = session.clone();
        let events = events.clone();
        let done = done.clone();
        tokio::task::spawn_local(async move {
            let reason = read_datagrams(&session, &events).await;
            let _ = done.send(reason).await;
        })
    };
    let reliable = {
        let events = events.clone();
        let done = done.clone();
        tokio::task::spawn_local(async move {
            let reason = read_reliable(recv, &events).await;
            let _ = done.send(reason).await;
        })
    };
    let outbound = {
        let session = session.clone();
        tokio::task::spawn_local(async move {
            let reason = write_outbound(&session, send, &commands).await;
            let _ = done.send(reason).await;
        })
    };

    let reason = ended
        .recv()
        .await
        .unwrap_or_else(|| String::from("the link ended"));
    datagrams.abort();
    reliable.abort();
    outbound.abort();
    reason
}

/// Datagrams, whole or not at all — no framing, which is half the reason to
/// use them.
async fn read_datagrams(session: &Session, events: &Sender<LinkEvent>) -> String {
    loop {
        match session.read_datagram().await {
            Ok(bytes) => {
                let payload = Received {
                    delivery: Delivery::Datagram,
                    bytes: bytes.to_vec(),
                };
                if events.send(LinkEvent::Payload(payload)).is_err() {
                    return String::from("the engine dropped the link");
                }
            }
            Err(e) => return format!("the session ended: {e}"),
        }
    }
}

/// Length-prefixed messages off the one reliable stream. Nothing cancels
/// these reads, so a message half-arrived stays half-arrived until the rest
/// of it turns up.
async fn read_reliable(mut recv: RecvStream, events: &Sender<LinkEvent>) -> String {
    let mut length = [0u8; 4];
    loop {
        if let Err(e) = recv.read_exact(&mut length).await {
            return format!("the reliable stream ended: {e}");
        }
        let want = u32::from_be_bytes(length);
        if want > MAX_RELIABLE {
            return format!("a peer announced a {want} byte message");
        }
        let mut bytes = vec![0u8; want as usize];
        if let Err(e) = recv.read_exact(&mut bytes).await {
            return format!("a reliable message was cut short: {e}");
        }
        let payload = Received {
            delivery: Delivery::Reliable,
            bytes,
        };
        if events.send(LinkEvent::Payload(payload)).is_err() {
            return String::from("the engine dropped the link");
        }
    }
}

/// Outbound work the engine queued. The command channel is the engine's
/// synchronous one, so this polls it rather than awaiting it.
async fn write_outbound(
    session: &Session,
    mut send: SendStream,
    commands: &Receiver<LinkCommand>,
) -> String {
    loop {
        match commands.try_recv() {
            Ok(LinkCommand::Send(Delivery::Datagram, bytes)) => {
                if let Err(e) = session.send_datagram(bytes.into()) {
                    tracing::warn!(error = %e, "a datagram was dropped");
                }
            }
            Ok(LinkCommand::Send(Delivery::Reliable, bytes)) => {
                let Ok(len) = u32::try_from(bytes.len()) else {
                    tracing::warn!("a reliable message was too long to frame");
                    continue;
                };
                if let Err(e) = send.write_all(&len.to_be_bytes()).await {
                    return format!("the reliable stream closed: {e}");
                }
                if let Err(e) = send.write_all(&bytes).await {
                    return format!("the reliable stream closed: {e}");
                }
            }
            Ok(LinkCommand::Close) => {
                session.close(0, b"closed by the engine");
                return String::from("closed by the engine");
            }
            Err(TryRecvError::Empty) => tokio::time::sleep(COMMAND_POLL).await,
            Err(TryRecvError::Disconnected) => return String::from("the engine dropped the link"),
        }
    }
}
