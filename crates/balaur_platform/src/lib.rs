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
//! has one (Apple's `ASWebAuthenticationSession`), and `unsupported` where
//! it does not, so a game opens the page in the browser there instead.
//!
//! A test answers any call from a file instead ([`stand_ins`]).

use std::sync::mpsc::Sender;

use crate::vocabulary::words;
use anyhow::{Result, anyhow};
use balaur_core::handler::{id_value, opt};
use balaur_core::replay::ExternalIo;
use balaur_core::{Engine, Stage};
use balaur_script::{Module, Value};

const ALIAS: &str = "alias";
const BACKEND: &str = "backend";
const CALL: &str = "call";
const CLOUD_READ: &str = "cloud_read";
const CLOSE_WEB_SIGN_IN: &str = "close_web_sign_in";
const CLOUD_WRITE: &str = "cloud_write";
const ID: &str = "id";
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
pub mod stand_ins;
mod vocabulary;

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
    /// A test's answer from its stand-in file ([`stand_ins`]), as the map
    /// the waiter receives.
    StandIn {
        request: u64,
        answer: serde_json::Value,
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
            | Self::StandIn { request, .. } => *request,
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

/// The calls in flight, the channel every backend reports into, and the
/// writes waiting for the tick that made them to become final.
#[derive(Default)]
pub struct PlatformState {
    io: ExternalIo<PlatformEvent>,
    backend: Option<Box<dyn PlatformBackend>>,
    /// `(tick, request, call)` for writes made on a tick that could still be
    /// rolled back. See [`Call::writes`].
    pending: Vec<(u64, u64, Call)>,
    player: Option<Player>,
    /// The sign-in page asked for last, and the `kind` it answered: empty
    /// while it has not.
    page: Option<(u64, String)>,
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
        self.backend.as_ref().map_or("none", |b| b.name())
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
        balaur_core::replay::event(
            eng,
            "platform.call",
            call.name().to_string(),
            Some(serde_json::json!({ "id": id, "call": call.name() })),
        );
        if matches!(call, Call::WebSignIn { .. }) {
            self.page = Some((id, String::new()));
        }
        let clock = balaur_core::rollback::clock(eng);
        if call.writes() && clock.tick >= clock.settled {
            self.pending.push((clock.tick, id, call));
            return;
        }
        self.issue(eng, id, &call);
    }

    fn issue(&mut self, eng: &Engine, id: u64, call: &Call) {
        let Self { io, backend, .. } = self;
        if let Some(answer) = stand_in(eng, PLATFORM, call.name()) {
            io.start(eng, |report| {
                if let Some(answer) = answer {
                    let _ = report.send(PlatformEvent::StandIn {
                        request: id,
                        answer,
                    });
                }
            });
            return;
        }
        io.start(eng, |report| match backend {
            Some(backend) => backend.start(id, call, report),
            None => {
                let _ = report.send(PlatformEvent::Unsupported {
                    request: id,
                    call: call.name().to_string(),
                });
            }
        });
    }

    /// Shut the open sign-in page. Never during a replay: the recording
    /// says when it shut.
    pub fn close_web_sign_in(&mut self, eng: &Engine) {
        if balaur_core::replay::suppressed(eng) {
            return;
        }
        if let Some(backend) = &mut self.backend {
            backend.close_web_sign_in();
        }
    }

    /// Issue every held write whose tick nothing can send the session back
    /// to. Never during a replay or a re-simulation: the recording answers
    /// there, and the outside world is not touched.
    fn release(&mut self, eng: &Engine, settled: u64) {
        if self.pending.is_empty() || balaur_core::replay::suppressed(eng) {
            return;
        }
        let mut ready = Vec::new();
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

/// This tick's arrivals, as the neutral values the handlers received.
///
/// Scripts never read it; it exists so Rust code can observe what the store
/// answered, and so a test has one place to look.
#[derive(Default)]
pub struct PlatformSnapshot {
    pub events: Vec<Value>,
}

/// Who is playing, heard with `events::listen` as `on_platform_event`: the one
/// answer that changes on its own, when the player signs out in the OS or signs
/// in as someone else.
pub const PLAYER_EVENT: &str = "platform_event";

/// Drain the backend's reports, record them in the frame's snapshot, then
/// wake what awaits each, in arrival order throughout.
fn pump_platform_system(eng: &Engine, _: f32) {
    let clock = balaur_core::rollback::clock(eng);
    let mut finished: Vec<(u64, Value)> = Vec::new();
    let mut player_changes = Vec::new();
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
            let PlatformState { io, backend, .. } = &mut *state;
            if let Some(backend) = backend
                && !io.start(eng, |report| backend.pump(report))
            {
                backend.discard();
            }
        }
        snapshot.events.clear();
        for event in state.io.drain() {
            let request = event.request();
            let announcement = match &event {
                PlatformEvent::SignedIn { player, .. } => {
                    state.player = Some(player.clone());
                    true
                }
                PlatformEvent::SignedOut { .. } => {
                    state.player = None;
                    true
                }
                _ => false,
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
                player_changes.push(value.clone());
            }
            finished.push((request, value));
        }
    }
    for change in player_changes {
        balaur_core::events::emit(eng, PLAYER_EVENT, change);
    }
    balaur_core::handler::wake_all(eng, finished);
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
        ERROR,
    ];
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
        PlatformEvent::StandIn { answer, .. } => {
            if let Ok(Value::Map(fields)) = balaur_core::engine_api::from_json(&answer) {
                pairs.extend(fields.into_iter().filter(|(key, _)| key != REQUEST));
            }
        }
    }
    Value::Map(pairs)
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
        reg.insert_resource(StandIns::from_env()?);
        balaur_core::events::declare(
            reg.engine(),
            &[(
                PLAYER_EVENT,
                "who is playing now",
                "The player signed out in the OS, or signed in as someone else.",
            )],
        );
        reg.add_system(Stage::First, pump_platform_system);
        reg.add_replay_source(PLATFORM, capture, restore);
        let mut m = reg.script_module(PLATFORM)?;
        install_platform_api(&mut m);
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

