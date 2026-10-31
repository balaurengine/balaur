//! The request pool: how many requests run at once, what waits, what is
//! reused, and what a request's own settings may say.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use balaur_core::{App, AppConfig};
use balaur_http::{HttpCall, HttpPlugin, HttpSnapshot, HttpState};
use balaur_script::Value;

/// An app whose `project.toml` carries `http`, a `[http]` table's lines.
fn app_with(dir: &std::path::Path, http: &str) -> App {
    std::fs::write(
        dir.join("project.toml"),
        format!(
            "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n\n[http]\n{http}\n"
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(dir.join("scenes/main.toml"), "").unwrap();
    let mut app = App::new(AppConfig::bare(dir.to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut HttpPlugin::default()).unwrap();
    app.load_project().unwrap();
    app
}

/// A keep-alive HTTP/1.1 server answering `ok` after `hold`, counting the
/// connections it took, the requests it read and the most it held at once.
struct Server {
    url: String,
    connections: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

fn serve(hold: Duration) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let counters = (connections.clone(), requests.clone(), peak.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            counters.0.fetch_add(1, Ordering::SeqCst);
            let (requests, peak, active) = (counters.1.clone(), counters.2.clone(), active.clone());
            std::thread::spawn(move || {
                let mut seen = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = stream.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        seen.extend_from_slice(&chunk[..n]);
                    }
                    let end = seen.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    seen.drain(..end);
                    requests.fetch_add(1, Ordering::SeqCst);
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(hold);
                    active.fetch_sub(1, Ordering::SeqCst);
                    let reply = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok";
                    if stream.write_all(reply.as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    Server {
        url,
        connections,
        requests,
        peak,
    }
}

fn get(url: &str) -> HttpCall {
    HttpCall {
        id: 0,
        method: "GET".into(),
        url: url.into(),
        headers: Vec::new(),
        body: None,
        timeout: Some(10.0),
        save_to: None,
    }
}

fn field<'a>(map: &'a Value, key: &str) -> Option<&'a Value> {
    match map {
        Value::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// Start `call` and hand back its id.
fn start(app: &App, call: HttpCall) -> u64 {
    let id = app.engine.next_token();
    let state = app.engine.resource::<HttpState>();
    state.borrow_mut().request(&app.engine, id, call, None);
    id
}

/// Tick until `want` events have been seen, or ten seconds pass.
fn collect(app: &mut App, want: usize) -> Vec<Value> {
    let mut seen = Vec::new();
    for _ in 0..2000 {
        app.tick(1.0 / 60.0);
        seen.extend(
            app.engine
                .resource::<HttpSnapshot>()
                .borrow()
                .responses
                .iter()
                .cloned(),
        );
        if seen.len() >= want {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    seen
}

/// Everything that arrives while ticking for `span`.
#[allow(clippy::disallowed_methods, reason = "a test's window")]
fn collect_for(app: &mut App, span: Duration) -> Vec<Value> {
    let until = std::time::Instant::now() + span;
    let mut seen = Vec::new();
    while std::time::Instant::now() < until {
        app.tick(1.0 / 60.0);
        seen.extend(
            app.engine
                .resource::<HttpSnapshot>()
                .borrow()
                .responses
                .iter()
                .cloned(),
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    seen
}

#[test]
fn requests_past_max_parallel_wait_their_turn_and_all_answer() {
    let server = serve(Duration::from_millis(30));
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with(dir.path(), "max_parallel = 3");
    for _ in 0..30 {
        start(&app, get(&server.url));
    }
    let answers = collect(&mut app, 30);
    assert_eq!(answers.len(), 30, "every request answered");
    assert!(
        answers
            .iter()
            .all(|answer| field(answer, "status") == Some(&Value::Int(200))),
        "and every answer was the server's"
    );
    assert_eq!(
        server.peak.load(Ordering::SeqCst),
        3,
        "never more than three at once, and three while others waited"
    );
}

#[test]
fn requests_one_after_another_reuse_one_connection() {
    let server = serve(Duration::ZERO);
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with(dir.path(), "");
    for round in 1..=5 {
        start(&app, get(&server.url));
        assert_eq!(collect(&mut app, 1).len(), 1, "request {round} answered");
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 5);
    assert_eq!(
        server.connections.load(Ordering::SeqCst),
        1,
        "the agent kept the connection alive between requests"
    );
}

#[test]
fn a_cancelled_request_still_waiting_never_reaches_the_server() {
    let hold = Duration::from_millis(200);
    let server = serve(hold);
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with(dir.path(), "max_parallel = 1");
    let first = start(&app, get(&server.url));
    let second = start(&app, get(&server.url));
    assert!(
        app.engine
            .resource::<HttpState>()
            .borrow_mut()
            .cancel(second)
    );
    let events = collect(&mut app, 2);
    let kinds: Vec<_> = events
        .iter()
        .map(|event| {
            (
                field(event, "request").cloned(),
                field(event, "kind").cloned(),
            )
        })
        .collect();
    let id = |n: u64| Some(Value::Int(i64::try_from(n).unwrap()));
    assert!(kinds.contains(&(id(second), Some(Value::Str("cancelled".into())))));
    assert!(kinds.contains(&(id(first), Some(Value::Str("response".into())))));
    // Long enough for the queued one to have gone out and come back, had it
    // been sent.
    let late = collect_for(&mut app, hold * 2);
    assert!(late.is_empty(), "nothing more arrived: {late:?}");
    assert_eq!(
        server.requests.load(Ordering::SeqCst),
        1,
        "only the first went out"
    );
}

#[test]
fn a_timeout_that_is_no_duration_fails_its_request_and_not_the_pool() {
    let server = serve(Duration::ZERO);
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with(dir.path(), "max_parallel = 1");
    let mut endless = get(&server.url);
    endless.timeout = Some(f64::INFINITY);
    start(&app, endless);
    let failed = collect(&mut app, 1);
    assert_eq!(field(&failed[0], "kind"), Some(&Value::Str("error".into())));
    start(&app, get(&server.url));
    let answered = collect(&mut app, 1);
    assert_eq!(
        field(&answered[0], "status"),
        Some(&Value::Int(200)),
        "the one worker lived to run the next request"
    );
}

#[test]
fn the_http_table_sets_how_many_run_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_with(dir.path(), "max_parallel = 12");
    assert_eq!(
        balaur_http::HttpConfig::from_settings(&app.engine).max_parallel,
        12
    );
}
