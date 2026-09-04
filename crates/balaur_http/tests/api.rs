//! The http plugin driven from Rust, against servers on a loopback port.
//!
//! No external network: every test binds `127.0.0.1:0` and speaks to itself,
//! so CI and offline runs behave like any other.

use std::io::{Read, Write};
use std::net::TcpListener;

use balaur_core::{App, AppConfig};
use balaur_http::{HttpCall, HttpPlugin, HttpSnapshot, HttpState};
use balaur_script::Value;

fn app_with_http(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut HttpPlugin::default()).unwrap();
    app
}

/// Tick until `pick` finds its event in the snapshot. Wall-clock bounded:
/// network threads take real time, so the test loop has to also.
fn wait_for<T>(app: &mut App, mut pick: impl FnMut(&HttpSnapshot) -> Option<T>) -> T {
    for _ in 0..1000 {
        app.tick(1.0 / 60.0);
        let snapshot = app.engine.resource::<HttpSnapshot>();
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
    let mut app = app_with_http(dir.path());
    let id = app.engine.next_token();
    {
        let state = app.engine.resource::<HttpState>();
        state
            .borrow_mut()
            .request(&app.engine, id, get_call(&url), None);
    }
    let response = wait_for(&mut app, |snapshot| snapshot.responses.first().cloned());
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
    let mut app = app_with_http(dir.path());
    {
        let state = app.engine.resource::<HttpState>();
        let id = app.engine.next_token();
        state
            .borrow_mut()
            .request(&app.engine, id, get_call(&url), None);
    }
    let response = wait_for(&mut app, |snapshot| snapshot.responses.first().cloned());
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
    let mut app = app_with_http(dir.path());
    {
        let state = app.engine.resource::<HttpState>();
        let id = app.engine.next_token();
        state.borrow_mut().request(
            &app.engine,
            id,
            get_call(&format!("http://127.0.0.1:{port}")),
            None,
        );
    }
    let response = wait_for(&mut app, |snapshot| snapshot.responses.first().cloned());
    assert!(
        matches!(field(&response, "error"), Some(Value::Str(_))),
        "a refused connection should surface as an error event: {response:?}"
    );
    assert_eq!(field(&response, "status"), None);
}

#[test]
fn the_http_table_of_the_manifest_sets_the_default_timeout() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n\n[http]\ntimeout = 2.5\n",
    )
    .unwrap();
    let app = app_with_http(dir.path());
    let config = app.engine.resource::<balaur_http::HttpConfig>();
    let config = config.borrow();
    assert!((config.timeout - 2.5).abs() < f64::EPSILON);
}
