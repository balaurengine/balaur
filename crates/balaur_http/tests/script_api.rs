//! The http bindings called the way a game calls them: from a script, through
//! `balaur::standard_app` — the same wiring a shipped game boots.
//!
//! One app boot per scenario, covering the callback path and the await path
//! in a single script — booting an app dominates the cost, so scenarios share
//! one.

use std::io::{Read, Write};
use std::net::TcpListener;

use balaur_testkit::{e2e_enabled, run_until};

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

/// Serve one request and answer with the body it carried, returning the url.
fn echo_body() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut seen = Vec::new();
        let mut chunk = [0u8; 4096];
        let body = loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            seen.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&seen).into_owned();
            let Some(end) = text.find("\r\n\r\n") else {
                if n == 0 {
                    break String::new();
                }
                continue;
            };
            let length = text[..end]
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if n == 0 || seen.len() >= end + 4 + length {
                break text[end + 4..].to_string();
            }
        };
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes());
    });
    format!("http://{addr}")
}

#[test]
fn a_delete_carries_the_body_it_was_given() {
    if !e2e_enabled() {
        return;
    }
    let url = echo_body();
    let source = format!(
        r#"
pub async fn init(this) {{
    let r = task::wait(http::request("{url}", #{{ method: "DELETE", body: "ids=7" }})).await;
    log::info(`delete-echo ${{r["status"]}} ${{r["body"]}}`);
}}
"#
    );
    run_until(&source, &["delete-echo 200 ids=7"]);
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
