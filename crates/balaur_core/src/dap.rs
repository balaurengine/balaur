//! A Debug Adapter Protocol server, so an editor outside Balaur can drive the
//! debugger the engine already has.
//!
//! Nothing here knows about scripts: it speaks DAP on one side and the five
//! [`ScriptHost`] debugger methods on the other. What phases 1-5 built —
//! breakpoints that land on the next line with code, a pause that freezes the
//! game while the frame loop keeps drawing, stepping, break on error, and an
//! asked-for break — is the whole of what a client can ask for.
//!
//! # Threads
//!
//! The socket has its own threads and the simulation is not thread safe, so
//! they only ever pass messages: a reader thread parses requests and queues
//! them, a writer thread drains replies onto the wire, and a system at
//! [`Stage::First`] does every piece of work that touches the engine. The same
//! model the net plugin uses for HTTP, and for the same reason — a debugger
//! command lands at one point in the frame, in arrival order.
//!
//! # One client
//!
//! A second connection replaces the first. Debugging is a conversation with
//! one editor, and two clients sharing one breakpoint set would confuse both.

// Every request handler shares one signature so `dispatch` reads as the
// table it is; several have nothing to fail at and some need no state.
#![allow(clippy::unnecessary_wraps, clippy::unused_self)]

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use balaur_script::{Pause, PauseReason, ScriptHost, StepMode, Value};
use serde_json::{json, Map as JsonMap, Value as Json};

use crate::app::App;
use crate::engine::Engine;
use crate::project::ProjectRoot;
use crate::Stage;

/// The one thread a client sees. The engine runs the game on a single thread
/// and stops all of it at once, so there is exactly one, always id 1.
const THREAD_ID: i64 = 1;

/// Log lines scanned per pump for the client's Debug Console. Past this a
/// burst is dropped rather than the frame being spent on it.
const LOG_SCAN: usize = 64;

/// Install the server and start listening. Port 0 takes any free port, which
/// the returned handle reports.
///
/// # Errors
/// If the port cannot be bound.
pub fn serve(app: &mut App, port: u16) -> Result<Server> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .with_context(|| format!("binding the debug adapter to port {port}"))?;
    let addr = listener.local_addr()?;
    let (requests, inbox) = channel();
    let out = Arc::new(Mutex::new(None));
    let connected = Arc::new(AtomicBool::new(false));
    accept_loop(listener, &requests, &out, &connected);

    let root = app
        .engine
        .try_resource::<ProjectRoot>()
        .map_or_else(|| PathBuf::from("."), |r| r.borrow().0.clone());
    let session = Rc::new(std::cell::RefCell::new(Session {
        inbox,
        out,
        connected,
        root: root.canonicalize().unwrap_or(root),
        seq: 0,
        configured: false,
        reported_stop: false,
        stop: None,
        reported_lines: Vec::new(),
        log_cursor: 0.0,
    }));
    let pump = Rc::clone(&session);
    app.add_system(Stage::First, move |eng, _| pump.borrow_mut().pump(eng));
    tracing::info!("debug adapter listening on {addr}");
    Ok(Server {
        addr,
        engine: app.engine.clone(),
        session,
    })
}

/// A listening server, for the code that started it.
pub struct Server {
    addr: SocketAddr,
    engine: Engine,
    session: Rc<std::cell::RefCell<Session>>,
}

impl Server {
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Serve requests until a client has attached and finished configuring,
    /// or `timeout` runs out.
    ///
    /// A game boots before its frame loop starts, so a breakpoint in `init`
    /// can only be set by a client that got its word in first. This is the
    /// only way to catch one.
    ///
    /// # Errors
    /// If no client finishes configuring in time.
    pub fn wait_for_attach(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.session.borrow_mut().pump(&self.engine);
            if self.session.borrow().configured {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        anyhow::bail!("no debugger attached on {} within {timeout:?}", self.addr)
    }
}

/// Accept clients forever, one at a time, each on its own reader thread.
fn accept_loop(
    listener: TcpListener,
    requests: &Sender<Json>,
    out: &Arc<Mutex<Option<TcpStream>>>,
    connected: &Arc<AtomicBool>,
) {
    let (requests, out, connected) = (requests.clone(), Arc::clone(out), Arc::clone(connected));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let Ok(reader) = stream.try_clone() else {
                continue;
            };
            *out.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(stream);
            connected.store(true, Ordering::Relaxed);
            read_messages(reader, &requests);
            connected.store(false, Ordering::Relaxed);
            *out.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    });
}

