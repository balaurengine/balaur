//! Rollback: when an input for an earlier tick arrives, put the world back
//! and simulate forward again.
//!
//! Only inputs cross a wire in this model, never state, so the cost is
//! O(players) and independent of world size. What makes it affordable here is
//! that the pieces already exist: [`crate::snapshot`] can put a tick back,
//! including nodes spawned or freed since, and [`crate::digest`] can say
//! whether two peers agree.
//!
//! A [`Session`] owns the tick number, a ring of recent snapshots, and the
//! journal of who pressed what when. It predicts a missing input by repeating
//! that player's last one, and when the real input turns up and disagrees, it
//! restores the tick before it and re-runs everything since.
//!
//! **Re-simulation must not be visible.** A tick running for the second time
//! would otherwise send its requests again. [`is_resimulating`] is the flag,
//! and `ExternalIo::start` refuses to spawn work while it is set — the same
//! choke point that keeps a replay off the network. A subsystem reaching the
//! outside world by some other route has the same gap it already has there.
//!
//! A session steps the app at [`App::fixed_step`] and takes no `dt` of its
//! own. That is not a preference: the app's substep accumulator lives outside
//! the snapshot, so a variable step would restore the world without restoring
//! how much time was owed, and a re-run could take a different number of
//! fixed steps than the run it is repeating. Stepping at the fixed step
//! leaves the accumulator at zero every time, which is why the question
//! cannot come up.

use std::collections::BTreeMap;

use balaur_script::Value;

use crate::app::App;
use crate::digest::{self, Digest};
use crate::engine::Engine;
use crate::snapshot::{self, SnapshotRing};

/// Which player an input belongs to.
pub type PlayerId = u32;

/// One player's input for one tick, in whatever shape the game gives it.
///
/// The engine never looks inside: what a game sends is its own vocabulary,
/// and the only thing rollback needs is to compare two of them.
pub type Input = Value;

/// The inputs for the tick being simulated, in player order.
///
/// Written by the session before the tick runs; read through [`input`].
#[derive(Default)]
pub struct TickInputs(pub Vec<(PlayerId, Input)>);

/// Set while a tick is running for the second time.
#[derive(Default)]
pub struct Resimulating(pub bool);

/// Whether this tick is a re-run of one already simulated.
///
/// Anything with an effect outside the simulation has to ask. Live-read
/// rather than cached, for the reason `replay::is_playing` is.
#[must_use]
pub fn is_resimulating(eng: &Engine) -> bool {
    eng.try_resource::<Resimulating>()
        .is_some_and(|flag| flag.borrow().0)
}

/// What `player` is doing this tick, as the session decided: their real
/// input, or the prediction standing in for it.
#[must_use]
pub fn input(eng: &Engine, player: PlayerId) -> Option<Input> {
    let inputs = eng.try_resource::<TickInputs>()?;
    let found = inputs
        .borrow()
        .0
        .iter()
        .find(|(id, _)| *id == player)
        .map(|(_, value)| value.clone());
    found
}

/// A local rollback session: the tick clock, the snapshot ring and the
/// journal, driving one [`App`].
pub struct Session {
    players: Vec<PlayerId>,
    /// The tick [`Session::advance`] will run next.
    next: u64,
    ring: SnapshotRing,
    /// Inputs that actually arrived.
    arrived: BTreeMap<(u64, PlayerId), Input>,
    /// Inputs a tick was simulated with, real or predicted. A late input
    /// matching what was already used costs nothing.
    used: BTreeMap<(u64, PlayerId), Input>,
    /// The earliest tick whose inputs changed after it ran.
    dirty: Option<u64>,
    /// Inputs that arrived for a tick the ring had already dropped.
    stale: u64,
    /// What each tick digested to, rewritten when a tick is re-simulated.
    ///
    /// This is what peers compare. Kept here rather than by the caller
    /// because only the session knows when a tick has actually run, and a
    /// re-run has to overwrite the number the first run produced.
    digests: BTreeMap<u64, Digest>,
}

impl Session {
    /// A session over `players`, keeping `depth` ticks of history.
    ///
    /// `depth` is how far back a late input can reach. One older than that
    /// cannot be answered at all, and [`Session::stale_inputs`] counts it.
    #[must_use]
    pub fn new(players: &[PlayerId], depth: usize) -> Self {
        Self {
            players: players.to_vec(),
            next: 1,
            ring: SnapshotRing::new(depth),
            arrived: BTreeMap::new(),
            used: BTreeMap::new(),
            dirty: None,
            stale: 0,
            digests: BTreeMap::new(),
        }
    }

