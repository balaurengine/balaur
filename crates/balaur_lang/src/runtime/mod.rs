//! The host side: runs a compiled module on wasmi, on wasmtime (`wasmtime`
//! feature) or in the browser's own engine (`web` feature, on wasm32).
//!
//! Script state lives on the host as [`Object`]s; a method is called with its
//! fields and hands them back, so a reload is a new instance plus a copy of
//! each object's fields by name.

mod debugger;
mod engine;
mod host;
mod stepping;
mod values;
#[cfg(feature = "wasmi")]
mod wasmi_engine;
#[cfg(feature = "wasmtime")]
mod wasmtime_engine;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
mod web_engine;

pub use debugger::{Command, Debugger, Frame, Paused, StopReason};
pub use engine::EngineKind;
pub use host::HostFns;
pub use stepping::Step;
pub use values::Value;

use crate::{Compiled, Literal, Meta, Ty, show, words};
use engine::{Raw, Setup, Shared, Wasm};
use std::cell::{RefCell, RefMut};
use std::rc::Rc;
use std::sync::Arc;
use values::{Reader, decode, has_heap, rd32, slots, zero};

/// A failure while running: a trap, with the script line when it is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunError {
    pub msg: String,
    pub line: Option<u32>,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(l) => write!(f, "line {l}: {}", self.msg),
            None => f.write_str(&self.msg),
        }
    }
}

impl std::error::Error for RunError {}

fn fail<T>(msg: impl Into<String>) -> Result<T, RunError> {
    Err(RunError {
        msg: msg.into(),
        line: None,
    })
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    /// Instructions one call may run before it traps; the browser's engine has none.
    pub fuel: Option<u64>,
    pub memory_bytes: Option<usize>,
    pub engine: EngineKind,
}

/// A struct value held by the host: its fields, owning their strings and arrays.
#[derive(Debug)]
pub struct Object {
    ty: String,
    raw: Vec<Raw>,
}

impl Object {
    pub fn type_name(&self) -> &str {
        &self.ty
    }
}

/// One run of an `async fn`, started by the host.
#[derive(Debug)]
pub struct Task {
    key: String,
    frame: u32,
    waiting: Option<i64>,
    done: bool,
}

impl Task {
    pub fn is_done(&self) -> bool {
        self.done
    }
}

struct Live<'a>(&'a dyn Wasm);

impl Reader for Live<'_> {
    fn bytes(&self, at: usize, len: usize) -> Vec<u8> {
        self.0.read(at, len)
    }
}

pub struct Runtime {
    wasm: Box<dyn Wasm>,
    host: Shared,
    meta: Arc<Meta>,
    limits: Limits,
    parked_call: Option<stepping::ParkedCall>,
}

