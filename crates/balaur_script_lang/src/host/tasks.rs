// Async methods: one that awaits the engine parks here as a task until the
// engine wakes its token, then runs on from the await.

use super::{File, Instance, LangHost};
use crate::convert::{from_neutral, to_neutral};
use anyhow::{Result, anyhow};
use balaur_lang::runtime::{Task, Value as L};
use balaur_script::Value;
use std::rc::Rc;

/// A method suspended on the engine.
pub(super) struct Pending {
    node: u64,
    path: String,
    method: String,
    task: Task,
    /// Where an awaiting caller hears the result (`call_on_async`).
    done: Option<u64>,
}

impl LangHost {
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
        let started = rt.start(&key, args).and_then(|mut task| {
            let out = rt.poll(&mut task, Some(&mut obj))?;
            Ok((task, out))
        });
        *inst.obj.borrow_mut() = Some(obj);
        let (task, out) = started.map_err(|e| anyhow!("[{}] {method}: {e}", inst.path))?;
        if let Some(v) = out {
            return Ok(Some(to_neutral(&v)));
        }
        self.inner.tasks.borrow_mut().push(Pending {
            node: inst.node,
            path: inst.path.clone(),
            method: method.to_string(),
            task,
            done,
        });
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

    /// Polls every parked task; one that finishes answers its caller.
    pub(super) fn run_tasks(&self) {
        let parked = std::mem::take(&mut *self.inner.tasks.borrow_mut());
        let mut kept = Vec::with_capacity(parked.len());
        for mut p in parked {
            match self.step(&mut p) {
                Ok(None) => kept.push(p),
                Ok(Some(v)) => {
                    if let Some(done) = p.done {
                        balaur_core::timers::wake_next_step(&self.inner.engine, done, v);
                    }
                }
                Err(e) => tracing::error!("{e:#}"),
            }
        }
        // A task these started meanwhile was filed behind them.
        let mut tasks = self.inner.tasks.borrow_mut();
        kept.append(&mut tasks);
        *tasks = kept;
    }

    // Busy file or instance: not yet, and the next frame asks again.
    fn step(&self, p: &mut Pending) -> Result<Option<Value>> {
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
            return Ok(None);
        };
        let Some(mut obj) = inst.obj.borrow_mut().take() else {
            return Ok(None);
        };
        let out = rt.poll(&mut p.task, Some(&mut obj));
        *inst.obj.borrow_mut() = Some(obj);
        let out = out.map_err(|e| anyhow!("[{}] {}: {e}", p.path, p.method))?;
        Ok(out.map(|v| to_neutral(&v)))
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
            if let Err(e) = file.rt.borrow_mut().cancel(p.task) {
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
