//! Store and platform services, portable: `platform.*` for scripts.
//!
//! One module over every store, so a script that unlocks an achievement says
//! the same thing on Game Center, on Play Games and on Steam. What only one
//! platform has stays in that platform's own module (`apple.*`), and with no
//! backend loaded (the editor, a desktop dev run, a replay) every call here
//! resolves to a `kind = "unsupported"` event rather than an error, so the
//! script still runs.
//!
//! Delivery is the engine's usual one: a call returns an id immediately, the
//! backend reports on a channel, and [`ExternalIo`] lands the result at
//! [`Stage::First`] of a later tick: recorded, replayable, and waking whatever
//! awaits the id. Who is playing can change with nobody asking, so a sign-in or
//! a sign-out is also the `platform_event` event.
//!
//! ```bs
//! pub async fn init() {
//!     events::listen(self.node, "platform_event");
//!     let player = await task::wait(platform::sign_in());
//!     let done = await task::wait(platform::unlock("first_blood"));
//!     if done["kind"].as_str() == platform::EVENT_UNSUPPORTED {
//!         log::info("no store here");
//!     }
//! }
//! ```
//!
//! Every event map carries a `kind`.
//!
//! A sign-in page, `web_sign_in`, is the OS's own sheet where the backend
//! has one (Apple's `ASWebAuthenticationSession`), a Custom Tab on Android
//! with no store loaded (`android/web_sign_in.rs`), and `unsupported`
//! elsewhere, so a game opens the page in the browser there instead.
//!
//! What the OS lends a game beside the store, whichever store is loaded,
//! sits behind a second seam ([`device`]): local notifications here, and the
//! motor, the share sheet and the review prompt as `device.*`. Whether any
//! service works in this run is `is_available` ([`services`]).
//!
//! A test answers any call from a file instead ([`stand_ins`]).

use std::sync::mpsc::Sender;

use crate::vocabulary::{keys as k, words};
use anyhow::{Result, anyhow};
use balaur_core::handler::{id_value, opt};
use balaur_core::replay::ExternalIo;
use balaur_core::{Engine, Stage};
use balaur_script::Value;

const ALIAS: &str = "alias";
const BACKEND: &str = "backend";
const CALL: &str = "call";
const CLOUD_READ: &str = "cloud_read";
const CLOSE_WEB_SIGN_IN: &str = "close_web_sign_in";
const IDENTITY: &str = "identity";
const CLOUD_WRITE: &str = "cloud_write";
const ID: &str = k::ID;
const KIND: &str = "kind";
const PLATFORM: &str = "platform";
const PLAYER: &str = "player";
const PROGRESS: &str = "progress";
const REQUEST: &str = "request";
const SET_PRESENCE: &str = "set_presence";
const SIGN_IN: &str = "sign_in";
const SUBMIT_SCORE: &str = "submit_score";
const UNLOCK: &str = "unlock";
const URL: &str = "url";
const WEB_SIGN_IN_STATE: &str = "web_sign_in_state";
/// What `web_sign_in_state` answers before any page was asked for.
pub const PAGE_NONE: &str = "none";
/// What it answers while the page asked for last has not answered.
pub const PAGE_OPEN: &str = "open";
#[cfg(target_os = "android")]
mod android;
pub mod device;
mod native;
pub mod photos;
mod purchases;
mod script_api;
pub mod secrets;
pub mod services;
pub mod stand_ins;
mod vocabulary;
#[cfg(target_family = "wasm")]
mod web;

#[cfg(target_os = "android")]
pub use android::{capability_available, keep_activity, shim, start_capability, with_activity};

pub use device::{DEVICE, DeviceBackend, DeviceCall, Haptic, set_device};
pub use native::start_natively;
pub use services::{Provider, Service, is_available, provide};
pub use stand_ins::{StandIns, set_stand_ins, stand_in};

/// How many leaderboard entries `platform.scores` reads when the call names
/// no `count`.
const DEFAULT_SCORES: u32 = 10;

/// One call on its way to a backend.
#[derive(Clone, Debug)]
pub enum Call {
    SignIn,
    Unlock {
        achievement: String,
    },
    Progress {
        achievement: String,
        percent: f64,
    },
    SubmitScore {
        board: String,
        score: i64,
    },
    Scores {
        board: String,
        count: u32,
        scope: Scope,
        period: Period,
        /// The rank to read from, counting from 1.
        start: u32,
    },
    CloudRead {
        key: String,
    },
    CloudWrite {
        key: String,
        value: String,
    },
    SetPresence {
        text: String,
    },
    /// A sign-in page in the OS's own sheet, over the game: a provider's
    /// page the player signs in on. `scheme`, when given, is the custom
    /// scheme the page ends on, whose URL the answer carries.
    WebSignIn {
        url: String,
        scheme: Option<String>,
    },
    /// A proof of who is playing that a server checks with the store:
    /// Steam's Web API ticket, Play Games' one-time server code.
    Identity,
    /// What the store knows of these product ids.
    Products {
        ids: Vec<String>,
    },
    /// Buy one product, in the store's own sheet.
    Purchase {
        product: String,
    },
    /// Tell the store a purchase is dealt with: StoreKit finishes it, Play
    /// acknowledges it, or consumes it so it can be bought again. Until then
    /// it comes back on every launch, and Play refunds it after three days.
    FinishPurchase {
        transaction: String,
        consume: bool,
    },
    /// Ask the store to hand this player's purchases back.
    RestorePurchases,
    /// What this player owns now.
    Entitlements,
}

