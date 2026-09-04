//! Apple platform services as a Balaur plugin: Game Center and iCloud behind
//! the portable `platform.*` verbs, and `apple.*` for what only Apple has.
//!
//! Everything here is asynchronous and answers on a queue the engine does not
//! own, so nothing here invents a mechanism: a call returns an id, the
//! completion crosses a channel, and [`ExternalIo`] lands it at
//! [`Stage::First`] of a later tick — recorded, replayable, dispatched to a
//! handler method and to whoever awaits the id. It is `balaur_http` with
//! GameKit where the socket is.
//!
//! ```rune
//! pub async fn init(this) {
//!     let me = task::wait(platform::sign_in()).await;
//!     let proof = task::wait(apple::identity()).await;
//!     gamend::call_hook(this.socket, "auth", "game_center", proof);
//! }
//! ```
//!
//! Off an Apple platform the crate still compiles and every call answers
//! `unsupported`, so a script does not change shape between a phone and a CI
//! runner.

use anyhow::anyhow;
use anyhow::Result;
use balaur_core::engine_api::from_json;
use balaur_core::handler::{handler_of, id_value, opt, Handler};
use balaur_core::replay::ExternalIo;
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_platform::{Call, PlatformBackend, PlatformEvent};
use balaur_script::{Bindings, BindingsExt, Value};

#[cfg(target_vendor = "apple")]
mod gamekit;
#[cfg(target_vendor = "apple")]
mod icloud;
mod queue;
#[cfg(target_vendor = "apple")]
mod signin;
#[cfg(target_vendor = "apple")]
mod storekit;
#[cfg(target_vendor = "apple")]
mod ui;

#[cfg(target_vendor = "apple")]
mod backend {
    pub(crate) use crate::gamekit::{apple_call, authenticated, platform_call};
    pub(crate) use crate::ui::access_point;

    /// Whether this build has the frameworks behind it.
    pub(crate) const AVAILABLE: bool = true;
}

/// Everywhere else: no frameworks, so every call answers `unsupported` and
/// the game keeps running.
#[cfg(not(target_vendor = "apple"))]
mod backend {
    use std::sync::mpsc::Sender;

    use balaur_platform::{Call, PlatformEvent};

    use crate::{AppleCall, AppleEvent};

    pub(crate) const AVAILABLE: bool = false;

    pub(crate) fn platform_call(request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        let _ = report.send(PlatformEvent::Unsupported {
            request,
            call: call.name().to_string(),
        });
    }

    pub(crate) fn apple_call(request: u64, call: &AppleCall, report: &Sender<AppleEvent>) {
        let _ = report.send(AppleEvent::Unsupported {
            request,
            call: call.name().to_string(),
        });
    }

    pub(crate) const fn authenticated() -> bool {
        false
    }

    pub(crate) const fn access_point(_active: bool, _location: isize) -> bool {
        false
    }
}

/// Whether this build has Apple's frameworks behind it. False everywhere
/// else, where every call answers `unsupported`.
pub const AVAILABLE: bool = backend::AVAILABLE;

/// A call only Apple answers.
#[derive(Clone, Debug)]
pub enum AppleCall {
    /// The items a server needs to check that a Game Center player is who
    /// the device says: `fetchItemsForIdentityVerificationSignature`.
    Identity,
    /// Sign in with Apple, which is a different account from Game Center's
    /// and answers with a token a server verifies against Apple's keys.
    SignIn,
    /// Whether a Sign in with Apple account is still good. The user id is the
    /// game's to keep — the engine stores no accounts.
    CredentialState { user: String },
    /// Game Center's own dashboard, opening on one of its screens.
    Dashboard { state: isize },
    /// The App Store, through the Swift shim.
    Store(StoreCall),
}

/// What StoreKit is asked for. Everything it answers comes back as the JSON
/// StoreKit itself describes a product or a transaction with.
#[derive(Clone, Debug)]
pub enum StoreCall {
    Products { ids: Vec<String> },
    Purchase { product: String },
    /// What this player currently owns, each with the signed transaction a
    /// server checks.
    Entitlements,
    /// Ask the App Store to hand the device's purchases back — the button an
    /// App Store review requires a game to have.
    Restore,
    /// Tell StoreKit a transaction is dealt with. Until then it comes back
    /// on every launch.
    Finish { transaction: String },
}

impl StoreCall {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Products { .. } => "products",
            Self::Purchase { .. } => "purchase",
            Self::Entitlements => "entitlements",
            Self::Restore => "restore_purchases",
            Self::Finish { .. } => "finish_purchase",
        }
    }
}

