//! The Debug Adapter Protocol server, driven over a real socket by a client
//! that speaks the wire format: what an editor outside Balaur sees when it
//! sets a breakpoint, reads a stopped game and lets it go.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use balaur::{App, AppConfig, DEFAULT_FIXED_DT, dap, standard_app};
use serde_json::{Value as Json, json};

/// Line 6 has no code on it, so a breakpoint asked for there lands on 7.
/// Line 8 calls out, for stepping into.
const SCRIPT: &str = "pub struct Runner { node: Node, n: int, ran: int }
impl Runner {
    fn init() { self.n = 0; }
    fn update(dt: float) {
        let before = self.n;
        // nothing on this line
        self.n = before + 1;
        self.ran = bump(self.ran);
    }
}
fn bump(n: int) -> int {
    return n + 1;
}
";

fn project(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Runner\"\nscript = { source = \"scripts/s.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.join("scripts").join("s.bs"), SCRIPT).unwrap();
}

fn app_in(dir: &std::path::Path) -> App {
    standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap()
}

fn script_path(dir: &std::path::Path) -> String {
    dir.join("scripts")
        .join("s.bs")
        .to_string_lossy()
        .into_owned()
}

/// A DAP client speaking `Content-Length` framing.
///
/// The adapter only works while the frame loop runs, and the frame loop is
/// this thread: every wait therefore ticks the app rather than blocking on
/// the socket. Bytes are accumulated, so a read that arrives in pieces or not
/// at all costs nothing but another tick.
struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
    seq: i64,
    events: Vec<Json>,
}

/// Ticks any one wait will spend before giving up. Generous: a loaded machine
/// may take several to get a message across the loopback.
const PATIENCE: usize = 600;

impl Client {
    fn connect(addr: SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(5)))
            .unwrap();
        Self {
            stream,
            buffer: Vec::new(),
            seq: 0,
            events: Vec::new(),
        }
    }

    fn send(&mut self, command: &str, arguments: &Json) -> i64 {
        self.seq += 1;
        let body = json!({
            "seq": self.seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        })
        .to_string();
        self.stream
            .write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes())
            .unwrap();
        self.seq
    }

    /// Let both sides move: one frame for the adapter, one read for us.
    fn drive(&mut self, app: &mut App) {
        app.tick(DEFAULT_FIXED_DT);
        let mut chunk = [0u8; 4096];
        if let Ok(read) = self.stream.read(&mut chunk) {
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }

    /// Take one whole message out of the buffer, if one has arrived.
    fn take(&mut self) -> Option<Json> {
        let head = self
            .buffer
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|at| at + 4)?;
        let length: usize = String::from_utf8_lossy(&self.buffer[..head])
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length:"))
            .and_then(|value| value.trim().parse().ok())
            .expect("a framed message");
        if self.buffer.len() < head + length {
            return None;
        }
        let message = serde_json::from_slice(&self.buffer[head..head + length]).unwrap();
        self.buffer.drain(..head + length);
        Some(message)
    }

    /// Read until the response to `seq`, keeping every event on the way.
    /// Responses to other requests are the answers to a client that did not
    /// wait for them, and are dropped.
    fn response(&mut self, app: &mut App, seq: i64) -> Json {
        for _ in 0..PATIENCE {
            while let Some(message) = self.take() {
                if message["type"] == "event" {
                    self.events.push(message);
                } else if message["request_seq"] == json!(seq) {
                    return message;
                }
            }
            self.drive(app);
        }
        panic!("no response to request {seq}");
    }

    /// Send, let the frame loop service it, and take the response body.
    fn ask(&mut self, app: &mut App, command: &str, arguments: &Json) -> Json {
        let seq = self.send(command, arguments);
        let response = self.response(app, seq);
        assert_eq!(
            response["success"],
            json!(true),
            "{command} failed: {}",
            response["message"]
        );
        response["body"].clone()
    }

    /// The first event of a kind, waiting for one if none has arrived.
    fn event(&mut self, app: &mut App, name: &str) -> Json {
        for _ in 0..PATIENCE {
            if let Some(i) = self.events.iter().position(|e| e["event"] == name) {
                return self.events.remove(i);
            }
            while let Some(message) = self.take() {
                if message["type"] == "event" {
                    self.events.push(message);
                }
            }
            self.drive(app);
        }
        panic!("no `{name}` event");
    }
}

