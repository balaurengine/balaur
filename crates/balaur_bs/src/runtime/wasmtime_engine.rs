// wasmtime: Cranelift compiles the module to machine code, near Rust's speed.
// Desktop and Android; iOS forbids the JIT and the web has its own engine.

use super::engine::{Args, ImportCtx, Raw, Setup, Shared, Wasm, read_from};
use super::host::dispatch;
use crate::{Slot, words};
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use wasmtime::{Caller, Engine, Extern, FuncType, Linker, Store, Val, ValRaw, ValType};

struct Data {
    mem: Option<wasmtime::Memory>,
    host: Shared,
    limits: Option<wasmtime::StoreLimits>,
}

struct Ctx<'a, 'b>(&'a mut Caller<'b, Data>);

impl ImportCtx for Ctx<'_, '_> {
    fn has_func(&mut self, name: &str) -> bool {
        self.0
            .get_export(name)
            .and_then(Extern::into_func)
            .is_some()
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

pub(crate) fn cache_in(dir: PathBuf) {
    let _ = CACHE.set(dir);
}

/// `wasm` compiled, from the cache when it has it: compiling is most of
/// what loading a script costs on wasmtime.
fn module(eng: &Engine, wasm: &[u8], fuel: bool) -> Result<wasmtime::Module, String> {
    let compile = || wasmtime::Module::new(eng, wasm).map_err(|e| format!("invalid module: {e}"));
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

pub(crate) fn instantiate(setup: Setup<'_>) -> Result<Box<dyn Wasm>, String> {
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
    Ok(Box::new(Wasmtime {
        store,
        inst,
        funcs: FxHashMap::default(),
        slots: Vec::new(),
    }))
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