impl AppleCall {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::SignIn => "sign_in",
            Self::CredentialState { .. } => "credential_state",
            Self::Dashboard { .. } => "show_dashboard",
            Self::Store(call) => call.name(),
        }
    }
}

/// What the frameworks report back, crossing into the frame loop.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum AppleEvent {
    /// Apple's four items, ready to post to a server. `signature` and `salt`
    /// are base64 because their destination is a JSON hook.
    Identity {
        request: u64,
        player: String,
        url: String,
        signature: String,
        salt: String,
        timestamp: u64,
    },
    /// Sign in with Apple's answer. `token` is the identity token and `code`
    /// the authorization code, both base64 for the same reason: the server
    /// that checks them is reached over JSON.
    SignedIn {
        request: u64,
        user: String,
        name: String,
        token: String,
        code: String,
        email: String,
    },
    /// What Apple says about an account the game saved: `authorized`,
    /// `revoked`, `not_found` or `transferred`.
    CredentialState {
        request: u64,
        state: String,
    },
    /// The player closed Game Center's dashboard.
    DashboardClosed {
        request: u64,
    },
    /// StoreKit's own answer, as it described it: a `kind` of `products`,
    /// `purchased`, `cancelled`, `pending`, `entitlements`, `restored`,
    /// `finished` or `transaction`, and the fields that go with it.
    Store {
        request: u64,
        payload: serde_json::Value,
    },
    Failed {
        request: u64,
        message: String,
    },
    Unsupported {
        request: u64,
        call: String,
    },
}

impl AppleEvent {
    const fn request(&self) -> u64 {
        match self {
            Self::Identity { request, .. }
            | Self::SignedIn { request, .. }
            | Self::CredentialState { request, .. }
            | Self::DashboardClosed { request }
            | Self::Store { request, .. }
            | Self::Failed { request, .. }
            | Self::Unsupported { request, .. } => *request,
        }
    }
}

/// Game Center and iCloud under the portable verbs.
///
/// Presence is not one of them: Game Center has no such concept, so
/// `platform.set_presence` answers `unsupported` here rather than pretending.
struct AppleBackend;

impl PlatformBackend for AppleBackend {
    fn name(&self) -> &'static str {
        "apple"
    }

    fn start(
        &mut self,
        request: u64,
        call: &Call,
        report: &std::sync::mpsc::Sender<PlatformEvent>,
    ) {
        backend::platform_call(request, call, report);
    }

    fn pump(&mut self, report: &std::sync::mpsc::Sender<PlatformEvent>) {
        queue::drain_store(report);
    }

    fn discard(&mut self) {
        queue::discard_store();
    }
}

/// The `apple.*` calls in flight, and the channel the frameworks report into.
#[derive(Default)]
pub struct AppleState {
    io: ExternalIo<AppleEvent>,
    handlers: DetHashMap<u64, Handler>,
}

impl AppleState {
    /// Start a call under `id` — an [`Engine::next_token`] value, so awaiting
    /// it cannot collide with another subsystem's ids.
    pub fn start(&mut self, eng: &Engine, id: u64, call: AppleCall, handler: Option<Handler>) {
        if let Some(handler) = handler {
            self.handlers.insert(id, handler);
        }
        balaur_core::replay::event(
            eng,
            "apple.call",
            call.name().to_string(),
            Some(serde_json::json!({ "id": id, "call": call.name() })),
        );
        self.io
            .start(eng, |report| backend::apple_call(id, &call, report));
    }
}

/// This tick's `apple.*` arrivals, as the handlers received them.
#[derive(Default)]
pub struct AppleSnapshot {
    pub events: Vec<Value>,
}

