// A method or a task's poll that a debugger stop parks at a line, for
// `resume` to carry on later; an engine that cannot park a call passes the
// stop by.

use super::debugger::{Command, Paused};
use super::engine::{Outcome, Raw};
use super::{Object, RunError, Runtime, Task, Value, fail, width};
use crate::{Ty, words};

/// How a steppable call came back.
#[derive(Debug, PartialEq)]
pub enum Step {
    Done(Value),
    /// A polled task reached its next `await`.
    Waiting,
    /// Stopped at a line; [`Runtime::resume`] or [`Runtime::resume_task`]
    /// goes on.
    Paused(Paused),
}

/// An export to call, and what it answers with.
pub(super) struct Call {
    pub name: String,
    pub args: Vec<Raw>,
    pub results: usize,
    pub ret: Ty,
}

/// What finishing a parked call needs.
pub(super) struct ParkedCall {
    ret: Ty,
    poll: bool,
    /// The unit whose function was called, whose types read its result.
    unit: usize,
    /// The shadow stack's top at the stop: a call made meanwhile that traps
    /// goes back to it.
    pub(super) top: Option<i32>,
}

/// Who a parked call hands its results to.
enum Owner<'a> {
    Method,
    Task(&'a mut Task),
}

impl Runtime {
    /// Whether a call here should be one a stop can hold: a unit of the
    /// instance has probes, and no other call is parked.
    #[must_use]
    pub fn steppable(&self) -> bool {
        self.parked_call.is_none() && self.host.borrow().debugger.probed()
    }

    /// [`Runtime::call_method`], but a stop parks the call, and
    /// [`Runtime::resume`] goes on with it.
    pub fn call_method_steppable(
        &mut self,
        obj: &Object,
        method: &str,
        args: &[Value],
    ) -> Result<Step, RunError> {
        self.in_unit(obj.unit, |rt| {
            let key = format!("{}.{method}", obj.ty);
            let info = rt.meta.function(&key).cloned().ok_or(RunError {
                msg: format!("no method `{key}`"),
                line: None,
            })?;
            let mut a = obj.raw.clone();
            for (t, v) in info.params.iter().zip(args) {
                a.extend(rt.encode(t, v)?);
            }
            let results = width(&rt.meta, &info.ret);
            if rt.parked_call.is_some() {
                return fail("the script is stopped at a breakpoint");
            }
            rt.prepare()?;
            rt.parked_call = Some(ParkedCall {
                ret: info.ret,
                poll: false,
                unit: rt.unit,
                top: None,
            });
            let outcome = rt.wasm.call_resumable(rt.unit, &key, &a, results);
            rt.settle(outcome, Owner::Method)
        })
    }

    /// [`Runtime::poll`], but a stop parks the poll; [`Runtime::resume_task`]
    /// goes on with the same `task`.
    pub fn poll_steppable(
        &mut self,
        task: &mut Task,
        obj: Option<&Object>,
    ) -> Result<Step, RunError> {
        if self.parked_call.is_some() {
            return fail("the script is stopped at a breakpoint");
        }
        self.in_unit(task.unit, |rt| {
            let Some(call) = rt.poll_call(task, obj)? else {
                return Ok(Step::Waiting);
            };
            rt.prepare()?;
            rt.parked_call = Some(ParkedCall {
                ret: call.ret,
                poll: true,
                unit: rt.unit,
                top: None,
            });
            let outcome = rt
                .wasm
                .call_resumable(rt.unit, &call.name, &call.args, call.results);
            rt.settle(outcome, Owner::Task(task))
        })
    }

    /// Lets the parked method go on under `cmd`.
    pub fn resume(&mut self, cmd: Command) -> Result<Step, RunError> {
        let unit = self
            .parked_call
            .as_ref()
            .map_or(self.unit, |call| call.unit);
        self.in_unit(unit, |rt| {
            let outcome = rt.go_on(cmd, false)?;
            rt.settle(outcome, Owner::Method)
        })
    }

    /// Lets the parked poll of `task` go on under `cmd`.
    pub fn resume_task(&mut self, cmd: Command, task: &mut Task) -> Result<Step, RunError> {
        let unit = self
            .parked_call
            .as_ref()
            .map_or(self.unit, |call| call.unit);
        self.in_unit(unit, |rt| {
            let outcome = rt.go_on(cmd, true)?;
            rt.settle(outcome, Owner::Task(task))
        })
    }

    /// The stop a parked call is held at.
    pub fn paused(&self) -> Option<Paused> {
        self.host.borrow().debugger.parked().cloned()
    }

    fn go_on(&mut self, cmd: Command, poll: bool) -> Result<Result<Outcome, String>, RunError> {
        if self.parked_call.as_ref().map(|c| c.poll) != Some(poll) {
            let what = if poll { "poll" } else { "method" };
            return fail(format!("no {what} is parked"));
        }
        let Some(flags) = self.host.borrow_mut().debugger.unpark(cmd) else {
            return fail("no call is parked");
        };
        self.wasm.set_global(words::DBG_FLAGS, flags);
        self.sync_debugger()?;
        self.host.borrow_mut().debugger.park = true;
        Ok(self.wasm.resume())
    }

    fn prepare(&mut self) -> Result<(), RunError> {
        self.error_stop = None;
        self.refuel();
        self.sync_debugger()?;
        let can_park = self.wasm.can_park();
        let mut host = self.host.borrow_mut();
        host.debugger.park = can_park && host.debugger.probed();
        Ok(())
    }

    fn settle(
        &mut self,
        outcome: Result<Outcome, String>,
        owner: Owner<'_>,
    ) -> Result<Step, RunError> {
        if let Ok(Outcome::Parked) = outcome {
            let paused = self.paused().ok_or(RunError {
                msg: "internal: a call parked with no stop".into(),
                line: None,
            })?;
            let top = self.wasm.global(words::DBG_SP);
            if let Some(call) = &mut self.parked_call {
                call.top = top;
            }
            return Ok(Step::Paused(paused));
        }
        self.host.borrow_mut().debugger.park = false;
        let call = self.parked_call.take().ok_or(RunError {
            msg: "internal: a call finished that was not started".into(),
            line: None,
        })?;
        let out = match outcome {
            Ok(Outcome::Done(out)) => out,
            Ok(Outcome::Parked) => unreachable!("handled above"),
            Err(msg) => return Err(self.trapped(msg)),
        };
        match owner {
            Owner::Method => Ok(Step::Done(self.take(&call.ret, &out))),
            Owner::Task(task) => Ok(self
                .polled(task, &call.ret, &out)?
                .map_or(Step::Waiting, Step::Done)),
        }
    }
}
