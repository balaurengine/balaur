//! The transport trait over a websocket, against a loopback echo server.
//!
//! What is being checked is the seam, not the socket: a caller asks for
//! reliable or unreliable delivery, and gets its bytes back labelled with the
//! promise they were sent under. The websocket makes both reliable, which is
//! allowed and is why the datagram test asserts the payload and the label
//! rather than any loss.

use std::net::TcpListener;

use balaur_core::transport::{Delivery, LinkState, Received, Transport};
use balaur_core::{App, AppConfig};
use balaur_websocket::transport::WebsocketTransport;
use balaur_websocket::SocketOptions;

fn app() -> App {
    App::new(AppConfig::bare("."))
    .unwrap()
}

/// Echo every binary frame back untouched, so whatever the transport framed
/// on the way out is what it has to parse on the way in.
fn serve_echo() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut connection = tungstenite::accept(stream).unwrap();
            loop {
                match connection.read() {
                    Ok(message) if message.is_binary() => {
                        let _ = connection.send(message);
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    });
    format!("ws://{addr}")
}

/// Poll until `want` payloads have come back, or give up. Wall-clock bounded
/// because a real socket takes real time.
fn poll_until(link: &mut WebsocketTransport, want: usize) -> Vec<Received> {
    let mut got = Vec::new();
    for _ in 0..1000 {
        got.extend(link.receive());
        if got.len() >= want {
            return got;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("timed out with {} of {want} payloads", got.len());
}

fn open_link(app: &App, url: &str) -> WebsocketTransport {
    let mut link = WebsocketTransport::connect(&app.engine, url, SocketOptions::default());
    for _ in 0..1000 {
        let _ = link.receive();
        if link.state() == LinkState::Open {
            return link;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the link never opened");
}

#[test]
fn both_deliveries_round_trip_and_keep_their_labels() {
    let url = serve_echo();
    let app = app();
    let mut link = open_link(&app, &url);

    link.send_reliable(b"handshake").unwrap();
    link.send_datagram(&[0x00, 0xff, 0x10]).unwrap();
    let got = poll_until(&mut link, 2);

    assert_eq!(
        got[0],
        Received {
            delivery: Delivery::Reliable,
            bytes: b"handshake".to_vec()
        }
    );
    assert_eq!(
        got[1],
        Received {
            delivery: Delivery::Datagram,
            bytes: vec![0x00, 0xff, 0x10]
        },
        "a datagram's bytes survive, non-UTF-8 and all"
    );
}

#[test]
fn a_datagram_over_the_size_limit_is_refused_rather_than_sent() {
    let url = serve_echo();
    let app = app();
    let mut link = open_link(&app, &url);
    let too_big = vec![0u8; link.max_datagram() + 1];
    assert!(link.send_datagram(&too_big).is_err());
    assert!(
        link.send_reliable(&too_big).is_ok(),
        "the reliable channel has no such limit"
    );
}

#[test]
fn sending_before_the_link_opens_is_an_error_not_a_queue() {
    let app = app();
    let mut link =
        WebsocketTransport::connect(&app.engine, "ws://127.0.0.1:1", SocketOptions::default());
    assert_eq!(link.state(), LinkState::Connecting);
    assert!(
        link.send_reliable(b"early").is_err(),
        "a caller has to wait for Open rather than fire and hope"
    );
}

/// The rule `ExternalIo` enforces for every other subsystem: a tick the
/// session is re-running opens no socket.
#[test]
fn a_link_opened_while_resimulating_never_connects() {
    let url = serve_echo();
    let app = app();
    app.engine
        .resource::<balaur_core::rollback::Resimulating>()
        .borrow_mut()
        .0 = true;

    let mut link = WebsocketTransport::connect(&app.engine, &url, SocketOptions::default());
    for _ in 0..20 {
        let _ = link.receive();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        link.state(),
        LinkState::Connecting,
        "no worker was started, so nothing ever opened"
    );
    assert!(link.send_reliable(b"x").is_err());
}
