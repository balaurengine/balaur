// wasmi: a pure-Rust interpreter, so it runs everywhere, iOS included.

use super::engine::{ImportCtx, Outcome, Raw, Setup, Shared, Wasm, read_from};
use super::host::dispatch;
use crate::{Slot, words};
use wasmi::{Caller, Engine, Extern, FuncType, Linker, Store, Val, ValType};

struct Data {
    mem: Option<wasmi::Memory>,
    host: Shared,
    limits: Option<wasmi::StoreLimits>,
}

struct Ctx<'a, 'b>(&'a mut Caller<'b, Data>);

/// What a probe traps with to park its call at a debugger stop.
#[derive(Debug)]
struct Park;

impl std::fmt::Display for Park {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("parked at a debugger stop")
    }
}

impl wasmi::errors::HostError for Park {}

impl ImportCtx for Ctx<'_, '_> {
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        self.0
            .data()
            .mem
            .map_or_else(|| vec![0; len], |m| read_from(m.data(&*self.0), at, len))
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        if let Some(m) = self.0.data().mem {
            m.data_mut(&mut *self.0)[at..at + bytes.len()].copy_from_slice(bytes);
        }
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        match self
            .0
            .get_export(name)
            .and_then(Extern::into_global)
            .map(|g| g.get(&*self.0))
        {
            Some(Val::I32(v)) => Some(v),
            _ => None,
        }
    }
    fn set_global(&mut self, name: &str, value: i32) {
        if let Some(g) = self.0.get_export(name).and_then(Extern::into_global) {
            let _ = g.set(&mut *self.0, Val::I32(value));
        }
    }
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        let f = self
            .0
            .get_export(name)
            .and_then(Extern::into_func)
            .ok_or_else(|| format!("no export `{name}`"))?;
        let a: Vec<Val> = args.iter().map(|r| val(*r)).collect();
        let mut out = vec![Val::I32(0); results];
        f.call(&mut *self.0, &a, &mut out)
            .map_err(|e| e.to_string())?;
        Ok(out.iter().map(raw).collect())
    }
}

fn vt(s: Slot) -> ValType {
    match s {
        Slot::I32 => ValType::I32,
        Slot::I64 => ValType::I64,
        Slot::F64 => ValType::F64,
    }
}

fn raw(v: &Val) -> Raw {
    match v {
        Val::I64(x) => Raw::I64(*x),
        Val::F64(x) => Raw::F64(f64::from(*x)),
        Val::I32(x) => Raw::I32(*x),
        _ => Raw::I32(0),
    }
}

fn val(r: Raw) -> Val {
    match r {
        Raw::I32(x) => Val::I32(x),
        Raw::I64(x) => Val::I64(x),
        Raw::F64(x) => Val::F64(x.into()),
    }
}

pub(crate) fn instantiate(setup: Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    let mut config = wasmi::Config::default();
    config.consume_fuel(setup.fuel.is_some());
    let engine = Engine::new(&config);
    let module =
        wasmi::Module::new(&engine, setup.wasm).map_err(|e| format!("invalid module: {e}"))?;
    let mut linker = Linker::<Data>::new(&engine);
    for (index, imp) in setup.meta.imports.iter().enumerate() {
        let ty = FuncType::new(
            imp.params.iter().map(|s| vt(*s)),
            imp.results.iter().map(|s| vt(*s)),
        );
        let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
            let host = c.data().host.clone();
            let args: Vec<Raw> = params.iter().map(raw).collect();
            let out = dispatch(&host, &mut Ctx(&mut c), index, &args).map_err(wasmi::Error::new)?;
            if host.borrow().debugger.is_parked() {
                return Err(wasmi::Error::host(Park));
            }
            for (slot, r) in results.iter_mut().zip(out) {
                *slot = val(r);
            }
            Ok(())
        };
        linker
            .func_new(words::HOST_MODULE, &imp.name, ty, call)
            .map_err(|e| e.to_string())?;
    }
    let limits = setup
        .memory_bytes
        .map(|m| wasmi::StoreLimitsBuilder::new().memory_size(m).build());
    let mut store = Store::new(
        &engine,
        Data {
            mem: None,
            host: setup.host,
            limits,
        },
    );
    if store.data().limits.is_some() {
        store.limiter(|d| {
            d.limits
                .as_mut()
                .expect("limits are built whenever a memory cap is set")
        });
    }
    if let Some(f) = setup.fuel {
        let _ = store.set_fuel(f);
    }
    let inst = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| e.to_string())?;
    store.data_mut().mem = inst.get_memory(&store, words::MEMORY);
    Ok(Box::new(Wasmi {
        store,
        inst,
        parked: None,
    }))
}

