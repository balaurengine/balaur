// The editor's debugger on `.bl`: a file asked about is rebuilt with probes on
// an engine that parks a call at a stop, and the game freezes as for Rune.

use super::{File, Instance, LangHost, key_of};
use crate::convert::to_neutral;
use anyhow::{Result, anyhow};
use balaur_lang::runtime::{
    Command, EngineKind, Object, Paused, Runtime, Step, StopReason, Value as L,
};
use balaur_script::{Frame, NodeId, Pause, PauseReason, StepMode, Value};
use std::rc::Rc;

/// A method parked at a stop, and the object it took with it.
pub(super) struct Held {
    node: u64,
    path: String,
    obj: Object,
}

impl LangHost {
    pub(super) fn engine_for(&self, key: &str) -> EngineKind {
        if self.inner.debugging.borrow().contains(key) {
            parking_engine()
        } else {
            EngineKind::Auto
        }
    }

    /// The breakpoints asked of `key`, on its fresh runtime.
    pub(super) fn arm(&self, key: &str, rt: &mut Runtime) {
        if let Some(lines) = self.inner.breaks.borrow().get(key) {
            let mut debugger = rt.debugger();
            for line in lines {
                debugger.set_breakpoint(u32::try_from(*line).unwrap_or(u32::MAX));
            }
        }
    }

    pub(super) fn is_held(&self) -> bool {
        self.inner.stopped.borrow().is_some()
    }

    pub(super) fn holds_file(&self, key: &str) -> bool {
        self.inner
            .stopped
            .borrow()
            .as_ref()
            .is_some_and(|h| h.path == key)
    }

    /// Runs `method` so that a stop holds it; `None` when it stopped.
    pub(super) fn run_steppable(
        &self,
        file: &File,
        inst: &Instance,
        method: &str,
        args: &[L],
    ) -> Result<Option<Value>> {
        let mut rt = file.rt.try_borrow_mut().map_err(|_| {
            anyhow!(
                "[{}] {method}: a script cannot call back into its own file yet",
                inst.path
            )
        })?;
        let mut obj = inst
            .obj
            .borrow_mut()
            .take()
            .ok_or_else(|| anyhow!("[{}] {method}: the instance is busy", inst.path))?;
        match rt.call_method_steppable(&mut obj, method, args) {
            Ok(Step::Done(v)) => {
                *inst.obj.borrow_mut() = Some(obj);
                Ok(Some(to_neutral(&v)))
            }
            Ok(Step::Paused(_)) => {
                *self.inner.stopped.borrow_mut() = Some(Held {
                    node: inst.node,
                    path: inst.path.clone(),
                    obj,
                });
                self.inner.engine.set_frozen(true);
                Ok(None)
            }
            Err(e) => {
                *inst.obj.borrow_mut() = Some(obj);
                Err(anyhow!("[{}] {method}: {e}", inst.path))
            }
        }
    }

    /// Builds `key` with probes from here on, rebuilding it if it is loaded.
    fn debug(&self, key: &str) -> Result<()> {
        if !self.inner.debugging.borrow_mut().insert(key.to_string()) {
            return Ok(());
        }
        let loaded = self.inner.files.borrow().contains_key(key);
        if loaded {
            balaur_script::ScriptHost::reload(self, key)?;
        }
        Ok(())
    }

    pub(super) fn debug_set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>> {
        let key = key_of(path);
        if lines.is_empty() {
            self.inner.breaks.borrow_mut().remove(&key);
        } else {
            self.inner
                .breaks
                .borrow_mut()
                .insert(key.clone(), lines.to_vec());
            self.debug(&key)?;
        }
        let Some(file) = self.inner.files.borrow().get(&key).cloned() else {
            return Ok(lines.to_vec());
        };
        let landed = landed(&file, lines);
        let mut rt = file.rt.borrow_mut();
        let mut debugger = rt.debugger();
        debugger.clear_breakpoints();
        for line in &landed {
            debugger.set_breakpoint(u32::try_from(*line).unwrap_or(u32::MAX));
        }
        Ok(landed)
    }

