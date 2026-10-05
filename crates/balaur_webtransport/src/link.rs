//! The worker thread: a current-thread tokio runtime bridging the async QUIC
//! session to the engine's two channels.
//!
//! One thread per link, one runtime per thread, and the runtime never leaves
//! it. The engine side sees only a command queue and `Receiver<LinkEvent>`,
//! as it does for a websocket; the tasks sleep on the queue and the session
//! until either has something.
//!
//! The reliable channel is one bidirectional stream, and a stream is bytes
//! rather than messages, so every reliable payload goes out behind a four
//! byte big-endian length. Datagrams need no framing: QUIC already delivers
//! them whole or not at all, which is the entire point of using them.

use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyhow::{Context, Result};
use balaur_core::transport::{Delivery, Received};
use web_transport_quinn::{Client, ClientBuilder, RecvStream, SendStream, ServerBuilder, Session};

use crate::queue::{Commands, Queued};
use crate::tls::Certificate;
use crate::{Accept, LinkCommand, LinkEvent, queue, reason};

/// The largest reliable payload the worker will assemble. A peer claiming
/// more is misbehaving, and the link fails rather than allocating it.
const MAX_RELIABLE: u32 = 16 * 1024 * 1024;

/// Dial a server and run the link until it closes.
pub(crate) fn dial(url: &str, accept: Accept, commands: Queued, events: &Sender<LinkEvent>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            balaur_core::replay::report(events, LinkEvent::Closed(format!("no runtime: {e}")));
            return;
        }
    };
    // A `LocalSet`: the pump's tasks hold channel ends that are not `Send`.
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        let session = match connect(url, accept).await {
            Ok(session) => session,
            Err(e) => {
                balaur_core::replay::report(events, LinkEvent::Closed(reason(&e)));
                return;
            }
        };
        // The dialling side opens the reliable stream; the accepting side
        // waits for it, so exactly one exists and both agree which.
        let stream = session.open_bi().await;
        match stream {
            Ok((send, recv)) => {
                balaur_core::replay::report(events, LinkEvent::Open);
                let ended = pump(session, send, recv, commands, events.clone()).await;
                balaur_core::replay::report(events, LinkEvent::Closed(ended));
            }
            Err(e) => {
                balaur_core::replay::report(
                    events,
                    LinkEvent::Closed(format!("no reliable stream: {e}")),
                );
            }
        }
    });
}

async fn connect(url: &str, accept: Accept) -> Result<Session> {
    let client = match accept {
        Accept::Hashes(hashes) => ClientBuilder::new()
            .with_server_certificate_hashes(hashes)
            .context("pinning the server's certificate hashes")?,
        Accept::SystemRoots => system_roots_client()?,
    };
    let url: url::Url = url.parse().with_context(|| format!("the url '{url}'"))?;
    client
        .connect(url)
        .await
        .with_context(|| String::from("connecting"))
}

/// The Mozilla roots `balaur_websocket` and ureq trust, on every platform:
/// `ClientBuilder::with_system_roots` reads a store an Android app cannot reach.
/// TLS 1.3 and `h3` are what `ClientBuilder` sets up around its own roots.
fn system_roots_client() -> Result<Client> {
    use web_transport_quinn::quinn;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut crypto = rustls::ClientConfig::builder_with_provider(
        web_transport_quinn::crypto::default_provider(),
    )
    .with_protocol_versions(&[&rustls::version::TLS13])
    .context("TLS 1.3 from the ring provider")?
    .with_root_certificates(roots)
    .with_no_client_auth();
    crypto.alpn_protocols = vec![web_transport_quinn::ALPN.as_bytes().to_vec()];
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
        .context("a QUIC config from the TLS one")?;
    let endpoint = quinn::Endpoint::client((std::net::Ipv6Addr::UNSPECIFIED, 0).into())
        .context("binding the QUIC socket")?;
    Ok(Client::new(
        endpoint,
        quinn::ClientConfig::new(std::sync::Arc::new(quic)),
    ))
}