impl Call {
    /// The name an `unsupported` event carries, and what the recording logs.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::SignIn => SIGN_IN,
            Self::Unlock { .. } => UNLOCK,
            Self::Progress { .. } => PROGRESS,
            Self::SubmitScore { .. } => SUBMIT_SCORE,
            Self::Scores { .. } => kind::SCORES,
            Self::CloudRead { .. } => CLOUD_READ,
            Self::CloudWrite { .. } => CLOUD_WRITE,
            Self::SetPresence { .. } => SET_PRESENCE,
            Self::WebSignIn { .. } => kind::WEB_SIGN_IN,
            Self::Identity => IDENTITY,
            Self::Products { .. } => kind::PRODUCTS,
            Self::Purchase { .. } => purchases::PURCHASE,
            Self::FinishPurchase { .. } => purchases::FINISH_PURCHASE,
            Self::RestorePurchases => purchases::RESTORE_PURCHASES,
            Self::Entitlements => kind::ENTITLEMENTS,
        }
    }

    /// Whether this call changes something a rollback cannot take back.
    ///
    /// A read may be issued from a tick that is re-run later: the worst it
    /// costs is a second read. An unlock may not: the achievement is in the
    /// player's profile and the correction cannot remove it.
    #[must_use]
    pub const fn writes(&self) -> bool {
        matches!(
            self,
            Self::Unlock { .. }
                | Self::Progress { .. }
                | Self::SubmitScore { .. }
                | Self::CloudWrite { .. }
                | Self::SetPresence { .. }
                | Self::Purchase { .. }
                | Self::FinishPurchase { .. }
        )
    }
}

/// Whose scores a leaderboard read returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Global,
    Friends,
}

/// How far back a leaderboard read reaches. A store that keeps one table per
/// leaderboard and no others answers the same thing whatever this says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Period {
    Today,
    Week,
    #[default]
    AllTime,
}

impl Scope {
    /// The word a script writes for it, which a Java shim reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Global => words::GLOBAL,
            Self::Friends => words::FRIENDS,
        }
    }

    fn parse(text: &str) -> Result<Self> {
        match text {
            words::GLOBAL => Ok(Self::Global),
            words::FRIENDS => Ok(Self::Friends),
            other => Err(anyhow!(
                "`scope` should be \"global\" or \"friends\", got {other:?}"
            )),
        }
    }
}

impl Period {
    /// The word a script writes for it, which a Java shim reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Today => words::TODAY,
            Self::Week => words::WEEK,
            Self::AllTime => words::ALL_TIME,
        }
    }

    fn parse(text: &str) -> Result<Self> {
        match text {
            words::TODAY => Ok(Self::Today),
            words::WEEK => Ok(Self::Week),
            words::ALL_TIME => Ok(Self::AllTime),
            other => Err(anyhow!(
                "`period` should be \"today\", \"week\" or \"all_time\", got {other:?}"
            )),
        }
    }
}

/// Who the store says is playing.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Player {
    /// The store's own id, stable for this player on this platform.
    pub id: String,
    /// What to show on screen.
    pub alias: String,
}

/// One row of a leaderboard.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Score {
    pub player: String,
    pub alias: String,
    pub rank: i64,
    pub score: i64,
}

/// A device's push token as a push server registers it: Gamend's
/// `POST /me/push_tokens` takes these three fields as they are.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PushToken {
    /// APNs' device token in hex, or FCM's registration token.
    pub token: String,
    /// Who delivers to it: `apns` or `fcm`.
    pub provider: String,
    /// Where it was registered from: `ios` (a Mac's too, APNs having one
    /// value for Apple) or `android`.
    pub platform: String,
}

impl PushToken {
    /// An APNs device token, hex.
    #[must_use]
    pub fn apns(token: String) -> Self {
        Self {
            token,
            provider: words::push::APNS.into(),
            platform: words::push::IOS.into(),
        }
    }

    /// An FCM registration token.
    #[must_use]
    pub fn fcm(token: String) -> Self {
        Self {
            token,
            provider: words::push::FCM.into(),
            platform: words::push::ANDROID.into(),
        }
    }
}

