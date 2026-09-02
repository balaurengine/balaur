//! The host's side of the debugger: an execution parked on a breakpoint
//! becoming the pause, the pause letting go, and the breakpoints resolved
//! against each unit.

use std::collections::HashSet;
use std::rc::Rc;

use anyhow::Result;
use balaur_script::{Pause, StepMode};
use hecs::Entity;
use rune::alloc::clone::TryClone as _;
use rune::runtime::{VmExecution, VmResult};
use rune::{TypeHash as _, Vm};

use crate::debugger::{self, Hit, Lines, Outcome, StepPlan};
use crate::{Paused, RuneHost, State};

/// The instance and method an execution is running for.
#[derive(Clone, Copy)]
struct Callee<'a> {
    owner: Entity,
    key: &'a str,
    label: &'a str,
}

impl RuneHost {
    /// Call `name` on one instance: through the stepping executor when its
    /// unit has breakpoints and the function is synchronous, through a plain
    /// call otherwise. The ticks pass `allow_async` false: they may not
    /// suspend.
    pub(crate) fn invoke(
        &self,
        owner: Entity,
        key: &str,
        name: &str,
        args: Vec<rune::Value>,
        allow_async: bool,
    ) -> Option<balaur_script::Value> {
        let stepping = {
            let state = self.state.borrow();
            let has_breakpoints = state
                .breakpoints
                .get(key)
                .is_some_and(|b| !b.ips.is_empty());
            let sync = state
                .scripts
                .get(key)
                .and_then(|s| s.functions.iter().find(|f| f.name == name))
                .is_some_and(|f| !f.is_async);
            // Breaking where a script threw needs the instruction it threw
            // on, which only the stepping executor still has.
            sync && (has_breakpoints || state.break_on_error)
        };
        if stepping {
            return self.invoke_stepping(owner, key, name, args);
        }
        let f = self.method(key, name)?;
        match f.call::<rune::Value>(args) {
            VmResult::Ok(value) => {
                if !allow_async && value.type_hash() == rune::runtime::Future::HASH {
                    tracing::error!(
                        "[{key}] {name} cannot be async; suspend in init or a handler instead"
                    );
                    return None;
                }
                self.settle_call(owner, key, name, value)
            }
            VmResult::Err(err) => {
                tracing::error!("[{key}] {name}: {err}");
                None
            }
        }
    }

    fn invoke_stepping(
        &self,
        owner: Entity,
        key: &str,
        name: &str,
        args: Vec<rune::Value>,
    ) -> Option<balaur_script::Value> {
        let (unit, lines) = {
            let state = self.state.borrow();
            let script = state.scripts.get(key)?;
            (script.unit.clone(), script.lines.clone())
        };
        let (_, runtime) = self.context().ok()?;
        let mut vm = Vm::new(runtime, unit);
        // `into_owned` moves the stack but not the instruction pointer the
        // entrypoint set, so it is carried across by hand.
        let exec = match vm.execute([name], args) {
            Ok(exec) => {
                let ip = exec.vm().ip();
                let mut execution = exec.into_owned();
                execution.vm_mut().set_ip(ip);
                execution
            }
            Err(err) => {
                tracing::error!("[{key}] {name}: {err}");
                return None;
            }
        };
        let callee = Callee {
            owner,
            key,
            label: name,
        };
        self.drive(&callee, exec, &lines, None, None)
    }

    /// Run an execution to its next stop. `leaving` is the instruction it
    /// was parked on, which runs without breaking again.
    fn drive(
        &self,
        callee: &Callee<'_>,
        mut exec: VmExecution<Vm>,
        lines: &Rc<Lines>,
        plan: Option<StepPlan>,
        leaving: Option<usize>,
    ) -> Option<balaur_script::Value> {
        let ips = self
            .state
            .borrow()
            .breakpoints
            .get(callee.key)
            .map(|b| b.ips.clone())
            .unwrap_or_default();
        match debugger::run(&mut exec, lines, &ips, plan, leaving) {
            Outcome::Finished(value) => {
                self.settle_call(callee.owner, callee.key, callee.label, value)
            }
            Outcome::Failed { error, line } => {
                tracing::error!("[{}] {}: {error}", callee.key, callee.label);
                if self.state.borrow().break_on_error {
                    let hit = Hit {
                        line,
                        reason: balaur_script::PauseReason::Error,
                    };
                    return self.park(callee, exec, lines, hit, error.to_string());
                }
                None
            }
            Outcome::Broke(hit) => self.park(callee, exec, lines, hit, String::new()),
        }
    }

    /// File a stopped execution as the pause and freeze the engine. A second
    /// stop while one is filed is waved on: the editor shows one pause.
    fn park(
        &self,
        callee: &Callee<'_>,
        exec: VmExecution<Vm>,
        lines: &Rc<Lines>,
        hit: Hit,
        message: String,
    ) -> Option<balaur_script::Value> {
        let ip = exec.vm().ip();
        if self.state.borrow().paused.is_some() {
            return self.drive(callee, exec, lines, None, Some(ip));
        }
        let Callee { owner, key, label } = *callee;
        let frames = debugger::frames(&exec, lines, key, hit.line);
        let pause = Pause {
            node: balaur_core::node_id_of(owner),
            path: key.to_string(),
            line: hit.line,
            reason: hit.reason,
            frames,
            message,
        };
        tracing::info!(
            "[{key}] paused at {}:{} ({})",
            pause.path,
            pause.line,
            pause.reason.name()
        );
        self.state.borrow_mut().paused = Some(Paused {
            owner,
            key: key.to_string(),
            label: label.to_string(),
            exec,
            ip,
            lines: lines.clone(),
            pause,
            remaining: Vec::new(),
            method: None,
        });
        self.engine.set_frozen(true);
        None
    }

