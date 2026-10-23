// wasmi: a pure-Rust interpreter, so it runs everywhere, iOS included.

use super::engine::{Args, ImportCtx, Outcome, Raw, Setup, Shared, Wasm, read_from, unit_layout};
use super::host::dispatch;
use crate::{Compiled, Slot, words};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use wasmi::{
    AsContextMut, Caller, Engine, Extern, ExternType, Func, FuncType, Global, Instance, Linker,
    Memory, MemoryType, Mutability, Nullable, Ref, RefType, Store, Table, TableType, Val, ValType,
};

struct Data {
    mem: Option<wasmi::Memory>,
    host: Shared,
    limits: Option<wasmi::StoreLimits>,
    /// What the host function a stop parked in still owes the module: nothing
    /// for a probe of its own, one i64 for the entry point a direct module's
    /// probe rides in.
    owed: Vec<ValType>,
    /// A script built as units: them all, its own first.
    links: Option<Rc<RefCell<Links>>>,
}

/// The units of one script, in one store: its memory, its `fn` table and its
/// allocator's globals shared, each unit made on its first call.
struct Links {
    units: Vec<LinkUnit>,
    by_key: FxHashMap<String, usize>,
    memory: Memory,
    table: Table,
    globals: Vec<Global>,
    fuel: bool,
    /// The link slots waiting for a unit not made yet, by that unit: each
    /// slot and the export it calls, filled the moment the unit is made.
    waiting: FxHashMap<usize, Vec<(u64, String)>>,
    /// The stand-in for each function of a unit not made yet, which every
    /// slot calling it holds until then.
    stand_ins: FxHashMap<(usize, String), Func>,
}

struct LinkUnit {
    /// `None` for the script's own, made with the store.
    compiled: Option<Arc<Compiled>>,
    /// Where its static data goes.
    base: u32,
    inst: Option<Instance>,
}

struct Ctx<'a, 'b>(&'a mut Caller<'b, Data>);

impl Ctx<'_, '_> {
    // An export of the unit making the call, else of the script's own unit:
    // a call back into the script from a module's engine call.
    fn func(&mut self, name: &str) -> Option<Func> {
        if let Some(f) = self.0.get_export(name).and_then(Extern::into_func) {
            return Some(f);
        }
        let root = self.0.data().links.as_ref()?.borrow().units[0].inst?;
        root.get_func(&*self.0, name)
    }
}

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
    fn has_func(&mut self, name: &str) -> bool {
        self.func(name).is_some()
    }
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
            .func(name)
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

