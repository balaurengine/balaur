//! Store and platform services, portable: `platform.*` for scripts.
//!
//! One module over every store, so a script that unlocks an achievement says
//! the same thing on Game Center, on Play Games and on Steam. What only one
//! platform has stays in that platform's own module (`apple.*`), and with no
//! backend loaded — the editor, a desktop dev run, a replay — every call here
//! resolves to a `kind = "unsupported"` event rather than an error, so the
//! script still runs.
//!
//! Delivery is the engine's usual one: a call returns an id immediately, the
//! backend reports on a channel, and [`ExternalIo`] lands the result at
//! [`Stage::First`] of a later tick — recorded, replayable, and dispatched to
//! the node's `on_platform` method as well as to whoever awaits the id.
//!
//! ```rune
//! pub async fn init(this) {
//!     let player = task::wait(platform::sign_in()).await;
//!     platform::unlock(this.node, "first_blood");
//! }
//!
//! pub fn on_platform(this, e) {
//!     if e["kind"] == "unsupported" { log::info("no store here"); }
//! }
//! ```
//!
//! Every event map carries a `kind`, so one handler can take them all.

use std::sync::mpsc::Sender;

use anyhow::{anyhow, Result};
use balaur_core::handler::{handler_of, id_value, opt, Handler};
use balaur_core::replay::ExternalIo;
use balaur_core::{DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};

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
}

impl Call {
    /// The name an `unsupported` event carries, and what the recording logs.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::SignIn => "sign_in",
            Self::Unlock { .. } => "unlock",
            Self::Progress { .. } => "progress",
            Self::SubmitScore { .. } => "submit_score",
            Self::Scores { .. } => "scores",
            Self::CloudRead { .. } => "cloud_read",
            Self::CloudWrite { .. } => "cloud_write",
            Self::SetPresence { .. } => "set_presence",
        }
    }

    /// Whether this call changes something a rollback cannot take back.
    ///
    /// A read may be issued from a tick that is re-run later — the worst it
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
            "global" => Ok(Self::Global),
            "friends" => Ok(Self::Friends),
            other => Err(anyhow!(
                "`scope` should be \"global\" or \"friends\", got {other:?}"
            )),
        }
    }
}

