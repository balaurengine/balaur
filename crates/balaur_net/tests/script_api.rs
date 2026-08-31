//! The net bindings called the way a game calls them: from a script, in both
//! languages, through `balaur::standard_app` — the same wiring a shipped
//! game boots.
//!
//! One app boot per language, covering the callback path, the await path and
//! a websocket round trip in a single script — booting an app dominates the
//! cost, so scenarios share one.

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
fn run_until(script_name: &str, language: &str, source: &str, markers: &[&str]) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("name = \"n\"\nmain_scene = \"main.toml\"\nlanguage = \"{language}\"\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        format!("[[nodes]]\nid = \"n\"\nname = \"Node\"\nscript = \"scripts/{script_name}\"\n"),
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts").join(script_name), source).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        app.tick(1.0 / 60.0);
        let recent = balaur_core::logbuf::recent(50);
        let errors: Vec<_> = recent
            .iter()
            .filter(|e| e.level.eq_ignore_ascii_case("error"))
            .collect();
        assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
        if markers
            .iter()
            .all(|m| recent.iter().any(|e| e.message.contains(m)))
        {
            return;
        }
    }
    panic!("the script never logged all of {markers:?}");
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

#[test]
fn a_lua_script_fetches_awaits_and_echoes() {
    if !e2e_enabled() {
        return;
    }
    let awaited = serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nquail");
    let handled = serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nhello");
    let echo = serve_echo();
    let source = format!(
        r#"
local S = {{}}
function S:init()
    local r = await(http.request("{awaited}"))
    log.info("lua-await " .. r.status .. " " .. r.body)
    self.request = http.request(self.node, "{handled}", {{ on_response = "on_login" }})
    self.socket = websocket.connect(self.node, "{echo}")
end
function S:on_login(r)
    if r.request == self.request then
        log.info("lua-http " .. r.status .. " " .. r.body)
    end
end
function S:on_websocket_event(e)
    if e.kind == "open" then
        websocket.send(self.socket, "ping")
    elseif e.kind == "message" then
        log.info("lua-websocket " .. e.text)
        websocket.close(self.socket)
    elseif e.kind == "closed" then
        log.info("lua-websocket-closed")
    end
end
return S
"#
    );
    run_until(
        "s.luau",
        "luau",
        &source,
        &[
            "lua-await 200 quail",
            "lua-http 200 hello",
            "lua-websocket ping",
            "lua-websocket-closed",
        ],
    );
}

#[test]
fn a_rune_script_fetches_awaits_and_echoes() {
    if !e2e_enabled() {
        return;
    }
    let awaited = serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nraven");
    let handled = serve_one("HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\nrhino");
    let echo = serve_echo();
    let source = format!(
        r#"
pub async fn init(this) {{
    let r = task::wait(http::request("{awaited}")).await;
    log::info(`rune-await ${{r["status"]}} ${{r["body"]}}`);
    this.request = http::request(this.node, "{handled}");
    this.socket = websocket::connect(this.node, "{echo}");
}}

pub fn on_response(this, r) {{
    if r["request"] == this.request {{
        log::info(`rune-http ${{r["status"]}} ${{r["body"]}}`);
    }}
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
    run_until(
        "s.rn",
        "rune",
        &source,
        &[
            "rune-await 200 raven",
            "rune-http 200 rhino",
            "rune-websocket ping",
            "rune-websocket-closed",
        ],
    );
}
