// A call back into a file whose module is itself inside an engine call: a
// script calling a method of its own file through `node.call`. It runs on the
// same instance, through the context the engine call holds.

use super::engine::{ImportCtx, Raw, Shared, Wasm};
use super::{Limits, Object, RunError, Runtime, Task, Value, fail};
use crate::{Meta, words};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Reaches a file's module while its [`Runtime`] is busy in an engine call.
#[derive(Clone)]
pub struct Reentry {
    host: Shared,
    units: Rc<RefCell<Vec<Arc<Meta>>>>,
    limits: Limits,
}

impl Runtime {
    pub fn reentry(&self) -> Reentry {
        Reentry {
            host: self.host.clone(),
            units: self.units.clone(),
            limits: self.limits,
        }
    }
}

impl Reentry {
    /// [`Runtime::call_method`] from inside the file's engine call.
    pub fn call_method(
        &self,
        obj: &mut Object,
        method: &str,
        args: &[Value],
    ) -> Result<Value, RunError> {
        self.inside(|rt| rt.call_method(obj, method, args))
    }

    /// [`Runtime::start`] and a first [`Runtime::poll`], from inside the
    /// file's engine call; the task is polled later on the runtime itself.
    pub fn start(
        &self,
        key: &str,
        args: &[Value],
        obj: &mut Object,
    ) -> Result<(Task, Option<Value>), RunError> {
        self.inside(|rt| {
            let mut task = rt.in_unit(obj.unit, |rt| rt.start(key, args))?;
            let out = rt.poll(&mut task, Some(obj))?;
            Ok((task, out))
        })
    }

    /// [`Runtime::get`] from inside the file's engine call.
    pub fn get(&self, obj: &Object, field: &str) -> Option<Value> {
        self.inside(|rt| Ok(rt.get(obj, field))).ok().flatten()
    }

    /// [`Runtime::object`] in unit `unit`, from inside the file's engine call.
    pub fn object(
        &self,
        unit: usize,
        ty: &str,
        init: &[(&str, Value)],
    ) -> Result<Object, RunError> {
        self.inside(|rt| rt.in_unit(unit, |rt| rt.object(ty, init)))
    }

    /// [`Runtime::link`] from inside the file's engine call: a script
    /// loaded while another of the instance runs.
    pub fn link(
        &self,
        key: &str,
        unit: &Arc<crate::Compiled>,
        uses: &[(String, Arc<crate::Compiled>)],
    ) -> Result<usize, RunError> {
        self.inside(|rt| rt.link(key, unit, uses))
    }

    /// [`Runtime::share`] from inside the file's engine call.
    pub fn share(&self, obj: &Object) -> Option<Object> {
        self.inside(|rt| Ok(rt.share(obj))).ok().flatten()
    }

    /// [`Runtime::drop_object`] from inside the file's engine call.
    pub fn drop_object(&self, obj: Object) -> Result<(), RunError> {
        self.inside(|rt| {
            rt.drop_object(obj);
            Ok(())
        })
    }

    /// [`Runtime::cancel`] from inside the file's engine call.
    pub fn cancel(&self, task: Task) -> Result<(), RunError> {
        self.inside(|rt| rt.cancel(task))
    }

    // A runtime over the engine call's context, for as long as `f` runs. It
    // spends the outer call's fuel, and a debugger stop inside it is passed
    // by, since only the outermost call can park.
    fn inside<R>(
        &self,
        f: impl FnOnce(&mut Runtime) -> Result<R, RunError>,
    ) -> Result<R, RunError> {
        let Some(ctx) = self.host.borrow().inside else {
            return fail("the file is busy outside an engine call");
        };
        let meta = self.units.borrow()[0].clone();
        let mut rt = Runtime {
            wasm: Box::new(Inside(ctx)),
            host: self.host.clone(),
            meta,
            units: self.units.clone(),
            unit: 0,
            limits: self.limits,
            parked_call: None,
            error_stop: None,
            nested: true,
        };
        let park = std::mem::replace(&mut self.host.borrow_mut().debugger.park, false);
        let top = rt.wasm.global(words::DBG_SP);
        let out = f(&mut rt);
        // A trap leaves the shadow stack where it died; the outer call's
        // frames are below it.
        if let (Err(_), Some(top)) = (&out, top) {
            rt.wasm.set_global(words::DBG_SP, top);
        }
        self.host.borrow_mut().debugger.park = park;
        out
    }
}

/// The module as the engine call sees it.
struct Inside(*mut (dyn ImportCtx + 'static));

impl Inside {
    fn ctx(&mut self) -> &mut dyn ImportCtx {
        // SAFETY: `Reentry::inside` builds this from `HostState::inside`,
        // which is set only while the engine call that owns the context runs,
        // and drops it before returning to that call.
        unsafe { &mut *self.0 }
    }
}

impl Wasm for Inside {
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        self.ctx().call(name, args, results)
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.ctx().has_func(name)
    }
    fn call_in(
        &mut self,
        unit: usize,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Vec<Raw>, String> {
        self.ctx().call_in(unit, name, args, results)
    }
    fn has_func_in(&mut self, unit: usize, name: &str) -> bool {
        self.ctx().has_func_in(unit, name)
    }
    fn link(&mut self, key: &str, unit: Arc<crate::Compiled>) -> Result<usize, String> {
        self.ctx().link(key, unit)
    }
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        // SAFETY: as in `ctx`.
        unsafe { &*self.0 }.read(at, len)
    }
    fn memory_len(&self) -> usize {
        0
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        self.ctx().write(at, bytes);
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        self.ctx().global(name)
    }
    fn set_global(&mut self, name: &str, value: i32) {
        self.ctx().set_global(name, value);
    }
    fn set_fuel(&mut self, _fuel: u64) {}
}