    pub(super) fn debug_breakpoints(&self, path: &str) -> Vec<usize> {
        self.inner
            .breaks
            .borrow()
            .get(&key_of(path))
            .cloned()
            .unwrap_or_default()
    }

    /// Every loaded file stops at the next line it runs.
    pub(super) fn debug_request_break(&self) {
        let keys: Vec<String> = self.inner.files.borrow().keys().cloned().collect();
        for key in keys {
            if let Err(e) = self.debug(&key) {
                tracing::error!("{e:#}");
                continue;
            }
            if let Some(file) = self.inner.files.borrow().get(&key) {
                file.rt.borrow_mut().debugger().pause();
            }
        }
    }

    pub(super) fn debug_paused(&self) -> Option<Pause> {
        let held = self.inner.stopped.borrow();
        let held = held.as_ref()?;
        let file = self.inner.files.borrow().get(&held.path).cloned()?;
        let paused = file.rt.borrow().paused()?;
        Some(pause_of(held, &paused))
    }

    pub(super) fn debug_resume(&self, mode: StepMode) {
        let Some(mut held) = self.inner.stopped.borrow_mut().take() else {
            return;
        };
        let Some(file) = self.inner.files.borrow().get(&held.path).cloned() else {
            self.inner.engine.set_frozen(false);
            return;
        };
        let step = file.rt.borrow_mut().resume(command(mode), &mut held.obj);
        match step {
            Ok(Step::Paused(_)) => {
                *self.inner.stopped.borrow_mut() = Some(held);
                return;
            }
            Ok(Step::Done(_)) => {}
            Err(e) => tracing::error!("[{}] {e}", held.path),
        }
        self.release_held(&file, held);
        self.inner.engine.set_frozen(false);
    }

    // The object goes back to its instance, or is freed if the node went.
    fn release_held(&self, file: &Rc<File>, held: Held) {
        let inst = self
            .inner
            .instances
            .borrow()
            .iter()
            .find(|i| i.node == held.node && i.path == held.path)
            .cloned();
        match inst {
            Some(inst) => *inst.obj.borrow_mut() = Some(held.obj),
            None => file.rt.borrow_mut().drop_object(held.obj),
        }
    }
}

#[cfg(feature = "wasmi")]
fn parking_engine() -> EngineKind {
    EngineKind::Wasmi
}

// Without wasmi nothing here can park a call, and a stop is passed by.
#[cfg(not(feature = "wasmi"))]
fn parking_engine() -> EngineKind {
    EngineKind::Auto
}

/// Each asked-for line moved to the first line at or after it with code.
fn landed(file: &File, lines: &[usize]) -> Vec<usize> {
    let mut code: Vec<usize> = file
        .compiled
        .meta
        .probes
        .iter()
        .map(|p| p.line as usize)
        .collect();
    code.sort_unstable();
    code.dedup();
    lines
        .iter()
        .map(|l| code.iter().copied().find(|c| c >= l).unwrap_or(*l))
        .collect()
}

fn command(mode: StepMode) -> Command {
    match mode {
        StepMode::Continue => Command::Continue,
        StepMode::Over => Command::StepOver,
        StepMode::Into => Command::StepInto,
        StepMode::Out => Command::StepOut,
    }
}

fn pause_of(held: &Held, paused: &Paused) -> Pause {
    let frames = paused
        .frames
        .iter()
        .map(|f| Frame {
            function: f.func.clone(),
            path: held.path.clone(),
            line: f.line as usize,
            locals: f
                .locals
                .iter()
                .map(|(n, v)| (n.clone(), to_neutral(v)))
                .collect(),
        })
        .collect();
    Pause {
        node: NodeId(held.node),
        path: held.path.clone(),
        line: paused.line as usize,
        reason: match paused.reason {
            StopReason::Breakpoint => PauseReason::Breakpoint,
            StopReason::Step => PauseReason::Step,
            StopReason::Asked => PauseReason::Pause,
        },
        frames,
        message: String::new(),
    }
}