/// Read `Content-Length` framed JSON off one connection until it closes.
fn read_messages(stream: TcpStream, requests: &Sender<Json>) {
    let mut reader = BufReader::new(stream);
    loop {
        let mut length = None;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => return,
                Ok(_) => {}
                Err(err) => {
                    tracing::debug!("debug adapter read: {err}");
                    return;
                }
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                length = value.trim().parse::<usize>().ok();
            }
        }
        let Some(length) = length else {
            tracing::warn!("debug adapter: a message arrived without a Content-Length");
            return;
        };
        let mut body = vec![0u8; length];
        if std::io::Read::read_exact(&mut reader, &mut body).is_err() {
            return;
        }
        match serde_json::from_slice::<Json>(&body) {
            Ok(message) => {
                if requests.send(message).is_err() {
                    return;
                }
            }
            Err(err) => tracing::warn!("debug adapter: unreadable message: {err}"),
        }
    }
}

/// One stop's variable tree, numbered for the client.
///
/// A `variablesReference` is an index into `arena`, so the numbers stay
/// meaningful for exactly as long as the pause they describe. Every stop
/// starts a fresh one.
struct Stop {
    pause: Pause,
    arena: Vec<Vec<(String, Value)>>,
}

impl Stop {
    fn new(pause: Pause) -> Self {
        Self {
            pause,
            arena: Vec::new(),
        }
    }

    /// Number a set of named values so the client can ask for them.
    fn reference(&mut self, values: Vec<(String, Value)>) -> i64 {
        self.arena.push(values);
        i64::try_from(self.arena.len()).unwrap_or(i64::MAX)
    }

    /// The children of a value, if it has any to show.
    fn children(&mut self, value: &Value) -> i64 {
        match value {
            Value::Map(entries) => self.reference(entries.clone()),
            Value::List(items) | Value::Many(items) => {
                let named = items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i.to_string(), v.clone()))
                    .collect();
                self.reference(named)
            }
            _ => 0,
        }
    }
}

/// The adapter's own state, touched only from the frame loop.
struct Session {
    inbox: Receiver<Json>,
    out: Arc<Mutex<Option<TcpStream>>>,
    connected: Arc<AtomicBool>,
    /// The project root incoming source paths are made relative to.
    root: PathBuf,
    seq: i64,
    /// A client has attached and sent `configurationDone`.
    configured: bool,
    /// The client has been told the game is stopped.
    reported_stop: bool,
    stop: Option<Stop>,
    /// Breakpoint lines as last reported, so a line that moves when its file
    /// finally compiles can be corrected.
    reported_lines: Vec<(String, Vec<usize>)>,
    /// Timestamp of the last log line forwarded to the Debug Console.
    log_cursor: f64,
}

impl Session {
    /// One frame's worth of adapter work: service what arrived, then tell the
    /// client anything that changed.
    fn pump(&mut self, eng: &Engine) {
        while let Ok(message) = self.inbox.try_recv() {
            self.dispatch(eng, &message);
        }
        if !self.connected.load(Ordering::Relaxed) {
            // Nobody to tell. Drop the stop so the next client starts clean.
            self.reported_stop = false;
            self.stop = None;
            self.configured = false;
            return;
        }
        self.report_stop(eng);
        self.report_moved_breakpoints(eng);
        self.forward_logs();
    }

    fn send(&mut self, mut message: JsonMap<String, Json>) {
        self.seq += 1;
        message.insert("seq".into(), json!(self.seq));
        let body = Json::Object(message).to_string();
        let mut guard = self
            .out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(stream) = guard.as_mut() else {
            return;
        };
        let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        if let Err(err) = stream.write_all(framed.as_bytes()) {
            tracing::debug!("debug adapter write: {err}");
        }
    }

    fn event(&mut self, name: &str, body: Json) {
        let mut message = JsonMap::new();
        message.insert("type".into(), json!("event"));
        message.insert("event".into(), json!(name));
        message.insert("body".into(), body);
        self.send(message);
    }

    fn respond(&mut self, request: &Json, body: Result<Json, String>) {
        let mut message = JsonMap::new();
        message.insert("type".into(), json!("response"));
        message.insert("request_seq".into(), request["seq"].clone());
        message.insert("command".into(), request["command"].clone());
        match body {
            Ok(body) => {
                message.insert("success".into(), json!(true));
                if !body.is_null() {
                    message.insert("body".into(), body);
                }
            }
            Err(reason) => {
                message.insert("success".into(), json!(false));
                message.insert("message".into(), json!(reason));
            }
        }
        self.send(message);
    }