/// What a backend reports back, crossing from wherever it runs into the
/// frame loop.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum PlatformEvent {
    SignedIn {
        request: u64,
        player: Player,
    },
    /// The player signed out, or the store lost them. Unsolicited: it
    /// arrives under request 0, which no call is ever given.
    SignedOut {
        request: u64,
    },
    /// A write the store accepted: an unlock, a score, a cloud file.
    Done {
        request: u64,
        call: String,
    },
    Scores {
        request: u64,
        board: String,
        entries: Vec<Score>,
    },
    Read {
        request: u64,
        key: String,
        value: Option<String>,
    },
    Failed {
        request: u64,
        message: String,
    },
    /// The loaded backend, or the absence of one, does not have this call.
    Unsupported {
        request: u64,
        call: String,
    },
    /// A sign-in page ended on its scheme: the URL it ended on.
    WebSignIn {
        request: u64,
        url: String,
    },
    /// A sign-in page was shut, by the player or by `close_web_sign_in`.
    Cancelled {
        request: u64,
    },
    /// An answer already shaped as the map its waiter receives, `kind` and
    /// all: a test's stand-in ([`stand_ins`]), or a Java shim's JSON.
    Answer {
        request: u64,
        answer: serde_json::Value,
    },
    /// A store's proof of who is playing, its fields as the store names
    /// them: Steam's `ticket`, hex.
    Identity {
        request: u64,
        proof: serde_json::Value,
    },
    /// Whether the player allowed notifications.
    Notifications {
        request: u64,
        allowed: bool,
    },
    /// A notification is on the schedule.
    Scheduled {
        request: u64,
        id: String,
    },
    /// A notification was tapped, with what the game put in it. Unsolicited,
    /// as the one below: request 0.
    NotificationOpened {
        id: String,
        data: serde_json::Value,
    },
    /// A notification arrived while the game was in front, and was shown.
    NotificationReceived {
        id: String,
        title: String,
        body: String,
        data: serde_json::Value,
    },
    /// The share sheet handed its items on: to `target` where the OS says.
    Shared {
        request: u64,
        target: String,
    },
    /// The picture the player picked, as the file's bytes and their media
    /// type: `image/jpeg`, `image/png`.
    Picked {
        request: u64,
        bytes: Vec<u8>,
        media_type: String,
    },
    /// The device's push token, for `register_for_push`.
    PushRegistered {
        request: u64,
        token: PushToken,
    },
    /// The OS would not hand a push token over, or the build cannot ask.
    PushFailed {
        request: u64,
        message: String,
    },
    /// A link opened the game, or reached it while it ran: a URL on one of
    /// its schemes, or a universal link on one of its associated domains.
    /// The link it was launched with arrives the same way, once the engine
    /// is up. Unsolicited: request 0.
    UrlOpened {
        url: String,
    },
    /// The OS replaced the device's push token. Unsolicited: request 0.
    PushTokenChanged {
        token: PushToken,
    },
    /// A push reached the game while it was in front, or a silent one
    /// reached it at all: the push's `id`, what its notification said, and
    /// its `data`. Unsolicited: request 0.
    PushReceived {
        id: String,
        title: String,
        body: String,
        data: serde_json::Value,
    },
}

impl PlatformEvent {
    const fn request(&self) -> u64 {
        match self {
            Self::SignedIn { request, .. }
            | Self::SignedOut { request }
            | Self::Done { request, .. }
            | Self::Scores { request, .. }
            | Self::Read { request, .. }
            | Self::Failed { request, .. }
            | Self::Unsupported { request, .. }
            | Self::WebSignIn { request, .. }
            | Self::Cancelled { request }
            | Self::Answer { request, .. }
            | Self::Identity { request, .. }
            | Self::Notifications { request, .. }
            | Self::Scheduled { request, .. }
            | Self::Shared { request, .. }
            | Self::Picked { request, .. }
            | Self::PushRegistered { request, .. }
            | Self::PushFailed { request, .. } => *request,
            Self::NotificationOpened { .. }
            | Self::NotificationReceived { .. }
            | Self::UrlOpened { .. }
            | Self::PushTokenChanged { .. }
            | Self::PushReceived { .. } => 0,
        }
    }
}

/// What a store's plugin implements. One trait, so `platform.*` never names
/// a store and a game never branches on one.
pub trait PlatformBackend {
    /// What `platform.backend()` answers.
    fn name(&self) -> &'static str;

    /// Start one call. Everything it produces goes down `report`, at whatever
    /// later moment the store answers.
    fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>);

    /// Whether this store answers `call`, or leaves it to what answers with
    /// no store loaded: Play leaves a sign-in page to the Custom Tab.
    fn serves(&self, _call: &Call) -> bool {
        true
    }

    /// Once per tick, for a backend whose SDK dispatches its own callbacks
    /// rather than pushing from a thread.
    fn pump(&mut self, _report: &Sender<PlatformEvent>) {}

    /// Called instead of [`PlatformBackend::pump`] on a tick the engine may
    /// not reach the outside world on: a replay, or a re-simulation. What
    /// arrived meanwhile is not this run's to deliver.
    fn discard(&mut self) {}

    /// Shut the sign-in page `web_sign_in` opened, if one is open. Its call
    /// answers `cancelled`.
    fn close_web_sign_in(&mut self) {}
}

