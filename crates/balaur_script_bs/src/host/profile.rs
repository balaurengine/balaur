// The profiler on `.bs`: switched on or off, every file is rebuilt between
// frames with counters or without, as a breakpoint rebuilds one with probes.

use super::BsHost;
use std::collections::HashMap;

impl BsHost {
    /// On from off, or off from on: every loaded file is rebuilt between
    /// frames, its instances carried across. On while on starts the tally
    /// again, which is how a profiler reads one frame at a time.
    pub(super) fn profile_set(&self, on: bool) {
        let inner = &self.inner;
        inner.tally.borrow_mut().clear();
        if inner.profiling.get() == on {
            if on {
                for file in inner.files.borrow().values() {
                    if let Ok(mut rt) = file.rt.try_borrow_mut() {
                        rt.clear_profile();
                    }
                }
            }
            return;
        }
        inner.profiling.set(on);
        let keys: Vec<String> = inner.files.borrow().keys().cloned().collect();
        let mut rebuild = inner.rebuild.borrow_mut();
        for key in keys {
            if !rebuild.contains(&key) {
                rebuild.push(key);
            }
        }
    }

    /// What one call from the engine into `path` ran, from the module's
    /// count before and after it. Nothing once switched off, though a file
    /// still runs with counters until the frame ends and it is rebuilt.
    pub(super) fn charge(&self, path: &str, before: Option<i32>, after: Option<i32>) {
        let (Some(before), Some(after)) = (before, after) else {
            return;
        };
        if !self.inner.profiling.get() {
            return;
        }
        let steps = u64::from(after.wrapping_sub(before).cast_unsigned());
        let mut tally = self.inner.tally.borrow_mut();
        let cost = tally.entry(path.to_string()).or_default();
        cost.0 += 1;
        cost.1 += steps;
    }

    /// Each script as `(path, calls, statements)`, dearest first.
    pub(super) fn profile_scripts(&self) -> Vec<(String, u64, u64)> {
        let mut out: Vec<(String, u64, u64)> = self
            .inner
            .tally
            .borrow()
            .iter()
            .map(|(path, (calls, steps))| (path.clone(), *calls, *steps))
            .collect();
        out.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// Each function as `(name, calls, own statements)`, dearest first. A
    /// module two files use is in both, and counts once for the two.
    pub(super) fn profile_functions(&self) -> Vec<(String, u64, u64)> {
        if !self.inner.profiling.get() {
            return Vec::new();
        }
        let mut summed: HashMap<String, (u64, u64)> = HashMap::new();
        for file in self.inner.files.borrow().values() {
            let Ok(mut rt) = file.rt.try_borrow_mut() else {
                continue;
            };
            for (name, calls, own) in rt.function_costs() {
                let cost = summed.entry(name).or_default();
                cost.0 += calls;
                cost.1 += own;
            }
        }
        let mut out: Vec<(String, u64, u64)> = summed
            .into_iter()
            .map(|(name, (calls, own))| (name, calls, own))
            .collect();
        out.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        out
    }
}
