//! The websocket bindings called the way a game calls them: from a script,
//! through `balaur::standard_app` — the same wiring a shipped game boots.
//!
//! One app boot per scenario; booting an app dominates the cost.

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
        "[application]\nname = \"n\"\nmain_scene = \"main.toml\"\n",
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

/// A round trip through the default `on_websocket_event` handler.
#[test]
fn a_rune_script_opens_echoes_and_closes() {
    if !e2e_enabled() {
        return;
    }
    let echo = serve_echo();
    let source = format!(
        r#"
pub fn init(this) {{
    this.socket = websocket::connect(this.node, "{echo}");
}}

pub fn on_websocket_event(this, e) {{
    if e["kind"] == "open" {{
        websocket::send(this.socket, "ping");
    }} else if e["kind"] == "message" {{
        log::info(`rune-websocket ${{e["text"]}}`);
        websocket::close(this.socket);
    }} else if e["kind"] == "closed" {{
        log::info("rune-websocket-closed");
    }}
}}
"#
    );
    run_until(&source, &["rune-websocket ping", "rune-websocket-closed"]);
}

#[test]
fn a_rune_script_sends_and_receives_a_binary_frame() {
    if !e2e_enabled() {
        return;
    }
    let echo = serve_echo();
    let source = format!(
        r#"
pub fn init(this) {{
    this.socket = websocket::connect(this.node, "{echo}");
}}

pub fn on_websocket_event(this, e) {{
    if e["kind"] == "open" {{
        websocket::send(this.socket, Bytes::from_vec([0, 255, 16, 254]));
    }} else if e["kind"] == "binary" {{
        log::info(`rune-binary ${{e["bytes"].len()}}`);
        websocket::close(this.socket);
    }} else if e["kind"] == "closed" {{
        log::info("rune-binary-closed");
    }}
}}
"#
    );
    run_until(&source, &["rune-binary 4", "rune-binary-closed"]);
}