/// A call to the store or to the OS, as it waits in `pending`.
#[derive(Clone)]
enum Outward {
    Store(Call),
    Device(DeviceCall),
}

impl Outward {
    const fn name(&self) -> &'static str {
        match self {
            Self::Store(call) => call.name(),
            Self::Device(call) => call.name(),
        }
    }

    const fn module(&self) -> &'static str {
        match self {
            Self::Store(_) => PLATFORM,
            Self::Device(call) => call.module(),
        }
    }

    const fn writes(&self) -> bool {
        match self {
            Self::Store(call) => call.writes(),
            Self::Device(call) => call.writes(),
        }
    }
}

/// The calls in flight, the channel every backend reports into, and the
/// writes waiting for the tick that made them to become final.
pub struct PlatformState {
    io: ExternalIo<PlatformEvent>,
    backend: Option<Box<dyn PlatformBackend>>,
    /// The OS's services: this crate's own until a plugin's replace them.
    device: Box<dyn DeviceBackend>,
    services: services::Services,
    /// `(tick, request, call)` for writes made on a tick that could still be
    /// rolled back. See [`Call::writes`].
    pending: Vec<(u64, u64, Outward)>,
    player: Option<Player>,
    /// The sign-in page asked for last, and the `kind` it answered: empty
    /// while it has not.
    page: Option<(u64, String)>,
}

impl Default for PlatformState {
    fn default() -> Self {
        Self {
            io: ExternalIo::default(),
            backend: None,
            device: Box::new(native::Native),
            services: services::Services::default(),
            pending: Vec::new(),
            player: None,
            page: None,
        }
    }
}

impl PlatformState {
    /// Take over the portable verbs. The last plugin to call this wins, which
    /// is what a build with one store loaded means.
    pub fn set_backend(&mut self, backend: Box<dyn PlatformBackend>) {
        self.backend = Some(backend);
    }

    /// The loaded backend's name, or `"none"`.
    #[must_use]
    pub fn backend_name(&self) -> &'static str {
        self.backend.as_ref().map_or(NONE, |b| b.name())
    }

    /// Who the store said is playing, once a `sign_in` has landed.
    #[must_use]
    pub const fn player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    /// How the sign-in page asked for last stands: [`PAGE_NONE`] before
    /// any, [`PAGE_OPEN`] while it has not answered, else the `kind` it
    /// answered (`web_sign_in`, `cancelled`, `unsupported`, `error`).
    #[must_use]
    pub fn web_sign_in_state(&self) -> &str {
        match &self.page {
            None => PAGE_NONE,
            Some((_, kind)) if kind.is_empty() => PAGE_OPEN,
            Some((_, kind)) => kind,
        }
    }

    /// Start a call under `id`: an [`Engine::next_token`] value, so awaiting
    /// it cannot collide with another subsystem's ids.
    ///
    /// A write made on a tick that a late input could still roll back waits
    /// in this state's own `pending` list until that tick is final; everything
    /// else goes out now.
    pub fn start(&mut self, eng: &Engine, id: u64, call: Call) {
        if matches!(call, Call::WebSignIn { .. }) {
            self.page = Some((id, String::new()));
        }
        self.begin(eng, id, Outward::Store(call));
    }

    /// Start a call to the OS under `id`, held as a store's write is when it
    /// writes ([`DeviceCall::writes`]).
    pub fn start_device(&mut self, eng: &Engine, id: u64, call: DeviceCall) {
        self.begin(eng, id, Outward::Device(call));
    }

    fn begin(&mut self, eng: &Engine, id: u64, call: Outward) {
        balaur_core::replay::event(
            eng,
            "platform.call",
            call.name().to_string(),
            Some(serde_json::json!({ "id": id, "call": call.name() })),
        );
        let clock = balaur_core::rollback::clock(eng);
        if call.writes() && clock.tick >= clock.settled {
            self.pending.push((clock.tick, id, call));
            return;
        }
        self.issue(eng, id, &call);
    }

    fn issue(&mut self, eng: &Engine, id: u64, call: &Outward) {
        let Self {
            io,
            backend,
            device,
            ..
        } = self;
        if let Some(answer) = stand_in(eng, call.module(), call.name()) {
            io.start(eng, |report| {
                if let Some(answer) = answer {
                    let _ = report.send(PlatformEvent::from_answer(id, answer));
                }
            });
            return;
        }
        io.start(eng, |report| match (call, backend) {
            (Outward::Store(call), Some(backend)) if backend.serves(call) => {
                backend.start(id, call, report);
            }
            (Outward::Store(call), _) => without_backend(id, call, report),
            (Outward::Device(call), _) => device.start(id, call, report),
        });
    }

    /// Drop a scheduled notification. Never during a replay: the recording
    /// is not the device's schedule.
    pub fn cancel_notification(&mut self, eng: &Engine, id: &str) {
        if !balaur_core::replay::suppressed(eng) {
            self.device.cancel_notification(id);
        }
    }

    /// Buzz the motor. Output, as rumble is: never on a replayed or a
    /// re-simulated tick, which would buzz twice.
    pub fn vibrate(&mut self, eng: &Engine, seconds: f64) {
        if !balaur_core::replay::suppressed(eng) {
            self.device.vibrate(seconds);
        }
    }

    /// One tap of feedback, held to what [`PlatformState::vibrate`] is.
    pub fn haptic(&mut self, eng: &Engine, effect: Haptic) {
        if !balaur_core::replay::suppressed(eng) {
            self.device.haptic(effect);
        }
    }

    /// Shut the open sign-in page. Never during a replay: the recording
    /// says when it shut.
    pub fn close_web_sign_in(&mut self, eng: &Engine) {
        if balaur_core::replay::suppressed(eng) {
            return;
        }
        let page = Call::WebSignIn {
            url: String::new(),
            scheme: None,
        };
        match &mut self.backend {
            Some(backend) if backend.serves(&page) => backend.close_web_sign_in(),
            #[cfg(target_os = "android")]
            _ => android::web_sign_in::close(),
            #[cfg(not(target_os = "android"))]
            _ => {}
        }
    }

    /// Issue every held write whose tick nothing can send the session back
    /// to. Never during a replay or a re-simulation: the recording answers
    /// there, and the outside world is not touched.
    fn release(&mut self, eng: &Engine, settled: u64) {
        if self.pending.is_empty() || balaur_core::replay::suppressed(eng) {
            return;
        }
        let mut ready: Vec<(u64, Outward)> = Vec::new();
        self.pending.retain(|(tick, id, call)| {
            let final_tick = *tick < settled;
            if final_tick {
                ready.push((*id, call.clone()));
            }
            !final_tick
        });
        for (id, call) in ready {
            self.issue(eng, id, &call);
        }
    }
}