// A direct module's entry points, each standing for every import that takes
// as many slots.
fn link_direct(linker: &mut Linker<Data>, meta: &crate::Meta) -> Result<(), String> {
    use super::host::{direct_arities, direct_call, direct_result};
    for n in direct_arities(meta) {
        let mut params = vec![ValType::I32, ValType::I32];
        params.extend(vec![ValType::I64; n]);
        let ty = FuncType::new(params, [ValType::I64]);
        let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
            let host = c.data().host.clone();
            let index = params[1].i32().unwrap_or(0) as usize;
            let bits: smallvec::SmallVec<[u64; 8]> = params[2..]
                .iter()
                .map(|v| v.i64().unwrap_or(0).cast_unsigned())
                .collect();
            let first =
                direct_call(&host, &mut Ctx(&mut c), index, &bits).map_err(wasmi::Error::new)?;
            if host.borrow().debugger.is_parked() {
                c.data_mut().owed = vec![ValType::I64];
                return Err(wasmi::Error::host(Park));
            }
            results[0] = Val::I64(first.cast_signed());
            Ok(())
        };
        let name = format!("{}{n}", words::DIRECT_CALL);
        linker
            .func_new(words::HOST_MODULE, &name, ty, call)
            .map_err(|e| e.to_string())?;
    }
    let ty = FuncType::new([ValType::I32, ValType::I32], [ValType::I64]);
    let result = move |c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
        let k = params[1].i32().unwrap_or(0) as usize;
        results[0] = Val::I64(direct_result(&c.data().host, k).cast_signed());
        Ok(())
    };
    linker
        .func_new(words::HOST_MODULE, words::DIRECT_RESULT, ty, result)
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn instantiate(setup: &Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    if setup.meta.unit.is_some() {
        return instantiate_linked(setup);
    }
    let mut config = wasmi::Config::default();
    config.consume_fuel(setup.fuel.is_some());
    let engine = Engine::new(&config);
    let module =
        wasmi::Module::new(&engine, setup.wasm).map_err(|e| format!("invalid module: {e}"))?;
    let mut linker = Linker::<Data>::new(&engine);
    if setup.meta.direct {
        link_direct(&mut linker, setup.meta)?;
    }
    // A direct module imports only its entry points.
    let one_each: &[crate::Import] = if setup.meta.direct {
        &[]
    } else {
        &setup.meta.imports
    };
    for (index, imp) in one_each.iter().enumerate() {
        let ty = FuncType::new(
            imp.params.iter().map(|s| vt(*s)),
            imp.results.iter().map(|s| vt(*s)),
        );
        let owed: Vec<ValType> = imp.results.iter().map(|s| vt(*s)).collect();
        let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
            let host = c.data().host.clone();
            let args: Args = params.iter().map(raw).collect();
            let out =
                dispatch(&host, &mut Ctx(&mut c), 0, index, &args).map_err(wasmi::Error::new)?;
            if host.borrow().debugger.is_parked() {
                c.data_mut().owed.clone_from(&owed);
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
    let mut store = store_for(&engine, setup);
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

fn store_for(eng: &Engine, setup: &Setup<'_>) -> Store<Data> {
    let limits = setup
        .memory_bytes
        .map(|m| wasmi::StoreLimitsBuilder::new().memory_size(m).build());
    let mut store = Store::new(
        eng,
        Data {
            mem: None,
            host: setup.host.clone(),
            limits,
            owed: Vec::new(),
            links: None,
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
    store
}

/// One engine per process for each fuel setting, so the units every script
/// shares are translated once.
fn shared_engine(fuel: bool) -> Engine {
    static METERED: OnceLock<Engine> = OnceLock::new();
    static FREE: OnceLock<Engine> = OnceLock::new();
    let cell = if fuel { &METERED } else { &FREE };
    cell.get_or_init(|| {
        let mut config = wasmi::Config::default();
        config.consume_fuel(fuel);
        Engine::new(&config)
    })
    .clone()
}

/// Units translated this run, by module hash and fuel.
#[allow(clippy::type_complexity, reason = "a cache keyed as its users ask")]
static UNITS: OnceLock<Mutex<FxHashMap<(String, bool), wasmi::Module>>> = OnceLock::new();

fn unit_module(eng: &Engine, wasm: &[u8], fuel: bool) -> Result<wasmi::Module, String> {
    let key = (crate::module_hash(wasm), fuel);
    let units = UNITS.get_or_init(Mutex::default);
    if let Some(m) = units.lock().map_err(|e| e.to_string())?.get(&key) {
        return Ok(m.clone());
    }
    let m = wasmi::Module::new(eng, wasm).map_err(|e| format!("invalid module: {e}"))?;
    units
        .lock()
        .map_err(|e| e.to_string())?
        .insert(key, m.clone());
    Ok(m)
}

// A script built as units: its memory, table and globals made here, its own
// unit instantiated now and each other one on its first call.
fn instantiate_linked(setup: &Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    let fuel = setup.fuel.is_some();
    let engine = shared_engine(fuel);
    let mut store = store_for(&engine, setup);
    let e = |e: wasmi::Error| e.to_string();
    let (bases, heap) = unit_layout(setup.meta, setup.linked);
    let memory = Memory::new(&mut store, MemoryType::new(heap.div_ceil(65536), None)).map_err(e)?;
    let table = Table::new(
        &mut store,
        TableType::new(RefType::Func, 1, None),
        Ref::Func(Nullable::Null),
    )
    .map_err(e)?;
    let mut globals = vec![];
    for (k, _) in words::UNIT_GLOBALS.iter().enumerate() {
        let start = if k == 0 { heap as i32 } else { 0 };
        globals.push(Global::new(&mut store, Val::I32(start), Mutability::Var));
    }
    let mut units = vec![LinkUnit {
        compiled: None,
        base: bases[0],
        inst: None,
    }];
    let mut by_key = FxHashMap::default();
    by_key.insert(setup.key.to_string(), 0);
    for ((key, compiled), base) in setup.linked.iter().zip(&bases[1..]) {
        by_key.insert(key.clone(), units.len());
        units.push(LinkUnit {
            compiled: Some(compiled.clone()),
            base: *base,
            inst: None,
        });
    }
    store.data_mut().mem = Some(memory);
    store.data_mut().links = Some(Rc::new(RefCell::new(Links {
        units,
        by_key,
        memory,
        table,
        globals,
        fuel,
        waiting: FxHashMap::default(),
        stand_ins: FxHashMap::default(),
    })));
    let unit = setup.meta.unit.as_ref().ok_or("not a unit")?;
    let inst = make_unit(&mut store, 0, setup.wasm, unit, 0, setup.key)?;
    Ok(Box::new(Wasmi {
        store,
        inst,
        parked: None,
    }))
}

// Unit `i` of the script, made now if this is its first call.
fn unit_instance(mut cx: impl AsContextMut<Data = Data>, i: usize) -> Result<Instance, String> {
    let links = cx
        .as_context()
        .data()
        .links
        .clone()
        .ok_or("the script is not built as units")?;
    let compiled = {
        let l = links.borrow();
        if let Some(inst) = l.units[i].inst {
            return Ok(inst);
        }
        l.units[i]
            .compiled
            .clone()
            .ok_or("the script's own unit is not made")?
    };
    let unit = compiled
        .meta
        .unit
        .as_ref()
        .ok_or_else(|| format!("{} is not built as a unit", compiled.key))?;
    let host = cx.as_context().data().host.clone();
    let numbered = host.borrow_mut().bind_unit(compiled.clone())?;
    make_unit(&mut cx, i, &compiled.wasm, unit, numbered, &compiled.key)
}

// Instantiates unit `i` from `wasm`, its engine calls dispatched as unit
// `numbered`, and fills its link slots.
fn make_unit(
    mut cx: impl AsContextMut<Data = Data>,
    i: usize,
    wasm: &[u8],
    unit: &crate::Unit,
    numbered: usize,
    key: &str,
) -> Result<Instance, String> {
    let links = cx
        .as_context()
        .data()
        .links
        .clone()
        .ok_or("the script is not built as units")?;
    let (table, fuel) = {
        let l = links.borrow();
        (l.table, l.fuel)
    };
    let engine = cx.as_context().engine().clone();
    let module = unit_module(&engine, wasm, fuel)?;
    let table_base = table.size(&cx) as u32;
    let bases = Bases {
        memory: links.borrow().units[i].base,
        table: table_base,
        link: table_base + unit.refs,
    };
    table
        .grow(
            &mut cx,
            u64::from(unit.refs) + unit.links.len() as u64,
            Ref::Func(Nullable::Null),
        )
        .map_err(|e| format!("{key}: {e}"))?;
    let imports = unit_imports(&mut cx, &links, &module, numbered, &bases, key)?;
    let inst = Instance::new(&mut cx, &module, &imports).map_err(|e| format!("{key}: {e}"))?;
    links.borrow_mut().units[i].inst = Some(inst);
    // Every slot that waited for this unit calls it now.
    let waited = links.borrow_mut().waiting.remove(&i).unwrap_or_default();
    for (slot, export) in waited {
        let f = inst
            .get_func(&cx, &export)
            .ok_or_else(|| format!("{key}: no `{}`", &export[1..]))?;
        table
            .set(&mut cx, slot, Ref::Func(Nullable::Val(f)))
            .map_err(|e| format!("{key}: {e}"))?;
    }
    links.borrow_mut().stand_ins.retain(|(t, _), _| *t != i);
    fill_links(&mut cx, &links, unit, bases.link, key)?;
    Ok(inst)
}

/// Where a unit's static data, own `fn` table slots and link slots start.
struct Bases {
    memory: u32,
    table: u32,
    link: u32,
}

// What a unit imports, in its module's order: its engine calls, dispatched as
// unit `numbered`, and the script's memory, table and globals.
fn unit_imports(
    mut cx: impl AsContextMut<Data = Data>,
    links: &Rc<RefCell<Links>>,
    module: &wasmi::Module,
    numbered: usize,
    bases: &Bases,
    key: &str,
) -> Result<Vec<Extern>, String> {
    let (memory, table, globals) = {
        let l = links.borrow();
        (l.memory, l.table, l.globals.clone())
    };
    let mut imports: Vec<Extern> = vec![];
    let mut index = 0usize;
    for imp in module.imports() {
        let ext: Extern = match imp.ty() {
            ExternType::Func(ty) => {
                let at = index;
                index += 1;
                let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
                    let host = c.data().host.clone();
                    let args: Args = params.iter().map(raw).collect();
                    let out = dispatch(&host, &mut Ctx(&mut c), numbered, at, &args)
                        .map_err(wasmi::Error::new)?;
                    for (slot, r) in results.iter_mut().zip(out) {
                        *slot = val(r);
                    }
                    Ok(())
                };
                Func::new(&mut cx, ty.clone(), call).into()
            }
            ExternType::Memory(_) => memory.into(),
            ExternType::Table(_) => table.into(),
            ExternType::Global(_) => {
                let at = match imp.name() {
                    words::MEMORY_BASE => Some(bases.memory),
                    words::TABLE_BASE => Some(bases.table),
                    words::LINK_BASE => Some(bases.link),
                    _ => None,
                };
                if let Some(v) = at {
                    Global::new(&mut cx, Val::I32(v as i32), Mutability::Const).into()
                } else {
                    let name = imp.name();
                    let at = words::UNIT_GLOBALS
                        .iter()
                        .position(|g| *g == name)
                        .ok_or_else(|| format!("{key}: imports an unknown global `{name}`"))?;
                    globals[at].into()
                }
            }
        };
        imports.push(ext);
    }
    Ok(imports)
}

// A unit's link slots, from `link_base`: each the function it calls when that
// one's unit is made, else a stand-in that makes it on the first call.
fn fill_links(
    mut cx: impl AsContextMut<Data = Data>,
    links: &Rc<RefCell<Links>>,
    unit: &crate::Unit,
    link_base: u32,
    key: &str,
) -> Result<(), String> {
    let table = links.borrow().table;
    for (j, link) in unit.links.iter().enumerate() {
        let slot = u64::from(link_base) + j as u64;
        let (target, ready) = {
            let l = links.borrow();
            let t = *l
                .by_key
                .get(&link.file)
                .ok_or_else(|| format!("{key}: no unit for {}", link.file))?;
            (t, l.units[t].inst)
        };
        let func = if let Some(t) = ready {
            t.get_func(&cx, &link.export)
                .ok_or_else(|| format!("{}: no `{}` for {key}", link.file, &link.export[1..]))?
        } else {
            links
                .borrow_mut()
                .waiting
                .entry(target)
                .or_default()
                .push((slot, link.export.clone()));
            let at = (target, link.export.clone());
            let kept = links.borrow().stand_ins.get(&at).copied();
            if let Some(f) = kept {
                f
            } else {
                let f = stand_in(&mut cx, link, target);
                links.borrow_mut().stand_ins.insert(at, f);
                f
            }
        };
        table
            .set(&mut cx, slot, Ref::Func(Nullable::Val(func)))
            .map_err(|e| format!("{key}: {e}"))?;
    }
    Ok(())
}

// What a link slot holds until the unit it calls is made: on the first call
// it makes that unit, which fills every slot waiting for it, and calls it.
fn stand_in(cx: impl AsContextMut<Data = Data>, link: &crate::Link, target: usize) -> Func {
    let ty = FuncType::new(
        link.params.iter().map(|s| vt(*s)),
        link.results.iter().map(|s| vt(*s)),
    );
    let (export, file) = (link.export.clone(), link.file.clone());
    let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
        let inst = unit_instance(&mut c, target).map_err(wasmi::Error::new)?;
        let f = inst
            .get_func(&c, &export)
            .ok_or_else(|| wasmi::Error::new(format!("{file}: no `{}`", &export[1..])))?;
        f.call(&mut c, params, results)
    };
    Func::new(cx, ty, call)
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
        // The probe answered nothing; what the function it trapped in owes is
        // handed back as zeros, which the module never reads.
        let owed: Vec<Val> = std::mem::take(&mut self.store.data_mut().owed)
            .into_iter()
            .map(Val::default_for_ty)
            .collect();
        let call = trap
            .resume(&mut self.store, &owed, &mut out)
            .map_err(|e| e.to_string())?;
        self.settle(call, &out)
    }
}