/// Attach, and stop asking questions until the adapter says it is ready.
fn handshake(app: &mut App, client: &mut Client) {
    let capabilities = client.ask(app, "initialize", &json!({ "adapterID": "balaur" }));
    assert_eq!(
        capabilities["supportsConfigurationDoneRequest"],
        json!(true)
    );
    client.event(app, "initialized");
    client.ask(app, "attach", &json!({}));
    client.ask(app, "configurationDone", &json!({}));
}

/// The stopped instance's fields, walked the way a client's variables pane
/// does: the top frame, its Locals scope, then `self`.
fn instance_fields(app: &mut App, client: &mut Client) -> Json {
    let trace = client.ask(app, "stackTrace", &json!({ "threadId": 1 }));
    let scopes = client.ask(
        app,
        "scopes",
        &json!({ "frameId": trace["stackFrames"][0]["id"] }),
    );
    let locals = client.ask(
        app,
        "variables",
        &json!({ "variablesReference": scopes["scopes"][0]["variablesReference"] }),
    );
    let this = variable(&locals, "self");
    assert_ne!(
        this["variablesReference"],
        json!(0),
        "the instance opens into its fields"
    );
    client.ask(
        app,
        "variables",
        &json!({ "variablesReference": this["variablesReference"] }),
    )
}