/// A call with no store loaded: a sign-in page in a Custom Tab on Android,
/// and `unsupported` for everything else.
fn without_backend(id: u64, call: &Call, report: &Sender<PlatformEvent>) {
    #[cfg(target_os = "android")]
    if let Call::WebSignIn { url, scheme } = call {
        android::web_sign_in::open(id, url, scheme.as_deref(), report);
        return;
    }
    let _ = report.send(PlatformEvent::Unsupported {
        request: id,
        call: call.name().to_string(),
    });
}

/// This tick's arrivals, as the neutral values the handlers received.
///
/// Scripts never read it; it exists so Rust code can observe what the store
/// answered, and so a test has one place to look.
#[derive(Default)]
pub struct PlatformSnapshot {
    pub events: Vec<Value>,
}

/// What changes or arrives with nobody asking, heard with `events::listen` as
/// `on_platform_event`: who is playing, when the player signs out in the OS or
/// signs in as someone else, a notification or a push tapped or shown in
/// front, a link that opened the game, a push token the OS replaced, and a
/// purchase that landed with nobody asking.
pub const ARRIVAL_EVENT: &str = "platform_event";

/// The loaded store's name, or `"none"`: what a provider asks when a service
/// is the store's.
#[must_use]
pub fn backend_name(eng: &Engine) -> &'static str {
    eng.try_resource::<PlatformState>()
        .map_or(NONE, |state| state.borrow().backend_name())
}

const NONE: &str = "none";

/// Drain the backend's reports, record them in the frame's snapshot, then
/// wake what awaits each, in arrival order throughout.
fn pump_platform_system(eng: &Engine, _: f32) {
    let clock = balaur_core::rollback::clock(eng);
    let mut finished: Vec<(u64, Value)> = Vec::new();
    let mut arrivals = Vec::new();
    {
        let state = eng.resource::<PlatformState>();
        let snapshot = eng.resource::<PlatformSnapshot>();
        let mut state = state.borrow_mut();
        let mut snapshot = snapshot.borrow_mut();
        // A tick being re-run queues its writes again, so whatever this tick
        // or a later one left behind belongs to the run being replaced.
        state.pending.retain(|(tick, ..)| *tick < clock.tick);
        state.release(eng, clock.settled);
        {
            let PlatformState {
                io,
                backend,
                device,
                ..
            } = &mut *state;
            if let Some(backend) = backend
                && !io.start(eng, |report| backend.pump(report))
            {
                backend.discard();
            }
            if !io.start(eng, |report| device.pump(report)) {
                device.discard();
            }
        }
        snapshot.events.clear();
        for event in state.io.drain() {
            let request = event.request();
            // Who is playing, and whatever arrived unasked under request 0.
            let announcement = match &event {
                PlatformEvent::SignedIn { player, .. } => {
                    state.player = Some(player.clone());
                    true
                }
                PlatformEvent::SignedOut { .. } => {
                    state.player = None;
                    true
                }
                // A Java shim's sign-in, or a stand-in's, says who as a map.
                PlatformEvent::Answer { answer, .. } => {
                    match answer.get(KIND).and_then(serde_json::Value::as_str) {
                        Some(kind::SIGNED_IN) => {
                            state.player = answered_player(answer);
                            true
                        }
                        Some(kind::SIGNED_OUT) => {
                            state.player = None;
                            true
                        }
                        _ => request == 0,
                    }
                }
                _ => request == 0,
            };
            let value = event_value(event);
            if let Some((page, kind)) = &mut state.page
                && *page == request
                && let Value::Map(pairs) = &value
                && let Some((_, Value::Str(answered))) = pairs.iter().find(|(key, _)| key == KIND)
            {
                kind.clone_from(answered);
            }
            snapshot.events.push(value.clone());
            if announcement {
                arrivals.push(value.clone());
            }
            finished.push((request, value));
        }
    }
    for arrival in arrivals {
        balaur_core::events::emit(eng, ARRIVAL_EVENT, arrival);
    }
    balaur_core::handler::wake_all(eng, finished);
}

