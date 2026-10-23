// wasmtime: Cranelift compiles the module to machine code, near Rust's speed.
// Desktop and Android; iOS forbids the JIT and the web has its own engine.

use super::engine::{Args, ImportCtx, Raw, Setup, Shared, Wasm, read_from, unit_layout};
use super::host::dispatch;
use crate::{Compiled, Slot, words};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use wasmtime::{
    AsContextMut, Caller, Engine, Extern, ExternType, Func, FuncType, Global, GlobalType, Instance,
    Linker, Memory, MemoryType, Mutability, Ref, RefType, Store, Table, TableType, Val, ValRaw,
    ValType,
};

struct Data {
    mem: Option<wasmtime::Memory>,
    host: Shared,
    limits: Option<wasmtime::StoreLimits>,
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
        root.get_func(&mut *self.0, name)
    }
}

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
            .func(name)
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

fn val_raw(r: Raw) -> ValRaw {
    match r {
        Raw::I32(x) => ValRaw::i32(x),
        Raw::I64(x) => ValRaw::i64(x),
        Raw::F64(x) => ValRaw::f64(x.to_bits()),
    }
}

fn slot_of(r: Raw) -> Slot {
    match r {
        Raw::I32(_) => Slot::I32,
        Raw::I64(_) => Slot::I64,
        Raw::F64(_) => Slot::F64,
    }
}

fn val(r: Raw) -> Val {
    match r {
        Raw::I32(x) => Val::I32(x),
        Raw::I64(x) => Val::I64(x),
        Raw::F64(x) => Val::F64(x.to_bits()),
    }
}

/// One engine per process for each fuel setting: making one is most of
/// what loading a script costs, and modules from one engine share its code.
fn engine(fuel: bool) -> Result<Engine, String> {
    static METERED: OnceLock<Result<Engine, String>> = OnceLock::new();
    static FREE: OnceLock<Result<Engine, String>> = OnceLock::new();
    let cell = if fuel { &METERED } else { &FREE };
    cell.get_or_init(|| {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(fuel);
        // NaN bits are the one result wasm leaves to the hardware.
        config.cranelift_nan_canonicalization(true);
        Engine::new(&config).map_err(|e| e.to_string())
    })
    .clone()
}

static CACHE: OnceLock<PathBuf> = OnceLock::new();

/// Units compiled this run, by module hash and fuel: every script using one
/// shares its machine code.
#[allow(clippy::type_complexity, reason = "a cache keyed as its users ask")]
static UNITS: OnceLock<Mutex<FxHashMap<(String, bool), wasmtime::Module>>> = OnceLock::new();

fn unit_module(eng: &Engine, wasm: &[u8], fuel: bool) -> Result<wasmtime::Module, String> {
    let key = (crate::module_hash(wasm), fuel);
    let units = UNITS.get_or_init(Mutex::default);
    if let Some(m) = units.lock().map_err(|e| e.to_string())?.get(&key) {
        return Ok(m.clone());
    }
    let m = module(eng, wasm, fuel)?;
    units
        .lock()
        .map_err(|e| e.to_string())?
        .insert(key, m.clone());
    Ok(m)
}

pub(crate) fn cache_in(dir: PathBuf) {
    let _ = CACHE.set(dir);
}

/// `wasm` compiled, from the cache when it has it: compiling is most of
/// what loading a script costs on wasmtime.
fn module(eng: &Engine, wasm: &[u8], fuel: bool) -> Result<wasmtime::Module, String> {
    let compile = || wasmtime::Module::new(eng, wasm).map_err(|e| format!("invalid module: {e:#}"));
    let Some(dir) = CACHE.get() else {
        return compile();
    };
    let name = format!(
        "{}{}.cwasm",
        crate::module_hash(wasm),
        if fuel { "f" } else { "" }
    );
    let path = dir.join(name);
    if let Ok(file) = std::fs::read(&path)
        && let Some((sum, body)) = file.split_first_chunk::<8>()
        && u64::from_le_bytes(*sum) == crate::fnv(body, 0x8422_2325_cbf2_9ce4)
    {
        // SAFETY: `body` is what `serialize` wrote for these exact wasm bytes,
        // its checksum intact; wasmtime refuses one from another version or
        // configuration, and a refusal compiles afresh.
        if let Ok(m) = unsafe { wasmtime::Module::deserialize(eng, body) } {
            return Ok(m);
        }
    }
    let m = compile()?;
    if let Ok(body) = m.serialize() {
        let mut file = crate::fnv(&body, 0x8422_2325_cbf2_9ce4)
            .to_le_bytes()
            .to_vec();
        file.extend(body);
        let partial = path.with_extension("partial");
        let _ = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&partial, file))
            .and_then(|()| std::fs::rename(&partial, &path));
    }
    Ok(m)
}