fn text(value: Option<&Value>, what: &str) -> Result<String> {
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

fn start_call(eng: &Engine, call: Call) -> Value {
    let id = eng.next_token();
    let state = eng.resource::<PlatformState>();
    state.borrow_mut().start(eng, id, call);
    id_value(id)
}

/// `platform.*`. Declared against the backend seam, so the same script runs
/// with a store, without one, and inside a replay.
fn install_platform_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Store services every platform shares: sign-in, achievements, leaderboards, cloud saves, a sign-in page in the OS's own sheet. A call answers `task::wait` later with a map whose `kind` is `signed_in`, `signed_out`, `done`, `scores`, `read`, `web_sign_in`, `cancelled`, `error` or `unsupported`, each an `EVENT_*` constant. A sign-in or sign-out, asked for or not, is also the `platform_event` event, `events::listen(node, \"platform_event\")`. A test answers calls from the file `BALAUR_STAND_INS` names.",
    );
    balaur_core::handler::install_event_kinds(m, kind::ALL);
    m.records(&[
        (
            "Player",
            "{ id: string, alias: string }",
            "Who the store says is playing: its stable `id` on this platform, and the `alias` to show.",
        ),
        (
            "Score",
            "{ player: string, alias: string, rank: int, score: int }",
            "One row of a leaderboard, as a `scores` answer's `entries` hold it: `let rows: [platform::Score] = reply[\"entries\"];`.",
        ),
        (
            "ScoresOptions",
            "{ count: int?, start: int?, scope: string?, period: string? }",
            "Which entries `scores` reads: how many, the rank to read from (from 1), `scope` (\"global\" or \"friends\") and `period` (\"today\", \"week\" or \"all_time\").",
        ),
    ]);
    m.describe(&[
        (
            BACKEND,
            &[],
            "() -> string",
            "The name of the loaded store, or \"none\".",
        ),
        (
            PLAYER,
            &[],
            "() -> Player?",
            "Who the store says is playing, or nil before a sign-in has landed.",
        ),
        (
            SIGN_IN,
            &[],
            "() -> int",
            "Ask the store who is playing, and return the id its answer carries. A later sign-out is the `platform_event` event.",
        ),
        (
            UNLOCK,
            &[],
            "(achievement: string) -> int",
            "Award an achievement whole, and return the id its answer carries.",
        ),
        (
            PROGRESS,
            &[],
            "(achievement: string, percent: float) -> int",
            "Report an achievement's completion, 0 to 100, and return the id its answer carries.",
        ),
        (
            SUBMIT_SCORE,
            &[],
            "(board: string, score: int) -> int",
            "Post a score to a leaderboard, and return the id its answer carries.",
        ),
        (
            kind::SCORES,
            &[],
            "(board: string, opts: ScoresOptions?) -> int",
            "Read a leaderboard's entries, and return the id whose `scores` answer holds them as `entries`, each a `Score`.",
        ),
        (
            CLOUD_READ,
            &[],
            "(key: string) -> int",
            "Read a value the store syncs between the player's devices, and return the id whose `read` answer holds it as `value`: a string, or nil when the key holds nothing.",
        ),
        (
            CLOUD_WRITE,
            &[],
            "(key: string, value: string) -> int",
            "Write a value for the store to sync between the player's devices, and return the id its answer carries.",
        ),
        (
            SET_PRESENCE,
            &[],
            "(text: string) -> int",
            "Say what the player is doing, where the store shows it, and return the id its answer carries.",
        ),
    ]);
    install_platform_reads(m);
    install_platform_session(m);
    install_platform_achievements(m);
    install_platform_leaderboards(m);
    install_platform_cloud(m);
    install_web_sign_in(m);
}

