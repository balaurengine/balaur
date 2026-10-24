// The editor's debugger on `.bs`: a file asked about is rebuilt with probes on
// an engine that parks a call at a stop, and the game freezes as for Rune.

use super::tasks::Pending;
use super::{BsHost, File, Instance, key_of};
use crate::convert::to_neutral;
use anyhow::{Result, anyhow};
use balaur_bs::Compiled;
use balaur_bs::runtime::{
    Command, EngineKind, Object, Paused, Runtime, Step, StopReason, Value as L,
};
use balaur_script::{Frame, NodeId, Pause, PauseReason, ScriptHost, StepMode, Value};
use std::rc::Rc;

/// What a stop holds, and whose it is.
pub(super) struct Held {
    node: u64,
    path: String,
    stop: Stop,
}

enum Stop {
    /// A call parked at a line with the object it took, and the task when
    /// it is an async method's poll.
    Parked { obj: Object, task: Option<Pending> },
    /// A call that trapped, with break on error on: its stack, to look at
    /// and dismiss.
    Error { paused: Paused, message: String },
}

impl BsHost {
    pub(super) fn engine_for(&self, key: &str) -> EngineKind {
        if self.inner.debugging.borrow().contains(key) {
            parking_engine()
        } else {
            EngineKind::Auto
        }
    }

    /// Built with probes: asked about by a debugger, or every file while
    /// break on error is on.
    pub(super) fn probed(&self, key: &str) -> bool {
        self.inner.break_on_error.get() || self.inner.debugging.borrow().contains(key)
    }

    /// The breakpoints asked of `key`, each on the line it lands on, on its
    /// fresh runtime.
    pub(super) fn arm(&self, key: &str, compiled: &Compiled, rt: &mut Runtime) {
        let mut debugger = rt.debugger();
        debugger.keep_locals(self.inner.break_on_error.get());
        if let Some(lines) = self.inner.breaks.borrow().get(key) {
            for line in landed(compiled, lines) {
                debugger.set_breakpoint(u32::try_from(line).unwrap_or(u32::MAX));
            }
        }
    }

    /// From here on a trap stops the game at the line that threw. Every
    /// loaded file is rebuilt with probes, on the engine it runs on.
    pub(super) fn debug_set_break_on_error(&self, on: bool) {
        self.inner.break_on_error.set(on);
        let files: Vec<(String, Rc<File>)> = self
            .inner
            .files
            .borrow()
            .iter()
            .map(|(k, f)| (k.clone(), f.clone()))
            .collect();
        for (key, file) in files {
            if on && !file.compiled.meta.debug {
                drop(file);
                if let Err(e) = ScriptHost::reload(self, &key) {
                    tracing::error!("{e:#}");
                }
            } else if let Ok(mut rt) = file.rt.try_borrow_mut() {
                rt.debugger().keep_locals(on);
            }
        }
    }

    pub(super) fn debug_break_on_error(&self) -> bool {
        self.inner.break_on_error.get()
    }

    /// With break on error on, the stack of the call that just trapped in
    /// `rt` holds the game.
    pub(super) fn hold_error(&self, rt: &mut Runtime, node: u64, path: &str, e: &anyhow::Error) {
        if !self.inner.break_on_error.get() || self.is_held() {
            return;
        }
        let Some(paused) = rt.error_stop() else {
            return;
        };
        *self.inner.stopped.borrow_mut() = Some(Held {
            node,
            path: path.to_string(),
            stop: Stop::Error {
                paused,
                message: format!("{e:#}"),
            },
        });
        self.inner.engine.set_frozen(true);
    }

    /// An async method's poll parked at a stop.
    pub(super) fn hold_task(&self, obj: Object, p: Pending) {
        self.hold(p.node, p.path.clone(), obj, Some(p));
    }

    fn hold(&self, node: u64, path: String, obj: Object, task: Option<Pending>) {
        *self.inner.stopped.borrow_mut() = Some(Held {
            node,
            path,
            stop: Stop::Parked { obj, task },
        });
        self.inner.engine.set_frozen(true);
    }

    pub(super) fn is_held(&self) -> bool {
        self.inner.stopped.borrow().is_some()
    }

    /// Whether a call of `key` is parked: its memory cannot be swapped yet.
    pub(super) fn holds_file(&self, key: &str) -> bool {
        self.inner
            .stopped
            .borrow()
            .as_ref()
            .is_some_and(|h| h.path == key && matches!(h.stop, Stop::Parked { .. }))
    }