    /// Forget a pause whose script or node is gone.
    pub(crate) fn drop_pause(&self, _paused: &Paused) {
        self.engine.set_frozen(false);
    }

    /// Stop where a script threw. Every synchronous call then runs through
    /// the stepping executor, which is what keeps the failing instruction.
    pub fn set_break_on_error(&self, on: bool) {
        self.state.borrow_mut().break_on_error = on;
    }

    #[must_use]
    pub fn break_on_error(&self) -> bool {
        self.state.borrow().break_on_error
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
        let Paused {
            owner,
            key,
            label,
            exec,
            ip,
            lines,
            pause,
            remaining,
            method,
        } = paused;
        // A throw ended the call: there is no instruction to go on from, so
        // letting go drops it and finishes the tick it interrupted.
        if pause.reason == balaur_script::PauseReason::Error {
            if let Some((method, dt)) = method {
                self.run_batch(&method, dt, remaining);
            }
            return;
        }
        let plan = (mode != StepMode::Continue).then(|| StepPlan {
            mode,
            depth: exec.vm().call_frames().len(),
            line: pause.line,
        });
        let callee = Callee {
            owner,
            key: &key,
            label: &label,
        };
        self.drive(&callee, exec, &lines, plan, Some(ip));
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
    /// file not loaded yet keeps the request for when its unit arrives.
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
        self.apply_breakpoints(&key);
        Ok(self.breakpoints(&key))
    }

    /// One file's breakpoints as they landed, or as requested while the file
    /// is not loaded.
    pub fn breakpoints(&self, path: &str) -> Vec<usize> {
        let key = Self::normalize_key(path);
        let state = self.state.borrow();
        state.breakpoints.get(&key).map_or_else(Vec::new, |b| {
            if state.scripts.contains_key(&key) {
                b.landed.clone()
            } else {
                b.requested.clone()
            }
        })
    }

    /// Resolve a file's requested lines against its loaded unit.
    pub(crate) fn apply_breakpoints(&self, key: &str) {
        let mut state = self.state.borrow_mut();
        let Some(lines) = state.scripts.get(key).map(|s| s.lines.clone()) else {
            return;
        };
        let Some(b) = state.breakpoints.get_mut(key) else {
            return;
        };
        let mut landed = Vec::new();
        let mut ips = HashSet::new();
        for line in &b.requested {
            if let Some((at, on)) = lines.breakpoint(*line) {
                landed.push(at);
                ips.extend(on);
            }
        }
        landed.sort_unstable();
        landed.dedup();
        b.landed = landed;
        b.ips = ips;
    }

    /// Whether the debugger keeps `entity` from running: it is the paused
    /// instance, or it lives under the frozen root.
    pub(crate) fn is_held(&self, entity: Entity, state: &State) -> bool {
        if state.paused.as_ref().is_some_and(|p| p.owner == entity) {
            return true;
        }
        self.engine
            .frozen_root()
            .is_some_and(|root| balaur_core::scene::is_within(&self.engine.world(), entity, root))
    }

    /// The instances a tick visits, collected first so a script may attach,
    /// detach or spawn during its own update without the host state being
    /// borrowed. The paused instance and everything under the frozen root
    /// stay out.
    pub(crate) fn live_batch(&self) -> Vec<(Entity, String, rune::Value)> {
        let state = self.state.borrow();
        state
            .instances
            .iter()
            .filter(|(e, _)| !self.is_held(**e, &state))
            .filter_map(|(e, i)| Some((*e, i.key.clone(), i.state.try_clone().ok()?)))
            .collect()
    }

    /// Run `method(dt)` over `batch`. A breakpoint stops the batch where it
    /// is; the remainder is filed with the pause and runs on resume.
    pub(crate) fn run_batch(
        &self,
        method: &str,
        dt: f32,
        batch: Vec<(Entity, String, rune::Value)>,
    ) {
        let dt_value = match rune::to_value(f64::from(dt)) {
            Ok(value) => value,
            Err(err) => {
                tracing::error!("{method}: {err}");
                return;
            }
        };
        let mut batch = batch.into_iter();
        while let Some((entity, key, state)) = batch.next() {
            let Ok(dt_arg) = dt_value.try_clone() else {
                continue;
            };
            self.invoke(entity, &key, method, vec![state, dt_arg], false);
            let mut host = self.state.borrow_mut();
            if let Some(paused) = host
                .paused
                .as_mut()
                .filter(|p| p.owner == entity && p.method.is_none())
            {
                paused.remaining = batch.collect();
                paused.method = Some((method.to_string(), dt));
                return;
            }
        }
    }
}
