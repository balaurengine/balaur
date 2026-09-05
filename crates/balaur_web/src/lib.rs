//! The page a browser build runs in, as a Balaur plugin: `web.*` for scripts.
//!
//! A page is outside the simulation, so it is reached the way a socket is.
//! Facts about it — the tab's visibility, the user agent, the location — are
//! read once per tick at [`Stage::First`], recorded, and answered from the
//! recording on replay. A message from the parent frame arrives the same
//! way, dispatched to `on_web_message` on the nodes that asked. Posting a
//! message is an effect on the world and goes out through [`ExternalIo`],
//! which never fires while a recording plays.
//!
//! Named verbs rather than `eval`: an arbitrary string run now, returning
//! anything, is both a determinism hole and a security one. A vendor SDK
//! that speaks `postMessage` is a Rune `mod` over `post_message` and
//! `on_web_message`; the engine carries the bridge, not the protocol.
//!
//! Off the web every query answers nil, `visible` answers true and
//! `post_message` answers false, so a script written for a page still runs
//! on a desktop.

use anyhow::{anyhow, Result};
use balaur_core::engine_api::{from_json, to_json};
use balaur_core::replay::ExternalIo;
use balaur_core::{Engine, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod browser;

/// What the page reports, recorded as it arrives.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum WebEvent {
    Message { payload: Json },
    Visibility { visible: bool },
}

/// What never changes for the life of the page, read once at load.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PageFacts {
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub hardware_concurrency: Option<u32>,
}

/// Where messages go: `on_web_message` on one node's script.
#[derive(Clone)]
pub struct Handler {
    pub node: NodeId,
    pub method: String,
}

/// The channel the page reports on, who listens, and what it last said.
pub struct WebState {
    io: ExternalIo<WebEvent>,
    listeners: Vec<Handler>,
    facts: PageFacts,
    visible: bool,
}

impl WebState {
    /// Whether the tab is in front of the player, as of this tick.
    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    #[must_use]
    pub const fn facts(&self) -> &PageFacts {
        &self.facts
    }
}

impl Default for WebState {
    fn default() -> Self {
        Self {
            io: ExternalIo::default(),
            listeners: Vec::new(),
            facts: PageFacts::default(),
            visible: true,
        }
    }
}

/// This tick's messages, as the values handlers received.
#[derive(Default)]
pub struct WebSnapshot {
    pub messages: Vec<Value>,
}

/// The facts and the visibility, alongside the channel's arrivals, so a
/// replay on a desktop answers `web.user_agent` as the browser did.
#[derive(Serialize, Deserialize)]
struct Captured {
    io: Json,
    facts: PageFacts,
    visible: bool,
}

fn capture_web(eng: &Engine) -> Json {
    let state = eng.resource::<WebState>();
    let state = state.borrow();
    serde_json::to_value(Captured {
        io: state.io.capture(),
        facts: state.facts.clone(),
        visible: state.visible,
    })
    .unwrap_or(Json::Null)
}

fn restore_web(eng: &Engine, value: &Json) {
    let Ok(captured) = serde_json::from_value::<Captured>(value.clone()) else {
        return;
    };
    let state = eng.resource::<WebState>();
    let mut state = state.borrow_mut();
    state.io.restore(&captured.io);
    state.facts = captured.facts;
    state.visible = captured.visible;
}

/// Drain the page's reports, record them, then dispatch — in arrival
/// order, after the borrows are released so a handler may post back.
fn pump_web_system(eng: &Engine, _: f32) {
    let mut dispatches: Vec<(Handler, Value)> = Vec::new();
    {
        let state = eng.resource::<WebState>();
        let snapshot = eng.resource::<WebSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        snapshot.messages.clear();
        for event in state.io.drain() {
            match event {
                WebEvent::Message { payload } => {
                    let value = from_json(&payload).unwrap_or(Value::Nil);
                    snapshot.messages.push(value.clone());
                    for listener in &state.listeners {
                        dispatches.push((listener.clone(), value.clone()));
                    }
                }
                WebEvent::Visibility { visible } => state.visible = visible,
            }
        }
    }
    if let Some(host) = eng.script_host() {
        for (handler, value) in dispatches {
            host.call_on(handler.node, &handler.method, std::slice::from_ref(&value));
        }
    }
}

