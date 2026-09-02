//! The host's side of the debugger: a broken coroutine becoming the pause,
//! the pause letting go, and the breakpoints patched into each chunk.

use anyhow::Result;
use balaur_script::{Pause, PauseReason, StepMode};
use hecs::Entity;

use crate::debugger;
use crate::{Paused, ScriptHost};

impl ScriptHost {
    /// File a broken thread as the pause and freeze the engine. A second
    /// break while one is filed is waved on: the editor shows one pause.
    pub(crate) fn park(
        &self,
        owner: Entity,
        label: &str,
        thread: mlua::Thread,
        hit: debugger::Hit,
    ) -> Option<balaur_script::Value> {
        if self.state.borrow().paused.is_some() {
            if hit.reason == PauseReason::Breakpoint {
                debugger::leaving_breakpoint(&thread);
            }
            let outcome = debugger::resume(&self.lua, &thread, mlua::MultiValue::new());
            return self.settle_task(owner, label, thread, outcome);
        }
        let key = self.attachment_path(owner).unwrap_or_default();
        let frames = debugger::frames(&self.lua);
        let (path, line) = frames
            .first()
            .map_or((key.clone(), hit.line), |f| (f.path.clone(), f.line));
        let pause = Pause {
            node: balaur_core::node_id_of(owner),
            path,
            line,
            reason: hit.reason,
            frames,
        };
        tracing::info!(
            "[{key}] paused at {}:{} ({})",
            pause.path,
            pause.line,
            pause.reason.name()
        );
        self.state.borrow_mut().paused = Some(Paused {
            owner,
            key,
            label: label.to_string(),
            thread,
            pause,
            remaining: Vec::new(),
            method: None,
        });
        self.engine.set_frozen(true);
        None
    }

    /// Forget a pause whose script or node is gone.
    pub(crate) fn drop_pause(&self, paused: &Paused) {
        debugger::forget(&paused.thread);
        self.engine.set_frozen(false);
    }

    /// Where a script is stopped, while one is.
    pub fn paused(&self) -> Option<Pause> {
        self.state.borrow().paused.as_ref().map(|p| p.pause.clone())
    }

    /// Let the paused script go on, then finish the tick it interrupted.
    pub fn resume(&self, mode: StepMode) {
        let Some(paused) = self.state.borrow_mut().paused.take() else {
            return;
        };
        self.engine.set_frozen(false);
        if paused.pause.reason == PauseReason::Breakpoint {
            debugger::leaving_breakpoint(&paused.thread);
        }
        if mode != StepMode::Continue {
            debugger::begin_step(&paused.thread, mode, paused.pause.line);
        }
        let Paused {
            owner,
            label,
            thread,
            remaining,
            method,
            ..
        } = paused;
        let outcome = debugger::resume(&self.lua, &thread, mlua::MultiValue::new());
        self.settle_task(owner, &label, thread, outcome);
        let Some((method, dt)) = method else {
            return;
        };
        let paused_again = {
            let mut state = self.state.borrow_mut();
            match state.paused.as_mut() {
                Some(again) => {
                    again.remaining.clone_from(&remaining);
                    again.method = Some((method.clone(), dt));
                    true
                }
                None => false,
            }
        };
        if !paused_again {
            self.run_batch(&method, dt, remaining);
        }
    }

    /// Replace one file's breakpoints; returns the lines they landed on. A
    /// file not loaded yet keeps the request for when its chunk arrives.
    pub fn set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>> {
        let key = Self::normalize_key(path);
        let mut requested = lines.to_vec();
        requested.sort_unstable();
        requested.dedup();
        self.state
            .borrow_mut()
            .breakpoints
            .entry(key.clone())
            .or_default()
            .requested = requested;
        self.apply_breakpoints(&key)?;
        Ok(self.breakpoints(&key))
    }

    /// One file's breakpoints as they landed, or as requested while the file
    /// is not loaded.
    pub fn breakpoints(&self, path: &str) -> Vec<usize> {
        let key = Self::normalize_key(path);
        let state = self.state.borrow();
        state.breakpoints.get(&key).map_or_else(Vec::new, |b| {
            if state.chunks.contains_key(&key) {
                b.landed.clone()
            } else {
                b.requested.clone()
            }
        })
    }

    /// Patch a file's requested breakpoints into its chunk, unpatching what
    /// landed before.
    pub(crate) fn apply_breakpoints(&self, key: &str) -> Result<()> {
        let (chunk, old, requested) = {
            let state = self.state.borrow();
            let Some(chunk) = state.chunks.get(key).cloned() else {
                return Ok(());
            };
            let (old, requested) = state
                .breakpoints
                .get(key)
                .map_or((Vec::new(), Vec::new()), |b| {
                    (b.landed.clone(), b.requested.clone())
                });
            (chunk, old, requested)
        };
        for line in old {
            debugger::set_breakpoint(&self.lua, &chunk, line, false)?;
        }
        let mut landed = Vec::new();
        for line in requested {
            if let Some(line) = debugger::set_breakpoint(&self.lua, &chunk, line, true)? {
                landed.push(line);
            }
        }
        landed.sort_unstable();
        landed.dedup();
        if let Some(b) = self.state.borrow_mut().breakpoints.get_mut(key) {
            b.landed = landed;
        }
        Ok(())
    }
}
