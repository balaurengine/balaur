use balaur_core::transport::{LinkState, Transport};
use balaur_core::{App, AppConfig};
use balaur_websocket::SocketOptions;
use balaur_websocket::listener::WebsocketListener;
use balaur_websocket::transport::WebsocketTransport;

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

#[test]
fn a_listener_accepts_and_carries_bytes_both_ways() {
    let a = app();
    let b = app();
    let mut listener = WebsocketListener::bind(&a.engine, "127.0.0.1:0").unwrap();
    let mut client =
        WebsocketTransport::connect(&b.engine, &listener.url(), SocketOptions::default());
    let mut server = None;
    for _ in 0..400 {
        if let Some(p) = listener.accept().into_iter().next() {
            server = Some(p);
            break;
        }
        let _ = client.receive();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut server = server.expect("accepted");
    for _ in 0..400 {
        let _ = server.receive();
        let _ = client.receive();
        if server.state() == LinkState::Open && client.state() == LinkState::Open {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(server.state(), LinkState::Open, "server side open");
    assert_eq!(client.state(), LinkState::Open, "client side open");

    client.send_datagram(b"ping").unwrap();
    let mut got = Vec::new();
    for _ in 0..400 {
        got.extend(server.receive());
        if !got.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(got.len(), 1, "server received the client's datagram");
    assert_eq!(got[0].bytes, b"ping".to_vec());

    server.send_reliable(b"pong").unwrap();
    let mut back = Vec::new();
    for _ in 0..400 {
        back.extend(client.receive());
        if !back.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(back.len(), 1, "client received the server's reply");
    assert_eq!(back[0].bytes, b"pong".to_vec());
}
