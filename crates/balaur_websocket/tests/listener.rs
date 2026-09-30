use std::time::{Duration, Instant};

use balaur_core::transport::{LinkState, Received, Transport};
use balaur_core::{App, AppConfig};
use balaur_websocket::SocketOptions;
use balaur_websocket::listener::WebsocketListener;
use balaur_websocket::transport::WebsocketTransport;

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

/// A client dialled into `listener`, and the server side it accepted, both
/// open.
fn open_pair(
    client_app: &App,
    listener: &mut WebsocketListener,
) -> (WebsocketTransport, WebsocketTransport) {
    let mut client = WebsocketTransport::connect(
        &client_app.engine,
        &listener.url(),
        SocketOptions::default(),
    );
    let mut server = None;
    for _ in 0..400 {
        if let Some(p) = listener.accept().into_iter().next() {
            server = Some(p);
            break;
        }
        let _ = client.receive();
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut server = server.expect("accepted");
    for _ in 0..400 {
        let _ = server.receive();
        let _ = client.receive();
        if server.state() == LinkState::Open && client.state() == LinkState::Open {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(server.state(), LinkState::Open, "server side open");
    assert_eq!(client.state(), LinkState::Open, "client side open");
    (client, server)
}

/// Whatever `link` receives until it holds `want` arrivals, or ten seconds
/// pass.
#[allow(clippy::disallowed_methods, reason = "a test's deadline")]
fn receive(link: &mut WebsocketTransport, want: usize) -> Vec<Received> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = Vec::new();
    while got.len() < want && Instant::now() < deadline {
        got.extend(link.receive());
        std::thread::sleep(Duration::from_millis(5));
    }
    got
}

#[test]
fn a_listener_accepts_and_carries_bytes_both_ways() {
    let a = app();
    let b = app();
    let mut listener = WebsocketListener::bind(&a.engine, "127.0.0.1:0").unwrap();
    let (mut client, mut server) = open_pair(&b, &mut listener);

    client.send_datagram(b"ping").unwrap();
    let got = receive(&mut server, 1);
    assert_eq!(got.len(), 1, "server received the client's datagram");
    assert_eq!(got[0].bytes, b"ping".to_vec());

    server.send_reliable(b"pong").unwrap();
    let back = receive(&mut client, 1);
    assert_eq!(back.len(), 1, "client received the server's reply");
    assert_eq!(back[0].bytes, b"pong".to_vec());
}

#[test]
fn a_message_larger_than_the_socket_buffers_arrives_whole() {
    let a = app();
    let b = app();
    let mut listener = WebsocketListener::bind(&a.engine, "127.0.0.1:0").unwrap();
    let (mut client, mut server) = open_pair(&b, &mut listener);
    // Far past what a loopback socket buffers, so the sender's writes stop
    // part way and resume when the socket drains.
    let big: Vec<u8> = (0..16 * 1024 * 1024)
        .map(|at: u32| (at % 251) as u8)
        .collect();
    client.send_reliable(&big).unwrap();
    client.send_reliable(b"after").unwrap();
    let got = receive(&mut server, 2);
    assert_eq!(got.len(), 2, "both messages arrived");
    assert!(got[0].bytes == big, "the big message arrived intact");
    assert_eq!(got[1].bytes, b"after".to_vec(), "and in order");
}

#[test]
#[allow(clippy::disallowed_methods, reason = "a test's deadline")]
fn a_dropped_listener_stops_accepting() {
    let a = app();
    let listener = WebsocketListener::bind(&a.engine, "127.0.0.1:0").unwrap();
    let addr = listener.addr();
    drop(listener);
    let deadline = Instant::now() + Duration::from_secs(5);
    let refused = loop {
        if std::net::TcpStream::connect(addr).is_err() {
            break true;
        }
        if Instant::now() > deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        refused,
        "the port still accepts after the listener was dropped"
    );
}
