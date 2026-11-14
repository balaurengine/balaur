//! The page a browser build runs in, as a Balaur plugin: `browser.*` for scripts.
//!
//! A page is outside the simulation, so it is reached the way a socket is.
//! Facts about it (the tab's visibility, the user agent, the location) are
//! read once per tick at [`Stage::First`], recorded, and answered from the
//! recording on replay. A message from the parent frame arrives the same
//! way, sent as the `browser_event` event. Posting a
//! message is an effect on the world and goes out through [`ExternalIo`],
//! which never fires while a recording plays.
//!
//! Named verbs rather than `eval`: an arbitrary string run now, returning
//! anything, is both a determinism hole and a security one. A vendor SDK
//! that speaks `postMessage` is a module over `post_message` and
//! `on_browser_event`; the engine carries the bridge, not the protocol.
//!
//! Off the web every query answers nil, `visible` answers true and
//! `post_message` answers false, so a script written for a page still runs
//! on a desktop.

use anyhow::Result;
use balaur_core::engine_api::{from_json, to_json};
use balaur_core::replay::ExternalIo;
use balaur_core::{Engine, Stage};
use balaur_script::{Module, Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[cfg(target_family = "wasm")]
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

/// The event a message from the parent frame arrives as, heard with
/// `events::listen` as `on_browser_event`.
pub const MESSAGE_EVENT: &str = "browser_event";

/// The channel the page reports on, and what it last said.
pub struct BrowserState {
    io: ExternalIo<WebEvent>,
    facts: PageFacts,
    visible: bool,
}

impl BrowserState {
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

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            io: ExternalIo::default(),
            facts: PageFacts::default(),
            visible: true,
        }
    }
}

/// The facts and the visibility, alongside the channel's arrivals, so a
/// replay on a desktop answers `browser.user_agent` as the browser did.
#[derive(Serialize, Deserialize)]
struct Captured {
    io: Json,
    facts: PageFacts,
    visible: bool,
}

fn capture_browser(eng: &Engine) -> Json {
    let state = eng.resource::<BrowserState>();
    let state = state.borrow();
    serde_json::to_value(Captured {
        io: state.io.capture(),
        facts: state.facts.clone(),
        visible: state.visible,
    })
    .unwrap_or(Json::Null)
}

fn restore_browser(eng: &Engine, value: &Json) {
    let Ok(captured) = Captured::deserialize(value) else {
        return;
    };
    let state = eng.resource::<BrowserState>();
    let mut state = state.borrow_mut();
    state.io.restore(&captured.io);
    state.facts = captured.facts;
    state.visible = captured.visible;
}

/// Drain the page's reports, sending each message as its event, in arrival
/// order.
fn pump_web_system(eng: &Engine, _: f32) {
    let mut messages = Vec::new();
    {
        let state = eng.resource::<BrowserState>();
        let mut state = state.borrow_mut();
        for event in state.io.drain() {
            match event {
                WebEvent::Message { payload } => {
                    let value = from_json(&payload).unwrap_or(Value::Nil);
                    messages.push(value);
                }
                WebEvent::Visibility { visible } => {
                    state.visible = visible;
                    // A hidden tab is a game in the background, as a phone app is.
                    balaur_core::facts::update_device(eng, |device| device.suspended = !visible);
                }
            }
        }
    }
    for message in messages {
        balaur_core::events::emit(eng, MESSAGE_EVENT, message);
    }
}

/// Send `payload` to the page that embeds this one. Answers whether it was
/// sent: false off the web, and false while a recording plays.
pub fn post_message(eng: &Engine, payload: &Value) -> Result<bool> {
    let json = to_json(payload)?;
    if !backend::SUPPORTED {
        return Ok(false);
    }
    let state = eng.resource::<BrowserState>();
    let state = state.borrow();
    Ok(state.io.start(eng, |_| backend::post_message(&json)))
}

fn install_web_api(m: &mut Module<Engine>) {
    m.module_doc(
        "The page a browser build runs in; off the web every query answers nil. A parent-frame message is the `browser_event` event: `events::listen(node, \"browser_event\")`, heard as `on_browser_event(payload)`.",
    );
    m.describe(&[
        (POST_MESSAGE, &[], "(payload: any) -> bool", "Post a value to the page that embeds this one. False off the web, and false while a recording plays."),
        (VISIBLE, &[], "() -> bool", "Whether the tab is in front of the player; true off the web."),
        (USER_AGENT, &[], "() -> string?", "The browser's user agent string, or nil off the web."),
        (LOCATION, &[], "() -> string?", "The page's URL, or nil off the web."),
        (HARDWARE_CONCURRENCY, &[], "() -> int?", "How many threads the browser reports, or nil off the web."),
    ]);
    m.function(POST_MESSAGE, |eng: &Engine, payload: Value| {
        post_message(eng, &payload)
    });
    m.function(VISIBLE, |eng: &Engine, ()| {
        Ok(eng.resource::<BrowserState>().borrow().visible)
    });
    m.function(USER_AGENT, |eng: &Engine, ()| {
        Ok(fact(eng, |f| f.user_agent.clone().map(Value::text)))
    });
    m.function(LOCATION, |eng: &Engine, ()| {
        Ok(fact(eng, |f| f.location.clone().map(Value::text)))
    });
    m.function(HARDWARE_CONCURRENCY, |eng: &Engine, ()| {
        Ok(fact(eng, |f| {
            f.hardware_concurrency.map(|n| Value::Int(i64::from(n)))
        }))
    });
}

fn fact(eng: &Engine, read: impl FnOnce(&PageFacts) -> Option<Value>) -> Value {
    read(&eng.resource::<BrowserState>().borrow().facts).unwrap_or(Value::Nil)
}

#[derive(Default)]
pub struct BrowserPlugin;

impl balaur_plugin::Plugin for BrowserPlugin {
    fn name(&self) -> &'static str {
        BROWSER
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        balaur_core::events::declare(
            reg.engine(),
            &[(
                MESSAGE_EVENT,
                "the message, as the parent frame posted it",
                "The page's parent frame posted a message.",
            )],
        );
        let state = BrowserState {
            facts: backend::facts(),
            visible: backend::visible(),
            ..BrowserState::default()
        };
        // Through `start`, so a replay registers nothing and the recording
        // supplies the page's reports instead.
        state
            .io
            .start(reg.engine(), |report| backend::listen(report.clone()));
        reg.insert_resource(state);
        reg.add_system(Stage::First, pump_web_system);
        reg.add_replay_source(BROWSER, capture_browser, restore_browser);
        let mut m = reg.script_module(BROWSER)?;
        install_web_api(&mut m);
        Ok(())
    }
}

#[cfg(target_family = "wasm")]
use browser as backend;

const BROWSER: &str = "browser";
const HARDWARE_CONCURRENCY: &str = "hardware_concurrency";
const LOCATION: &str = "location";
const POST_MESSAGE: &str = "post_message";
const USER_AGENT: &str = "user_agent";
const VISIBLE: &str = "visible";

/// Off the web: no page, so nothing to report and nowhere to post.
#[cfg(not(target_family = "wasm"))]
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
