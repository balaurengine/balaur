//! The net plugin driven from Rust, against servers on a loopback port.
//!
//! No external network: every test binds `127.0.0.1:0` and speaks to itself,
//! so CI and offline runs behave like any other.

use std::io::{Read, Write};
use std::net::TcpListener;

use balaur_core::{App, AppConfig};
use balaur_net::{HttpCall, NetPlugin, NetSnapshot, NetState, SocketOptions};
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
        state
            .borrow_mut()
            .request(&app.engine, id, get_call(&url), None);
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
        state
            .borrow_mut()
            .request(&app.engine, id, get_call(&url), None);
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
        state.borrow_mut().request(
            &app.engine,
            id,
            get_call(&format!("http://127.0.0.1:{port}")),
            None,
        );
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
                    Ok(message) if message.is_text() || message.is_binary() => {
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
        state
            .borrow_mut()
            .connect(&app.engine, id, &url, SocketOptions::default(), None);
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
fn a_websocket_carries_binary_frames_that_are_not_utf8() {
    let url = serve_echo();
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_net(dir.path());
    let id = app.engine.next_token();
    {
        let state = app.engine.resource::<NetState>();
        state
            .borrow_mut()
            .connect(&app.engine, id, &url, SocketOptions::default(), None);
    }
    wait_for(&mut app, |snapshot| socket_event(snapshot, "open"));

    // A lone 0xff is not UTF-8, so a payload that survives proves the frame
    // never went through a string.
    let payload = vec![0x00, 0xff, 0x10, 0xfe, 0x7f];
    {
        let state = app.engine.resource::<NetState>();
        assert!(state.borrow_mut().send_bytes(id, payload.clone()));
    }
    let event = wait_for(&mut app, |snapshot| socket_event(snapshot, "binary"));
    assert_eq!(field(&event, "bytes"), Some(&Value::Bytes(payload)));
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
        state.borrow_mut().connect(
            &app.engine,
            id,
            &format!("ws://127.0.0.1:{port}"),
            SocketOptions::default(),
            None,
        );
    }
    let event = wait_for(&mut app, |snapshot| socket_event(snapshot, "error"));
    assert!(matches!(field(&event, "reason"), Some(Value::Str(_))));
}

/// Raw deflate with a sync flush, tail stripped — one side of RFC 7692.
fn run_deflate(c: &mut flate2::Compress, input: &[u8]) -> Vec<u8> {
    use flate2::FlushCompress;
    let mut out = Vec::with_capacity(input.len() + 64);
    let mut consumed = 0;
    loop {
        if out.len() == out.capacity() {
            out.reserve(256);
        }
        let before = c.total_in();
        c.compress_vec(&input[consumed..], &mut out, FlushCompress::Sync)
            .unwrap();
        consumed += (c.total_in() - before) as usize;
        if consumed == input.len() && out.len() < out.capacity() {
            break;
        }
    }
    out.truncate(out.len() - 4);
    out
}
fn run_inflate(d: &mut flate2::Decompress, input: &[u8]) -> Vec<u8> {
    use flate2::FlushDecompress;
    let mut data = input.to_vec();
    data.extend_from_slice(&[0, 0, 0xff, 0xff]);
    let mut out = Vec::with_capacity(data.len() * 4 + 64);
    let mut consumed = 0;
    loop {
        if out.len() == out.capacity() {
            out.reserve(256);
        }
        let before = d.total_in();
        d.decompress_vec(&data[consumed..], &mut out, FlushDecompress::Sync)
            .unwrap();
        consumed += (d.total_in() - before) as usize;
        if consumed == data.len() && out.len() < out.capacity() {
            break;
        }
    }
    out
}

