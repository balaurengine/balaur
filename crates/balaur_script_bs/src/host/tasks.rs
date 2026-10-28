// Async methods: one that awaits the engine parks here as a task until the
// engine wakes its token, then runs on from the await.

use super::{BsHost, File, Instance};
use crate::convert::{from_neutral, to_neutral};
use anyhow::{Result, anyhow};
use balaur_bs::runtime::{Object, RunError, Runtime, Step, Task, Value as L};
use balaur_script::Value;
use std::rc::Rc;

/// A method suspended on the engine.
pub(super) struct Pending {
    pub(super) node: u64,
    pub(super) path: String,
    pub(super) method: String,
    pub(super) task: Task,
    /// Where an awaiting caller hears the result (`call_on_async`).
    pub(super) done: Option<u64>,
}

impl BsHost {
    /// Starts async `method` and runs it to its first await; `Some` when it
    /// finished there.
    pub(super) fn start(
        &self,
        file: &File,
        inst: &Instance,
        method: &str,
        args: &[L],
        done: Option<u64>,
    ) -> Result<Option<Value>> {
        let key = format!("{}.{method}", file.ty);
        let mut rt = file
            .rt
            .try_borrow_mut()
            .map_err(|_| anyhow!("[{}] {method}: the file is busy", inst.path))?;
        let task = rt
            .start(&key, args)
            .map_err(|e| anyhow!("[{}] {method}: {e}", inst.path))?;
        let p = Pending {
            node: inst.node,
            path: inst.path.clone(),
            method: method.to_string(),
            task,
            done,
        };
        match self.poll_held(file, &mut rt, inst, p)? {
            Polled::Waiting(p) => self.inner.tasks.borrow_mut().push(p),
            Polled::Done(v, _) => return Ok(Some(v)),
            Polled::Stopped => {}
        }
        Ok(None)
    }

    /// Hands `payload` to the file awaiting `token`, then runs on what can.
    pub(super) fn deliver(&self, token: u64, payload: &Value) {
        let files: Vec<Rc<File>> = self.inner.files.borrow().values().cloned().collect();
        let mut woke = false;
        for file in files {
            let Ok(mut rt) = file.rt.try_borrow_mut() else {
                continue;
            };
            if let Some(ty) = rt.awaits(token) {
                woke |= rt.wake(token, from_neutral(payload, &ty));
            }
        }
        if woke {
            self.run_tasks();
        }
    }

    /// Polls every parked task; one that finishes answers its caller. A
    /// stop freezes the game, and the tasks behind it wait with it.
    pub(super) fn run_tasks(&self) {
        let parked = std::mem::take(&mut *self.inner.tasks.borrow_mut());
        let mut kept = Vec::with_capacity(parked.len());
        for p in parked {
            if self.is_held() {
                kept.push(p);
                continue;
            }
            match self.step(p) {
                Ok(Polled::Waiting(p)) => kept.push(p),
                Ok(Polled::Done(v, done)) => self.answer(done, v),
                Ok(Polled::Stopped) => {}
                Err(e) => tracing::error!("{e:#}"),
            }
        }
        // A task these started meanwhile was filed behind them.
        let mut tasks = self.inner.tasks.borrow_mut();
        kept.append(&mut tasks);
        *tasks = kept;
    }

    pub(super) fn answer(&self, done: Option<u64>, v: Value) {
        if let Some(done) = done {
            balaur_core::timers::wake_next_step(&self.inner.engine, done, v);
        }
    }

    // Busy file or instance: not yet, and the next frame asks again.
    fn step(&self, p: Pending) -> Result<Polled> {
        let file = self.inner.files.borrow().get(&p.path).cloned();
        let inst = self
            .inner
            .instances
            .borrow()
            .iter()
            .find(|i| i.node == p.node)
            .cloned();
        let (Some(file), Some(inst)) = (file, inst) else {
            return Err(anyhow!("[{}] {}: the instance is gone", p.path, p.method));
        };
        let Ok(mut rt) = file.rt.try_borrow_mut() else {
            return Ok(Polled::Waiting(p));
        };
        let _turn = balaur_core::dependencies::calling(&self.inner.engine, &inst.path);
        self.poll_held(&file, &mut rt, &inst, p)
    }

    // One poll of `p` with its instance's object; in a file built for the
    // debugger a stop holds both, and a trap may hold its stack.
    fn poll_held(
        &self,
        file: &File,
        rt: &mut Runtime,
        inst: &Instance,
        mut p: Pending,
    ) -> Result<Polled> {
        let Some(mut obj) = inst.obj.borrow_mut().take() else {
            return Ok(Polled::Waiting(p));
        };
        // A call back into the node while the task runs is lent a holder of
        // the object, as one while a plain method runs is.
        *inst.lent.borrow_mut() = rt.share(&obj);
        let before = rt.steps();
        let step = poll(rt, file.compiled.meta.debug, &mut p.task, &mut obj);
        self.charge(&p.path, before, rt.steps());
        if let Some(lent) = inst.lent.borrow_mut().take() {
            rt.drop_object(lent);
        }
        if let Ok(Step::Paused(_)) = step {
            self.hold_task(obj, p);
            return Ok(Polled::Stopped);
        }
        *inst.obj.borrow_mut() = Some(obj);
        match step {
            Ok(Step::Waiting) => Ok(Polled::Waiting(p)),
            Ok(Step::Done(v)) => Ok(Polled::Done(to_neutral(&v), p.done)),
            Ok(Step::Paused(_)) => unreachable!("held above"),
            Err(e) => {
                let e = anyhow!("[{}] {}: {e}", p.path, p.method);
                self.hold_error(rt, p.node, &p.path, &e);
                Err(e)
            }
        }
    }

    pub(super) fn has_tasks(&self) -> bool {
        !self.inner.tasks.borrow().is_empty()
    }

    /// Ends what `node` was running.
    pub(super) fn cancel(&self, node: u64) {
        let ended = self.take_where(|p| p.node == node);
        for p in ended {
            let Some(file) = self.inner.files.borrow().get(&p.path).cloned() else {
                continue;
            };
            let ended = match file.rt.try_borrow_mut() {
                Ok(mut rt) => rt.cancel(p.task),
                Err(_) => file.reentry.cancel(p.task),
            };
            if let Err(e) = ended {
                tracing::error!("[{}] {}: {e}", p.path, p.method);
            }
        }
    }

    /// Drops what `key` was running: a reload replaces the memory it ran in.
    pub(super) fn forget(&self, key: &str) {
        let dropped = self.take_where(|p| p.path == key);
        if !dropped.is_empty() {
            tracing::warn!("{key}: a reload ended {} running task(s)", dropped.len());
        }
    }

    fn take_where(&self, pick: impl Fn(&Pending) -> bool) -> Vec<Pending> {
        let mut tasks = self.inner.tasks.borrow_mut();
        let (taken, kept) = std::mem::take(&mut *tasks).into_iter().partition(pick);
        *tasks = kept;
        taken
    }
}

/// What a poll left.
enum Polled {
    Waiting(Pending),
    /// The result, and who awaits it.
    Done(Value, Option<u64>),
    /// A debugger stop holds the task.
    Stopped,
}

fn poll(
    rt: &mut Runtime,
    debug: bool,
    task: &mut Task,
    obj: &mut Object,
) -> Result<Step, RunError> {
    if debug {
        rt.poll_steppable(task, Some(obj))
    } else {
        Ok(rt.poll(task, Some(obj))?.map_or(Step::Waiting, Step::Done))
    }
}
