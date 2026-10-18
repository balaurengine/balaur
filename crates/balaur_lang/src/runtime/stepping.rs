// A method a debugger stop parks at a line, for `resume` to carry on later;
// an engine that cannot park a call passes the stop by.

use super::debugger::{Command, Paused};
use super::engine::Outcome;
use super::{Object, RunError, Runtime, Value, fail, slots};
use crate::{Ty, words};

/// How a steppable call came back.
#[derive(Debug, PartialEq)]
pub enum Step {
    Done(Value),
    /// Stopped at a line; [`Runtime::resume`] goes on.
    Paused(Paused),
}

/// What finishing a parked method needs.
pub(super) struct ParkedCall {
    ret: Ty,
    nself: usize,
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
        let results = nself + slots(&self.meta, &info.ret).len();
        self.parked_call = Some(ParkedCall {
            ret: info.ret,
            nself,
        });
        self.prepare();
        let outcome = self.wasm.call_resumable(&key, &a, results);
        self.settle(outcome, obj)
    }

    /// Lets the parked call go on under `cmd`.
    pub fn resume(&mut self, cmd: Command, obj: &mut Object) -> Result<Step, RunError> {
        let Some(stepping) = self.host.borrow_mut().debugger.unpark(cmd) else {
            return fail("no call is parked");
        };
        self.wasm.set_global(words::DBG_FLAGS, i32::from(stepping));
        self.sync_debugger();
        self.host.borrow_mut().debugger.park = true;
        let outcome = self.wasm.resume();
        self.settle(outcome, obj)
    }

    /// The stop a parked call is held at.
    pub fn paused(&self) -> Option<Paused> {
        self.host.borrow().debugger.parked().cloned()
    }

    fn prepare(&mut self) {
        if let Some(fuel) = self.limits.fuel {
            self.wasm.set_fuel(fuel);
        }
        self.sync_debugger();
        self.host.borrow_mut().debugger.park = self.meta.debug && self.wasm.can_park();
    }

    fn settle(
        &mut self,
        outcome: Result<Outcome, String>,
        obj: &mut Object,
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
        match outcome {
            Ok(Outcome::Done(out)) => {
                obj.raw = out[..call.nself].to_vec();
                Ok(Step::Done(self.take(&call.ret, &out[call.nself..])))
            }
            Ok(Outcome::Parked) => unreachable!("handled above"),
            Err(msg) => {
                let line = self.trap_line();
                if self.wasm.has_func(words::DBG_RESET) {
                    let _ = self.wasm.call(words::DBG_RESET, &[], 0);
                }
                Err(RunError { msg, line })
            }
        }
    }
}