/// The player a `signed_in` map names, `player` or its own `id` and `alias`.
fn answered_player(answer: &serde_json::Value) -> Option<Player> {
    let who = answer
        .get(PLAYER)
        .filter(|p| p.is_object())
        .unwrap_or(answer);
    let field = |key: &str| {
        who.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    Some(Player {
        id: field(ID)?,
        alias: field(ALIAS).unwrap_or_default(),
    })
}

fn player_value(player: &Player) -> Value {
    Value::Map(vec![
        (ID.into(), Value::Str(player.id.clone())),
        (ALIAS.into(), Value::Str(player.alias.clone())),
    ])
}

/// The `kind` each event map carries, and the `EVENT_*` constants of it.
pub mod kind {
    pub const SIGNED_IN: &str = "signed_in";
    pub const SIGNED_OUT: &str = "signed_out";
    pub const DONE: &str = "done";
    pub const SCORES: &str = "scores";
    pub const READ: &str = "read";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const WEB_SIGN_IN: &str = "web_sign_in";
    pub const CANCELLED: &str = "cancelled";
    pub const IDENTITY: &str = super::IDENTITY;
    pub const NOTIFICATIONS: &str = "notifications";
    pub const SCHEDULED: &str = "scheduled";
    pub const NOTIFICATION_OPENED: &str = "notification_opened";
    pub const NOTIFICATION_RECEIVED: &str = "notification_received";
    pub const SHARED: &str = "shared";
    pub const PICKED: &str = "picked";
    pub const URL_OPENED: &str = "url_opened";
    pub const PRODUCTS: &str = "products";
    pub const PURCHASED: &str = "purchased";
    pub const PENDING: &str = "pending";
    pub const ENTITLEMENTS: &str = "entitlements";
    pub const RESTORED: &str = "restored";
    pub const FINISHED: &str = "finished";
    pub const TRANSACTION: &str = "transaction";
    pub const PUSH_REGISTERED: &str = "push_registered";
    pub const PUSH_FAILED: &str = "push_failed";
    pub const PUSH_TOKEN_CHANGED: &str = "push_token_changed";
    pub const PUSH_RECEIVED: &str = "push_received";
    pub const ERROR: &str = balaur_core::handler::ERROR;
    pub const ALL: &[&str] = &[
        SIGNED_IN,
        SIGNED_OUT,
        DONE,
        SCORES,
        READ,
        UNSUPPORTED,
        WEB_SIGN_IN,
        CANCELLED,
        IDENTITY,
        NOTIFICATIONS,
        SCHEDULED,
        NOTIFICATION_OPENED,
        NOTIFICATION_RECEIVED,
        URL_OPENED,
        PRODUCTS,
        PURCHASED,
        PENDING,
        ENTITLEMENTS,
        RESTORED,
        FINISHED,
        TRANSACTION,
        PUSH_REGISTERED,
        PUSH_FAILED,
        PUSH_TOKEN_CHANGED,
        PUSH_RECEIVED,
        ERROR,
    ];
    /// What `device.*` answers.
    pub const DEVICE: &[&str] = &[SHARED, PICKED, DONE, CANCELLED, UNSUPPORTED, ERROR];
}

fn event_value(event: PlatformEvent) -> Value {
    let mut pairs = vec![(REQUEST.into(), id_value(event.request()))];
    match event {
        PlatformEvent::SignedIn { player, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::SIGNED_IN.into())));
            pairs.push((PLAYER.into(), player_value(&player)));
            pairs.push((ID.into(), Value::Str(player.id)));
            pairs.push((ALIAS.into(), Value::Str(player.alias)));
        }
        PlatformEvent::SignedOut { .. } => {
            pairs.push((KIND.into(), Value::Str(kind::SIGNED_OUT.into())));
        }
        PlatformEvent::Done { call, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::DONE.into())));
            pairs.push((CALL.into(), Value::Str(call)));
        }
        PlatformEvent::Scores { board, entries, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::SCORES.into())));
            pairs.push(("board".into(), Value::Str(board)));
            pairs.push((
                "entries".into(),
                Value::List(
                    entries
                        .into_iter()
                        .map(|entry| {
                            Value::Map(vec![
                                (PLAYER.into(), Value::Str(entry.player)),
                                (ALIAS.into(), Value::Str(entry.alias)),
                                ("rank".into(), Value::Int(entry.rank)),
                                ("score".into(), Value::Int(entry.score)),
                            ])
                        })
                        .collect(),
                ),
            ));
        }
        PlatformEvent::Read { key, value, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::READ.into())));
            pairs.push(("key".into(), Value::Str(key)));
            pairs.push(("value".into(), value.map_or(Value::Nil, Value::text)));
        }
        PlatformEvent::Failed { message, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::ERROR.into())));
            pairs.push(("error".into(), Value::Str(message)));
        }
        PlatformEvent::Unsupported { call, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::UNSUPPORTED.into())));
            pairs.push((CALL.into(), Value::Str(call)));
        }
        PlatformEvent::WebSignIn { url, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::WEB_SIGN_IN.into())));
            pairs.push((URL.into(), Value::Str(url)));
        }
        PlatformEvent::Cancelled { .. } => {
            pairs.push((KIND.into(), Value::Str(kind::CANCELLED.into())));
        }
        PlatformEvent::Answer { answer, .. } => {
            if let Ok(Value::Map(fields)) = balaur_core::engine_api::from_json(&answer) {
                pairs.extend(fields.into_iter().filter(|(key, _)| key != REQUEST));
            }
        }
        PlatformEvent::Identity { proof, .. } => {
            pairs.push((KIND.into(), Value::Str(IDENTITY.into())));
            if let Ok(Value::Map(fields)) = balaur_core::engine_api::from_json(&proof) {
                pairs.extend(
                    fields
                        .into_iter()
                        .filter(|(key, _)| key != REQUEST && key != KIND),
                );
            }
        }
        device => device_value(device, &mut pairs),
    }
    Value::Map(pairs)
}

