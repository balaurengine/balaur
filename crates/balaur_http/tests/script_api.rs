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