    /// The tick that runs on the next [`Session::advance`].
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.next
    }

    /// How far back a late input can still be answered.
    #[must_use]
    pub fn earliest(&self) -> Option<u64> {
        self.ring.earliest()
    }

    /// Hand the session an input, on time or late.
    ///
    /// Late and matching what was predicted changes nothing. Late and
    /// different schedules a rollback to that tick, taken on the next
    /// [`Session::advance`] so one call answers a burst of arrivals.
    pub fn submit(&mut self, player: PlayerId, tick: u64, value: Input) {
        let changed = self.used.get(&(tick, player)) != Some(&value);
        self.arrived.insert((tick, player), value);
        if tick < self.next && changed {
            self.dirty = Some(self.dirty.map_or(tick, |at| at.min(tick)));
        }
    }

    /// How many inputs arrived too late to be answered.
    ///
    /// Not a warning to be logged and forgotten. Each one is a tick this peer
    /// simulated with a prediction it now knows was wrong and can no longer
    /// correct, so its world has diverged from the peer that sent it. The
    /// session keeps running because it has nothing better to do; the caller
    /// has to resync from an authority, or stop. A count that stays at zero
    /// is the healthy case, and the ring is what buys the headroom.
    #[must_use]
    pub const fn stale_inputs(&self) -> u64 {
        self.stale
    }

    /// What the world digested to at the end of `tick`.
    ///
    /// `None` for a tick that has not run, or one old enough to have been
    /// forgotten with its snapshot.
    #[must_use]
    pub fn digest_at(&self, tick: u64) -> Option<Digest> {
        self.digests.get(&tick).copied()
    }

    /// Run one tick, rolling back first when a late input asked for it.
    ///
    /// Steps at [`App::fixed_step`]; see the module note on why it takes no
    /// `dt` of its own.
    pub fn advance(&mut self, app: &mut App) {
        let dt = app.fixed_step();
        if let Some(from) = self.dirty.take() {
            self.resimulate(app, from, dt);
        }
        let tick = self.next;
        self.run_tick(app, tick, dt);
        self.next += 1;
    }

    /// Put the world back to `from` and run everything since.
    fn resimulate(&mut self, app: &mut App, from: u64, dt: f32) {
        let Some(snapshot) = self.ring.get(from).cloned() else {
            self.stale += 1;
            tracing::warn!(
                tick = from,
                earliest = ?self.ring.earliest(),
                "an input arrived for a tick the ring no longer holds; this peer has diverged"
            );
            return;
        };
        snapshot::restore(&app.engine, &snapshot);
        set_resimulating(&app.engine, true);
        for tick in from..self.next {
            self.run_tick(app, tick, dt);
        }
        set_resimulating(&app.engine, false);
    }

    /// Capture the world as it is, decide this tick's inputs, and step.
    ///
    /// The capture goes in before the step, so the ring holds the world a
    /// tick started from — which is what restoring that tick has to mean.
    fn run_tick(&mut self, app: &mut App, tick: u64, dt: f32) {
        self.ring.push(tick, snapshot::capture(&app.engine));
        let inputs = self.inputs_for(tick);
        for (player, value) in &inputs {
            self.used.insert((tick, *player), value.clone());
        }
        if let Some(slot) = app.engine.try_resource::<TickInputs>() {
            slot.borrow_mut().0 = inputs;
        }
        app.tick(dt);
        self.digests.insert(tick, digest::digest(&app.engine));
        // A tick the ring has dropped can never be re-run, so its digest can
        // never change and nobody can still be asking about it.
        if let Some(earliest) = self.ring.earliest() {
            self.digests.retain(|at, _| *at >= earliest);
        }
    }

    /// Each player's input for `tick`: what arrived, or the prediction.
    fn inputs_for(&self, tick: u64) -> Vec<(PlayerId, Input)> {
        self.players
            .iter()
            .map(|&player| {
                let value = self
                    .arrived
                    .get(&(tick, player))
                    .cloned()
                    .unwrap_or_else(|| self.predict(player, tick));
                (player, value)
            })
            .collect()
    }

    /// Repeat the player's most recent input. The cheapest predictor there
    /// is, and the right one for a held button, which is most of them.
    fn predict(&self, player: PlayerId, tick: u64) -> Input {
        self.arrived
            .range(..(tick, player))
            .rev()
            .find(|((_, id), _)| *id == player)
            .map_or(Value::Nil, |(_, value)| value.clone())
    }
}

fn set_resimulating(eng: &Engine, on: bool) {
    if let Some(flag) = eng.try_resource::<Resimulating>() {
        flag.borrow_mut().0 = on;
    }
}
