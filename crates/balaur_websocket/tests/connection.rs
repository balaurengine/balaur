//! `Connection`, the websocket Rust code holds: both kinds of message both
//! ways, and every way a connection ends or never starts.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use balaur_core::transport::LinkState;
use balaur_core::{App, AppConfig};
use balaur_websocket::SocketOptions;
use balaur_websocket::connection::{Arrival, Connection};
use balaur_websocket::listener::WebsocketListener;

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

/// Everything `connection` receives until `done` holds, or ten seconds pass.
#[allow(clippy::disallowed_methods, reason = "a test's deadline")]
fn receive_until(connection: &mut Connection, done: impl Fn(&[Arrival]) -> bool) -> Vec<Arrival> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = Vec::new();
    while !done(&got) && Instant::now() < deadline {
        got.extend(connection.receive());
        std::thread::sleep(Duration::from_millis(5));
    }
    got
}

fn opened(got: &[Arrival]) -> bool {
    got.contains(&Arrival::Opened)
}

/// A dialled connection and the one the listener accepted for it, both open.
fn open_pair(client_app: &App, listener: &mut WebsocketListener) -> (Connection, Connection) {
    let mut client = Connection::connect(
        &client_app.engine,
        &listener.url(),
        SocketOptions::default(),
    );
    let mut server = None;
    for _ in 0..400 {
        server = listener.accept().into_iter().next();
        if server.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut server = server.expect("the listener accepted the peer");
    assert!(
        opened(&receive_until(&mut client, opened)),
        "the client opened"
    );
    assert!(
        opened(&receive_until(&mut server, opened)),
        "the server side opened"
    );
    (client, server)
}

/// A port that answers the upgrade with `status` and hangs up.
fn serve_refusal(status: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let answer = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    url
}

#[test]
fn a_connection_carries_text_and_binary_both_ways() {
    let a = app();
    let b = app();
    let mut listener = WebsocketListener::bind(&a.engine, "127.0.0.1:0").unwrap();
    let (mut client, mut server) = open_pair(&b, &mut listener);

    client.send_text("hello").unwrap();
    client.send_bytes(vec![0, 159, 146, 150]).unwrap();
    let got = receive_until(&mut server, |got| got.len() >= 2);
    assert_eq!(
        got,
        vec![
            Arrival::Text(String::from("hello")),
            Arrival::Binary(vec![0, 159, 146, 150])
        ],
        "the server heard both, in order and as sent"
    );

    server.send_text("back").unwrap();
    let got = receive_until(&mut client, |got| !got.is_empty());
    assert_eq!(got, vec![Arrival::Text(String::from("back"))]);
    assert_eq!(client.state(), LinkState::Open);
}

#[test]
fn a_close_reaches_the_peer_and_both_sides_end() {
    let a = app();
    let b = app();
    let mut listener = WebsocketListener::bind(&a.engine, "127.0.0.1:0").unwrap();
    let (mut client, mut server) = open_pair(&b, &mut listener);

    client.close();
    let closed = |got: &[Arrival]| got.iter().any(|a| matches!(a, Arrival::Closed { .. }));
    let heard = receive_until(&mut server, closed);
    assert!(
        heard.contains(&Arrival::Closed {
            code: 1000,
            reason: String::from("bye")
        }),
        "the server heard a normal close: {heard:?}"
    );
    let answered = receive_until(&mut client, closed);
    assert!(
        closed(&answered),
        "the client's close finished: {answered:?}"
    );
    assert!(matches!(client.state(), LinkState::Closed(_)));
    assert!(
        client.send_text("late").is_err(),
        "a closed connection takes nothing"
    );
}

#[test]
fn sending_before_the_connection_opens_is_an_error() {
    let a = app();
    let b = app();
    let listener = WebsocketListener::bind(&a.engine, "127.0.0.1:0").unwrap();
    let mut client = Connection::connect(&b.engine, &listener.url(), SocketOptions::default());
    assert_eq!(client.state(), LinkState::Connecting);
    assert!(client.send_text("too soon").is_err());
}

#[test]
fn an_unreachable_address_fails_with_no_status() {
    // Bound, then dropped: nothing listens there now.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let b = app();
    let mut client = Connection::connect(
        &b.engine,
        &format!("ws://127.0.0.1:{port}"),
        SocketOptions::default(),
    );
    let got = receive_until(&mut client, |got| !got.is_empty());
    assert!(
        matches!(got.as_slice(), [Arrival::Failed { status: None, .. }]),
        "a refused TCP connect is a failure with no HTTP answer: {got:?}"
    );
    assert!(matches!(client.state(), LinkState::Closed(_)));
}

#[test]
fn a_refused_upgrade_reports_its_http_status() {
    let url = serve_refusal("401 Unauthorized");
    let b = app();
    let mut client = Connection::connect(&b.engine, &url, SocketOptions::default());
    let got = receive_until(&mut client, |got| !got.is_empty());
    match got.as_slice() {
        [Arrival::Failed { reason, status }] => {
            assert_eq!(*status, Some(401));
            assert!(
                reason.contains("401"),
                "the reason names the answer: {reason}"
            );
        }
        other => panic!("expected one failure, got {other:?}"),
    }
}

#[test]
fn a_connection_opened_while_resimulating_never_connects() {
    let a = app();
    let listener = WebsocketListener::bind(&a.engine, "127.0.0.1:0").unwrap();
    let b = app();
    b.engine
        .resource::<balaur_core::rollback::Resimulating>()
        .borrow_mut()
        .0 = true;
    let mut client = Connection::connect(&b.engine, &listener.url(), SocketOptions::default());
    for _ in 0..20 {
        assert!(client.receive().is_empty());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        client.state(),
        LinkState::Connecting,
        "no worker was started"
    );
    assert!(client.send_text("x").is_err());
}
