//! Whether the simulation still needs frames, for a loop that sleeps.
//!
//! Under `[window] low_processor` the loop draws no frame while nothing asks
//! for one, and simulated time moves only on frames: asleep, a timer never
//! runs out and a falling body hangs in the air. So the loop asks [`busy`]
//! before it sleeps. Each subsystem with a clock registers one [`Probe`],
//! which reads a count or a flag its own step keeps, and [`pending`] is
//! their OR.
//!
//! A step's flag cannot see what was started after the step ran: a handler
//! that plays a clip, a script that starts a timer. So [`busy`] also holds
//! the loop awake until one fixed step has run with nothing pending on
//! either side of it, which is the step that counts what the frame before
//! started. A frame owed for any other reason starts that over
//! ([`disturb`]).
//!
//! Nothing here reaches the simulation: a probe only reads, time stays on
//! the fixed step, and when a frame runs is a wall-clock matter a replay
//! already ignores.

use crate::engine::Engine;
use crate::hecs::{Entity, World};

/// What the kernel's probes answer to: `task::seconds`, `task::frames` and
/// parked jobs; a replay being fed; scenes a pack still has to warm.
pub const TASK: &str = "task";
pub const REPLAY: &str = "replay";
pub const PRELOAD: &str = "preload";

/// Whether a subsystem has work only more frames will finish. Asked before
/// every sleep, so it reads what the subsystem's step already counted.
pub type Probe = fn(&Engine) -> bool;

/// Every subsystem's probe, under the name it answers to.
#[derive(Default)]
pub struct ProbeRegistry(Vec<(&'static str, Probe)>);

/// What the sleeping loop has seen since something last asked for a frame.
#[derive(Default)]
pub struct RestState {
    /// Fixed steps run so far.
    steps: u64,
    /// The step count when the loop first found nothing pending; `None` while
    /// something is, and after a frame owed for another reason.
    quiet_at: Option<u64>,
}

/// Have `probe` asked before the loop sleeps, under `name`.
pub fn add_probe(eng: &Engine, name: &'static str, probe: Probe) {
    if let Some(probes) = eng.try_resource::<ProbeRegistry>() {
        probes.borrow_mut().0.push((name, probe));
    }
}

/// The subsystems with work pending, by the names they were added under.
#[must_use]
pub fn pending_names(eng: &Engine) -> Vec<&'static str> {
    eng.try_resource::<ProbeRegistry>()
        .map_or_else(Vec::new, |probes| {
            let probes = probes.borrow();
            let asked = probes.0.iter().filter(|(_, probe)| probe(eng));
            asked.map(|(name, _)| *name).collect()
        })
}

/// Whether any subsystem has work only more frames will finish.
#[must_use]
pub fn pending(eng: &Engine) -> bool {
    eng.try_resource::<ProbeRegistry>()
        .is_some_and(|probes| probes.borrow().0.iter().any(|(_, probe)| probe(eng)))
}

/// Whether work on `entity` keeps the loop awake. A game a tool hosts under
/// `hosted`, [`Engine::debug_scope`], never does: the tool decides when it
/// runs, and asks for the frames it runs on.
#[must_use]
pub fn counts(world: &World, entity: Entity, hosted: Option<Entity>) -> bool {
    hosted.is_none_or(|root| !crate::scene::is_within(world, entity, root))
}

/// A fixed step ran: every subsystem's count is as fresh as that.
pub(crate) fn note_step(eng: &Engine) {
    if let Some(state) = eng.try_resource::<RestState>() {
        state.borrow_mut().steps += 1;
    }
}

/// A frame is about to run for a reason of its own, input or a wake or a
/// repaint: what it starts is counted by the step after it, so the loop
/// stays up for that one.
pub fn disturb(eng: &Engine) {
    if let Some(state) = eng.try_resource::<RestState>() {
        state.borrow_mut().quiet_at = None;
    }
}

/// Whether a sleeping loop owes the simulation a frame: something is
/// pending, or no fixed step has run since the loop first found nothing was.
#[must_use]
pub fn busy(eng: &Engine) -> bool {
    let Some(state) = eng.try_resource::<RestState>() else {
        return false;
    };
    // No step runs under a debugger stop, a held replay or a zero time scale,
    // so nothing pending would finish however many frames were drawn.
    if eng.frozen_root().is_some() || eng.time_scale() <= 0.0 {
        return false;
    }
    let pending = pending(eng);
    let mut state = state.borrow_mut();
    if pending {
        state.quiet_at = None;
        return true;
    }
    if let Some(at) = state.quiet_at {
        return state.steps == at;
    }
    state.quiet_at = Some(state.steps);
    true
}

/// The kernel's own probes: waits and stepped jobs, a replay being fed, and
/// scenes still to warm.
pub(crate) fn add_core_probes(eng: &Engine) {
    add_probe(eng, TASK, |eng| {
        crate::timers::pending(eng) || crate::task::parked()
    });
    add_probe(eng, REPLAY, crate::replay::is_advancing);
    add_probe(eng, PRELOAD, crate::preload::warming);
}