    fn dispatch(&mut self, eng: &Engine, request: &Json) {
        if request["type"] != json!("request") {
            return;
        }
        let command = request["command"].as_str().unwrap_or_default().to_string();
        let arguments = &request["arguments"];
        let answer = match command.as_str() {
            "initialize" => Ok(capabilities()),
            // The game is already running: an attach and a launch are both
            // just this client saying hello.
            "attach" | "launch" | "configurationDone" => {
                self.configured = true;
                Ok(Json::Null)
            }
            "setBreakpoints" => self.set_breakpoints(eng, arguments),
            "setExceptionBreakpoints" => self.set_exception_breakpoints(eng, arguments),
            "threads" => Ok(json!({ "threads": [{ "id": THREAD_ID, "name": "game" }] })),
            "stackTrace" => self.stack_trace(),
            "scopes" => self.scopes(arguments),
            "variables" => self.variables(arguments),
            "evaluate" => self.evaluate(arguments),
            "pause" => self.request_break(eng),
            "continue" | "next" | "stepIn" | "stepOut" => self.resume(eng, &command),
            "disconnect" | "terminate" => self.disconnect(eng, &command),
            other => Err(format!(
                "the balaur debug adapter does not support `{other}`"
            )),
        };
        self.respond(request, answer);
        // The client may only configure once it has been told the adapter is
        // ready, and being told is the response to `initialize` going first.
        if command == "initialize" {
            self.event("initialized", Json::Null);
        }
        if command == "continue" {
            // A continue reports for every thread, and there is one.
            self.event("continued", json!({ "threadId": THREAD_ID }));
        }
    }