fn pump_apple_system(eng: &Engine, _: f32) {
    let mut dispatches: Vec<(Option<Handler>, u64, Value)> = Vec::new();
    {
        let state = eng.resource::<AppleState>();
        let snapshot = eng.resource::<AppleSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        // A transaction that landed on its own crosses here, or nowhere: a
        // replay is answered by the file.
        if !state.io.start(eng, queue::drain_apple) {
            queue::discard_apple();
        }
        snapshot.events.clear();
        for event in state.io.drain() {
            let request = event.request();
            let handler = state.handlers.shift_remove(&request);
            let value = event_value(event);
            snapshot.events.push(value.clone());
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

fn event_value(event: AppleEvent) -> Value {
    let mut pairs = vec![("request".into(), id_value(event.request()))];
    match event {
        AppleEvent::Identity {
            player,
            url,
            signature,
            salt,
            timestamp,
            ..
        } => {
            pairs.push(("kind".into(), Value::Str("identity".into())));
            pairs.push(("player".into(), Value::Str(player)));
            pairs.push(("url".into(), Value::Str(url)));
            pairs.push(("signature".into(), Value::Str(signature)));
            pairs.push(("salt".into(), Value::Str(salt)));
            pairs.push((
                "timestamp".into(),
                Value::Int(i64::try_from(timestamp).unwrap_or(i64::MAX)),
            ));
        }
        AppleEvent::SignedIn {
            user,
            name,
            token,
            code,
            email,
            ..
        } => {
            pairs.push(("kind".into(), Value::Str("signed_in".into())));
            pairs.push(("user".into(), Value::Str(user)));
            pairs.push(("name".into(), Value::Str(name)));
            pairs.push(("token".into(), Value::Str(token)));
            pairs.push(("code".into(), Value::Str(code)));
            pairs.push(("email".into(), Value::Str(email)));
        }
        AppleEvent::CredentialState { state, .. } => {
            pairs.push(("kind".into(), Value::Str("credential_state".into())));
            pairs.push(("state".into(), Value::Str(state)));
        }
        AppleEvent::DashboardClosed { .. } => {
            pairs.push(("kind".into(), Value::Str("dashboard_closed".into())));
        }
        // StoreKit's own shape, unflattened: the `kind` is already in there.
        AppleEvent::Store { payload, .. } => {
            if let Ok(Value::Map(fields)) = from_json(&payload) {
                pairs.extend(fields);
            }
        }
        AppleEvent::Failed { message, .. } => {
            pairs.push(("kind".into(), Value::Str("failed".into())));
            pairs.push(("error".into(), Value::Str(message)));
        }
        AppleEvent::Unsupported { call, .. } => {
            pairs.push(("kind".into(), Value::Str("unsupported".into())));
            pairs.push(("call".into(), Value::Str(call)));
        }
    }
    Value::Map(pairs)
}

pub struct ApplePlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for ApplePlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("apple", env!("CARGO_PKG_VERSION"))
                .requiring(&["platform"]),
        }
    }
}

impl balaur_plugin::Plugin for ApplePlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(AppleState::default());
        reg.insert_resource(AppleSnapshot::default());
        reg.add_system(Stage::First, pump_apple_system);
        reg.add_replay_source("apple", capture, restore);
        balaur_platform::set_backend(&reg.app().engine, Box::new(AppleBackend))?;
        let mut m = reg.script_module("apple")?;
        install_apple_api(&mut *m);
        Ok(())
    }
}

fn capture(eng: &Engine) -> serde_json::Value {
    eng.resource::<AppleState>().borrow().io.capture()
}

fn restore(eng: &Engine, value: &serde_json::Value) {
    eng.resource::<AppleState>().borrow().io.restore(value);
}

