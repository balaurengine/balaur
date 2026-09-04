//! HTTP as a Balaur plugin: `http.*` for scripts.
//!
//! Requests run on background threads; completions cross back over a channel
//! and enter the simulation once per tick, at [`Stage::First`], recorded in
//! [`HttpSnapshot`] — the same model as input. Recording the snapshot per
//! tick is all a replay needs.
//!
//! Scripts never poll. A request names the node that handles its reply, and
//! the pump dispatches it as a method call — the same signal shape a widget's
//! `on_click` or an animation key uses:
//!
//! ```lua
//! function S:init()
//!     self.request = http.request(self.node, "https://example.com")
//! end
//! function S:on_response(r) print(r.status, r.body) end
//! ```
//!
//! Or, sequentially: a request made without a node is a token the script
//! suspends on until the pump wakes it:
//! `let r = task::wait(http::request(url)).await`.
//!
//! Nothing here blocks the frame: `http.request` returns an id immediately,
//! and handlers and resumptions run at [`Stage::First`] of a later tick, in
//! arrival order, never from an I/O thread.

use anyhow::{anyhow, Result};
use balaur_core::handler::{handler_of, headers_of, id_value, opt, Handler};
use balaur_core::replay::ExternalIo;
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};

#[cfg(not(target_family = "wasm"))]
mod request;

/// The native backend: a thread per request.
#[cfg(not(target_family = "wasm"))]
mod backend {
    pub(crate) use crate::request::spawn_request;

    /// Threads deliver on their own; nothing to flush per tick.
    pub(crate) fn pump() {}
}

#[cfg(all(target_family = "wasm", target_os = "emscripten"))]
mod emscripten;

/// The browser backend: emscripten fetch, no threads.
#[cfg(all(target_family = "wasm", target_os = "emscripten"))]
mod backend {
    pub(crate) use crate::emscripten::spawn_request;

    /// Fetch completions arrive as callbacks; nothing is queued outbound.
    pub(crate) fn pump() {}
}

/// The browser outside emscripten: the Fetch API through web-sys.
#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod browser;

#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod backend {
    pub(crate) use crate::browser::{pump, spawn_request};
}

/// One `http.request`, on its way to a worker thread.
pub struct HttpCall {
    pub id: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// Seconds for the whole request; `None` takes the 10 second default.
    pub timeout: Option<f64>,
}

/// Project-wide defaults, the `[http]` table of `project.toml`:
///
/// ```toml
/// [http]
/// timeout = 10.0   # seconds, when a request names none
/// ```
///
/// A call's own options override these.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub timeout: f64,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self { timeout: 10.0 }
    }
}

impl HttpConfig {
    /// The `[http]` table of the project's manifest, or the defaults when the
    /// file or the table is missing. A table that does not parse is reported
    /// and ignored rather than failing the boot over a networking setting.
    #[must_use]
    pub fn load(files: &balaur_core::project::ProjectFiles) -> Self {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            http: HttpConfig,
        }
        let Ok(bytes) = files.read("project.toml") else {
            return Self::default();
        };
        match toml::from_str::<Manifest>(&String::from_utf8_lossy(&bytes)) {
            Ok(manifest) => manifest.http,
            Err(err) => {
                tracing::warn!("project.toml [http]: {err}; using the defaults");
                Self::default()
            }
        }
    }
}

/// A finished request crossing from a worker thread back to the frame loop.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum HttpEvent {
    Response {
        request: u64,
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    },
    Error {
        request: u64,
        message: String,
    },
}

/// The request table and the channel every worker thread reports into.
#[derive(Default)]
pub struct HttpState {
    /// The worker channel, this tick's arrivals, and the rule that a replay
    /// never reaches the network — all three live in here.
    io: ExternalIo<HttpEvent>,
    handlers: DetHashMap<u64, Handler>,
}

impl HttpState {
    /// Start a request under `id` — an [`Engine::next_token`] value, so
    /// awaiting it can never collide with another subsystem's ids. The
    /// response (or error) reaches `handler` on a later tick, and is recorded
    /// in that tick's [`HttpSnapshot`] either way.
    pub fn request(&mut self, eng: &Engine, id: u64, mut call: HttpCall, handler: Option<Handler>) {
        call.id = id;
        // The handler is wired up either way: on a replay the recorded reply
        // still has to find its way home.
        if let Some(handler) = handler {
            self.handlers.insert(id, handler);
        }
        // The outbound side, which no source records: a reply is captured
        // with the id it answers, and nothing else says the request went out.
        balaur_core::replay::event(
            eng,
            "http.request",
            format!("{} {}", call.method, call.url),
            Some(serde_json::json!({ "id": id, "method": call.method, "url": call.url })),
        );
        self.io
            .start(eng, |report| backend::spawn_request(call, report.clone()));
    }
}

/// This tick's completions, as the neutral values the handlers received.
/// Cleared and refilled by `pump_http_system` at [`Stage::First`]. Scripts
/// never read it — it exists so Rust code can observe traffic, and so a
/// recorder has one place to tap for replay.
#[derive(Default)]
pub struct HttpSnapshot {
    pub responses: Vec<Value>,
}