impl Runtime {
    pub fn new(compiled: &Compiled, host: HostFns, limits: Limits) -> Result<Runtime, RunError> {
        let meta = Arc::new(compiled.meta.clone());
        let state = Rc::new(RefCell::new(
            host::HostState::new(meta.clone(), host).map_err(|msg| RunError { msg, line: None })?,
        ));
        let setup = Setup {
            wasm: &compiled.wasm,
            meta: &meta,
            host: state.clone(),
            fuel: limits.fuel,
            memory_bytes: limits.memory_bytes,
        };
        let wasm = engine::instantiate(limits.engine, setup)
            .map_err(|msg| RunError { msg, line: None })?;
        Ok(Runtime {
            wasm,
            host: state,
            meta,
            limits,
            parked_call: None,
        })
    }

    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    pub fn debugger(&mut self) -> RefMut<'_, Debugger> {
        RefMut::map(self.host.borrow_mut(), |h| &mut h.debugger)
    }

    /// Objects allocated in the module and not yet freed: a leak shows here.
    pub fn live(&mut self) -> i32 {
        self.wasm.global(words::LIVE).unwrap_or(-1)
    }

    /// The module's linear memory, in bytes: it grows only when the free lists run dry.
    pub fn memory_bytes(&self) -> usize {
        self.wasm.memory_len()
    }

    pub fn now(&self) -> f64 {
        self.host.borrow().now
    }

    /// Moves the host clock on and resolves every async call that is due.
    pub fn advance(&mut self, dt: f64) {
        let mut h = self.host.borrow_mut();
        h.now += dt;
        let now = h.now;
        let due: Vec<i64> = h
            .timers
            .iter()
            .filter(|(_, (t, _))| *t <= now + 1e-9)
            .map(|(k, _)| *k)
            .collect();
        for k in due {
            if let Some((_, v)) = h.timers.remove(&k) {
                h.resolved.insert(k, v);
            }
        }
    }

    /// The engine the next [`Runtime::reload`] instantiates on: a debugger
    /// moves a file to one that can park a call.
    pub fn set_engine(&mut self, engine: EngineKind) {
        self.limits.engine = engine;
    }

    /// What a script awaiting the engine's `token` expects it to resolve to;
    /// `None` when nothing here awaits it.
    pub fn awaits(&self, token: u64) -> Option<Ty> {
        self.host
            .borrow()
            .tokens
            .get(&token)
            .map(|(_, ty)| ty.clone())
    }

    /// Resolves the engine's `token` to `value`: the task awaiting it runs on
    /// at its next [`Runtime::poll`]. False when nothing here awaits it.
    pub fn wake(&mut self, token: u64, value: Value) -> bool {
        let mut h = self.host.borrow_mut();
        let Some((handle, _)) = h.tokens.remove(&token) else {
            return false;
        };
        h.resolved.insert(handle, value);
        true
    }

    /// Ends `task` where it stands and frees its frame. What its locals held
    /// is not released: a cancelled task leaks its strings and arrays.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "taking the task is what keeps it from being polled again"
    )]
    pub fn cancel(&mut self, task: Task) -> Result<(), RunError> {
        let Task {
            frame,
            waiting,
            done,
            ..
        } = task;
        if done {
            return Ok(());
        }
        let mut h = self.host.borrow_mut();
        if let Some(handle) = waiting {
            h.resolved.remove(&handle);
            h.tokens.retain(|_, (awaiting, _)| *awaiting != handle);
        }
        drop(h);
        self.invoke(words::FREE, &[Raw::I32(frame as i32)], 0)?;
        Ok(())
    }

    /// A new `ty`: fields from `init`, then each field's declared default, then empty.
    pub fn object(&mut self, ty: &str, init: &[(&str, Value)]) -> Result<Object, RunError> {
        let fields = self
            .meta
            .fields(ty)
            .ok_or(RunError {
                msg: format!("no struct `{ty}`"),
                line: None,
            })?
            .to_vec();
        let defaults = self.meta.defaults(ty).to_vec();
        let mut raw = vec![];
        for (n, t) in &fields {
            let given = init.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
            let declared = defaults
                .iter()
                .find(|(k, _, _)| k == n)
                .map(|(_, _, l)| literal(l));
            match given.or(declared) {
                Some(v) => raw.extend(self.encode(t, &v)?),
                None => raw.extend(self.default_of(t)?),
            }
        }
        Ok(Object { ty: ty.into(), raw })
    }

    pub fn get(&mut self, obj: &Object, field: &str) -> Option<Value> {
        let (off, t) = self.field_at(&obj.ty, field)?;
        let n = slots(&self.meta, &t).len();
        Some(decode(
            &self.meta,
            &Live(&*self.wasm),
            &t,
            &obj.raw[off..off + n],
        ))
    }

    /// Every field of `obj`, by name.
    pub fn fields(&mut self, obj: &Object) -> Vec<(String, Value)> {
        let ty = Ty::Struct(obj.ty.clone());
        match decode(&self.meta, &Live(&*self.wasm), &ty, &obj.raw) {
            Value::Struct(_, fields) => fields,
            _ => vec![],
        }
    }

    pub fn set(&mut self, obj: &mut Object, field: &str, v: &Value) -> Result<(), RunError> {
        let (off, t) = self.field_at(&obj.ty, field).ok_or(RunError {
            msg: format!("no field `{field}`"),
            line: None,
        })?;
        let new = self.encode(&t, v)?;
        let old: Vec<Raw> = obj.raw.splice(off..off + new.len(), new).collect();
        self.release(&t, &old);
        Ok(())
    }

    // Taking the object is the point: once released, its fields must not be read.
    #[allow(clippy::needless_pass_by_value)]
    pub fn drop_object(&mut self, obj: Object) {
        let ty = Ty::Struct(obj.ty.clone());
        self.release(&ty, &obj.raw);
    }

    /// Calls a free function.
    pub fn call(&mut self, key: &str, args: &[Value]) -> Result<Value, RunError> {
        let info = self.meta.function(key).cloned().ok_or(RunError {
            msg: format!("no function `{key}`"),
            line: None,
        })?;
        if info.owner.is_some() || info.is_async {
            return fail(format!(
                "`{key}` is a method or async; use `call_method` or `start`"
            ));
        }
        let mut a = vec![];
        for (t, v) in info.params.iter().zip(args) {
            a.extend(self.encode(t, v)?);
        }
        let out = self.invoke(key, &a, slots(&self.meta, &info.ret).len())?;
        Ok(self.take(&info.ret, &out))
    }

    /// Calls `Struct.method` on `obj`, which comes back with whatever the method changed.
    pub fn call_method(
        &mut self,
        obj: &mut Object,
        method: &str,
        args: &[Value],
    ) -> Result<Value, RunError> {
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
        let out = self.invoke(&key, &a, nself + slots(&self.meta, &info.ret).len())?;
        obj.raw = out[..nself].to_vec();
        Ok(self.take(&info.ret, &out[nself..]))
    }

    /// Starts `key` (`fn` or `Struct.method`); [`Runtime::poll`] runs it.
    pub fn start(&mut self, key: &str, args: &[Value]) -> Result<Task, RunError> {
        let info = self.meta.function(key).cloned().ok_or(RunError {
            msg: format!("no function `{key}`"),
            line: None,
        })?;
        if !info.is_async {
            return fail(format!("`{key}` is not async"));
        }
        let frame = self.alloc(info.frame_size)?;
        let mut a = vec![Raw::I32(frame as i32)];
        for (t, v) in info.params.iter().zip(args) {
            a.extend(self.encode(t, v)?);
        }
        self.invoke(&format!("{key}{}", words::START), &a, 0)?;
        Ok(Task {
            key: key.into(),
            frame,
            waiting: None,
            done: false,
        })
    }

    /// Runs `task` up to its next `await`, if what it waits for has resolved;
    /// `Some(value)` once it has finished.
    pub fn poll(
        &mut self,
        task: &mut Task,
        obj: Option<&mut Object>,
    ) -> Result<Option<Value>, RunError> {
        if task.done {
            return fail("the task has finished");
        }
        if task
            .waiting
            .is_some_and(|h| !self.host.borrow().resolved.contains_key(&h))
        {
            return Ok(None);
        }
        let info = self.meta.function(&task.key).cloned().ok_or(RunError {
            msg: format!("no function `{}`", task.key),
            line: None,
        })?;
        let mut a = vec![Raw::I32(task.frame as i32)];
        let nself = obj.as_ref().map_or(0, |o| o.raw.len());
        if let Some(o) = &obj {
            a.extend(o.raw.iter().copied());
        }
        let out = self.invoke(&format!("{}{}", task.key, words::POLL), &a, 1 + nself)?;
        if let Some(o) = obj {
            o.raw = out[1..].to_vec();
        }
        if let Some(h) = task.waiting.take() {
            self.host.borrow_mut().resolved.remove(&h);
        }
        match out[0] {
            Raw::I64(0) => {
                task.done = true;
                let raw: Vec<Raw> = slots(&self.meta, &info.ret)
                    .iter()
                    .enumerate()
                    .map(|(j, (s, _))| {
                        slot_at(&Live(&*self.wasm), *s, task.frame as usize + 8 + 8 * j)
                    })
                    .collect();
                let v = self.take(&info.ret, &raw);
                self.invoke(words::FREE, &[Raw::I32(task.frame as i32)], 0)?;
                Ok(Some(v))
            }
            Raw::I64(h) => {
                task.waiting = Some(h);
                Ok(None)
            }
            _ => fail("internal: a poll returned no status"),
        }
    }

    /// Swaps in `next` and carries each object across by field name: a field
    /// that kept its name and type keeps its value, a new one starts from its
    /// default. Running tasks do not survive a reload.
    pub fn reload(
        &mut self,
        next: &Compiled,
        host: HostFns,
        objects: &mut [&mut Object],
    ) -> Result<Vec<Vec<String>>, RunError> {
        let mut fresh = Runtime::new(next, host, self.limits)?;
        fresh.host.borrow_mut().now = self.now();
        let mut kept_all = vec![];
        let mut replacements = vec![];
        for obj in objects.iter() {
            let old_fields = self.meta.fields(&obj.ty).unwrap_or(&[]).to_vec();
            let carried: Vec<(String, Value)> = self
                .fields(obj)
                .into_iter()
                .filter(|(n, _)| {
                    let t = old_fields
                        .iter()
                        .find(|(k, _)| k == n)
                        .map(|(_, t)| t.clone());
                    fresh
                        .meta
                        .fields(&obj.ty)
                        .unwrap_or(&[])
                        .iter()
                        .any(|(k, kt)| k == n && Some(kt) == t.as_ref())
                })
                .collect();
            let init: Vec<(&str, Value)> = carried
                .iter()
                .map(|(n, v)| (n.as_str(), v.clone()))
                .collect();
            replacements.push(fresh.object(&obj.ty, &init)?);
            kept_all.push(carried.into_iter().map(|(n, _)| n).collect());
        }
        // Nothing changes until every object has its replacement.
        for (obj, replacement) in objects.iter_mut().zip(replacements) {
            let ty = Ty::Struct(obj.ty.clone());
            self.release(&ty, &obj.raw);
            obj.raw = replacement.raw;
        }
        *self = fresh;
        Ok(kept_all)
    }

    fn field_at(&self, ty: &str, field: &str) -> Option<(usize, Ty)> {
        let mut off = 0;
        for (n, t) in self.meta.fields(ty)? {
            if n == field {
                return Some((off, t.clone()));
            }
            off += slots(&self.meta, t).len();
        }
        None
    }

    fn alloc(&mut self, n: u32) -> Result<u32, RunError> {
        let p = self.invoke(words::ALLOC, &[Raw::I32(n as i32)], 1)?[0].i32() as u32;
        self.wasm.write(p as usize, &vec![0; n as usize]);
        Ok(p)
    }

    fn invoke(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, RunError> {
        // A parked call owns the shadow stack until it goes on.
        if self.parked_call.is_some() {
            return fail("the script is stopped at a breakpoint");
        }
        if let Some(fuel) = self.limits.fuel {
            self.wasm.set_fuel(fuel);
        }
        self.sync_debugger();
        match self.wasm.call(name, args, results) {
            Ok(out) => Ok(out),
            Err(msg) => {
                let line = self.trap_line();
                if self.wasm.has_func(words::DBG_RESET) {
                    let _ = self.wasm.call(words::DBG_RESET, &[], 0);
                }
                Err(RunError { msg, line })
            }
        }
    }

    // Debug builds: the breakpoint table and the stepping flag, before running.
    fn sync_debugger(&mut self) {
        if !self.meta.debug {
            return;
        }
        let Some((table, stepping)) = self.host.borrow_mut().debugger.take_table() else {
            return;
        };
        let at = self.wasm.global(words::DBG_BREAKS).unwrap_or(0) as u32 as usize;
        self.wasm.write(at, &table);
        self.wasm.set_global(words::DBG_FLAGS, i32::from(stepping));
    }

    fn trap_line(&mut self) -> Option<u32> {
        if !self.meta.debug {
            return None;
        }
        let (base, top) = (
            self.wasm.global(words::DBG_BASE)?,
            self.wasm.global(words::DBG_SP)?,
        );
        let stack = debugger::shadow_stack(&Live(&*self.wasm), base as u32, top as u32);
        self.host.borrow().debugger.trap_line(&stack)
    }

    // The module's own copy of `v`: strings and arrays are allocated in it.
    fn encode(&mut self, ty: &Ty, v: &Value) -> Result<Vec<Raw>, RunError> {
        Ok(match (ty, v) {
            (Ty::Str, Value::Str(s)) => {
                let p = self.alloc(8 + s.len() as u32)?;
                let mut b = 1i32.to_le_bytes().to_vec();
                b.extend((s.len() as u32).to_le_bytes());
                b.extend(s.as_bytes());
                self.wasm.write(p as usize, &b);
                vec![Raw::I32(p as i32)]
            }
            (Ty::Array(et), Value::Array(items)) => {
                let esz = 8 * slots(&self.meta, et).len();
                let data = if items.is_empty() {
                    0
                } else {
                    self.alloc((items.len() * esz) as u32)?
                };
                for (i, it) in items.iter().enumerate() {
                    let b: Vec<u8> = self
                        .encode(et, it)?
                        .into_iter()
                        .flat_map(Raw::bytes)
                        .collect();
                    self.wasm.write(data as usize + i * esz, &b);
                }
                let h = self.alloc(16)?;
                let mut b = 1i32.to_le_bytes().to_vec();
                for x in [items.len() as u32, items.len() as u32, data] {
                    b.extend(x.to_le_bytes());
                }
                self.wasm.write(h as usize, &b);
                vec![Raw::I32(h as i32)]
            }
            (Ty::Struct(s), Value::Struct(_, fields)) => {
                let decl = self.meta.fields(s).unwrap_or(&[]).to_vec();
                let mut raw = vec![];
                for (n, t) in &decl {
                    match fields.iter().find(|(k, _)| k == n) {
                        Some((_, fv)) => raw.extend(self.encode(t, fv)?),
                        None => raw.extend(self.default_of(t)?),
                    }
                }
                raw
            }
            _ => values::encode_plain(&self.meta, ty, v).map_err(|_| RunError {
                msg: format!("{v:?} is not a {}", show(ty)),
                line: None,
            })?,
        })
    }

    fn default_of(&mut self, ty: &Ty) -> Result<Vec<Raw>, RunError> {
        match ty {
            Ty::Str => self.encode(ty, &Value::Str(String::new())),
            Ty::Array(_) => self.encode(ty, &Value::Array(vec![])),
            Ty::Struct(s) if has_heap(&self.meta, ty) => {
                let decl = self.meta.fields(s).unwrap_or(&[]).to_vec();
                let mut raw = vec![];
                for (_, t) in &decl {
                    raw.extend(self.default_of(t)?);
                }
                Ok(raw)
            }
            _ => Ok(zero(&self.meta, ty)),
        }
    }

    // Reads a returned value and gives its references back.
    fn take(&mut self, ty: &Ty, raw: &[Raw]) -> Value {
        let v = decode(&self.meta, &Live(&*self.wasm), ty, raw);
        if has_heap(&self.meta, ty) {
            self.release(ty, raw);
        }
        v
    }

    fn release(&mut self, ty: &Ty, raw: &[Raw]) {
        for ((_, heap), r) in slots(&self.meta, ty).into_iter().zip(raw) {
            if let Some(h) = heap {
                let name = format!("{}{}", words::DEC, show(&h));
                if self.wasm.has_func(&name) {
                    let _ = self.invoke(&name, &[*r], 0);
                } else {
                    // No release export for the type: nothing in the module keeps one, so
                    // the host made it; strings and plain arrays have no children to release.
                    self.release_flat(&h, r.i32());
                }
            }
        }
    }

    fn release_flat(&mut self, ty: &Ty, p: i32) {
        if p == 0 {
            return;
        }
        if let Ty::Array(et) = ty {
            let live = Live(&*self.wasm);
            let (len, data) = (rd32(&live, p as usize + 4), rd32(&live, p as usize + 12));
            let esz = 8 * slots(&self.meta, et).len() as u32;
            let elements: Vec<Vec<Raw>> = (0..len)
                .map(|i| {
                    slots(&self.meta, et)
                        .iter()
                        .enumerate()
                        .map(|(j, (s, _))| slot_at(&live, *s, (data + i * esz) as usize + 8 * j))
                        .collect()
                })
                .collect();
            for raw in elements {
                self.release(et, &raw);
            }
            if data != 0 {
                let _ = self.invoke(words::FREE, &[Raw::I32(data as i32)], 0);
            }
        }
        let _ = self.invoke(words::FREE, &[Raw::I32(p)], 0);
    }
}

fn slot_at(m: &(impl Reader + ?Sized), s: crate::Slot, at: usize) -> Raw {
    match s {
        crate::Slot::I64 => Raw::I64(values::rd64(m, at) as i64),
        crate::Slot::F64 => Raw::F64(f64::from_bits(values::rd64(m, at))),
        crate::Slot::I32 => Raw::I32(rd32(m, at) as i32),
    }
}

fn literal(l: &Literal) -> Value {
    match l {
        Literal::Int(v) => Value::Int(*v),
        Literal::Float(v) => Value::Float(*v),
        Literal::Bool(v) => Value::Bool(*v),
        Literal::Str(v) => Value::Str(v.clone()),
    }
}