    /// Runs `method` so that a stop holds it; `None` when it stopped.
    pub(super) fn run_steppable(
        &self,
        file: &File,
        inst: &Instance,
        method: &str,
        args: &[L],
    ) -> Result<Option<Value>> {
        let mut rt = file
            .rt
            .try_borrow_mut()
            .map_err(|_| anyhow!("[{}] {method}: the file is busy", inst.path))?;
        let mut obj = inst
            .obj
            .borrow_mut()
            .take()
            .ok_or_else(|| anyhow!("[{}] {method}: the instance is busy", inst.path))?;
        match rt.call_method_steppable(&mut obj, method, args) {
            Ok(Step::Paused(_)) => {
                self.hold(inst.node, inst.path.clone(), obj, None);
                Ok(None)
            }
            Ok(Step::Done(v)) => {
                *inst.obj.borrow_mut() = Some(obj);
                Ok(Some(to_neutral(&v)))
            }
            Ok(Step::Waiting) => {
                *inst.obj.borrow_mut() = Some(obj);
                Ok(None)
            }
            Err(e) => {
                *inst.obj.borrow_mut() = Some(obj);
                let e = anyhow!("[{}] {method}: {e}", inst.path);
                self.hold_error(&mut rt, inst.node, &inst.path, &e);
                Err(e)
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
        let landed = landed(&file.compiled, lines);
        let mut rt = file.rt.borrow_mut();
        let mut debugger = rt.debugger();
        debugger.clear_breakpoints();
        for line in &landed {
            debugger.set_breakpoint(u32::try_from(*line).unwrap_or(u32::MAX));
        }
        Ok(landed)
    }

    /// The lines asked of `path`, where they landed once it compiled.
    pub(super) fn debug_breakpoints(&self, path: &str) -> Vec<usize> {
        let key = key_of(path);
        let asked = self.inner.breaks.borrow().get(&key).cloned();
        let asked = asked.unwrap_or_default();
        match self.inner.files.borrow().get(&key) {
            Some(file) => landed(&file.compiled, &asked),
            None => asked,
        }
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
        match &held.stop {
            Stop::Parked { .. } => {
                let file = self.inner.files.borrow().get(&held.path).cloned()?;
                let paused = file.rt.borrow().paused()?;
                Some(pause_of(held, &paused, String::new()))
            }
            Stop::Error { paused, message } => Some(pause_of(held, paused, message.clone())),
        }
    }

    pub(super) fn debug_resume(&self, mode: StepMode) {
        let Some(Held { node, path, stop }) = self.inner.stopped.borrow_mut().take() else {
            return;
        };
        let Stop::Parked { mut obj, mut task } = stop else {
            // A trap's stop is dismissed: its call is over.
            self.inner.engine.set_frozen(false);
            return;
        };
        let Some(file) = self.inner.files.borrow().get(&path).cloned() else {
            self.inner.engine.set_frozen(false);
            return;
        };
        let mut rt = file.rt.borrow_mut();
        let step = match &mut task {
            Some(p) => rt.resume_task(command(mode), &mut p.task, Some(&mut obj)),
            None => rt.resume(command(mode), &mut obj),
        };
        match step {
            Ok(Step::Paused(_)) => {
                drop(rt);
                self.hold(node, path, obj, task);
                return;
            }
            Ok(Step::Done(v)) => {
                if let Some(p) = &task {
                    self.answer(p.done, to_neutral(&v));
                }
            }
            Ok(Step::Waiting) => self.inner.tasks.borrow_mut().extend(task),
            Err(e) => {
                let e = anyhow!("[{path}] {e}");
                tracing::error!("{e:#}");
                self.hold_error(&mut rt, node, &path, &e);
            }
        }
        drop(rt);
        self.release_held(&file, node, &path, obj);
        if !self.is_held() {
            self.inner.engine.set_frozen(false);
        }
    }

    // The object goes back to its instance, or is freed if the node went.
    fn release_held(&self, file: &Rc<File>, node: u64, path: &str, obj: Object) {
        let inst = self
            .inner
            .instances
            .borrow()
            .iter()
            .find(|i| i.node == node && i.path == path)
            .cloned();
        match inst {
            Some(inst) => *inst.obj.borrow_mut() = Some(obj),
            None => file.drop_object(obj),
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
fn landed(compiled: &Compiled, lines: &[usize]) -> Vec<usize> {
    let mut code: Vec<usize> = compiled
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

fn pause_of(held: &Held, paused: &Paused, message: String) -> Pause {
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
            StopReason::Error => PauseReason::Error,
        },
        frames,
        message,
    }
}
