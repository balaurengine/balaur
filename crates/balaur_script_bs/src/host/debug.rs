// The editor's debugger on `.bs`: a file asked about is built with probes, and
// its instance made again on an engine that parks a call at a stop; the rest of
// the instance keeps the units it has. The game freezes while a call is parked.

use super::tasks::Pending;
use super::{BsHost, File, Home, Instance, PLACES, Place, key_of, place_of};
use crate::convert::to_neutral;
use anyhow::{Result, anyhow};
use balaur_bs::Compiled;
use balaur_bs::runtime::{
    Command, EngineKind, Object, Paused, Runtime, Step, StopReason, Value as L,
};
use balaur_script::{Frame, NodeId, Pause, PauseReason, StepMode, Value};
use std::rc::Rc;

/// What a stop holds, and whose it is.
pub(super) struct Held {
    node: u64,
    path: String,
    stop: Stop,
}

enum Stop {
    /// A call parked at a line with the object it took, the task when it is
    /// an async method's poll, and where it stands.
    Parked {
        obj: Object,
        task: Option<Pending>,
        paused: Paused,
    },
    /// A call that trapped, with break on error on: its stack, to look at
    /// and dismiss.
    Error { paused: Paused, message: String },
}

impl BsHost {
    /// Whether a debugger asked about a file of the instance at `place`.
    pub(super) fn debugged_in(&self, place: Place) -> bool {
        self.inner
            .debugging
            .borrow()
            .iter()
            .any(|key| place_of(key) == place)
    }