/// Bind a server and start accepting, returning the address it landed on.
///
/// Binding happens inside the worker's runtime, because quinn needs one to
/// open its socket, and the address travels back so the caller can report a
/// real port after asking for zero. Accepting stops when `stop` resolves;
/// the thread then runs until the last peer it accepted ends.
pub(crate) fn listen(
    addr: SocketAddr,
    certificate: &Certificate,
    peers: Sender<(Receiver<LinkEvent>, Commands)>,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> Result<SocketAddr> {
    let chain = certificate.chain.clone();
    let key = certificate.key();
    // `listen` is reached only through the `replay::suppressed` guard in
    // lib.rs, so a replay opens no socket and this channel carries nothing.
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
            let serving = Serving::default();
            accept_until_dropped(&mut server, &mut stop, &peers, &serving).await;
            refuse_while_serving(&mut server, &serving).await;
        });
        // Every peer has ended; let any teardown they left finish.
        runtime.block_on(local);
    });
    match bound_rx.recv() {
        Ok(Ok(addr)) => Ok(addr),
        Ok(Err(e)) => anyhow::bail!("{e}"),
        Err(_) => anyhow::bail!("the server thread stopped before binding"),
    }
}

/// How many accepted peers are still running, and a signal each time one
/// ends.
#[derive(Default)]
struct Serving {
    live: std::rc::Rc<std::cell::Cell<usize>>,
    ended: std::rc::Rc<tokio::sync::Notify>,
}

/// Hand each peer that opens a session to the engine, until the server is
/// dropped: its stop sender goes, which resolves `stop`.
async fn accept_until_dropped(
    server: &mut web_transport_quinn::Server,
    stop: &mut tokio::sync::oneshot::Receiver<()>,
    peers: &Sender<(Receiver<LinkEvent>, Commands)>,
    serving: &Serving,
) {
    loop {
        let request = tokio::select! {
            request = server.accept() => request,
            _ = &mut *stop => return,
        };
        let Some(request) = request else {
            return;
        };
        let session = match request.ok().await {
            Ok(session) => session,
            Err(e) => {
                tracing::warn!(error = %e, "a peer failed to open a session");
                continue;
            }
        };
        // Per accepted peer, inside the guarded listener above.
        let (commands, command_rx) = queue::new();
        let (event_tx, events) = channel();
        if peers.send((events, commands)).is_err() {
            return;
        }
        // One task per peer, all on this thread's runtime: a session is
        // IO-bound and there is no work to spread.
        serving.live.set(serving.live.get() + 1);
        let (live, ended) = (serving.live.clone(), serving.ended.clone());
        tokio::task::spawn_local(async move {
            serve(session, command_rx, event_tx).await;
            live.set(live.get() - 1);
            ended.notify_one();
        });
    }
}

/// The server is dropped but its peers still run on this endpoint: answer a
/// newcomer with a refusal rather than leave it waiting on a handshake,
/// until the last peer ends.
async fn refuse_while_serving(server: &mut web_transport_quinn::Server, serving: &Serving) {
    while serving.live.get() > 0 {
        tokio::select! {
            request = server.accept() => match request {
                Some(request) => {
                    tokio::task::spawn_local(async move {
                        let unavailable = web_transport_quinn::http::StatusCode::SERVICE_UNAVAILABLE;
                        let _ = request.reject(unavailable).await;
                    });
                }
                None => return,
            },
            () = serving.ended.notified() => {}
        }
    }
}

/// The accepting side: wait for the dialler's reliable stream, then pump.
async fn serve(session: Session, commands: Queued, events: Sender<LinkEvent>) {
    match session.accept_bi().await {
        Ok((send, recv)) => {
            balaur_core::replay::report(&events, LinkEvent::Open);
            let ended = pump(session, send, recv, commands, events.clone()).await;
            balaur_core::replay::report(&events, LinkEvent::Closed(ended));
        }
        Err(e) => {
            balaur_core::replay::report(
                &events,
                LinkEvent::Closed(format!("no reliable stream: {e}")),
            );
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
    commands: Queued,
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
            let reason = write_outbound(&session, send, commands).await;
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

/// Outbound work the engine queued, asleep until there is some.
async fn write_outbound(session: &Session, mut send: SendStream, mut commands: Queued) -> String {
    while let Some(command) = commands.recv().await {
        match command {
            LinkCommand::Send(Delivery::Datagram, bytes) => {
                if let Err(e) = session.send_datagram(bytes.into()) {
                    tracing::warn!(error = %e, "a datagram was dropped");
                }
            }
            LinkCommand::Send(Delivery::Reliable, bytes) => {
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
            LinkCommand::Close => {
                session.close(0, b"closed by the engine");
                return String::from("closed by the engine");
            }
        }
    }
    String::from("the engine dropped the link")
}
