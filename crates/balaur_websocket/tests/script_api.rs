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
fn a_script_opens_echoes_and_closes() {
    if !e2e_enabled() {
        return;
    }
    let echo = serve_echo();
    let source = format!(
        r#"
pub struct S {{ node: Node, socket: int }}
impl S {{
    fn init() {{
        self.socket = websocket::connect(self.node, "{echo}").as_int();
    }}

    pub fn on_websocket_event(e: any) {{
        let kind = e.get("kind").as_str();
        if kind == "open" {{
            websocket::send(self.socket, "ping");
        }} else if kind == "message" {{
            let text = e.get("text");
            log::info("echoed {{text}}");
            websocket::close(self.socket);
        }} else if kind == "closed" {{
            log::info("echo-closed");
        }}
    }}
}}
"#
    );
    run_until(&source, &["echoed ping", "echo-closed"]);
}

#[test]
#[ignore = "needs engine: a .bs script cannot hand `websocket::send` bytes"]
fn a_script_sends_and_receives_a_binary_frame() {
    if !e2e_enabled() {
        return;
    }
    let echo = serve_echo();
    let source = format!(
        r#"
pub struct S {{ node: Node, socket: int }}
impl S {{
    fn init() {{
        self.socket = websocket::connect(self.node, "{echo}").as_int();
    }}

    pub fn on_websocket_event(e: any) {{
        let kind = e.get("kind").as_str();
        if kind == "open" {{
            // 0, 255, 16, 254
            websocket::send(self.socket, encoding::from_base64("AP8Q/g=="));
        }} else if kind == "binary" {{
            let count = e.get("bytes").len();
            log::info("binary {{count}}");
            websocket::close(self.socket);
        }} else if kind == "closed" {{
            log::info("binary-closed");
        }}
    }}
}}
"#
    );
    run_until(&source, &["binary 4", "binary-closed"]);
}

/// `connect`'s id resumes a waiting script with the `open` event, and with
/// the `error` event when nothing listens.
#[test]
fn a_script_awaits_connect_until_it_opens_or_fails() {
    if !e2e_enabled() {
        return;
    }
    let echo = serve_echo();
    let nothing = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("ws://{}", listener.local_addr().unwrap())
    };
    let source = format!(
        r#"
pub struct S {{ node: Node }}
impl S {{
    async fn init() {{
        let socket = websocket::connect(self.node, "{echo}").as_int();
        let opened = await task::wait(socket);
        let kind = opened.get("kind");
        log::info("awaited {{kind}}");
        websocket::close(socket);
        let refused = await task::wait(websocket::connect(self.node, "{nothing}").as_int());
        let why = refused.get("kind");
        log::info("refused {{why}}");
    }}

    pub fn on_websocket_event(e: any) {{}}
}}
"#
    );
    run_until(&source, &["awaited open", "refused error"]);
}
