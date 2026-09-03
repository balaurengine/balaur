//! The gamend bindings called the way a game calls them: from a script,
//! through `balaur::standard_app`.
//!
//! Most tests speak to a miniature in-process Gamend — one port serving the
//! login REST call and a Phoenix-ish websocket — so CI needs no Elixir. The
//! `live_` test at the bottom is ignored by default and runs the same flow
//! against a real `mix dev.start` server.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use balaur::{standard_app, AppConfig};
use serde_json::{json, Value};

/// The log buffer is global and tests run in parallel, so one test's lines
/// would surface in another's assertions.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

/// Boot a one-node project whose script is `source`, then tick until the log
/// contains `marker`. Panics on any logged error or on timeout.
#[allow(
    clippy::disallowed_methods,
    reason = "a test's timeout, not simulation"
)]
fn run_until(source: &str, marker: &str) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"g\"\nmain_scene = \"main.toml\"\n",
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        app.tick(1.0 / 60.0);
        let recent = balaur_core::logbuf::recent(50);
        let errors: Vec<_> = recent
            .iter()
            .filter(|e| e.level.eq_ignore_ascii_case("error"))
            .collect();
        assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
        if recent.iter().any(|e| e.message.contains(marker)) {
            return;
        }
    }
    panic!("the script never logged `{marker}`");
}

/// A one-port Gamend stand-in: device login and a generic GET over HTTP,
/// joins and an echoing `call_hook` over the websocket.
fn serve_gamend() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        while let Ok((stream, _)) = listener.accept() {
            std::thread::spawn(move || serve_connection(stream));
        }
    });
    format!("http://{addr}")
}

fn serve_connection(stream: TcpStream) {
    let mut probe = [0u8; 16];
    let peeked = stream.peek(&mut probe).unwrap_or(0);
    if String::from_utf8_lossy(&probe[..peeked]).starts_with("GET /socket") {
        serve_socket(stream);
    } else {
        serve_http(stream);
    }
}

fn serve_http(mut stream: TcpStream) {
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&request);
    let body = if head.starts_with("POST /api/v1/login/device") {
        json!({"data": {"access_token": "tok", "refresh_token": "ref", "expires_in": 900,
                        "user_id": "00000000-0000-7000-8000-000000000001",
                        "username": "tester", "display_name": ""}})
        .to_string()
    } else {
        json!({"data": {"pong": true}}).to_string()
    };
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
}

fn serve_socket(stream: TcpStream) {
    let Ok(mut connection) = tungstenite::accept(stream) else {
        return;
    };
    loop {
        let message = match connection.read() {
            Ok(m) if m.is_text() => m,
            Ok(tungstenite::Message::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };
        let frame: Value = match serde_json::from_str(message.to_text().unwrap_or_default()) {
            Ok(frame) => frame,
            Err(_) => continue,
        };
        let (join_ref, reference, topic, event, payload) = (
            frame[0].clone(),
            frame[1].clone(),
            frame[2].clone(),
            frame[3].as_str().unwrap_or_default().to_string(),
            frame[4].clone(),
        );
        let reply = |status: &str, response: Value| {
            json!([join_ref, reference, topic, "phx_reply",
                   {"status": status, "response": response}])
            .to_string()
        };
        let text = match event.as_str() {
            "phx_join" | "heartbeat" | "phx_leave" => reply("ok", json!({})),
            "call_hook" => reply("ok", json!({"data": payload["args"][0]})),
            _ => reply("error", json!({"error": "unknown_event"})),
        };
        if connection
            .send(tungstenite::Message::Text(text.into()))
            .is_err()
        {
            break;
        }
    }
}

#[test]
fn a_script_logs_in_rests_and_calls_a_hook() {
    if !e2e_enabled() {
        return;
    }
    let url = serve_gamend();
    let source = format!(
        r#"
pub async fn init(this) {{
    gamend::configure("{url}");
    let login = task::wait(gamend::login(#{{ device_id: "dev-1" }})).await;
    log::info(format!("gamend-login {{}}", login["username"]));
    let r = task::wait(gamend::rest((), "GET", "/api/v1/ping")).await;
    log::info(format!("gamend-rest {{}} {{}}", r["status"], r["body"]["data"]["pong"]));
    this.socket = gamend::connect(this.node);
}}

pub async fn on_gamend_event(this, e) {{
    if e["kind"] == "open" {{
        let reply = task::wait(gamend::call_hook(this.socket, "arena", "echo", ["hi"])).await;
        log::info(format!("gamend-hook {{}} {{}}", reply["status"], reply["response"]["data"]));
    }}
}}
"#
    );
    run_until(&source, "gamend-hook ok hi");
}

#[test]
fn a_rune_script_logs_in_and_calls_a_hook() {
    if !e2e_enabled() {
        return;
    }
    let url = serve_gamend();
    let source = format!(
        r#"
pub async fn init(this) {{
    gamend::configure("{url}");
    let login = task::wait(gamend::login(#{{ "device_id": "dev-3" }})).await;
    log::info(`gamend-login ${{login["username"]}}`);
    this.socket = gamend::connect(this.node);
}}

pub async fn on_gamend_event(this, e) {{
    if e["kind"] == "open" {{
        let reply = task::wait(gamend::call_hook(this.socket, "arena", "echo", ["hi"])).await;
        log::info(`gamend-hook ${{reply["status"]}} ${{reply["response"]["data"]}}`);
    }}
}}
"#
    );
    run_until(&source, "gamend-hook ok hi");
}

#[test]
#[ignore = "needs a running gamend server on localhost:4000"]
fn live_a_script_talks_to_a_real_server() {
    let source = r#"
pub async fn init(this) {
    gamend::configure("http://localhost:4000");
    let login = task::wait(gamend::login(#{ device_id: "balaur-plugin-live-test" })).await;
    if login.contains_key("error") {
        log::info(format!("gamend-live login failed: {}", login["error"]));
        return;
    }
    this.socket = gamend::connect(this.node);
}

pub async fn on_gamend_event(this, e) {
    if e["kind"] == "open" {
        let me = task::wait(gamend::rest((), "GET", "/api/v1/me")).await;
        let hook = task::wait(gamend::call_hook(this.socket, "sdk_probe", "echo", ["hi"])).await;
        log::info(format!("gamend-live {} {}", me["status"], hook["status"]));
    }
}
"#;
    // The hook has no plugin behind it on a stock server, so its reply is an
    // error — which still proves the whole path.
    run_until(source, "gamend-live 200 error");
}
