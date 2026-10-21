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
    nself: usize,
    poll: bool,
}

/// Who a parked call hands its results to.
enum Owner<'a> {
    Method(&'a mut Object),
    Task(&'a mut Task, Option<&'a mut Object>),
}

impl Runtime {
    /// [`Runtime::call_method`], but a stop parks the call. `obj` is handed
    /// back only once the call is done, by this or by [`Runtime::resume`].
    pub fn call_method_steppable(
        &mut self,
        obj: &mut Object,
        method: &str,
        args: &[Value],
    ) -> Result<Step, RunError> {
        let key = format!("{}.{method}", obj.ty);
        let info = self.meta.function(&key).cloned().ok_or(RunError {
            msg: format!("no method `{key}`"),
            line: None,
        })?;
        let mut a = obj.raw.clone();
        for (t, v) in info.params.iter().zip(args) {
            a.extend(self.encode(t, v)?);
        }
        let nself = obj.raw.len();
        let results = nself + width(&self.meta, &info.ret);
        self.parked_call = Some(ParkedCall {
            ret: info.ret,
            nself,
            poll: false,
        });
        self.prepare();
        let outcome = self.wasm.call_resumable(&key, &a, results);
        self.settle(outcome, Owner::Method(obj))
    }

    /// [`Runtime::poll`], but a stop parks the poll; [`Runtime::resume_task`]
    /// goes on with the same `task` and `obj`.
    pub fn poll_steppable(
        &mut self,
        task: &mut Task,
        obj: Option<&mut Object>,
    ) -> Result<Step, RunError> {
        if self.parked_call.is_some() {
            return fail("the script is stopped at a breakpoint");
        }
        let Some(call) = self.poll_call(task, obj.as_deref())? else {
            return Ok(Step::Waiting);
        };
        self.parked_call = Some(ParkedCall {
            ret: call.ret,
            nself: call.results - 1,
            poll: true,
        });
        self.prepare();
        let outcome = self
            .wasm
            .call_resumable(&call.name, &call.args, call.results);
        self.settle(outcome, Owner::Task(task, obj))
    }

    /// Lets the parked method go on under `cmd`.
    pub fn resume(&mut self, cmd: Command, obj: &mut Object) -> Result<Step, RunError> {
        let outcome = self.go_on(cmd, false)?;
        self.settle(outcome, Owner::Method(obj))
    }

    /// Lets the parked poll of `task` go on under `cmd`.
    pub fn resume_task(
        &mut self,
        cmd: Command,
        task: &mut Task,
        obj: Option<&mut Object>,
    ) -> Result<Step, RunError> {
        let outcome = self.go_on(cmd, true)?;
        self.settle(outcome, Owner::Task(task, obj))
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
        self.sync_debugger();
        self.host.borrow_mut().debugger.park = true;
        Ok(self.wasm.resume())
    }

    fn prepare(&mut self) {
        self.error_stop = None;
        self.refuel();
        self.sync_debugger();
        self.host.borrow_mut().debugger.park = self.meta.debug && self.wasm.can_park();
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
            Owner::Method(obj) => {
                obj.raw = out[..call.nself].to_vec();
                Ok(Step::Done(self.take(&call.ret, &out[call.nself..])))
            }
            Owner::Task(task, obj) => Ok(self
                .polled(task, obj, &call.ret, &out)?
                .map_or(Step::Waiting, Step::Done)),
        }
    }
}
