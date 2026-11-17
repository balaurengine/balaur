//! The request pool: how many requests run at once, what waits, what is
//! reused, and what a request's own settings may say.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
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
    balaur_plugin::load(&mut app, &mut HttpPlugin).unwrap();
    app.load_project().unwrap();
    app
}

/// A keep-alive HTTP/1.1 server answering `ok`, counting the connections it
/// took and the requests it read.
struct Server {
    url: String,
    connections: Arc<AtomicUsize>,
    flight: Arc<(Mutex<Flight>, Condvar)>,
}

/// The requests a server has read, the ones it is still answering, and the
/// most it answered at once; `open` is the gate the test lifts.
#[derive(Clone, Copy, Default)]
struct Flight {
    read: usize,
    active: usize,
    peak: usize,
    open: bool,
}

impl Server {
    fn flight(&self) -> Flight {
        *lock(&self.flight)
    }

    fn open(&self) {
        lock(&self.flight).open = true;
        self.flight.1.notify_all();
    }
}

fn lock(flight: &(Mutex<Flight>, Condvar)) -> std::sync::MutexGuard<'_, Flight> {
    flight.0.lock().unwrap_or_else(PoisonError::into_inner)
}

fn serve(hold: Duration) -> Server {
    serve_held(hold, |_| true)
}

/// A server that answers each request once `release` holds of its flight,
/// and `hold` after that.
fn serve_held(hold: Duration, release: fn(&Flight) -> bool) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let flight = Arc::new((Mutex::new(Flight::default()), Condvar::new()));
    let (taken, shared) = (connections.clone(), flight.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            taken.fetch_add(1, Ordering::SeqCst);
            let flight = shared.clone();
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
                    let mut now = lock(&flight);
                    now.read += 1;
                    now.active += 1;
                    now.peak = now.peak.max(now.active);
                    flight.1.notify_all();
                    while !release(&now) {
                        now = flight.1.wait(now).unwrap_or_else(PoisonError::into_inner);
                    }
                    drop(now);
                    std::thread::sleep(hold);
                    lock(&flight).active -= 1;
                    flight.1.notify_all();
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
        flight,
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
    state.borrow_mut().request(&app.engine, id, call);
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

// Each request is held until three are in, or all thirty have been read:
// three at once is then the pool's doing, whatever the machine's load.
#[test]
fn requests_past_max_parallel_wait_their_turn_and_all_answer() {
    let server = serve_held(Duration::from_millis(30), |flight| {
        flight.active >= 3 || flight.read >= 30
    });
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
        server.flight().peak,
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
    assert_eq!(server.flight().read, 5);
    assert_eq!(
        server.connections.load(Ordering::SeqCst),
        1,
        "the agent kept the connection alive between requests"
    );
}

// One worker takes the queue in order, and the first request is held until
// the second is cancelled: a third answered means the second's turn is past.
#[test]
fn a_cancelled_request_still_waiting_never_reaches_the_server() {
    let server = serve_held(Duration::ZERO, |flight| flight.open);
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
    server.open();
    let third = start(&app, get(&server.url));
    let events = collect(&mut app, 3);
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
    let kind = |name: &str| Some(Value::Str(name.into()));
    assert_eq!(kinds.len(), 3, "{kinds:?}");
    assert!(
        kinds.contains(&(id(second), kind("cancelled"))),
        "{kinds:?}"
    );
    assert!(kinds.contains(&(id(first), kind("response"))), "{kinds:?}");
    assert!(kinds.contains(&(id(third), kind("response"))), "{kinds:?}");
    assert_eq!(
        server.flight().read,
        2,
        "the first and the third went out, the second never"
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