/// Send `payload` to the page that embeds this one. Answers whether it was
/// sent: false off the web, and false while a recording plays.
pub fn post_message(eng: &Engine, payload: &Value) -> Result<bool> {
    let json = to_json(payload)?;
    if !backend::SUPPORTED {
        return Ok(false);
    }
    let state = eng.resource::<WebState>();
    let state = state.borrow();
    Ok(state.io.start(eng, |_| backend::post_message(&json)))
}

fn opt<'a>(opts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    match opts? {
        Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn install_web_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "The page a browser build runs in. Facts about it are read once per \
         tick and recorded, so a replay answers as the browser did; a \
         message from the parent frame reaches `on_web_message` on every \
         node that called `listen`. Off the web every query answers nil.",
    );
    m.describe(&[
        ("listen", &[], "(node: node, options: map)", "Have the node's `on_web_message(payload)` — or the `on_event` method the options name — called for every message the parent frame posts."),
        ("messages", &[], "()", "Every message the parent frame posted this tick, for a script that would rather ask than declare a method."),
        ("post_message", &[], "(payload: map)", "Post a value to the page that embeds this one. False off the web, and false while a recording plays."),
        ("visible", &[], "()", "Whether the tab is in front of the player; true off the web."),
        ("user_agent", &[], "()", "The browser's user agent string, or nil off the web."),
        ("location", &[], "()", "The page's URL, or nil off the web."),
        ("hardware_concurrency", &[], "()", "How many threads the browser reports, or nil off the web."),
    ]);
    m.function(
        "listen",
        |eng: &Engine, (node, opts): (NodeId, Option<Value>)| {
            let method = match opt(opts.as_ref(), "on_event") {
                Some(Value::Str(name)) => name.clone(),
                Some(other) => {
                    return Err(anyhow!("`on_event` should be a method name, got {other:?}"))
                }
                None => "on_web_message".to_string(),
            };
            eng.resource::<WebState>()
                .borrow_mut()
                .listeners
                .push(Handler { node, method });
            Ok(())
        },
    );
    m.function("messages", |eng: &Engine, ()| {
        Ok(Value::List(
            eng.resource::<WebSnapshot>().borrow().messages.clone(),
        ))
    });
    m.function("post_message", |eng: &Engine, payload: Value| {
        post_message(eng, &payload)
    });
    m.function("visible", |eng: &Engine, ()| {
        Ok(eng.resource::<WebState>().borrow().visible)
    });
    m.function("user_agent", |eng: &Engine, ()| {
        Ok(fact(eng, |f| f.user_agent.clone().map(Value::Str)))
    });
    m.function("location", |eng: &Engine, ()| {
        Ok(fact(eng, |f| f.location.clone().map(Value::Str)))
    });
    m.function("hardware_concurrency", |eng: &Engine, ()| {
        Ok(fact(eng, |f| {
            f.hardware_concurrency.map(|n| Value::Int(i64::from(n)))
        }))
    });
}

fn fact(eng: &Engine, read: impl FnOnce(&PageFacts) -> Option<Value>) -> Value {
    read(&eng.resource::<WebState>().borrow().facts).unwrap_or(Value::Nil)
}

pub struct WebPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for WebPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("web", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for WebPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        let state = WebState {
            facts: backend::facts(),
            visible: backend::visible(),
            ..WebState::default()
        };
        // Through `start`, so a replay registers nothing and the recording
        // supplies the page's reports instead.
        state
            .io
            .start(reg.engine(), |report| backend::listen(report.clone()));
        reg.insert_resource(state);
        reg.insert_resource(WebSnapshot::default());
        reg.add_system(Stage::First, pump_web_system);
        reg.add_replay_source("web", capture_web, restore_web);
        let mut m = reg.script_module("web")?;
        install_web_api(&mut *m);
        Ok(())
    }
}

#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
use browser as backend;

/// Off the web: no page, so nothing to report and nowhere to post.
#[cfg(not(all(target_family = "wasm", not(target_os = "emscripten"))))]
mod backend {
    use std::sync::mpsc::Sender;

    use super::{PageFacts, WebEvent};

    pub(crate) const SUPPORTED: bool = false;

    pub(crate) fn facts() -> PageFacts {
        PageFacts::default()
    }

    pub(crate) const fn visible() -> bool {
        true
    }

    pub(crate) fn listen(_: Sender<WebEvent>) {}

    pub(crate) fn post_message(_: &serde_json::Value) {}
}