/// A named variable out of a `variables` response.
fn variable(body: &Json, name: &str) -> Json {
    body["variables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("no variable `{name}` in {body}"))
        .clone()
}

#[test]
fn a_client_sets_a_breakpoint_then_reads_and_walks_the_stopped_game() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut app = app_in(dir.path());
    let server = dap::serve(&mut app, 0).unwrap();
    app.load_project().unwrap();
    let mut client = Client::connect(server.addr());
    handshake(&mut app, &mut client);

    let source = json!({ "path": script_path(dir.path()) });
    let set = client.ask(
        &mut app,
        "setBreakpoints",
        &json!({ "source": source, "breakpoints": [{ "line": 6 }] }),
    );
    let breakpoint = &set["breakpoints"][0];
    assert_eq!(breakpoint["verified"], json!(true));
    assert_eq!(
        breakpoint["line"],
        json!(7),
        "line 6 has no code; the breakpoint moves to the next line that has"
    );

    let stopped = client.event(&mut app, "stopped");
    assert_eq!(stopped["body"]["reason"], json!("breakpoint"));
    assert_eq!(stopped["body"]["threadId"], json!(1));
    assert_eq!(
        app.engine.frozen_root(),
        Some(app.engine.root()),
        "a stop freezes the game"
    );

    let threads = client.ask(&mut app, "threads", &json!({}));
    assert_eq!(threads["threads"][0]["name"], json!("game"));

    let trace = client.ask(&mut app, "stackTrace", &json!({ "threadId": 1 }));
    let top = trace["stackFrames"][0].clone();
    assert_eq!(top["name"], json!("Runner.update"));
    assert_eq!(top["line"], json!(7));
    assert_eq!(top["source"]["name"], json!("s.bs"));
    assert!(
        std::path::Path::new(top["source"]["path"].as_str().unwrap()).is_absolute(),
        "the client must be able to open the file"
    );

    let frame_id = top["id"].clone();
    let scopes = client.ask(&mut app, "scopes", &json!({ "frameId": frame_id }));
    assert_eq!(scopes["scopes"][0]["name"], json!("Locals"));
    let locals = scopes["scopes"][0]["variablesReference"].clone();
    let variables = client.ask(
        &mut app,
        "variables",
        &json!({ "variablesReference": locals }),
    );
    assert_eq!(variable(&variables, "dt")["type"], json!("number"));

    // Each update raises `n` on line 7 and `ran` on line 8, so the two agree
    // for exactly as long as the stop on line 7 holds — whatever tick it is.
    let fields = instance_fields(&mut app, &mut client);
    let (n, ran) = (variable(&fields, "n"), variable(&fields, "ran"));
    assert_eq!(n["type"], json!("int"));
    assert_eq!(n["value"], ran["value"], "line 7 has not run yet");

    let watch = client.ask(
        &mut app,
        "evaluate",
        &json!({ "expression": "dt", "frameId": frame_id }),
    );
    assert_eq!(watch["type"], json!("number"));
    let seq = client.send(
        "evaluate",
        &json!({ "expression": "1 + 1", "frameId": frame_id }),
    );
    let refused = client.response(&mut app, seq);
    assert_eq!(refused["success"], json!(false));

    client.ask(&mut app, "next", &json!({ "threadId": 1 }));
    let after_step = client.event(&mut app, "stopped");
    assert_eq!(after_step["body"]["reason"], json!("step"));
    let trace = client.ask(&mut app, "stackTrace", &json!({ "threadId": 1 }));
    assert_eq!(trace["stackFrames"][0]["line"], json!(8));

    // Line 7 has run now, and the new stop hands out its own references.
    let fields = instance_fields(&mut app, &mut client);
    let stepped_n: i64 = variable(&fields, "n")["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let earlier: i64 = n["value"].as_str().unwrap().parse().unwrap();
    assert_eq!(stepped_n, earlier + 1, "one line, one increment");

    client.ask(&mut app, "stepIn", &json!({ "threadId": 1 }));
    client.event(&mut app, "stopped");
    let trace = client.ask(&mut app, "stackTrace", &json!({ "threadId": 1 }));
    assert_eq!(trace["stackFrames"][0]["name"], json!("bump"));
    assert_eq!(trace["stackFrames"][1]["name"], json!("Runner.update"));

    client.ask(&mut app, "stepOut", &json!({ "threadId": 1 }));
    client.event(&mut app, "stopped");
    let trace = client.ask(&mut app, "stackTrace", &json!({ "threadId": 1 }));
    assert_eq!(trace["stackFrames"][0]["name"], json!("Runner.update"));

    client.ask(&mut app, "continue", &json!({ "threadId": 1 }));
    client.event(&mut app, "continued");
}

#[test]
#[allow(clippy::disallowed_methods, reason = "a test's clock")]
fn waiting_for_a_client_that_never_comes_gives_up_at_the_deadline() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut app = app_in(dir.path());
    let server = dap::serve(&mut app, 0).unwrap();
    let patience = Duration::from_millis(200);
    let started = std::time::Instant::now();
    let waited = server.wait_for_attach(patience);
    let took = started.elapsed();
    assert!(waited.is_err_and(|err| err.to_string().contains("no debugger attached")));
    assert!(took >= patience, "it waited the whole time: {took:?}");
    assert!(took < patience * 10, "and gave up then: {took:?}");
}