/// What a script can ask without waiting: which store is loaded, and who it
/// says is playing.
fn install_platform_reads(m: &mut Module<Engine>) {
    m.function(BACKEND, |eng: &Engine, ()| {
        Ok(Value::Str(
            eng.resource::<PlatformState>()
                .borrow()
                .backend_name()
                .into(),
        ))
    });
    m.function(PLAYER, |eng: &Engine, ()| {
        let state = eng.resource::<PlatformState>();
        let state = state.borrow();
        Ok(state.player().map_or(Value::Nil, player_value))
    });
}

/// Signing in, and saying what the player is doing.
fn install_platform_session(m: &mut Module<Engine>) {
    m.function(SIGN_IN, |eng: &Engine, ()| {
        Ok(start_call(eng, Call::SignIn))
    });
    m.function(SET_PRESENCE, |eng: &Engine, presence: Value| {
        let text = text(Some(&presence), "what the player is doing")?;
        Ok(start_call(eng, Call::SetPresence { text }))
    });
}

fn install_platform_achievements(m: &mut Module<Engine>) {
    m.function(UNLOCK, |eng: &Engine, achievement: Value| {
        let achievement = text(Some(&achievement), "an achievement id")?;
        Ok(start_call(eng, Call::Unlock { achievement }))
    });
    m.function(
        PROGRESS,
        |eng: &Engine, (achievement, percent): (Value, Value)| {
            let achievement = text(Some(&achievement), "an achievement id")?;
            let percent = number(Some(&percent), "a percentage")?;
            Ok(start_call(
                eng,
                Call::Progress {
                    achievement,
                    percent,
                },
            ))
        },
    );
}