    /// The engine the instance at `place` runs on.
    pub(super) fn engine_of(&self, place: Place) -> EngineKind {
        if self.debugged_in(place) {
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

    /// The breakpoints asked of its files, each on the line it lands on, on a
    /// fresh instance or one that a file just joined; and a pause owed.
    pub(super) fn arm(&self, home: &Home) {
        let breaks = self.inner.breaks.borrow();
        let on_error = self.inner.break_on_error.get();
        let mut debugger = home.reentry.debugger();
        debugger.keep_locals(on_error);
        if !debugger.probed() {
            return;
        }
        for (key, lines) in breaks.iter() {
            let Some(unit) = debugger.unit_named(key) else {
                continue;
            };
            let Ok(compiled) = self.unit(key) else {
                continue;
            };
            debugger.clear_breakpoints_in(unit);
            for line in landed(&compiled, lines) {
                debugger.set_breakpoint_in(unit, u32::try_from(line).unwrap_or(u32::MAX));
            }
        }
        if self.inner.pause_owed.take() {
            debugger.pause();
        }
    }

    /// From here on a trap stops the game at the line that threw. Every
    /// loaded file is built with probes, on the engine its instance runs on.
    pub(super) fn debug_set_break_on_error(&self, on: bool) {
        self.inner.break_on_error.set(on);
        if on {
            self.rebuild_stale();
        }
        for home in self.homes() {
            home.reentry.debugger().keep_locals(on);
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
    pub(super) fn hold_task(&self, obj: Object, p: Pending, paused: Paused) {
        self.hold(p.node, p.path.clone(), obj, Some(p), paused);
    }

    fn hold(&self, node: u64, path: String, obj: Object, task: Option<Pending>, paused: Paused) {
        *self.inner.stopped.borrow_mut() = Some(Held {
            node,
            path,
            stop: Stop::Parked { obj, task, paused },
        });
        self.inner.engine.set_frozen(true);
    }

    pub(super) fn is_held(&self) -> bool {
        self.inner.stopped.borrow().is_some()
    }

    /// Whether the file at `key` has the call a stop holds: nothing is asked
    /// of its nodes until that one goes on.
    pub(super) fn stopped_in(&self, key: &str) -> bool {
        self.inner
            .stopped
            .borrow()
            .as_ref()
            .is_some_and(|held| held.path == key)
    }

    /// Whether a call of the instance at `place` is parked: its memory cannot
    /// be swapped yet.
    pub(super) fn holds_instance(&self, place: Place) -> bool {
        let stopped = self.inner.stopped.borrow();
        stopped.as_ref().is_some_and(|held| {
            matches!(held.stop, Stop::Parked { .. })
                && self
                    .inner
                    .files
                    .borrow()
                    .get(&held.path)
                    .is_some_and(|file| file.place == place)
        })
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
            .home
            .rt
            .try_borrow_mut()
            .map_err(|_| anyhow!("[{}] {method}: the file is busy", inst.path))?;
        let obj = inst
            .obj
            .borrow_mut()
            .take()
            .ok_or_else(|| anyhow!("[{}] {method}: the instance is busy", inst.path))?;
        // A call back into the node meanwhile is lent a holder of the object,
        // as one while a plain method runs is.
        *inst.lent.borrow_mut() = rt.share(&obj);
        let step = match rt.call_method_steppable(&obj, method, args) {
            Ok(Step::Paused(paused)) => {
                self.hold(inst.node, inst.path.clone(), obj, None, paused);
                return Ok(None);
            }
            other => other,
        };
        *inst.obj.borrow_mut() = Some(obj);
        if let Some(lent) = inst.lent.borrow_mut().take() {
            rt.drop_object(lent);
        }
        match step {
            Ok(Step::Done(v)) => Ok(Some(to_neutral(&v))),
            Ok(Step::Waiting | Step::Paused(_)) => Ok(None),
            Err(e) => {
                let e = anyhow!("[{}] {method}: {e}", inst.path);
                self.hold_error(&mut rt, inst.node, &inst.path, &e);
                Err(e)
            }
        }
    }

    /// Whether `key` is a unit of a loaded script.
    fn in_use(&self, key: &str) -> bool {
        self.inner.files.borrow().iter().any(|(file, loaded)| {
            file == key || loaded.compiled.linked.iter().any(|(used, _)| used == key)
        })
    }

    /// Builds `key` with probes from here on, its instance made again.
    fn debug(&self, key: &str) -> Result<()> {
        if !self.inner.debugging.borrow_mut().insert(key.to_string()) {
            return Ok(());
        }
        if self.stale(place_of(key)) {
            self.rebuild_instance(place_of(key), &[])?;
        }
        Ok(())
    }

    pub(super) fn debug_set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>> {
        let key = key_of(path);
        let place = place_of(&key);
        if lines.is_empty() {
            self.inner.breaks.borrow_mut().remove(&key);
            // No probes left to pay for, once nothing runs in them.
            if !self.holds_instance(place)
                && self.inner.debugging.borrow_mut().remove(&key)
                && self.stale(place)
            {
                self.rebuild_instance(place, &[])?;
            }
        } else {
            self.inner
                .breaks
                .borrow_mut()
                .insert(key.clone(), lines.to_vec());
            self.debug(&key)?;
        }
        if !self.in_use(&key) {
            return Ok(lines.to_vec());
        }
        let landed = landed(&*self.unit(&key)?, lines);
        let home = self.place_slot(place).borrow().clone();
        if let Some(home) = home {
            let mut debugger = home.reentry.debugger();
            if let Some(unit) = debugger.unit_named(&key) {
                debugger.clear_breakpoints_in(unit);
                for line in &landed {
                    debugger.set_breakpoint_in(unit, u32::try_from(*line).unwrap_or(u32::MAX));
                }
            }
        }
        Ok(landed)
    }

    /// The lines asked of `path`, where they landed once it compiled.
    pub(super) fn debug_breakpoints(&self, path: &str) -> Vec<usize> {
        let key = key_of(path);
        let asked = self.inner.breaks.borrow().get(&key).cloned();
        let asked = asked.unwrap_or_default();
        if !self.in_use(&key) {
            return asked;
        }
        match self.unit(&key) {
            Ok(unit) => landed(&unit, &asked),
            Err(_) => asked,
        }
    }

    /// Every loaded file stops at the next line it runs.
    pub(super) fn debug_request_break(&self) {
        let keys: Vec<String> = self.inner.files.borrow().keys().cloned().collect();
        self.inner.debugging.borrow_mut().extend(keys);
        self.rebuild_stale();
        // An instance that has to wait to be made again is told when it is.
        let waiting = PLACES.iter().any(|place| self.stale(*place));
        self.inner.pause_owed.set(waiting);
        for home in self.homes() {
            let mut debugger = home.reentry.debugger();
            if debugger.probed() {
                debugger.pause();
            }
        }
    }

    // Nil once asked to go on: a script of the stopped instance asks again
    // until it does, and it goes on between frames.
    pub(super) fn debug_paused(&self) -> Option<Pause> {
        if self.inner.resume_owed.get().is_some() {
            return None;
        }
        let held = self.inner.stopped.borrow();
        let held = held.as_ref()?;
        match &held.stop {
            Stop::Parked { paused, .. } => Some(pause_of(held, paused, String::new())),
            Stop::Error { paused, message } => Some(pause_of(held, paused, message.clone())),
        }
    }

    pub(super) fn debug_resume(&self, mode: StepMode) {
        let Some(Held { node, path, stop }) = self.inner.stopped.borrow_mut().take() else {
            return;
        };
        let Stop::Parked {
            obj,
            mut task,
            paused,
        } = stop
        else {
            // A trap's stop is dismissed: its call is over.
            self.inner.engine.set_frozen(false);
            return;
        };
        let Some(file) = self.inner.files.borrow().get(&path).cloned() else {
            self.inner.engine.set_frozen(false);
            return;
        };
        let Ok(mut rt) = file.home.rt.try_borrow_mut() else {
            // Asked by a script of the instance: it goes on between frames.
            *self.inner.stopped.borrow_mut() = Some(Held {
                node,
                path,
                stop: Stop::Parked { obj, task, paused },
            });
            self.inner.resume_owed.set(Some(mode));
            return;
        };
        let step = match &mut task {
            Some(p) => rt.resume_task(command(mode), &mut p.task),
            None => rt.resume(command(mode)),
        };
        match step {
            Ok(Step::Paused(paused)) => {
                drop(rt);
                self.hold(node, path, obj, task, paused);
                return;
            }
            Ok(Step::Done(v)) => {
                if let Some(p) = &task {
                    self.answer(p.done, to_neutral(&v));
                }
            }
            // Its scene may have parked while the debugger held it.
            Ok(Step::Waiting) => {
                let parked = self.inner.parked.borrow().iter().any(|i| i.node == node);
                let tasks = if parked {
                    &self.inner.parked_tasks
                } else {
                    &self.inner.tasks
                };
                tasks.borrow_mut().extend(task);
            }
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
        let parked = self.inner.parked.borrow();
        let instances = self.inner.instances.borrow();
        let inst = instances
            .iter()
            .chain(parked.iter())
            .find(|i| i.node == node && i.path == path)
            .cloned();
        match inst {
            Some(inst) => {
                *inst.obj.borrow_mut() = Some(obj);
                if let Some(lent) = inst.lent.borrow_mut().take() {
                    file.drop_object(lent);
                }
            }
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
    let frames: Vec<Frame> = paused
        .frames
        .iter()
        .map(|f| Frame {
            function: f.func.clone(),
            path: f.file.clone(),
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
        path: frames
            .first()
            .map_or_else(|| held.path.clone(), |top| top.path.clone()),
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