    /// Replace one file's breakpoints. The response reports the line each one
    /// landed on, which is the next line with code at or after the one asked
    /// for — or, for a file not compiled yet, the line as asked.
    fn set_breakpoints(&mut self, eng: &Engine, arguments: &Json) -> Result<Json, String> {
        let host = host(eng)?;
        let path = arguments["source"]["path"].as_str().unwrap_or_default();
        let key = self.key_of(path);
        let wanted: Vec<usize> = arguments["breakpoints"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|b| b["line"].as_u64().and_then(|l| usize::try_from(l).ok()))
                    .collect()
            })
            .unwrap_or_default();
        let landed = host
            .set_breakpoints(&key, &wanted)
            .map_err(|e| e.to_string())?;
        self.remember_lines(&key, &landed);
        Ok(json!({ "breakpoints": self.breakpoint_list(&key, &landed) }))
    }

    fn set_exception_breakpoints(
        &mut self,
        eng: &Engine,
        arguments: &Json,
    ) -> Result<Json, String> {
        let on = arguments["filters"]
            .as_array()
            .is_some_and(|f| f.iter().any(|filter| filter == "error"));
        host(eng)?.set_break_on_error(on);
        Ok(Json::Null)
    }

    fn stack_trace(&mut self) -> Result<Json, String> {
        let Some(stop) = &self.stop else {
            return Ok(json!({ "stackFrames": [], "totalFrames": 0 }));
        };
        let root = self.root.clone();
        let frames: Vec<Json> = stop
            .pause
            .frames
            .iter()
            .enumerate()
            .map(|(i, frame)| {
                json!({
                    "id": i64::try_from(i).unwrap_or(i64::MAX) + 1,
                    "name": frame.function,
                    "line": frame.line,
                    "column": 1,
                    "source": source_of(&root, &frame.path),
                })
            })
            .collect();
        Ok(json!({ "totalFrames": frames.len(), "stackFrames": frames }))
    }

    fn scopes(&mut self, arguments: &Json) -> Result<Json, String> {
        let index = frame_index(arguments)?;
        let Some(stop) = &mut self.stop else {
            return Ok(json!({ "scopes": [] }));
        };
        let locals = stop
            .pause
            .frames
            .get(index)
            .map(|f| f.locals.clone())
            .unwrap_or_default();
        let reference = stop.reference(locals);
        Ok(json!({
            "scopes": [{
                "name": "Locals",
                "presentationHint": "locals",
                "variablesReference": reference,
                "expensive": false,
            }]
        }))
    }

    fn variables(&mut self, arguments: &Json) -> Result<Json, String> {
        let reference = arguments["variablesReference"].as_u64().unwrap_or(0) as usize;
        let Some(stop) = &mut self.stop else {
            return Ok(json!({ "variables": [] }));
        };
        let Some(values) = reference.checked_sub(1).and_then(|i| stop.arena.get(i)) else {
            return Err(format!("no variables under reference {reference}"));
        };
        let values = values.clone();
        let variables: Vec<Json> = values
            .iter()
            .map(|(name, value)| {
                let children = stop.children(value);
                json!({
                    "name": name,
                    "value": render(value),
                    "type": value.type_name(),
                    "variablesReference": children,
                })
            })
            .collect();
        Ok(json!({ "variables": variables }))
    }

    /// A watch or a hover. There is no expression evaluator behind the seam,
    /// so this answers for a name in the frame it was asked about and says so
    /// plainly for anything else.
    fn evaluate(&mut self, arguments: &Json) -> Result<Json, String> {
        let expression = arguments["expression"].as_str().unwrap_or_default().trim();
        let index = frame_index(arguments).unwrap_or(0);
        let Some(stop) = &mut self.stop else {
            return Err("nothing is stopped".into());
        };
        let found = stop
            .pause
            .frames
            .get(index)
            .and_then(|f| f.locals.iter().find(|(n, _)| n == expression))
            .map(|(_, v)| v.clone());
        let Some(value) = found else {
            return Err(format!(
                "`{expression}` is not a local here; balaur evaluates names, not expressions"
            ));
        };
        let children = stop.children(&value);
        Ok(json!({
            "result": render(&value),
            "type": value.type_name(),
            "variablesReference": children,
        }))
    }

    /// The client's Pause button. Nothing stops here: the request arms the
    /// host, and the `stopped` event follows when a script next runs.
    fn request_break(&mut self, eng: &Engine) -> Result<Json, String> {
        host(eng)?.request_break();
        Ok(Json::Null)
    }

    fn resume(&mut self, eng: &Engine, command: &str) -> Result<Json, String> {
        let mode = match command {
            "next" => StepMode::Over,
            "stepIn" => StepMode::Into,
            "stepOut" => StepMode::Out,
            _ => StepMode::Continue,
        };
        // Cleared before resuming, because the host may stop again inside the
        // call — a step arrives that way — and the client is owed an event
        // for the new stop, not silence.
        self.reported_stop = false;
        self.stop = None;
        host(eng)?.resume(mode);
        Ok(json!({ "allThreadsContinued": true }))
    }

    /// Leave the game as it was found: no breakpoints, nothing frozen. A
    /// `terminate` also asks the game to quit; a `disconnect` lets it run on.
    fn disconnect(&mut self, eng: &Engine, command: &str) -> Result<Json, String> {
        if let Ok(host) = host(eng) {
            for (path, _) in std::mem::take(&mut self.reported_lines) {
                let _ = host.set_breakpoints(&path, &[]);
            }
            host.set_break_on_error(false);
            if host.paused().is_some() {
                host.resume(StepMode::Continue);
            }
        }
        self.reported_stop = false;
        self.stop = None;
        self.configured = false;
        if command == "terminate" {
            eng.request_quit();
        }
        Ok(Json::Null)
    }

    /// Tell the client when the game stops, and when it goes again.
    fn report_stop(&mut self, eng: &Engine) {
        let paused = eng.script_host().and_then(|h| h.paused());
        match (paused, self.reported_stop) {
            (Some(pause), false) => {
                let (reason, description) = match pause.reason {
                    PauseReason::Breakpoint => ("breakpoint", "Hit breakpoint"),
                    PauseReason::Step => ("step", "Stepped"),
                    PauseReason::Pause => ("pause", "Paused"),
                    PauseReason::Error => ("exception", "Script error"),
                };
                let text = pause.message.clone();
                self.stop = Some(Stop::new(pause));
                self.reported_stop = true;
                self.event(
                    "stopped",
                    json!({
                        "reason": reason,
                        "description": description,
                        "text": text,
                        "threadId": THREAD_ID,
                        "allThreadsStopped": true,
                    }),
                );
            }
            (None, true) => {
                self.reported_stop = false;
                self.stop = None;
                self.event("continued", json!({ "threadId": THREAD_ID }));
            }
            _ => {}
        }
    }

    /// A breakpoint set before its file compiled was answered with the line as
    /// asked. Once the unit arrives the line may have moved, and the client is
    /// told where to.
    fn report_moved_breakpoints(&mut self, eng: &Engine) {
        let Some(host) = eng.script_host() else {
            return;
        };
        let paths: Vec<String> = self.reported_lines.iter().map(|(p, _)| p.clone()).collect();
        for path in paths {
            let landed = host.breakpoints(&path);
            let reported = self
                .reported_lines
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, l)| l.clone())
                .unwrap_or_default();
            if landed == reported {
                continue;
            }
            self.remember_lines(&path, &landed);
            for breakpoint in self.breakpoint_list(&path, &landed) {
                self.event(
                    "breakpoint",
                    json!({ "reason": "changed", "breakpoint": breakpoint }),
                );
            }
        }
    }

    /// Put the engine's log through to the client's Debug Console.
    fn forward_logs(&mut self) {
        let fresh: Vec<crate::logbuf::LogEntry> = crate::logbuf::recent(LOG_SCAN)
            .into_iter()
            .filter(|entry| entry.time > self.log_cursor)
            .collect();
        for entry in fresh {
            self.log_cursor = entry.time;
            let category = match entry.level.as_str() {
                "error" | "warn" => "stderr",
                _ => "stdout",
            };
            self.event(
                "output",
                json!({
                    "category": category,
                    "output": format!("[{}] {}\n", entry.tag, entry.message),
                }),
            );
        }
    }

    /// The host's key for a path the client named: project-relative, with
    /// forward slashes, which is how scripts are keyed everywhere.
    fn key_of(&self, path: &str) -> String {
        let path = Path::new(path);
        let absolute = path.canonicalize();
        let relative = absolute
            .as_deref()
            .unwrap_or(path)
            .strip_prefix(&self.root)
            .unwrap_or(path);
        relative.to_string_lossy().replace('\\', "/")
    }

    fn remember_lines(&mut self, key: &str, landed: &[usize]) {
        match self.reported_lines.iter_mut().find(|(p, _)| p == key) {
            Some((_, lines)) => landed.clone_into(lines),
            None => self.reported_lines.push((key.to_string(), landed.to_vec())),
        }
    }

    /// A file's breakpoints as the client numbers them: the id is the line's
    /// position in the file's set, which is stable for as long as the set is.
    fn breakpoint_list(&self, key: &str, landed: &[usize]) -> Vec<Json> {
        let root = &self.root;
        landed
            .iter()
            .enumerate()
            .map(|(i, line)| {
                json!({
                    "id": i64::try_from(i).unwrap_or(i64::MAX) + 1,
                    "verified": true,
                    "line": line,
                    "source": source_of(root, key),
                })
            })
            .collect()
    }
}