/// Drain the worker threads' reports, record them in the frame's snapshot,
/// then dispatch each to its handler — in arrival order throughout. Arrival
/// order is an input to the simulation, like a key press: not reproducible
/// across runs, but stable for everyone this tick.
///
/// Dispatch happens after the borrows are released, so a handler may itself
/// make another request.
fn pump_http_system(eng: &Engine, _: f32) {
    // Backends with no delivery threads (the browser) flush their queues
    // here; the native one is a no-op.
    backend::pump();
    let mut dispatches: Vec<(Option<Handler>, u64, Value)> = Vec::new();
    {
        let state = eng.resource::<HttpState>();
        let snapshot = eng.resource::<HttpSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        snapshot.responses.clear();
        for event in state.io.drain() {
            let request = match &event {
                HttpEvent::Response { request, .. } | HttpEvent::Error { request, .. } => *request,
            };
            // shift_remove: keeps the remaining entries in insertion order,
            // so iteration stays deterministic.
            let handler = state.handlers.shift_remove(&request);
            let value = event_value(event);
            snapshot.responses.push(value.clone());
            // Every completion wakes its id too, so a script that chose
            // `await` over a handler resumes here.
            dispatches.push((handler, request, value));
        }
    }
    if let Some(host) = eng.script_host() {
        for (handler, token, value) in dispatches {
            if let Some(handler) = handler {
                host.call_on(handler.node, &handler.method, std::slice::from_ref(&value));
            }
            host.wake(token, &value);
        }
    }
}

fn event_value(event: HttpEvent) -> Value {
    let pairs = match event {
        HttpEvent::Response {
            request,
            status,
            headers,
            body,
        } => vec![
            ("request".into(), id_value(request)),
            ("status".into(), Value::Int(i64::from(status))),
            (
                "headers".into(),
                Value::Map(
                    headers
                        .into_iter()
                        .map(|(k, v)| (k, Value::Str(v)))
                        .collect(),
                ),
            ),
            ("body".into(), Value::Str(body)),
        ],
        HttpEvent::Error { request, message } => vec![
            ("request".into(), id_value(request)),
            ("error".into(), Value::Str(message)),
        ],
    };
    Value::Map(pairs)
}

pub struct HttpPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for HttpPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("http", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for HttpPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        let config = {
            let files = reg
                .app()
                .engine
                .resource::<balaur_core::project::ProjectFiles>();
            let files = files.borrow();
            HttpConfig::load(&files)
        };
        reg.insert_resource(config);
        reg.insert_resource(HttpState::default());
        reg.insert_resource(HttpSnapshot::default());
        reg.add_system(Stage::First, pump_http_system);
        reg.add_replay_source("http", capture, restore);
        balaur_core::settings::register(
            &reg.app().engine,
            balaur_core::settings::SettingsPage {
                category: String::from("HTTP"),
                table: String::from("http"),
                scope: balaur_core::settings::Scope::Project,
                schema: balaur_core::ComponentDef::parse_schema(
                    "settings.http",
                    r#"
timeout = { type = "float", default = 10.0, min = 0.1, max = 600.0, help = "Seconds for a whole request, when the call names none." }
"#,
                ),
            },
        );
        let mut m = reg.script_module("http")?;
        install_http_api(&mut *m);
        Ok(())
    }
}

/// This tick's arrivals, raw. The pump has already stashed them.
fn capture(eng: &Engine) -> serde_json::Value {
    eng.resource::<HttpState>().borrow().io.capture()
}

/// Push recorded arrivals back down the same channel the worker threads use,
/// so the pump dispatches them exactly as it did when they were real.
fn restore(eng: &Engine, value: &serde_json::Value) {
    eng.resource::<HttpState>().borrow().io.restore(value);
}

fn call_of(url: &str, opts: Option<&Value>) -> Result<HttpCall> {
    let method = match opt(opts, "method") {
        Some(Value::Str(m)) => m.to_uppercase(),
        Some(other) => return Err(anyhow!("method should be a string, got {other:?}")),
        None => "GET".into(),
    };
    let body = match opt(opts, "body") {
        Some(Value::Str(b)) => Some(b.clone()),
        Some(other) => return Err(anyhow!("body should be a string, got {other:?}")),
        None => None,
    };
    let headers = headers_of(opts)?;
    let timeout = match opt(opts, "timeout") {
        Some(Value::Num(n)) => Some(*n),
        #[allow(clippy::cast_precision_loss, reason = "a timeout in seconds")]
        Some(Value::Int(n)) => Some(*n as f64),
        _ => None,
    };
    Ok(HttpCall {
        id: 0,
        method,
        url: url.to_string(),
        headers,
        body,
        timeout,
    })
}

/// `http.*`. Declared against the neutral seam, so it works on any backend.
fn install_http_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "HTTP calls, off the frame: the reply arrives on a later tick as a map \
         with `status`, `headers` and `body`, or with `error`, both to the \
         node's `on_response` method and to whoever awaits the returned id. \
         Options are `method`, `headers`, `body` and a `timeout` in seconds, \
         which falls back to the project's `[http] timeout`.",
    );
    m.describe(&[
        ("request", &[], "", "Start an HTTP request and return the id its reply carries, to await or to match inside the handler."),
    ]);
    // An HTTP error status is a response, not an error. With a nil node the
    // returned id is a token to suspend on (`await` / `task::wait`).
    m.function(
        "request",
        |eng: &Engine, (first, second, third): (Value, Option<Value>, Option<Value>)| {
            let (node, url, opts) = match &first {
                Value::Str(url) => (Value::Nil, url.clone(), second),
                _ => match second {
                    Some(Value::Str(url)) => (first, url, third),
                    other => return Err(anyhow!("argument 1 should be a url, got {other:?}")),
                },
            };
            let handler = handler_of(&node, opts.as_ref(), "on_response", "on_response")?;
            let mut call = call_of(&url, opts.as_ref())?;
            if call.timeout.is_none() {
                call.timeout = Some(eng.resource::<HttpConfig>().borrow().timeout);
            }
            let id = eng.next_token();
            let state = eng.resource::<HttpState>();
            state.borrow_mut().request(eng, id, call, handler);
            Ok(id_value(id))
        },
    );
}
