//! The http bindings called the way a game calls them: from a script, through
//! `balaur::standard_app` — the same wiring a shipped game boots.
//!
//! One app boot per scenario, covering the callback path and the await path
//! in a single script — booting an app dominates the cost, so scenarios share
//! one.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use balaur::{standard_app, AppConfig};

/// These tests boot full apps and speak real sockets: CI's job. A plain
/// local `cargo test` skips them so iteration stays fast; `BALAUR_E2E=1`
/// (what `scripts/e2e_tests.sh` and CI set) runs them.
fn e2e_enabled() -> bool {
    if std::env::var_os("BALAUR_E2E").is_some() {
        return true;
    }
    eprintln!("skipped: e2e suite; run scripts/e2e_tests.sh or set BALAUR_E2E=1");
    false
}

/// The log buffer is global and tests run in parallel, so one test's lines
/// would surface in another's assertions.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Boot a one-node project whose script is `source`, then tick until every
/// marker shows up in the log. No sleeps: ticking full-tilt costs little and
/// the sockets answer in milliseconds. Panics on any logged error or on the
/// deadline.
#[allow(
    clippy::disallowed_methods,
    reason = "a test's timeout, not simulation"
)]
fn run_until(source: &str, markers: &[&str]) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"n\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Node\"\nscript = \"scripts/s.rn\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), source).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    // Markers accumulate across ticks: dependency debug logging (ureq's
    // pool chatter) floods the bounded buffer, so all four are never in one
    // window together.
    let mut seen: Vec<bool> = markers.iter().map(|_| false).collect();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        app.tick(1.0 / 60.0);
        let recent = balaur_core::logbuf::recent(50);
        let errors: Vec<_> = recent
            .iter()
            .filter(|e| e.level.eq_ignore_ascii_case("error"))
            .collect();
        assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
        for entry in &recent {
            for (at, marker) in markers.iter().enumerate() {
                if entry.message.contains(marker) {
                    seen[at] = true;
                }
            }
        }
        if seen.iter().all(|s| *s) {
            return;
        }
    }
    panic!(
        "the script never logged all of {markers:?}; log: {:#?}",
        balaur_core::logbuf::recent(50)
    );
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

/// The response handler named through `on_response`, rather than the default
/// `on_response` method.
#[test]
fn a_script_awaits_a_request_and_takes_another_through_a_named_handler() {
    if !e2e_enabled() {
        return;
    }
    let awaited =
        serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nquail");
    let handled =
        serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nhello");
    let source = format!(
        r#"
pub async fn init(this) {{
    let r = task::wait(http::request("{awaited}")).await;
    log::info(format!("named-await {{}} {{}}", r["status"], r["body"]));
    this.request = http::request(this.node, "{handled}", #{{ on_response: "on_login" }});
}}

pub fn on_login(this, r) {{
    if r["request"] == this.request {{
        log::info(format!("named-http {{}} {{}}", r["status"], r["body"]));
    }}
}}
"#
    );
    run_until(&source, &["named-await 200 quail", "named-http 200 hello"]);
}

/// The same two paths through the default handler name.
#[test]
fn a_rune_script_awaits_a_request_and_takes_another_through_on_response() {
    if !e2e_enabled() {
        return;
    }
    let awaited =
        serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nraven");
    let handled =
        serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nrhino");
    let source = format!(
        r#"
pub async fn init(this) {{
    let r = task::wait(http::request("{awaited}")).await;
    log::info(`rune-await ${{r["status"]}} ${{r["body"]}}`);
    this.request = http::request(this.node, "{handled}");
}}

pub fn on_response(this, r) {{
    if r["request"] == this.request {{
        log::info(`rune-http ${{r["status"]}} ${{r["body"]}}`);
    }}
}}
"#
    );
    run_until(&source, &["rune-await 200 raven", "rune-http 200 rhino"]);
}