/// `apple.*`: what the portable module cannot carry because no other store
/// has it.
fn install_apple_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Apple platform services that `platform.*` does not cover. \
         `identity` fetches what a server needs to verify a Game Center \
         player — url, signature, salt and timestamp — and answers on a later \
         tick as a map carrying `kind`, both to the node's `on_apple` method \
         and to whoever awaits the id. Achievements, leaderboards, sign-in \
         and cloud saves are `platform.*`, which speaks Game Center here.",
    );
    m.describe(&[
        (
            "available",
            &[],
            "",
            "Whether this build has Apple's frameworks behind it.",
        ),
        (
            "authenticated",
            &[],
            "",
            "Whether Game Center has a signed-in player right now.",
        ),
        (
            "identity",
            &[],
            "",
            "Fetch the items a server needs to verify this Game Center player, and return the id the answer carries.",
        ),
        (
            "sign_in",
            &[],
            "",
            "Sign in with Apple, and return the id the identity token comes back on. Game Center's own sign-in is `platform.sign_in`.",
        ),
        (
            "credential_state",
            &[],
            "",
            "Ask whether a saved Sign in with Apple account is still authorized, revoked, transferred or unknown.",
        ),
        (
            "show_dashboard",
            &[],
            "",
            "Open Game Center's dashboard, and answer when the player closes it. `state` in the options picks the screen: \"default\", \"leaderboards\", \"achievements\", \"challenges\", \"profile\", \"dashboard\" or \"friends\".",
        ),
        (
            "access_point",
            &[],
            "",
            "Show or hide Game Center's access point badge; `location` in the options is a corner. Returns whether this system has one.",
        ),
    ]);
    m.function("available", |_: &Engine, ()| Ok(Value::Bool(AVAILABLE)));
    m.function("authenticated", |_: &Engine, ()| {
        Ok(Value::Bool(backend::authenticated()))
    });
    m.function(
        "identity",
        |eng: &Engine, (first, second): (Option<Value>, Option<Value>)| {
            let (node, opts) = node_and_options(first, second);
            start_call(eng, node, opts, AppleCall::Identity)
        },
    );
    m.function(
        "sign_in",
        |eng: &Engine, (first, second): (Option<Value>, Option<Value>)| {
            let (node, opts) = node_and_options(first, second);
            start_call(eng, node, opts, AppleCall::SignIn)
        },
    );
    m.function(
        "credential_state",
        |eng: &Engine, (first, second, third): (Value, Option<Value>, Option<Value>)| {
            let (node, user, opts) = match first {
                Value::Str(user) => (None, user, second),
                node => match second {
                    Some(Value::Str(user)) => (Some(node), user, third),
                    other => return Err(anyhow!("argument 1 should be a user id, got {other:?}")),
                },
            };
            start_call(eng, node, opts, AppleCall::CredentialState { user })
        },
    );
    m.function(
        "show_dashboard",
        |eng: &Engine, (first, second): (Option<Value>, Option<Value>)| {
            let (node, opts) = node_and_options(first, second);
            let state = match opt(opts.as_ref(), "state") {
                Some(Value::Str(name)) => screen(name)?,
                Some(other) => return Err(anyhow!("`state` should be a string, got {other:?}")),
                None => DEFAULT_SCREEN,
            };
            start_call(eng, node, opts, AppleCall::Dashboard { state })
        },
    );
    m.function(
        "access_point",
        |_: &Engine, (first, second): (Value, Option<Value>)| {
            let Value::Bool(active) = first else {
                return Err(anyhow!("argument 0 should be true or false, got {first:?}"));
            };
            let corner = match opt(second.as_ref(), "location") {
                Some(Value::Str(name)) => corner_of(name)?,
                Some(other) => return Err(anyhow!("`location` should be a string, got {other:?}")),
                None => DEFAULT_CORNER,
            };
            Ok(Value::Bool(backend::access_point(active, corner)))
        },
    );
}

/// A call whose only arguments are the node and the options: `f(node, opts)`
/// names a handler, `f(opts)` or `f()` is awaited instead.
fn node_and_options(first: Option<Value>, second: Option<Value>) -> (Option<Value>, Option<Value>) {
    match first {
        Some(node @ Value::Node(_)) => (Some(node), second),
        Some(opts) => (None, Some(opts)),
        None => (None, None),
    }
}

/// Where the dashboard opens when a call names no screen, and where the
/// access point sits when it names no corner. Both are GameKit's own
/// numbering; `ui` translates the names.
const DEFAULT_SCREEN: isize = -1;
const DEFAULT_CORNER: isize = 1;

/// Which screen the dashboard opens on, in GameKit's own numbering.
fn screen(name: &str) -> Result<isize> {
    match name {
        "default" => Ok(-1),
        "leaderboards" => Ok(0),
        "achievements" => Ok(1),
        "challenges" => Ok(2),
        "profile" => Ok(3),
        "dashboard" => Ok(4),
        "friends" => Ok(5),
        other => Err(anyhow!("no Game Center screen called {other:?}")),
    }
}

/// Which corner the access point sits in, in GameKit's own numbering.
fn corner_of(name: &str) -> Result<isize> {
    match name {
        "top_leading" => Ok(0),
        "top_trailing" => Ok(1),
        "bottom_leading" => Ok(2),
        "bottom_trailing" => Ok(3),
        other => Err(anyhow!("no access point corner called {other:?}")),
    }
}

/// The node-or-options first argument both calls take: with a node the answer
/// reaches its `on_apple` method, without one the returned id is awaited.
fn start_call(
    eng: &Engine,
    node: Option<Value>,
    opts: Option<Value>,
    call: AppleCall,
) -> Result<Value> {
    let node = node.unwrap_or(Value::Nil);
    let handler = handler_of(&node, opts.as_ref(), "on_apple", "on_apple")?;
    let id = eng.next_token();
    let state = eng.resource::<AppleState>();
    state.borrow_mut().start(eng, id, call, handler);
    Ok(id_value(id))
}
