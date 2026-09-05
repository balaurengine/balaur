//! Replaying network traffic: the recording answers, not the server.
//!
//! The point of these is the *absence* of a server during playback. A
//! recorded session that quietly re-issued its requests would pass a digest
//! check and still be wrong — it would be talking to the world again.

use std::io::{Read, Write};
use std::net::TcpListener;

use balaur_core::replay::{self, ReplayFeed, ReplayMode};
use balaur_core::{App, AppConfig};
use balaur_http::{HttpCall, HttpPlugin, HttpSnapshot, HttpState};
use balaur_script::Value;

fn app_with_http() -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut HttpPlugin::default()).unwrap();
    app
}

fn field<'a>(map: &'a Value, key: &str) -> Option<&'a Value> {
    match map {
        Value::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn body_of(snapshot: &HttpSnapshot) -> Option<String> {
    let event = snapshot.responses.first()?;
    match field(event, "body")? {
        Value::Str(s) => Some(s.clone()),
        _ => None,
    }
}

/// Serve one canned response on a fresh port, returning the url.
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

/// Record a session that fetches `url`, returning each tick's captured
/// sources. The recording stops once the reply has landed.
fn record_a_fetch(url: &str) -> Vec<serde_json::Map<String, serde_json::Value>> {
    let mut app = app_with_http();
    app.engine.resource::<HttpState>().borrow_mut().request(
        &app.engine,
        1,
        HttpCall {
            id: 1,
            method: "GET".into(),
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            timeout: Some(5.0),
            save_to: None,
        },
        None,
    );

    let mut frames = Vec::new();
    for _ in 0..1000 {
        app.tick(1.0 / 60.0);
        frames.push(replay::capture(&app.engine));
        let snapshot = app.engine.resource::<HttpSnapshot>();
        let landed = body_of(&snapshot.borrow()).is_some();
        drop(snapshot);
        if landed {
            return frames;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the recording never saw a response");
}

#[test]
fn a_recorded_response_replays_with_no_server_listening() {
    let url = serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nhello");
    let frames = record_a_fetch(&url);
    assert!(
        frames.len() > 1,
        "the reply should land a tick or more later"
    );

    // The one-shot server is spent, and the port is dead. Anything that
    // reaches the network now gets nothing.
    let mut app = app_with_http();
    *app.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    app.engine.resource::<HttpState>().borrow_mut().request(
        &app.engine,
        1,
        HttpCall {
            id: 1,
            method: "GET".into(),
            url,
            headers: Vec::new(),
            body: None,
            timeout: Some(5.0),
            save_to: None,
        },
        None,
    );

    let feed = app.engine.resource::<ReplayFeed>();
    let mut replayed = None;
    for frame in &frames {
        feed.borrow_mut().0 = Some(frame.clone());
        app.tick(1.0 / 60.0);
        let snapshot = app.engine.resource::<HttpSnapshot>();
        let found = body_of(&snapshot.borrow());
        if let Some(body) = found {
            replayed = Some(body);
        }
    }
    assert_eq!(
        replayed.as_deref(),
        Some("hello"),
        "the recorded reply has to arrive from the file"
    );
}

/// The suppression half. Without it a replay re-issues every request, which
/// is wrong even when the digests happen to line up.
#[test]
fn replaying_does_not_reach_the_network() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if listener.accept().is_ok() {
            let _ = tx.send(());
        }
    });

    let mut app = app_with_http();
    *app.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;

    app.engine.resource::<HttpState>().borrow_mut().request(
        &app.engine,
        1,
        HttpCall {
            id: 1,
            method: "GET".into(),
            url: format!("http://{addr}"),
            headers: Vec::new(),
            body: None,
            timeout: Some(5.0),
            save_to: None,
        },
        None,
    );
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        rx.try_recv().is_err(),
        "a replayed request must not open a connection"
    );
}
