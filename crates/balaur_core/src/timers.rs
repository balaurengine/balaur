//! `task.frames` and `task.seconds`: tokens the fixed step wakes.
//!
//! Counted on the fixed step rather than the frame, so a wait replays
//! exactly and a rollback puts it back where it was. A token is minted the
//! way a request id is and woken with nil, which is what `task::wait` on an
//! engine token expects.

use std::sync::mpsc::Receiver;

use serde::{Deserialize, Serialize};

use crate::engine::Engine;

/// Every wait in flight, in the order it was asked for, each as the steps it
/// has left.
#[derive(Default, Serialize, Deserialize)]
pub struct Timers {
    frames: Vec<(u64, u32)>,
    /// Tokens to wake on the next step with a payload: an awaited call's
    /// result, which cannot be delivered before its caller has parked.
    #[serde(skip)]
    results: Vec<(u64, balaur_script::Value)>,
    /// Tokens to wake on the next step with what a worker answers
    /// (`task::compute`), waited for there if it has not answered yet.
    #[serde(skip)]
    computing: Vec<(u64, Receiver<balaur_script::Value>)>,
}

/// Wake `token` on the next fixed step with the value `answer` brings,
/// waiting for it there if the worker has not sent it: work off the frame
/// that asked, whose caller resumes on the same step on every run.
pub fn wake_with(eng: &Engine, token: u64, answer: Receiver<balaur_script::Value>) {
    eng.resource::<Timers>()
        .borrow_mut()
        .computing
        .push((token, answer));
}

pub fn wake_next_step(eng: &Engine, token: u64, payload: balaur_script::Value) {
    eng.resource::<Timers>()
        .borrow_mut()
        .results
        .push((token, payload));
}

/// A token woken after `count` fixed steps; zero wakes on the next.
pub fn after_frames(eng: &Engine, count: u32) -> u64 {
    let token = eng.next_token();
    eng.resource::<Timers>()
        .borrow_mut()
        .frames
        .push((token, count));
    token
}

/// A token woken on the fixed step `ceil(seconds * tick_hz)` from now, the
/// next one for zero: counted in steps, as [`after_frames`] counts.
pub fn after_seconds(eng: &Engine, seconds: f64) -> u64 {
    after_frames(eng, steps_in(seconds, crate::tick_hz()))
}

/// The whole steps `seconds` spans at `hz`. A product within 1e-6 of a whole
/// step is that step: `4.0 / 60.0` is 4 steps, not the 5 its rounding gives.
#[must_use]
pub fn steps_in(seconds: f64, hz: u32) -> u32 {
    let steps = seconds.max(0.0) * f64::from(hz);
    let nearest = steps.round();
    let whole = if (steps - nearest).abs() < 1e-6 {
        nearest
    } else {
        steps.ceil()
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to the u32 range first"
    )]
    let count = whole.min(f64::from(u32::MAX)) as u32;
    count
}

/// Whether a wait is still to be woken: a result owed on the next step, or a
/// count the pause is not holding.
pub(crate) fn pending(eng: &Engine) -> bool {
    eng.try_resource::<Timers>().is_some_and(|timers| {
        let timers = timers.borrow();
        let counting = !timers.frames.is_empty();
        !timers.results.is_empty() || !timers.computing.is_empty() || (counting && !eng.paused())
    })
}

pub(crate) fn step_timers_system(eng: &Engine, _: f32) {
    // An awaited call's result is owed to its caller, not timed: it is
    // delivered through a pause, or a script that awaited one inside an
    // `always` subtree never gets its answer.
    let (mut results, computing) = {
        let timers = eng.resource::<Timers>();
        let mut timers = timers.borrow_mut();
        (
            std::mem::take(&mut timers.results),
            std::mem::take(&mut timers.computing),
        )
    };
    // A worker's answer, waited for where it is not in yet; a worker gone
    // without one answers nil.
    for (token, answer) in computing {
        results.push((token, answer.recv().unwrap_or(balaur_script::Value::Nil)));
    }
    if let Some(host) = eng.script_host() {
        for (token, payload) in &results {
            host.wake(*token, payload);
        }
    }
    // A wait counts simulation time, and a paused game takes none. Every wait
    // is held, `always` subtrees included: a token carries no node to ask.
    if eng.paused() {
        return;
    }
    let due: Vec<u64> = {
        let timers = eng.resource::<Timers>();
        let mut timers = timers.borrow_mut();
        let mut due = Vec::new();
        for (token, left) in &mut timers.frames {
            *left = left.saturating_sub(1);
            if *left == 0 {
                due.push(*token);
            }
        }
        timers.frames.retain(|(_, left)| *left > 0);
        due
    };
    if let Some(host) = eng.script_host() {
        for token in due {
            host.wake(token, &balaur_script::Value::Nil);
        }
    }
}

pub(crate) fn save_timers(eng: &Engine) -> serde_json::Value {
    serde_json::to_value(&*eng.resource::<Timers>().borrow()).unwrap_or_default()
}

pub(crate) fn load_timers(eng: &Engine, value: &serde_json::Value) {
    if let Ok(timers) = Timers::deserialize(value) {
        *eng.resource::<Timers>().borrow_mut() = timers;
    }
}
