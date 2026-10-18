// wasmtime: Cranelift compiles the module to machine code, near Rust's speed.
// Desktop and Android; iOS forbids the JIT and the web has its own engine.

use super::engine::{ImportCtx, Raw, Setup, Shared, Wasm, read_from};
use super::host::dispatch;
use crate::{Slot, words};
use wasmtime::{Caller, Engine, Extern, FuncType, Linker, Store, Val, ValType};

struct Data {
    mem: Option<wasmtime::Memory>,
    host: Shared,
    limits: Option<wasmtime::StoreLimits>,
}

struct Ctx<'a, 'b>(&'a mut Caller<'b, Data>);

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
        let g = self.0.get_export(name).and_then(Extern::into_global)?;
        g.get(&mut *self.0).i32()
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
            .map_err(|e| format!("{:#}", e.root_cause()))?;
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
        Val::F64(x) => Raw::F64(f64::from_bits(*x)),
        Val::I32(x) => Raw::I32(*x),
        _ => Raw::I32(0),
    }
}

fn val(r: Raw) -> Val {
    match r {
        Raw::I32(x) => Val::I32(x),
        Raw::I64(x) => Val::I64(x),
        Raw::F64(x) => Val::F64(x.to_bits()),
    }
}

pub(crate) fn instantiate(setup: Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(setup.fuel.is_some());
    // NaN bits are the one result wasm leaves to the hardware.
    config.cranelift_nan_canonicalization(true);
    let engine = Engine::new(&config).map_err(|e| e.to_string())?;
    let module =
        wasmtime::Module::new(&engine, setup.wasm).map_err(|e| format!("invalid module: {e}"))?;
    let mut linker = Linker::<Data>::new(&engine);
    for (index, imp) in setup.meta.imports.iter().enumerate() {
        let ty = FuncType::new(
            &engine,
            imp.params.iter().map(|s| vt(*s)),
            imp.results.iter().map(|s| vt(*s)),
        );
        let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
            let host = c.data().host.clone();
            let args: Vec<Raw> = params.iter().map(raw).collect();
            let out =
                dispatch(&host, &mut Ctx(&mut c), index, &args).map_err(wasmtime::Error::msg)?;
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
        .map(|m| wasmtime::StoreLimitsBuilder::new().memory_size(m).build());
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
        .instantiate(&mut store, &module)
        .map_err(|e| e.to_string())?;
    store.data_mut().mem = inst.get_memory(&mut store, words::MEMORY);
    Ok(Box::new(Wasmtime { store, inst }))
}

struct Wasmtime {
    store: Store<Data>,
    inst: wasmtime::Instance,
}

impl Wasm for Wasmtime {
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        let f = self
            .inst
            .get_func(&mut self.store, name)
            .ok_or_else(|| format!("no export `{name}`"))?;
        let a: Vec<Val> = args.iter().map(|r| val(*r)).collect();
        let mut out = vec![Val::I32(0); results];
        f.call(&mut self.store, &a, &mut out)
            .map_err(|e| format!("{:#}", e.root_cause()))?;
        Ok(out.iter().map(raw).collect())
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.inst.get_func(&mut self.store, name).is_some()
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
        let g = self.inst.get_global(&mut self.store, name)?;
        g.get(&mut self.store).i32()
    }
    fn set_global(&mut self, name: &str, value: i32) {
        if let Some(g) = self.inst.get_global(&mut self.store, name) {
            let _ = g.set(&mut self.store, Val::I32(value));
        }
    }
    fn set_fuel(&mut self, fuel: u64) {
        let _ = self.store.set_fuel(fuel);
    }
}