/// What the OS's answers and arrivals carry. Split from [`event_value`]
/// under `MAX_FN_LINES`.
fn device_value(event: PlatformEvent, pairs: &mut Vec<(String, Value)>) {
    let json =
        |data: &serde_json::Value| balaur_core::engine_api::from_json(data).unwrap_or(Value::Nil);
    match event {
        PlatformEvent::Notifications { allowed, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::NOTIFICATIONS.into())));
            pairs.push((k::ALLOWED.into(), Value::Bool(allowed)));
        }
        PlatformEvent::Scheduled { id, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::SCHEDULED.into())));
            pairs.push((ID.into(), Value::Str(id)));
        }
        PlatformEvent::NotificationOpened { id, data } => {
            pairs.push((KIND.into(), Value::Str(kind::NOTIFICATION_OPENED.into())));
            pairs.push((ID.into(), Value::Str(id)));
            pairs.push((k::DATA.into(), json(&data)));
        }
        PlatformEvent::NotificationReceived {
            id,
            title,
            body,
            data,
        } => {
            pairs.push((KIND.into(), Value::Str(kind::NOTIFICATION_RECEIVED.into())));
            pairs.push((ID.into(), Value::Str(id)));
            pairs.push((k::TITLE.into(), Value::Str(title)));
            pairs.push((k::BODY.into(), Value::Str(body)));
            pairs.push((k::DATA.into(), json(&data)));
        }
        PlatformEvent::Shared { target, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::SHARED.into())));
            pairs.push((k::TARGET.into(), Value::Str(target)));
        }
        PlatformEvent::Picked {
            bytes, media_type, ..
        } => {
            pairs.push((KIND.into(), Value::Str(kind::PICKED.into())));
            pairs.push((k::BYTES.into(), Value::Bytes(bytes)));
            pairs.push((k::TYPE.into(), Value::Str(media_type)));
        }
        PlatformEvent::UrlOpened { url } => {
            pairs.push((KIND.into(), Value::Str(kind::URL_OPENED.into())));
            pairs.push((URL.into(), Value::Str(url)));
        }
        push => push_value(push, pairs),
    }
}

