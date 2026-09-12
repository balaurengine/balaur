//! `task.frames` and `task.seconds`: tokens the fixed step wakes.
//!
//! Counted on the fixed step rather than the frame, so a wait replays
//! exactly and a rollback puts it back where it was. A token is minted the
//! way a request id is and woken with nil, which is what `task::wait` on an
//! engine token expects.

use serde::{Deserialize, Serialize};

use crate::FIXED_DT;
use crate::engine::Engine;

/// Every wait in flight, in the order it was asked for.
#[derive(Default, Serialize, Deserialize)]
pub struct Timers {
    frames: Vec<(u64, u32)>,
    seconds: Vec<(u64, f32)>,
    /// Tokens to wake on the next step with a payload: an awaited call's
    /// result, which cannot be delivered before its caller has parked.
    #[serde(skip)]
    results: Vec<(u64, balaur_script::Value)>,
}

/// Wake `token` with `payload` on the next fixed step.
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

/// A token woken once `seconds` of fixed steps have passed.
pub fn after_seconds(eng: &Engine, seconds: f32) -> u64 {
    let token = eng.next_token();
    eng.resource::<Timers>()
        .borrow_mut()
        .seconds
        .push((token, seconds.max(0.0)));
    token
}

pub(crate) fn step_timers_system(eng: &Engine, _: f32) {
    let results = std::mem::take(&mut eng.resource::<Timers>().borrow_mut().results);
    if let Some(host) = eng.script_host() {
        for (token, payload) in &results {
            host.wake(*token, payload);
        }
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
        for (token, left) in &mut timers.seconds {
            *left -= FIXED_DT;
            if *left <= 0.0 {
                due.push(*token);
            }
        }
        timers.seconds.retain(|(_, left)| *left > 0.0);
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