// A direct module's entry points, each standing for every import that takes
// as many slots.
fn link_direct(eng: &Engine, linker: &mut Linker<Data>, meta: &crate::Meta) -> Result<(), String> {
    use super::host::{direct_arities, direct_call, direct_result};
    for n in direct_arities(meta) {
        let mut params = vec![ValType::I32, ValType::I32];
        params.extend(vec![ValType::I64; n]);
        let ty = FuncType::new(eng, params, [ValType::I64]);
        let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
            let host = c.data().host.clone();
            let index = params[1].i32().unwrap_or(0) as usize;
            let bits: smallvec::SmallVec<[u64; 8]> = params[2..]
                .iter()
                .map(|v| v.i64().unwrap_or(0).cast_unsigned())
                .collect();
            let first =
                direct_call(&host, &mut Ctx(&mut c), index, &bits).map_err(wasmtime::Error::msg)?;
            results[0] = Val::I64(first.cast_signed());
            Ok(())
        };
        let name = format!("{}{n}", words::DIRECT_CALL);
        linker
            .func_new(words::HOST_MODULE, &name, ty, call)
            .map_err(|e| e.to_string())?;
    }
    let ty = FuncType::new(eng, [ValType::I32, ValType::I32], [ValType::I64]);
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
    let engine = engine(setup.fuel.is_some())?;
    let module = module(&engine, setup.wasm, setup.fuel.is_some())?;
    let mut linker = Linker::<Data>::new(&engine);
    if setup.meta.direct {
        link_direct(&engine, &mut linker, setup.meta)?;
    }
    // A direct module imports only its entry points.
    let one_each: &[crate::Import] = if setup.meta.direct {
        &[]
    } else {
        &setup.meta.imports
    };
    for (index, imp) in one_each.iter().enumerate() {
        let ty = FuncType::new(
            &engine,
            imp.params.iter().map(|s| vt(*s)),
            imp.results.iter().map(|s| vt(*s)),
        );
        let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
            let host = c.data().host.clone();
            let args: Args = params.iter().map(raw).collect();
            let out =
                dispatch(&host, &mut Ctx(&mut c), 0, index, &args).map_err(wasmtime::Error::msg)?;
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
        .instantiate(&mut store, &module)
        .map_err(|e| e.to_string())?;
    store.data_mut().mem = inst.get_memory(&mut store, words::MEMORY);
    Ok(Box::new(Wasmtime {
        store,
        inst,
        funcs: FxHashMap::default(),
        slots: Vec::new(),
    }))
}

fn store_for(engine: &Engine, setup: &Setup<'_>) -> Store<Data> {
    let limits = setup
        .memory_bytes
        .map(|m| wasmtime::StoreLimitsBuilder::new().memory_size(m).build());
    let mut store = Store::new(
        engine,
        Data {
            mem: None,
            host: setup.host.clone(),
            limits,
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

// A script built as units: its memory, table and globals made here, its own
// unit instantiated now and each other one on its first call.
fn instantiate_linked(setup: &Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    let fuel = setup.fuel.is_some();
    let engine = engine(fuel)?;
    let mut store = store_for(&engine, setup);
    let e = |e: wasmtime::Error| format!("{e:#}");
    let (bases, heap) = unit_layout(setup.meta, setup.linked);
    let memory = Memory::new(&mut store, MemoryType::new(heap.div_ceil(65536), None)).map_err(e)?;
    let table = Table::new(
        &mut store,
        TableType::new(RefType::FUNCREF, 1, None),
        Ref::Func(None),
    )
    .map_err(e)?;
    let mut globals = vec![];
    for (k, _) in words::UNIT_GLOBALS.iter().enumerate() {
        let start = if k == 0 { heap as i32 } else { 0 };
        let ty = GlobalType::new(ValType::I32, Mutability::Var);
        globals.push(Global::new(&mut store, ty, Val::I32(start)).map_err(e)?);
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
    Ok(Box::new(Wasmtime {
        store,
        inst,
        funcs: FxHashMap::default(),
        slots: Vec::new(),
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
    let e = |e: wasmtime::Error| format!("{key}: {e:#}");
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
            Ref::Func(None),
        )
        .map_err(e)?;
    let imports = unit_imports(&mut cx, &links, &module, numbered, &bases, key)?;
    let inst = Instance::new(&mut cx, &module, &imports).map_err(e)?;
    links.borrow_mut().units[i].inst = Some(inst);
    // Every slot that waited for this unit calls it now.
    let waited = links.borrow_mut().waiting.remove(&i).unwrap_or_default();
    for (slot, export) in waited {
        let f = inst
            .get_func(&mut cx, &export)
            .ok_or_else(|| format!("{key}: no `{}`", &export[1..]))?;
        table.set(&mut cx, slot, Ref::Func(Some(f))).map_err(e)?;
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
    module: &wasmtime::Module,
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
                        .map_err(wasmtime::Error::msg)?;
                    for (slot, r) in results.iter_mut().zip(out) {
                        *slot = val(r);
                    }
                    Ok(())
                };
                Func::new(&mut cx, ty, call).into()
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
                    let ty = GlobalType::new(ValType::I32, Mutability::Const);
                    Global::new(&mut cx, ty, Val::I32(v as i32))
                        .map_err(|e| format!("{key}: {e:#}"))?
                        .into()
                } else {
                    let name = imp.name();
                    let at = words::UNIT_GLOBALS
                        .iter()
                        .position(|g| *g == name)
                        .ok_or_else(|| format!("{key}: imports an unknown global `{name}`"))?;
                    globals[at].into()
                }
            }
            ExternType::Tag(_) => return Err(format!("{key}: imports a tag")),
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
    let engine = cx.as_context().engine().clone();
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
            t.get_func(&mut cx, &link.export)
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
                let f = stand_in(&mut cx, &engine, link, target);
                links.borrow_mut().stand_ins.insert(at, f);
                f
            }
        };
        table
            .set(&mut cx, slot, Ref::Func(Some(func)))
            .map_err(|e| format!("{key}: {e:#}"))?;
    }
    Ok(())
}

// What a link slot holds until the unit it calls is made: on the first call
// it makes that unit, which fills every slot waiting for it, and calls it.
fn stand_in(
    cx: impl AsContextMut<Data = Data>,
    engine: &Engine,
    link: &crate::Link,
    target: usize,
) -> Func {
    let ty = FuncType::new(
        engine,
        link.params.iter().map(|s| vt(*s)),
        link.results.iter().map(|s| vt(*s)),
    );
    let (export, file) = (link.export.clone(), link.file.clone());
    let call = move |mut c: Caller<'_, Data>, params: &[Val], results: &mut [Val]| {
        let inst = unit_instance(&mut c, target).map_err(wasmtime::Error::msg)?;
        let f = inst
            .get_func(&mut c, &export)
            .ok_or_else(|| wasmtime::Error::msg(format!("{file}: no `{}`", &export[1..])))?;
        f.call(&mut c, params, results)
    };
    Func::new(cx, ty, call)
}

