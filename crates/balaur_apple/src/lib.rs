//! Apple platform services as a Balaur plugin: Game Center and iCloud behind
//! the portable `platform.*` verbs, notifications, push, the share sheet, the
//! iPhone's motor and the review prompt behind its device seam (`device.rs`),
//! and `apple.*` for what only Apple has.
//!
//! Everything here is asynchronous and answers on a queue the engine does not
//! own, so nothing here invents a mechanism: a call returns an id, the
//! completion crosses a channel, and [`ExternalIo`] lands it at
//! [`Stage::First`] of a later tick: recorded, replayable, waking whoever
//! awaits the id. It is `balaur_http` with
//! GameKit where the socket is.
//!
//! ```bs
//! pub async fn init() {
//!     let me = await task::wait(platform::sign_in());
//!     let proof = await task::wait(apple::identity());
//!     await client::call(self.socket, "game_center", proof);
//! }
//! ```
//!
//! Off an Apple platform the crate still compiles and every call answers
//! `unsupported`, so a script does not change shape between a phone and a CI
//! runner.

use crate::vocabulary::words;
use anyhow::Result;
use anyhow::anyhow;
use balaur_core::engine_api::from_json;
use balaur_core::handler::{id_value, opt};
use balaur_core::replay::ExternalIo;
use balaur_core::{Engine, Stage};
use balaur_platform::{Call, PlatformBackend, PlatformEvent};
use balaur_script::{Module, Value};

const ACCESS_POINT: &str = "access_point";
const APPLE: &str = "apple";
const AVAILABLE_WORD: &str = "available";
const ERROR_WORD: &str = "error";
const KIND: &str = "kind";
const LISTEN_FOR_CLOUD_CHANGES: &str = "listen_for_cloud_changes";
const LISTEN_FOR_INVITES: &str = "listen_for_invites";
const NAME: &str = "name";
const PLAYER: &str = "player";
const REQUEST: &str = "request";
const SHOW_DASHBOARD: &str = "show_dashboard";
const SIGN_IN: &str = "sign_in";
const STATE: &str = "state";
const TOKEN: &str = "token";
const URL: &str = "url";

#[cfg(test)]
mod arrival_tests;
#[cfg(target_vendor = "apple")]
mod arrivals;
#[cfg(target_vendor = "apple")]
mod device;
#[cfg(target_vendor = "apple")]
mod entitlement;
#[cfg(target_vendor = "apple")]
mod gamekit;
#[cfg(target_vendor = "apple")]
mod icloud;
#[cfg(target_vendor = "apple")]
mod notify;
#[cfg(target_vendor = "apple")]
mod photos;
#[cfg(target_vendor = "apple")]
mod push;
mod queue;
#[cfg(target_vendor = "apple")]
mod signin;
#[cfg(target_vendor = "apple")]
mod storekit;
#[cfg(target_vendor = "apple")]
mod ui;
mod vocabulary;
#[cfg(target_vendor = "apple")]
mod websignin;

#[cfg(target_vendor = "apple")]
mod backend {
    pub(crate) use crate::gamekit::{apple_call, platform_call, watch_invites};
    pub(crate) use crate::icloud::watch_changes as watch_cloud;
    pub(crate) use crate::ui::access_point;
    pub(crate) use crate::websignin::close as close_web_sign_in;

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

    pub(crate) const fn access_point(_active: bool, _location: isize) -> bool {
        false
    }

    pub(crate) const fn watch_invites() -> bool {
        false
    }

    pub(crate) const fn watch_cloud() -> bool {
        false
    }

    pub(crate) const fn close_web_sign_in() {}
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
    /// game's to keep: the engine stores no accounts.
    CredentialState { user: String },
    /// Game Center's own dashboard, opening on one of its screens.
    Dashboard { state: isize },
}