/// An echo server that speaks `permessage-deflate` when offered, over raw
/// frames — tungstenite's message API refuses the compression bit on both
/// sides. Reports whether the first text frame arrived compressed.
#[allow(
    clippy::result_large_err,
    reason = "tungstenite's handshake callback names its own error type"
)]
fn serve_deflate_echo(saw_compressed: std::sync::mpsc::Sender<bool>) -> String {
    use flate2::{Compress, Compression, Decompress};
    use tungstenite::handshake::server::{Request, Response};
    use tungstenite::http::HeaderValue;
    use tungstenite::protocol::frame::coding::{Control, Data, OpCode};
    use tungstenite::protocol::frame::{Frame, FrameSocket};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut negotiated = false;
        let connection =
            tungstenite::accept_hdr(stream, |request: &Request, mut response: Response| {
                let offered = request
                    .headers()
                    .get("sec-websocket-extensions")
                    .is_some_and(|v| v.to_str().unwrap_or("").contains("permessage-deflate"));
                if offered {
                    negotiated = true;
                    response.headers_mut().insert(
                        "Sec-WebSocket-Extensions",
                        HeaderValue::from_static("permessage-deflate"),
                    );
                }
                Ok(response)
            })
            .unwrap();
        // The client sends nothing before the 101, so no bytes are lost.
        let mut socket = FrameSocket::new(connection.into_inner());
        let mut inflate = Decompress::new(false);
        let mut deflate = Compress::new(Compression::default(), false);
        let mut reported = false;
        loop {
            let Ok(Some(frame)) = socket.read(None) else {
                break;
            };
            // Raw reads leave the client's mask on; take it off.
            let mut payload = frame.payload().to_vec();
            if let Some(mask) = frame.header().mask {
                for (i, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[i % 4];
                }
            }
            match frame.header().opcode {
                OpCode::Data(Data::Text) => {
                    let compressed = frame.header().rsv1;
                    if !reported {
                        reported = true;
                        let _ = saw_compressed.send(compressed);
                    }
                    let text = if compressed {
                        run_inflate(&mut inflate, &payload)
                    } else {
                        payload
                    };
                    let (body, rsv1) = if negotiated {
                        (run_deflate(&mut deflate, &text), true)
                    } else {
                        (text, false)
                    };
                    let mut reply = Frame::message(body, OpCode::Data(Data::Text), true);
                    reply.header_mut().rsv1 = rsv1;
                    if socket.send(reply).is_err() {
                        break;
                    }
                }
                OpCode::Control(Control::Close) => {
                    let _ = socket.send(Frame::close(None));
                    break;
                }
                _ => {}
            }
        }
    });
    format!("ws://{addr}")
}

fn echo_once(options: SocketOptions) -> (String, bool) {
    let (report, saw) = std::sync::mpsc::channel();
    let url = serve_deflate_echo(report);
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_net(dir.path());
    let id = app.engine.next_token();
    {
        let state = app.engine.resource::<NetState>();
        state
            .borrow_mut()
            .connect(&app.engine, id, &url, options, None);
    }
    wait_for(&mut app, |snapshot| socket_event(snapshot, "open"));
    // Long and repetitive, so compression has something to do.
    let text = "the quick brown fox jumps over the lazy dog; ".repeat(40);
    {
        let state = app.engine.resource::<NetState>();
        assert!(state.borrow_mut().send_text(id, &text));
    }
    let message = wait_for(&mut app, |snapshot| socket_event(snapshot, "message"));
    let echoed = match field(&message, "text") {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("no text in {other:?}"),
    };
    assert_eq!(echoed, text);
    {
        let state = app.engine.resource::<NetState>();
        assert!(state.borrow_mut().close(id));
    }
    wait_for(&mut app, |snapshot| socket_event(snapshot, "closed"));
    let compressed = saw
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the server saw the first frame");
    (echoed, compressed)
}

#[test]
fn a_websocket_compresses_when_the_server_agrees() {
    let (_, compressed) = echo_once(SocketOptions::default());
    assert!(
        compressed,
        "the frame should have carried permessage-deflate"
    );
}

#[test]
fn a_websocket_sends_plain_frames_when_compression_is_off() {
    let (_, compressed) = echo_once(SocketOptions {
        compression: false,
        headers: Vec::new(),
    });
    assert!(
        !compressed,
        "compression was not offered, so no frame may set rsv1"
    );
}

#[test]
fn the_net_table_of_the_manifest_sets_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n\n[net]\nwebsocket_compression = false\nhttp_timeout = 2.5\n",
    )
    .unwrap();
    let app = app_with_net(dir.path());
    let config = app.engine.resource::<balaur_net::NetConfig>();
    let config = config.borrow();
    assert!(!config.websocket_compression);
    assert!((config.http_timeout - 2.5).abs() < f64::EPSILON);
}