struct Wasmtime {
    store: Store<Data>,
    inst: wasmtime::Instance,
    /// Each export called so far, with its parameter and result kinds: the
    /// host calls `update` on every instance every frame, and looking the
    /// export up and checking its type each time was most of a call.
    funcs: FxHashMap<String, Export>,
    /// The parameters going in and the results coming out, kept between
    /// calls so a call allocates nothing.
    slots: Vec<ValRaw>,
}

struct Export {
    func: wasmtime::Func,
    params: Box<[Slot]>,
    results: Box<[Slot]>,
}

impl Wasmtime {
    fn export(&mut self, name: &str) -> Result<&Export, String> {
        if !self.funcs.contains_key(name) {
            let func = self
                .inst
                .get_func(&mut self.store, name)
                .ok_or_else(|| format!("no export `{name}`"))?;
            let ty = func.ty(&self.store);
            let kinds = |v: &mut dyn Iterator<Item = ValType>| -> Result<Box<[Slot]>, String> {
                v.map(|t| match t {
                    ValType::I32 => Ok(Slot::I32),
                    ValType::I64 => Ok(Slot::I64),
                    ValType::F64 => Ok(Slot::F64),
                    other => Err(format!("`{name}` takes or gives a {other}")),
                })
                .collect()
            };
            let export = Export {
                func,
                params: kinds(&mut ty.params())?,
                results: kinds(&mut ty.results())?,
            };
            self.funcs.insert(name.to_string(), export);
        }
        Ok(&self.funcs[name])
    }
}

impl Wasm for Wasmtime {
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        let export = self.export(name)?;
        let fits = args.len() == export.params.len()
            && results == export.results.len()
            && args
                .iter()
                .zip(&export.params)
                .all(|(a, p)| slot_of(*a) == *p);
        if !fits {
            return Err(format!("`{name}` was called with the wrong arguments"));
        }
        let (func, kinds) = (export.func, export.results.clone());
        self.slots.clear();
        self.slots.extend(args.iter().map(|r| val_raw(*r)));
        self.slots.resize(args.len().max(results), ValRaw::i32(0));
        // SAFETY: the arguments were just checked against the export's own
        // parameter kinds and count, the buffer holds the larger of its
        // parameters and results, and a module of ours passes no references.
        unsafe { func.call_unchecked(&mut self.store, &raw mut self.slots[..]) }
            .map_err(|e| format!("{:#}", e.root_cause()))?;
        Ok(self
            .slots
            .iter()
            .zip(kinds.iter())
            .map(|(v, k)| match k {
                Slot::I32 => Raw::I32(v.get_i32()),
                Slot::I64 => Raw::I64(v.get_i64()),
                Slot::F64 => Raw::F64(f64::from_bits(v.get_f64())),
            })
            .collect())
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