impl AppleCall {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Identity => kind::IDENTITY,
            Self::SignIn => SIGN_IN,
            Self::CredentialState { .. } => kind::CREDENTIAL_STATE,
            Self::Dashboard { .. } => SHOW_DASHBOARD,
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
    /// The player accepted a Game Center invite, from `player`. Unsolicited,
    /// like everything below it.
    InviteAccepted {
        player: String,
        name: String,
    },
    /// The player asked Game Center to start a match with these players.
    MatchRequested {
        players: Vec<String>,
    },
    /// Another device changed the iCloud key-value store: why, and which keys.
    CloudChanged {
        reason: String,
        keys: Vec<String>,
    },
    Failed {
        request: u64,
        message: String,
    },
    Unsupported {
        request: u64,
        call: String,
    },
    /// A test's answer from its stand-in file (`balaur_platform::stand_ins`),
    /// as the map the waiter receives.
    StandIn {
        request: u64,
        answer: serde_json::Value,
    },
}

impl AppleEvent {
    const fn request(&self) -> u64 {
        match self {
            Self::Identity { request, .. }
            | Self::SignedIn { request, .. }
            | Self::CredentialState { request, .. }
            | Self::DashboardClosed { request }
            | Self::Failed { request, .. }
            | Self::Unsupported { request, .. }
            | Self::StandIn { request, .. } => *request,
            // Nobody asked for these, so they carry the id no call is given.
            Self::InviteAccepted { .. }
            | Self::MatchRequested { .. }
            | Self::CloudChanged { .. } => 0,
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
        APPLE
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

    fn close_web_sign_in(&mut self) {
        backend::close_web_sign_in();
    }
}

/// What nobody asked for, carrying request 0: an invite, an iCloud change.
/// Heard with `events::listen` as `on_apple_event`; a notification or a
/// push, a link that opened the game, a push token and a transaction that
/// landed elsewhere are `platform_event`.
pub const ARRIVAL_EVENT: &str = "apple_event";

/// The `apple.*` calls in flight, and the channel the frameworks report into.
#[derive(Default)]
pub struct AppleState {
    io: ExternalIo<AppleEvent>,
}

impl AppleState {
    /// Start a call under `id`: an [`Engine::next_token`] value, so awaiting
    /// it cannot collide with another subsystem's ids.
    pub fn start(&mut self, eng: &Engine, id: u64, call: &AppleCall) {
        balaur_core::replay::event(
            eng,
            "apple.call",
            call.name().to_string(),
            Some(serde_json::json!({ "id": id, "call": call.name() })),
        );
        if let Some(answer) = balaur_platform::stand_in(eng, APPLE, call.name()) {
            self.io.start(eng, |report| {
                if let Some(answer) = answer {
                    let _ = report.send(AppleEvent::StandIn {
                        request: id,
                        answer,
                    });
                }
            });
            return;
        }
        self.io
            .start(eng, |report| backend::apple_call(id, call, report));
    }
}

/// This tick's `apple.*` arrivals, as their waiters received them.
#[derive(Default)]
pub struct AppleSnapshot {
    pub events: Vec<Value>,
}

fn pump_apple_system(eng: &Engine, _: f32) {
    #[cfg(target_vendor = "apple")]
    if arrivals::quit_asked() {
        eng.request_quit();
    }
    let mut finished: Vec<(u64, Value)> = Vec::new();
    let mut arrivals = Vec::new();
    {
        let state = eng.resource::<AppleState>();
        let snapshot = eng.resource::<AppleSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        // What arrived on its own crosses here, or nowhere: a replay is
        // answered by the file.
        if !state.io.start(eng, queue::drain_apple) {
            queue::discard_apple();
        }
        snapshot.events.clear();
        for event in state.io.drain() {
            let request = event.request();
            let value = event_value(event);
            snapshot.events.push(value.clone());
            if request == 0 {
                arrivals.push(value);
            } else {
                finished.push((request, value));
            }
        }
    }
    balaur_core::handler::wake_all(eng, finished);
    for arrival in arrivals {
        balaur_core::events::emit(eng, ARRIVAL_EVENT, arrival);
    }
}

/// The `kind` each event map carries, and the `EVENT_*` constants of it.
pub mod kind {
    pub const IDENTITY: &str = "identity";
    pub const SIGNED_IN: &str = "signed_in";
    pub const CREDENTIAL_STATE: &str = "credential_state";
    pub const DASHBOARD_CLOSED: &str = "dashboard_closed";
    pub const INVITE_ACCEPTED: &str = "invite_accepted";
    pub const MATCH_REQUESTED: &str = "match_requested";
    pub const CLOUD_CHANGED: &str = "cloud_changed";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const ERROR: &str = balaur_core::handler::ERROR;
    pub const ALL: &[&str] = &[
        IDENTITY,
        SIGNED_IN,
        CREDENTIAL_STATE,
        DASHBOARD_CLOSED,
        INVITE_ACCEPTED,
        MATCH_REQUESTED,
        CLOUD_CHANGED,
        UNSUPPORTED,
        ERROR,
    ];
}

fn event_value(event: AppleEvent) -> Value {
    let mut pairs = vec![(REQUEST.into(), id_value(event.request()))];
    match event {
        AppleEvent::Identity {
            player,
            url,
            signature,
            salt,
            timestamp,
            ..
        } => {
            pairs.push((KIND.into(), Value::Str(kind::IDENTITY.into())));
            pairs.push((PLAYER.into(), Value::Str(player)));
            pairs.push((URL.into(), Value::Str(url)));
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
            pairs.push((KIND.into(), Value::Str(kind::SIGNED_IN.into())));
            pairs.push(("user".into(), Value::Str(user)));
            pairs.push((NAME.into(), Value::Str(name)));
            pairs.push((TOKEN.into(), Value::Str(token)));
            pairs.push(("code".into(), Value::Str(code)));
            pairs.push(("email".into(), Value::Str(email)));
        }
        AppleEvent::CredentialState { state, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::CREDENTIAL_STATE.into())));
            pairs.push((STATE.into(), Value::Str(state)));
        }
        AppleEvent::DashboardClosed { .. } => {
            pairs.push((KIND.into(), Value::Str(kind::DASHBOARD_CLOSED.into())));
        }
        arrival @ (AppleEvent::InviteAccepted { .. }
        | AppleEvent::MatchRequested { .. }
        | AppleEvent::CloudChanged { .. }) => arrived(arrival, &mut pairs),
        AppleEvent::StandIn { answer, .. } => {
            if let Ok(Value::Map(fields)) = from_json(&answer) {
                pairs.extend(fields.into_iter().filter(|(key, _)| key != REQUEST));
            }
        }
        AppleEvent::Failed { message, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::ERROR.into())));
            pairs.push((ERROR_WORD.into(), Value::Str(message)));
        }
        AppleEvent::Unsupported { call, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::UNSUPPORTED.into())));
            pairs.push(("call".into(), Value::Str(call)));
        }
    }
    Value::Map(pairs)
}