/// What a push answer or arrival carries: a token's three fields as Gamend
/// takes them, or a push's own. Split from [`device_value`] under
/// `MAX_FN_LINES`.
fn push_value(event: PlatformEvent, pairs: &mut Vec<(String, Value)>) {
    let token_fields = |pairs: &mut Vec<(String, Value)>, token: PushToken| {
        pairs.push((k::TOKEN.into(), Value::Str(token.token)));
        pairs.push((k::PROVIDER.into(), Value::Str(token.provider)));
        pairs.push((k::PLATFORM.into(), Value::Str(token.platform)));
    };
    match event {
        PlatformEvent::PushRegistered { token, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::PUSH_REGISTERED.into())));
            token_fields(pairs, token);
        }
        PlatformEvent::PushTokenChanged { token } => {
            pairs.push((KIND.into(), Value::Str(kind::PUSH_TOKEN_CHANGED.into())));
            token_fields(pairs, token);
        }
        PlatformEvent::PushFailed { message, .. } => {
            pairs.push((KIND.into(), Value::Str(kind::PUSH_FAILED.into())));
            pairs.push((kind::ERROR.into(), Value::Str(message)));
        }
        PlatformEvent::PushReceived {
            id,
            title,
            body,
            data,
        } => {
            pairs.push((KIND.into(), Value::Str(kind::PUSH_RECEIVED.into())));
            pairs.push((ID.into(), Value::Str(id)));
            pairs.push((k::TITLE.into(), Value::Str(title)));
            pairs.push((k::BODY.into(), Value::Str(body)));
            let data = balaur_core::engine_api::from_json(&data).unwrap_or(Value::Nil);
            pairs.push((k::DATA.into(), data));
        }
        _ => {}
    }
}

#[derive(Default)]
pub struct PlatformPlugin;

impl balaur_plugin::Plugin for PlatformPlugin {
    fn name(&self) -> &'static str {
        PLATFORM
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(PlatformState::default());
        reg.insert_resource(PlatformSnapshot::default());
        let stand_ins = StandIns::from_env()?;
        stand_ins::hold(reg.engine(), &stand_ins)?;
        reg.insert_resource(stand_ins);
        balaur_core::events::declare(
            reg.engine(),
            &[(
                ARRIVAL_EVENT,
                "what arrived, carrying request 0",
                "The player signed out in the OS or signed in as someone else, a notification or a push was tapped (`notification_opened`) or arrived while the game was in front (`notification_received`, `push_received`), a link opened the game (`url_opened`, with its `url`; the one it was launched with too), the OS replaced the push token (`push_token_changed`), or a purchase landed with nobody asking (`transaction`).",
            )],
        );
        reg.add_system(Stage::First, pump_platform_system);
        #[cfg(target_os = "android")]
        reg.add_system(Stage::First, android::device::follow_reduced_motion_system);
        reg.add_replay_source(PLATFORM, capture, restore);
        reg.add_replay_setup(services::SERVICES, services::capture, services::restore);
        native::provide(reg.engine())?;
        let mut m = reg.script_module(PLATFORM)?;
        script_api::install(&mut m);
        let mut m = reg.script_module(DEVICE)?;
        device::install_device(&mut m);
        let mut m = reg.script_module(secrets::SECRETS)?;
        secrets::install(&mut m);
        Ok(())
    }
}

/// This tick's arrivals, raw. The pump has already stashed them.
fn capture(eng: &Engine) -> serde_json::Value {
    eng.resource::<PlatformState>().borrow().io.capture()
}

/// Push recorded arrivals back down the channel a backend reports on, so the
/// pump dispatches them exactly as it did when the store answered.
fn restore(eng: &Engine, value: &serde_json::Value) {
    eng.resource::<PlatformState>().borrow().io.restore(value);
}

/// Give the loaded engine its store. Called by a platform plugin's `declare`,
/// which runs after this crate's.
///
/// # Errors
/// If `PlatformPlugin` was never loaded, so there is nothing to register in.
pub fn set_backend(eng: &Engine, backend: Box<dyn PlatformBackend>) -> Result<()> {
    let state = eng
        .try_resource::<PlatformState>()
        .ok_or_else(|| anyhow!("the platform plugin has to be loaded before a store's"))?;
    state.borrow_mut().set_backend(backend);
    Ok(())
}

pub(crate) fn text(value: Option<&Value>, what: &str) -> Result<String> {
    match value {
        Some(Value::Str(text)) => Ok(text.clone()),
        other => Err(anyhow!("{what} should be a string, got {other:?}")),
    }
}

fn number(value: Option<&Value>, what: &str) -> Result<f64> {
    match value {
        Some(Value::Num(n)) => Ok(*n),
        #[allow(clippy::cast_precision_loss, reason = "a percentage or a score")]
        Some(Value::Int(n)) => Ok(*n as f64),
        other => Err(anyhow!("{what} should be a number, got {other:?}")),
    }
}

/// A whole-number option, or `fallback` when the call names none.
fn counted(opts: Option<&Value>, key: &str, fallback: u32) -> Result<u32> {
    match opt(opts, key) {
        Some(Value::Int(n)) => Ok(u32::try_from(*n).unwrap_or(fallback)),
        Some(other) => Err(anyhow!("`{key}` should be a whole number, got {other:?}")),
        None => Ok(fallback),
    }
}

pub(crate) fn start_call(eng: &Engine, call: Call) -> Value {
    let id = eng.next_token();
    let state = eng.resource::<PlatformState>();
    state.borrow_mut().start(eng, id, call);
    id_value(id)
}
