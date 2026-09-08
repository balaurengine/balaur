//! The websocket bindings called the way a game calls them: from a script,
//! through `balaur::standard_app` — the same wiring a shipped game boots.
//!
//! One app boot per scenario; booting an app dominates the cost.

use std::net::TcpListener;

use balaur_testkit::{e2e_enabled, run_until};

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