fn install_platform_leaderboards(m: &mut Module<Engine>) {
    m.function(
        SUBMIT_SCORE,
        |eng: &Engine, (board, score): (Value, Value)| {
            let board = text(Some(&board), "a leaderboard id")?;
            #[allow(clippy::cast_possible_truncation, reason = "a leaderboard score")]
            let score = number(Some(&score), "a score")? as i64;
            Ok(start_call(eng, Call::SubmitScore { board, score }))
        },
    );
    m.function(
        kind::SCORES,
        |eng: &Engine, (board, opts): (Value, Option<Value>)| {
            let board = text(Some(&board), "a leaderboard id")?;
            let opts = opts.as_ref();
            let count = counted(opts, "count", DEFAULT_SCORES)?;
            let start = counted(opts, "start", 1)?.max(1);
            let scope = match opt(opts, "scope") {
                Some(Value::Str(text)) => Scope::parse(text)?,
                Some(other) => return Err(anyhow!("`scope` should be a string, got {other:?}")),
                None => Scope::default(),
            };
            let period = match opt(opts, "period") {
                Some(Value::Str(text)) => Period::parse(text)?,
                Some(other) => return Err(anyhow!("`period` should be a string, got {other:?}")),
                None => Period::default(),
            };
            Ok(start_call(
                eng,
                Call::Scores {
                    board,
                    count,
                    scope,
                    period,
                    start,
                },
            ))
        },
    );
}

fn install_platform_cloud(m: &mut Module<Engine>) {
    m.function(CLOUD_READ, |eng: &Engine, key: Value| {
        let key = text(Some(&key), "a key")?;
        Ok(start_call(eng, Call::CloudRead { key }))
    });
    m.function(CLOUD_WRITE, |eng: &Engine, (key, value): (Value, Value)| {
        let key = text(Some(&key), "a key")?;
        let value = text(Some(&value), "a value")?;
        Ok(start_call(eng, Call::CloudWrite { key, value }))
    });
}

/// A provider's sign-in page in the OS's own sheet, and shutting it.
fn install_web_sign_in(m: &mut Module<Engine>) {
    m.records(&[(
            "WebSignInOptions",
            "{ scheme: string? }",
            "The custom scheme a sign-in page ends on (`com.example.app` for `com.example.app:/done`): the sheet shuts there and its `web_sign_in` answer carries that URL as `url`.",
    )]);
    m.describe(&[
        (
            kind::WEB_SIGN_IN,
            &[],
            "(url: string, opts: WebSignInOptions?) -> int",
            "Open a sign-in page in the OS's own sheet over the game, and return the id its answer carries: `web_sign_in` with the `url` it ended on when it reached `opts.scheme`, `cancelled` when shut, `unsupported` where the platform has no such sheet (open the page with `engine::open_url` there).",
        ),
        (
            WEB_SIGN_IN_STATE,
            &[],
            "() -> string",
            "How the page `web_sign_in` opened last stands, without waiting: `PAGE_NONE` before any, `PAGE_OPEN` while it has not answered, else the `kind` it answered. A game polling a server meanwhile reads it to hear the player shut the page.",
        ),
        (
            CLOSE_WEB_SIGN_IN,
            &[],
            "() -> nil",
            "Shut the sign-in page `web_sign_in` opened, once the game has what it opened it for: its call answers `cancelled`.",
        ),
    ]);
    m.function(
        kind::WEB_SIGN_IN,
        |eng: &Engine, (url, opts): (Value, Option<Value>)| {
            let url = text(Some(&url), "a sign-in page's address")?;
            let scheme = match opt(opts.as_ref(), "scheme") {
                Some(Value::Str(scheme)) => Some(scheme.clone()),
                Some(other) => return Err(anyhow!("`scheme` should be a string, got {other:?}")),
                None => None,
            };
            Ok(start_call(eng, Call::WebSignIn { url, scheme }))
        },
    );
    m.constant("PAGE_NONE", Value::Str(PAGE_NONE.into()));
    m.constant("PAGE_OPEN", Value::Str(PAGE_OPEN.into()));
    m.function(WEB_SIGN_IN_STATE, |eng: &Engine, ()| {
        Ok(Value::Str(
            eng.resource::<PlatformState>()
                .borrow()
                .web_sign_in_state()
                .into(),
        ))
    });
    m.function(CLOSE_WEB_SIGN_IN, |eng: &Engine, ()| {
        eng.resource::<PlatformState>()
            .borrow_mut()
            .close_web_sign_in(eng);
        Ok(())
    });
}
