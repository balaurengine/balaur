//! The gamend bindings called the way a game calls them: from a script,
//! through `balaur::standard_app`.
//!
//! Most tests speak to a miniature in-process Gamend — one port serving the
//! login REST call and a Phoenix-ish websocket — so CI needs no Elixir. The
//! `live_` test at the bottom is ignored by default and runs the same flow
//! against a real `mix dev.start` server.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use balaur_testkit::{e2e_enabled, run_until, run_until_with};
use serde_json::{Value, json};

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
    // Read the body out too, not just the head: answering and closing over
    // bytes the client is still sending resets the connection on Windows, and
    // the reply the client never read is lost with it.
    let head_end = request
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(request.len(), |at| at + 4);
    let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    while request.len() < head_end + length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&chunk[..n]),
        }
    }
    let body = if head.starts_with("POST /api/v1/login/device") {
        json!({"data": {"access_token": "tok", "refresh_token": "ref", "expires_in": 900,
                        "user_id": "00000000-0000-7000-8000-000000000001",
                        "username": "tester", "display_name": ""}})
        .to_string()
    } else {
        // The request line and body come back too, so a test can assert what
        // a call put on the wire without a second server.
        let line = head.lines().next().unwrap_or_default();
        let (method, rest) = line.split_once(' ').unwrap_or(("", ""));
        let path = rest.split_whitespace().next().unwrap_or_default();
        let sent = String::from_utf8_lossy(&request[head_end..]).into_owned();
        json!({"data": {"pong": true, "method": method, "path": path, "sent": sent}}).to_string()
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
    run_until(&source, &["gamend-hook ok hi"]);
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
    run_until(&source, &["gamend-hook ok hi"]);
}

/// The SDK addon `editor/library/addons/gamend` holds, as a game requires it.
fn gamend_addon() -> Vec<(String, String)> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor/library/addons/gamend");
    std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("no SDK addon at {}: {e}", root.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?.to_string();
            if path.extension().is_none_or(|e| e != "rn") {
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            Some((format!("addons/gamend/{name}"), text))
        })
        .collect()
}

#[test]
fn the_sdk_addon_puts_an_operation_on_the_wire() {
    if !e2e_enabled() {
        return;
    }
    let url = serve_gamend();
    let files = gamend_addon();
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    // The stand-in echoes the request line and body, so this asserts the
    // wire itself. One line at the end, because the harness reads a
    // fifty-entry ring the HTTP client's traces would fill.
    let source = format!(
        r#"
pub async fn init(this) {{
    let api = script::require("addons/gamend/api.rn");
    let events = script::require("addons/gamend/events.rn");
    gamend::configure("{url}");
    task::wait(gamend::login((), #{{ "device_id": "dev-sdk" }})).await;

    let one = task::wait((api.lobbies_get_lobby)((), "lob-7")).await["body"]["data"];
    let listed = task::wait((api.quests_my_quests)((), #{{ "page": 2 }})).await["body"]["data"];
    let made = task::wait((api.lobbies_quick_join)((), #{{ "title": "duel" }})).await["body"]["data"];
    let named = (events.decode)("lobby:7", "user_joined", #{{ "user_id": 3 }});

    log::info(`sdk ${{one["method"]}} ${{one["path"]}}`
        + ` | ${{listed["path"]}}`
        + ` | ${{made["method"]}} ${{made["sent"]}}`
        + ` | ${{named["kind"]}}`);
}}
"#
    );
    run_until_with(
        &borrowed,
        &source,
        &[concat!(
            "sdk GET /api/v1/lobbies/lob-7",
            " | /api/v1/me/quests?page=2",
            r#" | POST {"title":"duel"}"#,
            " | lobby_member_joined",
        )],
    );
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
    run_until(source, &["gamend-live 200 error"]);
}