struct Wasmi {
    store: Store<Data>,
    inst: wasmi::Instance,
    /// A call a debugger stop parked, and how many results it owes.
    parked: Option<(wasmi::ResumableCallHostTrap, usize)>,
}

impl Wasmi {
    fn settle(&mut self, call: wasmi::ResumableCall, out: &[Val]) -> Result<Outcome, String> {
        match call {
            wasmi::ResumableCall::Finished => Ok(Outcome::Done(out.iter().map(raw).collect())),
            wasmi::ResumableCall::HostTrap(trap)
                if trap.host_error().downcast_ref::<Park>().is_some() =>
            {
                self.parked = Some((trap, out.len()));
                Ok(Outcome::Parked)
            }
            wasmi::ResumableCall::HostTrap(trap) => Err(trap.into_host_error().to_string()),
            wasmi::ResumableCall::OutOfFuel(_) => Err("all fuel consumed".into()),
        }
    }
}

impl Wasm for Wasmi {
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        let f = self
            .inst
            .get_func(&self.store, name)
            .ok_or_else(|| format!("no export `{name}`"))?;
        let a: Vec<Val> = args.iter().map(|r| val(*r)).collect();
        let mut out = vec![Val::I32(0); results];
        f.call(&mut self.store, &a, &mut out)
            .map_err(|e| e.to_string())?;
        Ok(out.iter().map(raw).collect())
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.inst.get_func(&self.store, name).is_some()
    }
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        self.store
            .data()
            .mem
            .map_or_else(|| vec![0; len], |m| read_from(m.data(&self.store), at, len))
    }
    fn memory_len(&self) -> usize {
        self.store
            .data()
            .mem
            .map_or(0, |m| m.data(&self.store).len())
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        if let Some(m) = self.store.data().mem {
            m.data_mut(&mut self.store)[at..at + bytes.len()].copy_from_slice(bytes);
        }
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        match self
            .inst
            .get_global(&self.store, name)
            .map(|g| g.get(&self.store))
        {
            Some(Val::I32(v)) => Some(v),
            _ => None,
        }
    }
    fn set_global(&mut self, name: &str, value: i32) {
        if let Some(g) = self.inst.get_global(&self.store, name) {
            let _ = g.set(&mut self.store, Val::I32(value));
        }
    }
    fn set_fuel(&mut self, fuel: u64) {
        let _ = self.store.set_fuel(fuel);
    }
    fn can_park(&self) -> bool {
        true
    }
    fn call_resumable(
        &mut self,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Outcome, String> {
        let f = self
            .inst
            .get_func(&self.store, name)
            .ok_or_else(|| format!("no export `{name}`"))?;
        let a: Vec<Val> = args.iter().map(|r| val(*r)).collect();
        let mut out = vec![Val::I32(0); results];
        let call = f
            .call_resumable(&mut self.store, &a, &mut out)
            .map_err(|e| e.to_string())?;
        self.settle(call, &out)
    }
    fn resume(&mut self) -> Result<Outcome, String> {
        let (trap, results) = self.parked.take().ok_or("no call is parked")?;
        let mut out = vec![Val::I32(0); results];
        // A probe returns nothing, so it is resumed with nothing.
        let call = trap
            .resume(&mut self.store, &[], &mut out)
            .map_err(|e| e.to_string())?;
        self.settle(call, &out)
    }
}