impl Period {
    fn parse(text: &str) -> Result<Self> {
        match text {
            "today" => Ok(Self::Today),
            "week" => Ok(Self::Week),
            "all_time" => Ok(Self::AllTime),
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
    /// The loaded backend — or the absence of one — does not have this call.
    Unsupported {
        request: u64,
        call: String,
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
            | Self::Unsupported { request, .. } => *request,
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
    /// not reach the outside world on — a replay, or a re-simulation. What
    /// arrived meanwhile is not this run's to deliver.
    fn discard(&mut self) {}
}

/// The calls in flight, the channel every backend reports into, and the
/// writes waiting for the tick that made them to become final.
#[derive(Default)]
pub struct PlatformState {
    io: ExternalIo<PlatformEvent>,
    handlers: DetHashMap<u64, Handler>,
    backend: Option<Box<dyn PlatformBackend>>,
    /// `(tick, request, call)` for writes made on a tick that could still be
    /// rolled back. See [`Call::writes`].
    pending: Vec<(u64, u64, Call)>,
    /// Handlers a sign-in left subscribed. Signing in is the one call whose
    /// answer can change again later — the player signs out in the OS, or
    /// signs in as someone else — and the method that took the first answer
    /// is the one that should hear about it.
    watchers: Vec<Handler>,
    player: Option<Player>,
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

    /// Start a call under `id` — an [`Engine::next_token`] value, so awaiting
    /// it cannot collide with another subsystem's ids.
    ///
    /// A write made on a tick that a late input could still roll back waits
    /// in this state's own `pending` list until that tick is final; everything
    /// else goes out now.
    pub fn start(&mut self, eng: &Engine, id: u64, call: Call, handler: Option<Handler>) {
        if let Some(handler) = handler {
            if matches!(call, Call::SignIn) && !self.watching(&handler) {
                self.watchers.push(handler.clone());
            }
            self.handlers.insert(id, handler);
        }
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

    fn watching(&self, handler: &Handler) -> bool {
        self.watchers
            .iter()
            .any(|w| w.node == handler.node && w.method == handler.method)
    }

    fn issue(&mut self, eng: &Engine, id: u64, call: &Call) {
        let Self { io, backend, .. } = self;
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

/// Drain the backend's reports, record them in the frame's snapshot, then
/// dispatch each to its handler — in arrival order throughout.
fn pump_platform_system(eng: &Engine, _: f32) {
    let clock = balaur_core::rollback::clock(eng);
    let mut dispatches: Vec<(Vec<Handler>, u64, Value)> = Vec::new();
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
            if let Some(backend) = backend {
                if !io.start(eng, |report| backend.pump(report)) {
                    backend.discard();
                }
            }
        }
        snapshot.events.clear();
        for event in state.io.drain() {
            let request = event.request();
            // Who is playing is the one answer that changes on its own, so
            // these two also go to everything watching, not just to whoever
            // asked.
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
            // shift_remove: the remaining entries keep their insertion order,
            // so iteration stays deterministic.
            let mut targets: Vec<Handler> =
                state.handlers.shift_remove(&request).into_iter().collect();
            if announcement {
                for watcher in &state.watchers {
                    if !targets
                        .iter()
                        .any(|h| h.node == watcher.node && h.method == watcher.method)
                    {
                        targets.push(watcher.clone());
                    }
                }
            }
            let value = event_value(event);
            snapshot.events.push(value.clone());
            dispatches.push((targets, request, value));
        }
    }
    balaur_core::handler::dispatch(eng, dispatches);
}

fn player_value(player: &Player) -> Value {
    Value::Map(vec![
        ("id".into(), Value::Str(player.id.clone())),
        ("alias".into(), Value::Str(player.alias.clone())),
    ])
}

fn event_value(event: PlatformEvent) -> Value {
    let mut pairs = vec![("request".into(), id_value(event.request()))];
    match event {
        PlatformEvent::SignedIn { player, .. } => {
            pairs.push(("kind".into(), Value::Str("signed_in".into())));
            pairs.push(("player".into(), player_value(&player)));
            pairs.push(("id".into(), Value::Str(player.id)));
            pairs.push(("alias".into(), Value::Str(player.alias)));
        }
        PlatformEvent::SignedOut { .. } => {
            pairs.push(("kind".into(), Value::Str("signed_out".into())));
        }
        PlatformEvent::Done { call, .. } => {
            pairs.push(("kind".into(), Value::Str("done".into())));
            pairs.push(("call".into(), Value::Str(call)));
        }
        PlatformEvent::Scores { board, entries, .. } => {
            pairs.push(("kind".into(), Value::Str("scores".into())));
            pairs.push(("board".into(), Value::Str(board)));
            pairs.push((
                "entries".into(),
                Value::List(
                    entries
                        .into_iter()
                        .map(|entry| {
                            Value::Map(vec![
                                ("player".into(), Value::Str(entry.player)),
                                ("alias".into(), Value::Str(entry.alias)),
                                ("rank".into(), Value::Int(entry.rank)),
                                ("score".into(), Value::Int(entry.score)),
                            ])
                        })
                        .collect(),
                ),
            ));
        }
        PlatformEvent::Read { key, value, .. } => {
            pairs.push(("kind".into(), Value::Str("read".into())));
            pairs.push(("key".into(), Value::Str(key)));
            pairs.push(("value".into(), value.map_or(Value::Nil, Value::Str)));
        }
        PlatformEvent::Failed { message, .. } => {
            pairs.push(("kind".into(), Value::Str("failed".into())));
            pairs.push(("error".into(), Value::Str(message)));
        }
        PlatformEvent::Unsupported { call, .. } => {
            pairs.push(("kind".into(), Value::Str("unsupported".into())));
            pairs.push(("call".into(), Value::Str(call)));
        }
    }
    Value::Map(pairs)
}

pub struct PlatformPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for PlatformPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("platform", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for PlatformPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(PlatformState::default());
        reg.insert_resource(PlatformSnapshot::default());
        reg.add_system(Stage::First, pump_platform_system);
        reg.add_replay_source("platform", capture, restore);
        let mut m = reg.script_module("platform")?;
        install_platform_api(&mut *m);
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

/// The node-and-payload first argument every call takes: `f(node, a, ..)`
/// names a handler, `f(a, ..)` is awaited instead.
fn split(
    first: Value,
    second: Option<Value>,
    third: Option<Value>,
) -> (Value, Option<Value>, Option<Value>) {
    match first {
        Value::Node(_) | Value::Nil => (first, second, third),
        payload => (Value::Nil, Some(payload), second),
    }
}

/// The same, for a call with two values of its own: `f(node, a, b, ..)` or
/// `f(a, b, ..)`.
fn split2(
    first: Value,
    second: Option<Value>,
    third: Option<Value>,
    fourth: Option<Value>,
) -> (Value, Option<Value>, Option<Value>, Option<Value>) {
    match first {
        Value::Node(_) | Value::Nil => (first, second, third, fourth),
        payload => (Value::Nil, Some(payload), second, third),
    }
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

fn start_call(eng: &Engine, node: &Value, opts: Option<&Value>, call: Call) -> Result<Value> {
    let handler = handler_of(node, opts, "on_platform", "on_platform")?;
    let id = eng.next_token();
    let state = eng.resource::<PlatformState>();
    state.borrow_mut().start(eng, id, call, handler);
    Ok(id_value(id))
}

/// `platform.*`. Declared against the backend seam, so the same script runs
/// with a store, without one, and inside a replay.
fn install_platform_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Store services every platform shares: sign-in, achievements, \
         leaderboards and cloud saves. A call returns an id and answers on a \
         later tick, as a map carrying `kind` — `signed_in`, `done`, \
         `scores`, `read`, `failed` or `unsupported` — both to the node's \
         `on_platform` method and to whoever awaits the id. With no store \
         loaded every call answers `unsupported`, so a script written against \
         this runs anywhere. What only one platform has lives in that \
         platform's own module.",
    );
    m.describe(&[
        (
            "backend",
            &[],
            "",
            "The name of the loaded store, or \"none\".",
        ),
        (
            "player",
            &[],
            "",
            "Who the store says is playing, or nil before a sign-in has landed.",
        ),
        ("signed_in", &[], "", "Whether a sign-in has landed."),
        (
            "sign_in",
            &[],
            "",
            "Ask the store who is playing, and return the id its answer carries. A node given here keeps hearing: a later sign-out reaches the same method.",
        ),
        ("unlock", &[], "", "Award an achievement whole."),
        (
            "progress",
            &[],
            "",
            "Report an achievement's completion, 0 to 100.",
        ),
        ("submit_score", &[], "", "Post a score to a leaderboard."),
        (
            "scores",
            &[],
            "",
            "Read a leaderboard's entries. The options take `count`, `start` (a rank, from 1), `scope` (\"global\" or \"friends\") and `period` (\"today\", \"week\" or \"all_time\").",
        ),
        (
            "cloud_read",
            &[],
            "",
            "Read a value the store syncs between the player's devices.",
        ),
        (
            "cloud_write",
            &[],
            "",
            "Write a value for the store to sync between the player's devices.",
        ),
        (
            "set_presence",
            &[],
            "",
            "Say what the player is doing, where the store shows it.",
        ),
    ]);
    install_platform_reads(m);
    install_platform_session(m);
    install_platform_achievements(m);
    install_platform_leaderboards(m);
    install_platform_cloud(m);
}

/// What a script can ask without waiting: which store is loaded, and who it
/// says is playing.
fn install_platform_reads(m: &mut dyn Bindings<Engine>) {
    m.function("backend", |eng: &Engine, ()| {
        Ok(Value::Str(
            eng.resource::<PlatformState>()
                .borrow()
                .backend_name()
                .into(),
        ))
    });
    m.function("player", |eng: &Engine, ()| {
        let state = eng.resource::<PlatformState>();
        let state = state.borrow();
        Ok(state.player().map_or(Value::Nil, player_value))
    });
    m.function("signed_in", |eng: &Engine, ()| {
        Ok(Value::Bool(
            eng.resource::<PlatformState>().borrow().player().is_some(),
        ))
    });
}

/// Signing in, and saying what the player is doing.
fn install_platform_session(m: &mut dyn Bindings<Engine>) {
    m.function(
        "sign_in",
        |eng: &Engine, (first, second): (Option<Value>, Option<Value>)| {
            let (node, opts) = match first {
                Some(node @ Value::Node(_)) => (node, second),
                Some(opts) => (Value::Nil, Some(opts)),
                None => (Value::Nil, None),
            };
            start_call(eng, &node, opts.as_ref(), Call::SignIn)
        },
    );
    m.function(
        "set_presence",
        |eng: &Engine, (first, second, third): (Value, Option<Value>, Option<Value>)| {
            let (node, presence, opts) = split(first, second, third);
            let text = text(presence.as_ref(), "what the player is doing")?;
            start_call(eng, &node, opts.as_ref(), Call::SetPresence { text })
        },
    );
}

fn install_platform_achievements(m: &mut dyn Bindings<Engine>) {
    m.function(
        "unlock",
        |eng: &Engine, (first, second, third): (Value, Option<Value>, Option<Value>)| {
            let (node, arg, opts) = split(first, second, third);
            let achievement = text(arg.as_ref(), "an achievement id")?;
            start_call(eng, &node, opts.as_ref(), Call::Unlock { achievement })
        },
    );
    m.function(
        "progress",
        |eng: &Engine,
         (first, second, third, fourth): (Value, Option<Value>, Option<Value>, Option<Value>)| {
            let (node, achievement, percent, opts) = split2(first, second, third, fourth);
            let achievement = text(achievement.as_ref(), "an achievement id")?;
            let percent = number(percent.as_ref(), "a percentage")?;
            start_call(
                eng,
                &node,
                opts.as_ref(),
                Call::Progress {
                    achievement,
                    percent,
                },
            )
        },
    );
}

fn install_platform_leaderboards(m: &mut dyn Bindings<Engine>) {
    m.function(
        "submit_score",
        |eng: &Engine,
         (first, second, third, fourth): (Value, Option<Value>, Option<Value>, Option<Value>)| {
            let (node, board, score, opts) = split2(first, second, third, fourth);
            let board = text(board.as_ref(), "a leaderboard id")?;
            #[allow(clippy::cast_possible_truncation, reason = "a leaderboard score")]
            let score = number(score.as_ref(), "a score")? as i64;
            start_call(eng, &node, opts.as_ref(), Call::SubmitScore { board, score })
        },
    );
    m.function(
        "scores",
        |eng: &Engine, (first, second, third): (Value, Option<Value>, Option<Value>)| {
            let (node, board, opts) = split(first, second, third);
            let board = text(board.as_ref(), "a leaderboard id")?;
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
            start_call(
                eng,
                &node,
                opts,
                Call::Scores {
                    board,
                    count,
                    scope,
                    period,
                    start,
                },
            )
        },
    );
}

fn install_platform_cloud(m: &mut dyn Bindings<Engine>) {
    m.function(
        "cloud_read",
        |eng: &Engine, (first, second, third): (Value, Option<Value>, Option<Value>)| {
            let (node, key, opts) = split(first, second, third);
            let key = text(key.as_ref(), "a key")?;
            start_call(eng, &node, opts.as_ref(), Call::CloudRead { key })
        },
    );
    m.function(
        "cloud_write",
        |eng: &Engine,
         (first, second, third, fourth): (Value, Option<Value>, Option<Value>, Option<Value>)| {
            let (node, key, value, opts) = split2(first, second, third, fourth);
            let key = text(key.as_ref(), "a key")?;
            let value = text(value.as_ref(), "a value")?;
            start_call(eng, &node, opts.as_ref(), Call::CloudWrite { key, value })
        },
    );
}