/// Hand over the URL the game was launched with, `None` for none, once the
/// engine is up.
///
/// The window layer holds it, because the application delegate is handed it
/// before this crate exists. A game reads it as `url_opened` on
/// `platform_event`, as a link handed over mid-run is; one the delegate
/// heard too is heard once.
///
/// Does nothing off Apple, so a caller needs no target of its own.
#[cfg(target_vendor = "apple")]
pub fn deliver_launch_url(url: Option<String>) {
    crate::arrivals::deliver_launch_url(url);
}

/// Hand over the URL the game was launched with. Nothing does off Apple.
#[cfg(not(target_vendor = "apple"))]
pub fn deliver_launch_url(_url: Option<String>) {}

#[derive(Default)]
pub struct ApplePlugin;

impl balaur_plugin::Plugin for ApplePlugin {
    fn name(&self) -> &'static str {
        APPLE
    }

    fn requires(&self) -> Vec<String> {
        vec!["platform".to_string()]
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(AppleState::default());
        reg.insert_resource(AppleSnapshot::default());
        balaur_core::events::declare(
            reg.engine(),
            &[(
                ARRIVAL_EVENT,
                "what arrived, carrying request 0",
                "Something nobody asked for arrived: an invite, an iCloud change. A link that opened the game, a push and its token are `platform_event`.",
            )],
        );
        reg.add_system(Stage::First, pump_apple_system);
        reg.add_replay_source(APPLE, capture, restore);
        // Off Apple the stores are not there, and `platform.backend` says
        // `none` as it does in a build without this plugin.
        if backend::AVAILABLE {
            balaur_platform::set_backend(reg.engine(), Box::new(AppleBackend))?;
        }
        // Notifications and the rest are the OS's, whichever store is loaded.
        #[cfg(target_vendor = "apple")]
        {
            balaur_platform::set_device(reg.engine(), Box::new(device::AppleDevice))?;
            device::provide(reg.engine())?;
            // Before launch ends, so the tap or link that launched the game
            // is heard; the notification centre throws with no bundle id.
            if gamekit::bundled() {
                notify::watch_taps();
                arrivals::watch_launch();
            }
        }
        let mut m = reg.script_module(APPLE)?;
        install_apple_api(&mut m);
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
fn install_apple_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Apple services beyond `platform.*`: Game Center `identity`, Sign in with Apple, Game Center's screens. Calls answer `task::wait` on a later tick as a map whose `kind` is an `EVENT_*` constant. What arrives unasked is the `apple_event` event, `events::listen(node, \"apple_event\")`: an invite, an iCloud change. A link that opened the game is `url_opened` on `platform_event`. Notifications are `platform.notify`, push `platform.register_for_push`, and purchases `platform.purchase` and the rest.",
    );
    balaur_core::handler::install_event_kinds(m, kind::ALL);
    m.describe(&[
        (
            AVAILABLE_WORD,
            &[],
            "() -> bool",
            "Whether this build has Apple's frameworks behind it.",
        ),
        (
            kind::IDENTITY,
            &[],
            "() -> int",
            "Fetch the items a server needs to verify this Game Center player, and return the id the answer carries.",
        ),
        (
            SIGN_IN,
            &[],
            "() -> int",
            "Sign in with Apple, and return the id the identity token comes back on. Game Center's own sign-in is `platform.sign_in`.",
        ),
        (
            kind::CREDENTIAL_STATE,
            &[],
            "(user: string) -> int",
            "Ask whether a saved Sign in with Apple account is still authorized, revoked, transferred or unknown, and return the id the answer carries.",
        ),
    ]);
    m.function(AVAILABLE_WORD, |_: &Engine, ()| Ok(Value::Bool(AVAILABLE)));
    m.function(kind::IDENTITY, |eng: &Engine, ()| {
        Ok(start_call(eng, &AppleCall::Identity))
    });
    m.function(SIGN_IN, |eng: &Engine, ()| {
        Ok(start_call(eng, &AppleCall::SignIn))
    });
    m.function(kind::CREDENTIAL_STATE, |eng: &Engine, user: Value| {
        let Value::Str(user) = user else {
            return Err(anyhow!("the user id should be a string, got {user:?}"));
        };
        Ok(start_call(eng, &AppleCall::CredentialState { user }))
    });
    install_screens_api(m);
    install_listen_for_api(m);
}

