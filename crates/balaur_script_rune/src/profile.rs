//! What each script costs a frame.
//!
//! Counted in instructions rather than nanoseconds: two runs of a
//! deterministic simulation execute the same instructions, so a number that
//! moved is a real change in what a script does, not noise from the machine
//! it ran on. Wall time is what a sampling profiler is for.

use std::collections::HashMap;
use std::rc::Rc;

use crate::RuneHost;

/// What one script has cost since profiling started.
#[derive(Clone, Copy, Default)]
pub struct ScriptCost {
    pub calls: u64,
    pub instructions: u64,
}

impl RuneHost {
    /// Start or stop counting what each script costs. Turning it on clears
    /// what was counted before.
    ///
    /// Only synchronous calls are counted. An async method runs on a VM the
    /// future owns, and its instructions land wherever it is resumed.
    pub fn set_profiling(&self, on: bool) {
        self.state.borrow_mut().profile = on.then(HashMap::new);
    }

    pub fn profiling(&self) -> bool {
        self.state.borrow().profile.is_some()
    }

    /// What each script has cost since profiling started, dearest first.
    pub fn script_costs(&self) -> Vec<(String, ScriptCost)> {
        let state = self.state.borrow();
        let Some(profile) = &state.profile else {
            return Vec::new();
        };
        let mut out: Vec<(String, ScriptCost)> = profile
            .iter()
            .map(|(key, cost)| (key.to_string(), *cost))
            .collect();
        out.sort_by(|a, b| {
            b.1.instructions
                .cmp(&a.1.instructions)
                .then_with(|| a.0.cmp(&b.0))
        });
        out
    }

    /// Attribute `instructions` to `key`. A no-op while profiling is off.
    pub(crate) fn charge(&self, key: &str, instructions: u64) {
        let mut state = self.state.borrow_mut();
        let Some(profile) = &mut state.profile else {
            return;
        };
        let cost = match profile.get_mut(key) {
            Some(cost) => cost,
            None => profile.entry(Rc::from(key)).or_default(),
        };
        cost.calls = cost.calls.wrapping_add(1);
        cost.instructions = cost.instructions.wrapping_add(instructions);
    }

}
