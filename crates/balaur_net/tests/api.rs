//! The net plugin driven from Rust, against servers on a loopback port.
//!
//! No external network: every test binds `127.0.0.1:0` and speaks to itself,
//! so CI and offline runs behave like any other.

use std::io::{Read, Write};
use std::net::TcpListener;

use balaur_core::{App, AppConfig};
use balaur_net::{HttpCall, NetPlugin, NetSnapshot, NetState};
use balaur_script::Value;

fn app_with_net(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut NetPlugin::default()).unwrap();
    app
}

/// Tick until `pick` finds its event in the snapshot. Wall-clock bounded:
/// network threads take real time, so the test loop has to also.
fn wait_for<T>(app: &mut App, mut pick: impl FnMut(&NetSnapshot) -> Option<T>) -> T {
    for _ in 0..1000 {
        app.tick(1.0 / 60.0);
        let snapshot = app.engine.resource::<NetSnapshot>();
        let found = pick(&snapshot.borrow());
        if let Some(value) = found {
            return value;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("timed out waiting for a network event");
}

fn field<'a>(map: &'a Value, key: &str) -> Option<&'a Value> {
    match map {
        Value::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn get_call(url: &str) -> HttpCall {
    HttpCall {
        id: 0,
        method: "GET".into(),
        url: url.into(),
        headers: Vec::new(),
        body: None,
        timeout: Some(5.0),
    }
}

/// Serve one canned HTTP/1.1 response on a fresh port, returning the url.
fn serve_one(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{addr}")
}

#[test]
fn a_response_arrives_in_the_snapshot_with_status_and_body() {
    let url = serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nhello");
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_net(dir.path());
    let id = app.engine.next_token();
    {
        let state = app.engine.resource::<NetState>();
        state.borrow_mut().request(id, get_call(&url), None);
    }
    let response = wait_for(&mut app, |snapshot| snapshot.http.first().cloned());
    assert_eq!(
        field(&response, "request"),
        Some(&Value::Int(i64::try_from(id).unwrap()))
    );
    assert_eq!(field(&response, "status"), Some(&Value::Int(200)));
    assert_eq!(field(&response, "body"), Some(&Value::Str("hello".into())));
}

#[test]
fn an_http_error_status_is_a_response_not_an_error() {
    let url =
        serve_one("HTTP/1.1 404 Not Found\r\ncontent-length: 4\r\nconnection: close\r\n\r\ngone");
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_net(dir.path());
    {
        let state = app.engine.resource::<NetState>();
        let id = app.engine.next_token();
        state.borrow_mut().request(id, get_call(&url), None);
    }
    let response = wait_for(&mut app, |snapshot| snapshot.http.first().cloned());
    assert_eq!(field(&response, "status"), Some(&Value::Int(404)));
    assert_eq!(field(&response, "error"), None);
}

#[test]
fn a_failed_transfer_reports_an_error_event() {
    // Bind a port and drop the listener: connecting to it must fail fast.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_net(dir.path());
    {
        let state = app.engine.resource::<NetState>();
        let id = app.engine.next_token();
        state
            .borrow_mut()
            .request(id, get_call(&format!("http://127.0.0.1:{port}")), None);
    }
    let response = wait_for(&mut app, |snapshot| snapshot.http.first().cloned());
    assert!(
        matches!(field(&response, "error"), Some(Value::Str(_))),
        "a refused connection should surface as an error event: {response:?}"
    );
    assert_eq!(field(&response, "status"), None);
}

/// An echo server for one websocket connection on a fresh port.
fn serve_echo() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut connection = tungstenite::accept(stream).unwrap();
            loop {
                match connection.read() {
                    Ok(message) if message.is_text() => {
                        let _ = connection.send(message);
                    }
                    // Reading through the close frame lets tungstenite flush
                    // its ack before the stream drops; breaking on Ok(Close)
                    // resets the client mid-handshake instead.
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    });
    format!("ws://{addr}")
}

fn socket_event(snapshot: &NetSnapshot, kind: &str) -> Option<Value> {
    snapshot
        .socket
        .iter()
        .find(|event| field(event, "kind") == Some(&Value::Str(kind.into())))
        .cloned()
}

#[test]
fn a_websocket_opens_echoes_and_closes() {
    let url = serve_echo();
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_net(dir.path());
    let id = app.engine.next_token();
    {
        let state = app.engine.resource::<NetState>();
        state.borrow_mut().connect(id, &url, None);
    }

    let open = wait_for(&mut app, |snapshot| socket_event(snapshot, "open"));
    assert_eq!(
        field(&open, "socket"),
        Some(&Value::Int(i64::try_from(id).unwrap()))
    );

    {
        let state = app.engine.resource::<NetState>();
        assert!(state.borrow_mut().send_text(id, "ping"));
    }
    let message = wait_for(&mut app, |snapshot| socket_event(snapshot, "message"));
    assert_eq!(field(&message, "text"), Some(&Value::Str("ping".into())));

    {
        let state = app.engine.resource::<NetState>();
        assert!(state.borrow_mut().close(id));
    }
    wait_for(&mut app, |snapshot| socket_event(snapshot, "closed"));

    // The handle is gone once the close lands; sending is a quiet false.
    let state = app.engine.resource::<NetState>();
    assert!(!state.borrow_mut().send_text(id, "late"));
}

#[test]
fn an_unreachable_websocket_reports_an_error_event() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_net(dir.path());
    {
        let state = app.engine.resource::<NetState>();
        let id = app.engine.next_token();
        state
            .borrow_mut()
            .connect(id, &format!("ws://127.0.0.1:{port}"), None);
    }
    let event = wait_for(&mut app, |snapshot| socket_event(snapshot, "error"));
    assert!(matches!(field(&event, "reason"), Some(Value::Str(_))));
}