/// Game Center's own screens.
fn install_screens_api(m: &mut Module<Engine>) {
    m.records(&[
        (
            "ShowDashboardOptions",
            "{ state: string? }",
            "The page Game Center's dashboard opens on: default, leaderboards, achievements, challenges, profile, dashboard or friends.",
        ),
        (
            "AccessPointOptions",
            "{ location: string? }",
            "The corner Game Center's access point sits in: top_leading, top_trailing, bottom_leading or bottom_trailing.",
        ),
    ]);
    m.describe(&[
        (
            SHOW_DASHBOARD,
            &[],
            "(opts: ShowDashboardOptions?) -> int",
            "Open Game Center's dashboard over the game, and return the id its `dashboard_closed` answer carries.",
        ),
        (
            ACCESS_POINT,
            &[],
            "(active: bool, opts: AccessPointOptions?) -> bool",
            "Show or hide Game Center's floating access point, and answer whether it is showing.",
        ),
    ]);
    m.function(SHOW_DASHBOARD, |eng: &Engine, opts: Option<Value>| {
        let state = match opt(opts.as_ref(), STATE) {
            Some(Value::Str(name)) => screen(name)?,
            Some(other) => return Err(anyhow!("`state` should be a string, got {other:?}")),
            None => DEFAULT_SCREEN,
        };
        Ok(start_call(eng, &AppleCall::Dashboard { state }))
    });
    m.function(
        ACCESS_POINT,
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

/// What an invite or an iCloud change carries. Split from [`event_value`]
/// under `MAX_FN_LINES`.
fn arrived(event: AppleEvent, pairs: &mut Vec<(String, Value)>) {
    match event {
        AppleEvent::InviteAccepted { player, name } => {
            pairs.push((KIND.into(), Value::Str(kind::INVITE_ACCEPTED.into())));
            pairs.push((PLAYER.into(), Value::Str(player)));
            pairs.push((NAME.into(), Value::Str(name)));
        }
        AppleEvent::MatchRequested { players } => {
            pairs.push((KIND.into(), Value::Str(kind::MATCH_REQUESTED.into())));
            let players = players.into_iter().map(Value::Str).collect();
            pairs.push(("players".into(), Value::List(players)));
        }
        AppleEvent::CloudChanged { reason, keys } => {
            pairs.push((KIND.into(), Value::Str(kind::CLOUD_CHANGED.into())));
            pairs.push(("reason".into(), Value::Str(reason)));
            let keys = keys.into_iter().map(Value::Str).collect();
            pairs.push(("keys".into(), Value::List(keys)));
        }
        _ => {}
    }
}

/// The arrivals a game has to ask to hear, each installing what hears it.
fn install_listen_for_api(m: &mut Module<Engine>) {
    m.describe(&[
        (
            LISTEN_FOR_INVITES,
            &[],
            "() -> bool",
            "Hear about Game Center invites the player accepts, as `invite_accepted` with the sender's `player` id and `name`, and matches they ask for, as `match_requested` with the `players`.",
        ),
        (
            LISTEN_FOR_CLOUD_CHANGES,
            &[],
            "() -> bool",
            "Hear about another device changing the iCloud key-value store, as `cloud_changed` with the `reason` and the `keys` it touched.",
        ),
    ]);
    m.function(LISTEN_FOR_INVITES, |_: &Engine, ()| {
        Ok(Value::Bool(backend::watch_invites()))
    });
    m.function(LISTEN_FOR_CLOUD_CHANGES, |_: &Engine, ()| {
        Ok(Value::Bool(backend::watch_cloud()))
    });
}

/// Where the dashboard opens when a call names no screen, and where the
/// access point sits when it names no corner. Both are GameKit's own
/// numbering; `ui` translates the names.
const DEFAULT_SCREEN: isize = -1;
const DEFAULT_CORNER: isize = 1;

/// Which screen the dashboard opens on, in GameKit's own numbering.
fn screen(name: &str) -> Result<isize> {
    match name {
        words::DEFAULT => Ok(-1),
        words::LEADERBOARDS => Ok(0),
        words::ACHIEVEMENTS => Ok(1),
        words::CHALLENGES => Ok(2),
        words::PROFILE => Ok(3),
        words::DASHBOARD => Ok(4),
        words::FRIENDS => Ok(5),
        other => Err(anyhow!("no Game Center screen called {other:?}")),
    }
}

/// Which corner the access point sits in, in GameKit's own numbering.
fn corner_of(name: &str) -> Result<isize> {
    match name {
        words::TOP_LEADING => Ok(0),
        words::TOP_TRAILING => Ok(1),
        words::BOTTOM_LEADING => Ok(2),
        words::BOTTOM_TRAILING => Ok(3),
        other => Err(anyhow!("no access point corner called {other:?}")),
    }
}

/// Start a call, and answer the id `task::wait` hears it on.
fn start_call(eng: &Engine, call: &AppleCall) -> Value {
    let id = eng.next_token();
    let state = eng.resource::<AppleState>();
    state.borrow_mut().start(eng, id, call);
    id_value(id)
}

#[cfg(test)]
mod tests {
    use super::{corner_of, screen};

    #[test]
    fn every_dashboard_screen_carries_gamekits_own_number() {
        assert_eq!(screen("default").unwrap(), -1);
        assert_eq!(screen("achievements").unwrap(), 1);
        assert_eq!(screen("friends").unwrap(), 5);
    }

    #[test]
    fn a_screen_nobody_has_is_named_back_in_the_error() {
        let err = screen("acheivements").unwrap_err().to_string();
        assert!(err.contains("acheivements"), "{err}");
    }

    #[test]
    fn an_access_point_corner_is_one_of_four() {
        assert_eq!(corner_of("top_trailing").unwrap(), 1);
        let err = corner_of("middle").unwrap_err().to_string();
        assert!(err.contains("middle"), "{err}");
    }
}