/// What this adapter can do. Everything absent is a request it answers with a
/// plain refusal rather than a half-truth.
fn capabilities() -> Json {
    json!({
        "supportsConfigurationDoneRequest": true,
        "supportsEvaluateForHovers": true,
        "supportsTerminateRequest": true,
        "supportsExceptionFilterOptions": true,
        "exceptionBreakpointFilters": [{
            "filter": "error",
            "label": "Uncaught script errors",
            "description": "Stop where a script throws. Puts every synchronous call through the stepping executor.",
            "default": false,
        }],
    })
}

fn host(eng: &Engine) -> Result<Rc<dyn ScriptHost<Engine>>, String> {
    eng.script_host()
        .ok_or_else(|| "no script backend is running".to_string())
}

/// A DAP source for a host key, named absolutely so the client can open it.
fn source_of(root: &Path, key: &str) -> Json {
    let path = root.join(key);
    json!({
        "name": Path::new(key).file_name().map_or_else(|| key.to_string(), |n| n.to_string_lossy().into_owned()),
        "path": path.to_string_lossy(),
    })
}

/// The frame a `scopes` or `evaluate` request is about, as an index into the
/// pause's frames. Ids are one-based on the wire.
fn frame_index(arguments: &Json) -> Result<usize, String> {
    let id = arguments["frameId"].as_i64().unwrap_or(1);
    usize::try_from(id - 1).map_err(|_| format!("{id} is not a frame id"))
}

/// A value as one line in the variables pane.
fn render(value: &Value) -> String {
    match value {
        Value::Nil => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Num(n) => n.to_string(),
        Value::Str(s) => format!("{s:?}"),
        Value::Vec2([x, y]) => format!("({x}, {y})"),
        Value::Vec3([x, y, z]) => format!("({x}, {y}, {z})"),
        Value::Color([r, g, b, a]) => format!("rgba({r}, {g}, {b}, {a})"),
        Value::Node(bits) => format!("node #{bits}"),
        Value::Callback(_) => "function".into(),
        Value::List(items) | Value::Many(items) => format!("[{} items]", items.len()),
        Value::Map(entries) => format!("{{{} fields}}", entries.len()),
    }
}
