//! A QUIC link to itself: the same assertions the websocket transport passes,
//! over a transport where a datagram is genuinely a datagram.
//!
//! Nothing here proves loss or reordering, because loopback drops nothing.
//! What it proves is the seam: a caller asks for reliable or unreliable
//! delivery and gets its bytes back labelled with the promise they went out
//! under, over a real encrypted connection with a real handshake.

use balaur_core::transport::{Delivery, LinkState, Received, Transport};
use balaur_core::{App, AppConfig};
use balaur_webtransport::{Accept, WebTransportLink, WebTransportServer};

fn app() -> App {
    App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap()
}

/// Poll until both ends report open, or give up. A QUIC handshake is a round
/// trip and some TLS, so it takes real time even on loopback.
fn open_pair() -> (WebTransportServer, WebTransportLink, WebTransportLink) {
    let host = app();
    let guest = app();
    let mut server = WebTransportServer::bind(&host.engine, "127.0.0.1:0").unwrap();
    let accept = Accept::Hashes(server.certificate().hashes());
    let mut client = WebTransportLink::connect(&guest.engine, &server.url(), accept).unwrap();

    let mut accepted = None;
    for _ in 0..600 {
        if accepted.is_none() {
            accepted = server.accept().into_iter().next();
        }
        let _ = client.receive();
        if let Some(peer) = accepted.as_mut() {
            let _ = peer.receive();
            if peer.state() == LinkState::Open && client.state() == LinkState::Open {
                return (server, accepted.expect("just checked"), client);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!(
        "the link never opened: server {:?}, client {:?}",
        accepted.map(|p| p.state()),
        client.state()
    );
}

fn poll_until(link: &mut WebTransportLink, want: usize) -> Vec<Received> {
    let mut got = Vec::new();
    for _ in 0..600 {
        got.extend(link.receive());
        if got.len() >= want {
            return got;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out with {} of {want} payloads", got.len());
}

#[test]
fn a_quic_link_carries_both_deliveries_and_keeps_their_labels() {
    let (_server, mut peer, mut client) = open_pair();

    client.send_reliable(b"handshake").unwrap();
    client.send_datagram(&[0x00, 0xff, 0x10]).unwrap();
    let got = poll_until(&mut peer, 2);

    // Order between the two channels is not promised, so match by delivery.
    let reliable = got
        .iter()
        .find(|r| r.delivery == Delivery::Reliable)
        .expect("the reliable message arrived");
    assert_eq!(reliable.bytes, b"handshake".to_vec());
    let datagram = got
        .iter()
        .find(|r| r.delivery == Delivery::Datagram)
        .expect("the datagram arrived");
    assert_eq!(
        datagram.bytes,
        vec![0x00, 0xff, 0x10],
        "a datagram's bytes survive, non-UTF-8 and all"
    );
}

#[test]
fn the_reliable_channel_keeps_message_boundaries() {
    let (_server, mut peer, mut client) = open_pair();
    // A stream is bytes, not messages: without the length prefix these three
    // would arrive as one run of nine.
    for message in [b"one", b"two", b"six"] {
        client.send_reliable(message).unwrap();
    }
    let got = poll_until(&mut peer, 3);
    let messages: Vec<&[u8]> = got.iter().map(|r| r.bytes.as_slice()).collect();
    assert_eq!(messages, vec![b"one".as_slice(), b"two", b"six"]);
}

#[test]
fn both_ends_can_send() {
    let (_server, mut peer, mut client) = open_pair();
    peer.send_reliable(b"from the server").unwrap();
    let got = poll_until(&mut client, 1);
    assert_eq!(got[0].bytes, b"from the server".to_vec());
}

#[test]
fn a_datagram_over_the_path_limit_is_refused_rather_than_sent() {
    let (_server, _peer, mut client) = open_pair();
    let too_big = vec![0u8; client.max_datagram() + 1];
    assert!(client.send_datagram(&too_big).is_err());
    assert!(
        client.send_reliable(&too_big).is_ok(),
        "the reliable channel has no such limit"
    );
}

#[test]
fn sending_before_the_link_opens_is_an_error_not_a_queue() {
    let guest = app();
    let mut link = WebTransportLink::connect(
        &guest.engine,
        "https://127.0.0.1:1/",
        Accept::Hashes(Vec::new()),
    )
    .unwrap();
    assert_eq!(link.state(), LinkState::Connecting);
    assert!(
        link.send_reliable(b"early").is_err(),
        "a caller has to wait for Open rather than fire and hope"
    );
}

/// The rule every other subsystem follows: a tick the session is re-running
/// opens nothing.
#[test]
fn a_link_opened_while_resimulating_never_connects() {
    let host = app();
    let guest = app();
    let server = WebTransportServer::bind(&host.engine, "127.0.0.1:0").unwrap();
    guest
        .engine
        .resource::<balaur_core::rollback::Resimulating>()
        .borrow_mut()
        .0 = true;

    let accept = Accept::Hashes(server.certificate().hashes());
    let mut link = WebTransportLink::connect(&guest.engine, &server.url(), accept).unwrap();
    for _ in 0..20 {
        let _ = link.receive();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        link.state(),
        LinkState::Connecting,
        "no worker was started, so nothing ever opened"
    );
    assert!(link.send_reliable(b"x").is_err());
}

/// A server bound while re-simulating refuses outright, since there is no
/// "connecting" state for a listener to sit in.
#[test]
fn a_server_bound_while_resimulating_is_refused() {
    let host = app();
    host.engine
        .resource::<balaur_core::rollback::Resimulating>()
        .borrow_mut()
        .0 = true;
    assert!(WebTransportServer::bind(&host.engine, "127.0.0.1:0").is_err());
}