#[test]
fn waiting_for_a_client_catches_a_breakpoint_in_init() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut app = app_in(dir.path());
    let server = dap::serve(&mut app, 0).unwrap();
    let mut client = Client::connect(server.addr());

    // The whole handshake goes out before the project loads. Nothing has
    // compiled, so both breakpoints are answered at the line as asked.
    client.send("initialize", &json!({ "adapterID": "balaur" }));
    let set = client.send(
        "setBreakpoints",
        &json!({
            "source": { "path": script_path(dir.path()) },
            "breakpoints": [{ "line": 3 }, { "line": 6 }],
        }),
    );
    client.send("configurationDone", &json!({}));
    server.wait_for_attach(Duration::from_secs(10)).unwrap();
    let answered = client.response(&mut app, set)["body"]["breakpoints"].clone();
    assert_eq!(answered[0]["line"], json!(3));
    assert_eq!(answered[1]["line"], json!(6), "as asked, nothing compiled");

    // `init` runs during the load, and stops on the way.
    app.load_project().unwrap();
    assert_eq!(app.engine.frozen_root(), Some(app.engine.root()));
    let stopped = client.event(&mut app, "stopped");
    assert_eq!(stopped["body"]["reason"], json!("breakpoint"));
    let trace = client.ask(&mut app, "stackTrace", &json!({ "threadId": 1 }));
    assert_eq!(trace["stackFrames"][0]["name"], json!("Runner.init"));

    // The line that could not be resolved before now can be, and the client
    // is told where it went.
    let moved = client.event(&mut app, "breakpoint");
    assert_eq!(moved["body"]["reason"], json!("changed"));
    let lines: Vec<Json> = std::iter::once(moved["body"]["breakpoint"]["line"].clone())
        .chain(std::iter::once(
            client.event(&mut app, "breakpoint")["body"]["breakpoint"]["line"].clone(),
        ))
        .collect();
    assert_eq!(lines, vec![json!(3), json!(7)]);
}

#[test]
fn the_pause_request_stops_the_game_at_the_next_line_a_script_runs() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut app = app_in(dir.path());
    let server = dap::serve(&mut app, 0).unwrap();
    app.load_project().unwrap();
    let mut client = Client::connect(server.addr());
    handshake(&mut app, &mut client);

    // No breakpoint anywhere: asking is what makes the next call stoppable.
    client.ask(&mut app, "pause", &json!({ "threadId": 1 }));
    let stopped = client.event(&mut app, "stopped");
    assert_eq!(stopped["body"]["reason"], json!("pause"));
    let trace = client.ask(&mut app, "stackTrace", &json!({ "threadId": 1 }));
    assert_eq!(trace["stackFrames"][0]["name"], json!("Runner.update"));

    client.ask(&mut app, "continue", &json!({ "threadId": 1 }));
    assert_eq!(app.engine.frozen_root(), None);
}

#[test]
fn disconnecting_leaves_the_game_running_with_no_breakpoints_left() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut app = app_in(dir.path());
    let server = dap::serve(&mut app, 0).unwrap();
    app.load_project().unwrap();
    let mut client = Client::connect(server.addr());
    handshake(&mut app, &mut client);
    client.ask(
        &mut app,
        "setBreakpoints",
        &json!({
            "source": { "path": script_path(dir.path()) },
            "breakpoints": [{ "line": 7 }],
        }),
    );
    client.event(&mut app, "stopped");

    client.ask(&mut app, "disconnect", &json!({}));
    assert_eq!(app.engine.frozen_root(), None, "the game is let go");
    assert!(
        app.engine
            .script_host()
            .unwrap()
            .breakpoints("scripts/s.bs")
            .is_empty(),
        "a debugger that walks away leaves nothing behind"
    );
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(app.engine.frozen_root(), None, "and it stays let go");
}

/// A client that stops reading costs itself Debug Console lines, never the
/// game a frame: the adapter writes from its own thread and drops log lines
/// past a backlog, so the frame loop runs on and the client is answered once
/// it reads again.
#[test]
fn a_client_that_stops_reading_never_stalls_the_game() {
    balaur_core::logbuf::capture_for_test();
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut app = app_in(dir.path());
    let server = dap::serve(&mut app, 0).unwrap();
    app.load_project().unwrap();
    let mut client = Client::connect(server.addr());
    handshake(&mut app, &mut client);
    // Far more than the socket holds, and nothing read.
    for frame in 0..200 {
        for line in 0..100 {
            tracing::info!("flood {frame} {line}");
        }
        app.tick(DEFAULT_FIXED_DT);
    }
    let threads = client.ask(&mut app, "threads", &json!({}));
    assert_eq!(threads["threads"][0]["name"], json!("game"));
}
