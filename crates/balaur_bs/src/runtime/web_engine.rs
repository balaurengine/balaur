// The browser's own engine: the page compiles the module natively, so a web
// build runs scripts at the speed wasmtime gives a desktop one.

use super::engine::{
    Args, Bases, ImportCtx, Raw, Setup, Shared, Wasm, own_global, shared_global, shared_starts,
    unit_layout,
};
use super::host::{direct_arities, direct_call, direct_result, dispatch};
use crate::{Compiled, Slot, words};
use js_sys::{Array, BigInt, Function, Object, Reflect, Uint8Array, WebAssembly};
use rustc_hash::FxHashMap;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

struct Live {
    /// The module's, or a script built as units' own unit's.
    exports: Object,
    memory: WebAssembly::Memory,
    /// A script built as units: them all.
    links: Option<Rc<Links>>,
}

/// The units of one script: its memory, its `fn` table and its allocator's
/// globals shared. A slot of a table takes only a module's own export, so no
/// stand-in can hold one until its unit is made: what is linked is made
/// together, before the next call, and only then are the slots filled.
struct Links {
    host: Shared,
    live: Rc<RefCell<Option<Live>>>,
    errors: Rc<RefCell<Option<String>>>,
    memory: WebAssembly::Memory,
    table: WebAssembly::Table,
    globals: Vec<WebAssembly::Global>,
    units: RefCell<Vec<LinkUnit>>,
    by_key: RefCell<FxHashMap<String, usize>>,
    /// A unit was linked or made whose link slots are not filled yet.
    pending: Cell<bool>,
    /// What the page calls for the imports of a unit that is not direct.
    closures: RefCell<Vec<Import>>,
}

struct LinkUnit {
    /// `None` for the script's own, made first.
    compiled: Option<Arc<Compiled>>,
    /// Where its static data goes.
    base: u32,
    /// Its first link slot.
    link_base: u32,
    links: Vec<crate::Link>,
    exports: Option<Object>,
    /// Its id for the engine's entry points, as a direct unit.
    direct: Option<usize>,
}

thread_local! {
    /// Units compiled this run, by module hash: every instance using one
    /// shares it, and a rebuild of the project compiles only what changed.
    static MODULES: RefCell<FxHashMap<String, WebAssembly::Module>> =
        RefCell::new(FxHashMap::default());
}

fn unit_module(wasm: &[u8]) -> Result<WebAssembly::Module, String> {
    let hash = crate::module_hash(wasm);
    if let Some(module) = MODULES.with(|modules| modules.borrow().get(&hash).cloned()) {
        return Ok(module);
    }
    let module =
        WebAssembly::Module::new(&Uint8Array::from(wasm).into()).map_err(|error| js_err(&error))?;
    MODULES.with(|modules| modules.borrow_mut().insert(hash, module.clone()));
    Ok(module)
}

fn set(on: &Object, name: &str, value: &JsValue) -> Result<(), String> {
    Reflect::set(on, &name.into(), value)
        .map(|_| ())
        .map_err(|error| js_err(&error))
}

fn mutable_global(value: i32) -> Result<WebAssembly::Global, String> {
    let descriptor = Object::new();
    set(&descriptor, "value", &"i32".into())?;
    set(&descriptor, "mutable", &true.into())?;
    WebAssembly::Global::new(&descriptor, &value.into()).map_err(|error| js_err(&error))
}

impl Links {
    /// `unit` as unit `key`, added unless it is one: its index. Its static
    /// data takes a block of the heap, since the space before the heap was
    /// laid out for the units known when the script was made.
    fn link(&self, key: &str, unit: Arc<Compiled>) -> Result<usize, String> {
        if let Some(index) = self.by_key.borrow().get(key) {
            return Ok(*index);
        }
        let len = unit.meta.unit.as_ref().map_or(0, |own| own.data_len);
        let base = if len == 0 { 0 } else { self.block(len)? };
        let mut units = self.units.borrow_mut();
        let index = units.len();
        self.by_key.borrow_mut().insert(key.to_string(), index);
        units.push(LinkUnit {
            compiled: Some(unit),
            base,
            link_base: 0,
            links: vec![],
            exports: None,
            direct: None,
        });
        self.pending.set(true);
        Ok(index)
    }

    // A block held as long as the instance, so no object a leak count watches.
    fn block(&self, len: u32) -> Result<u32, String> {
        let guard = self.live.borrow();
        let live = guard.as_ref().ok_or("the script's own unit is not made")?;
        let size = Raw::I32((len + 15) as i32);
        let alloc = Reflect::get(&live.exports, &words::ALLOC.into())
            .map_err(|error| js_err(&error))?
            .dyn_into::<Function>()
            .map_err(|_| format!("no export `{}`", words::ALLOC))?;
        let block = apply(&alloc, &[size], 1)?;
        if let Some(count) = live.global(0, words::LIVE) {
            live.set_global(0, words::LIVE, count - 1);
        }
        Ok((block[0].i32() as u32).next_multiple_of(16))
    }

    // Every unit linked and not made, then every link slot not filled.
    fn make_pending(self: &Rc<Self>) -> Result<(), String> {
        if !self.pending.replace(false) {
            return Ok(());
        }
        let count = self.units.borrow().len();
        for at in 0..count {
            let wanted = {
                let units = self.units.borrow();
                units[at]
                    .compiled
                    .clone()
                    .filter(|_| units[at].exports.is_none())
            };
            if let Some(compiled) = wanted {
                self.host.borrow_mut().bind_unit(at, compiled.clone())?;
                self.make(at, &compiled.wasm, &compiled.meta)?;
            }
        }
        for at in 0..count {
            let unfilled = !self.units.borrow()[at].links.is_empty();
            if unfilled {
                self.fill_links(at)?;
            }
        }
        Ok(())
    }

    // Instantiates unit `at` from `wasm`: its slots reserved, not yet filled.
    fn make(self: &Rc<Self>, at: usize, wasm: &[u8], meta: &crate::Meta) -> Result<(), String> {
        let unit = meta.unit.as_ref().ok_or("not a unit")?;
        let module = unit_module(wasm)?;
        let table_base = self.table.length();
        self.table
            .grow(unit.refs + unit.links.len() as u32)
            .map_err(|error| js_err(&error))?;
        let direct = meta.direct.then(|| {
            register(Direct {
                host: self.host.clone(),
                live: self.live.clone(),
                unit: at,
                errors: self.errors.clone(),
            })
        });
        let bases = Bases {
            index: at as u32,
            memory: self.units.borrow()[at].base,
            table: table_base,
            link: table_base + unit.refs,
            host_ctx: direct.unwrap_or(0) as u32,
        };
        let env = Object::new();
        set(&env, words::MEMORY, &self.memory)?;
        set(&env, words::TABLE, &self.table)?;
        for (name, global) in words::UNIT_GLOBALS
            .iter()
            .chain(&words::COUNT_GLOBALS)
            .zip(&self.globals)
        {
            set(&env, name, global)?;
        }
        for name in [
            words::MEMORY_BASE,
            words::TABLE_BASE,
            words::LINK_BASE,
            words::PROF,
            words::DBG_UNIT,
            words::HOST_CTX,
        ] {
            let value = own_global(name, &bases, unit).unwrap_or(0);
            set(&env, name, &JsValue::from(value as i32))?;
        }
        // Imported mutable, as every unit imports the shared ones.
        let breaks = own_global(words::DBG_BREAKS, &bases, unit).unwrap_or(0);
        let breaks = mutable_global(breaks as i32)?;
        set(&env, words::DBG_BREAKS, &breaks)?;
        if meta.direct {
            link_direct(&env, meta)?;
        } else {
            let closures = imports_each(&env, &self.host, &self.live, &meta.imports, at)?;
            self.closures.borrow_mut().extend(closures);
        }
        let imports = Object::new();
        set(&imports, words::HOST_MODULE, &env)?;
        let inst = WebAssembly::Instance::new(&module, &imports).map_err(|error| js_err(&error))?;
        let mut units = self.units.borrow_mut();
        let made = &mut units[at];
        made.link_base = bases.link;
        made.links.clone_from(&unit.links);
        made.exports = Some(inst.exports());
        made.direct = direct;
        Ok(())
    }

    fn fill_links(&self, at: usize) -> Result<(), String> {
        let (link_base, links) = {
            let mut units = self.units.borrow_mut();
            (units[at].link_base, std::mem::take(&mut units[at].links))
        };
        for (at_slot, link) in links.iter().enumerate() {
            let target = *self
                .by_key
                .borrow()
                .get(&link.file)
                .ok_or_else(|| format!("no unit for {}", link.file))?;
            let exports = self.units.borrow()[target]
                .exports
                .clone()
                .ok_or_else(|| format!("{} is not made", link.file))?;
            let func = Reflect::get(&exports, &link.export.as_str().into())
                .map_err(|error| js_err(&error))?
                .dyn_into::<Function>()
                .map_err(|_| format!("{}: no `{}`", link.file, &link.export[1..]))?;
            self.table
                .set(link_base + at_slot as u32, &func)
                .map_err(|error| js_err(&error))?;
        }
        Ok(())
    }
}

impl Live {
    fn bytes(&self) -> Uint8Array {
        Uint8Array::new(&self.memory.buffer())
    }
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        let buf = self.bytes();
        if at + len > buf.length() as usize {
            return vec![0; len];
        }
        buf.subarray(at as u32, (at + len) as u32).to_vec()
    }
    fn write(&self, at: usize, bytes: &[u8]) {
        self.bytes().set(&Uint8Array::from(bytes), at as u32);
    }
    // A shared global, else an export of unit `unit` or of the script's own.
    fn global_object(&self, unit: usize, name: &str) -> Option<JsValue> {
        if let Some(links) = &self.links {
            if let Some(at) = shared_global(name) {
                return Some(links.globals[at].clone().into());
            }
            if let Some(exports) = &links.units.borrow().get(unit)?.exports {
                let found = Reflect::get(exports, &name.into()).ok()?;
                if !found.is_undefined() {
                    return Some(found);
                }
            }
        }
        Reflect::get(&self.exports, &name.into()).ok()
    }
    fn global(&self, unit: usize, name: &str) -> Option<i32> {
        value_of(&self.global_object(unit, name)?)
    }
    // Global `name` of unit `unit`'s own, once it is made.
    fn own_global(&self, unit: usize, name: &str) -> Option<i32> {
        let exports = match &self.links {
            Some(links) => links.units.borrow().get(unit)?.exports.clone()?,
            None => self.exports.clone(),
        };
        value_of(&Reflect::get(&exports, &name.into()).ok()?)
    }
    fn set_global(&self, unit: usize, name: &str, value: i32) {
        if let Some(g) = self.global_object(unit, name) {
            let _ = Reflect::set(&g, &"value".into(), &JsValue::from(value));
        }
    }
    // An export of unit `unit`, once what is linked is made.
    fn export(&self, unit: usize, name: &str) -> Result<Function, String> {
        let exports = match &self.links {
            Some(links) => {
                links.make_pending()?;
                links
                    .units
                    .borrow()
                    .get(unit)
                    .and_then(|made| made.exports.clone())
                    .ok_or_else(|| format!("no unit {unit}"))?
            }
            None => self.exports.clone(),
        };
        let found = Reflect::get(&exports, &name.into()).map_err(|error| js_err(&error))?;
        found.dyn_into().map_err(|_| format!("no export `{name}`"))
    }
}

// What a `WebAssembly.Global` holds; `None` for what is no such thing.
fn value_of(global: &JsValue) -> Option<i32> {
    if !global.is_object() {
        return None;
    }
    Reflect::get(global, &"value".into())
        .ok()?
        .as_f64()
        .map(|value| value as i32)
}

// Re-entrant from an import: the page's engine lets a host function call back in.
fn apply(f: &Function, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
    let a: Array = args.iter().map(|r| to_js(*r)).collect();
    let out = f
        .apply(&JsValue::NULL, &a)
        .map_err(|error| js_err(&error))?;
    Ok(match results {
        0 => vec![],
        1 => vec![from_js(&out, None)],
        _ => Array::from(&out)
            .iter()
            .map(|v| from_js(&v, None))
            .collect(),
    })
}

/// The module as a host function sees it, called from unit `unit`.
struct Ctx<'a> {
    live: &'a Live,
    unit: usize,
}

impl ImportCtx for Ctx<'_> {
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        self.live.read(at, len)
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        self.live.write(at, bytes);
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        self.live.global(self.unit, name)
    }
    fn set_global(&mut self, name: &str, value: i32) {
        self.live.set_global(self.unit, name, value);
    }
    fn global_in(&mut self, unit: usize, name: &str) -> Option<i32> {
        self.live.own_global(unit, name)
    }
    fn make(&mut self, _unit: usize) -> Result<(), String> {
        match &self.live.links {
            Some(links) => links.make_pending(),
            None => Ok(()),
        }
    }
    // An export of the unit making the call, else of the script's own.
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        let func = self
            .live
            .export(self.unit, name)
            .or_else(|_| self.live.export(0, name))?;
        apply(&func, args, results)
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.live.export(self.unit, name).is_ok() || self.live.export(0, name).is_ok()
    }
    fn call_in(
        &mut self,
        unit: usize,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Vec<Raw>, String> {
        if unit == 0 {
            return self.call(name, args, results);
        }
        apply(&self.live.export(unit, name)?, args, results)
    }
    fn has_func_in(&mut self, unit: usize, name: &str) -> bool {
        if unit == 0 {
            return self.has_func(name);
        }
        self.live.export(unit, name).is_ok()
    }
    fn link(&mut self, key: &str, unit: Arc<Compiled>) -> Result<usize, String> {
        match &self.live.links {
            Some(links) => links.link(key, unit),
            None => Err(format!("{key}: this script is not built as units")),
        }
    }
}

fn to_js(r: Raw) -> JsValue {
    match r {
        Raw::I32(x) => JsValue::from(x),
        Raw::I64(x) => BigInt::from(x).into(),
        Raw::F64(x) => JsValue::from(x),
    }
}

// JavaScript hands an i64 over as a BigInt and everything else as a number.
fn from_js(v: &JsValue, slot: Option<Slot>) -> Raw {
    if v.is_bigint() || slot == Some(Slot::I64) {
        return Raw::I64(i64::try_from(BigInt::from(v.clone())).unwrap_or(0));
    }
    match slot {
        Some(Slot::I32) => Raw::I32(v.as_f64().unwrap_or(0.0) as i32),
        _ => Raw::F64(super::values::float(v.as_f64().unwrap_or(0.0).to_bits())),
    }
}

fn js_err(e: &JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

type Import = Closure<dyn FnMut(Array) -> Result<JsValue, JsValue>>;

// One closure per import, for a module whose imports do not all fit the
// engine's entry points.
fn imports_each(
    env: &Object,
    host: &Shared,
    live: &Rc<RefCell<Option<Live>>>,
    one_each: &[crate::Import],
    unit: usize,
) -> Result<Vec<Import>, String> {
    let mut keep = vec![];
    // A host function is variadic from JavaScript's side; this hands the Rust
    // closure its arguments as one array. Built only here: a page whose
    // content policy refuses `unsafe-eval` runs direct modules alone.
    let shim = (!one_each.is_empty()).then(|| {
        Function::new_with_args(
            "f",
            "return function(){ return f(Array.prototype.slice.call(arguments)); }",
        )
    });
    for (index, imp) in one_each.iter().enumerate() {
        let (host, live, params) = (host.clone(), live.clone(), imp.params.clone());
        let f: Import = Closure::new(move |a: Array| {
            let args: Args = params
                .iter()
                .enumerate()
                .map(|(i, s)| from_js(&a.get(i as u32), Some(*s)))
                .collect();
            let guard = live.borrow();
            let l = guard
                .as_ref()
                .ok_or_else(|| JsValue::from_str("the module is not ready"))?;
            let out = dispatch(&host, &mut Ctx { live: l, unit }, unit, index, &args)
                .map_err(|error| JsValue::from_str(&error))?;
            Ok(match out.len() {
                0 => JsValue::UNDEFINED,
                1 => to_js(out[0]),
                _ => out.into_iter().map(to_js).collect::<Array>().into(),
            })
        });
        let wrapped = shim
            .as_ref()
            .ok_or_else(|| "no shim for an import".to_string())?
            .call1(&JsValue::NULL, f.as_ref())
            .map_err(|error| js_err(&error))?;
        Reflect::set(env, &imp.name.as_str().into(), &wrapped).map_err(|error| js_err(&error))?;
        keep.push(f);
    }
    Ok(keep)
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "every engine is handed its setup the same way"
)]
pub(crate) fn instantiate(setup: Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    if setup.meta.unit.is_some() {
        return instantiate_linked(&setup);
    }
    // The page's engine meters no fuel, and the module owns its memory, so
    // neither limit holds here: a runaway script stalls the tab.
    let _ = (setup.fuel, setup.memory_bytes);
    let module = WebAssembly::Module::new(&Uint8Array::from(setup.wasm).into())
        .map_err(|error| js_err(&error))?;
    let live: Rc<RefCell<Option<Live>>> = Rc::new(RefCell::new(None));
    let errors = Rc::new(RefCell::new(None));
    let env = Object::new();
    let direct = setup.meta.direct.then(|| {
        register(Direct {
            host: setup.host.clone(),
            live: live.clone(),
            unit: 0,
            errors: errors.clone(),
        })
    });
    if direct.is_some() {
        link_direct(&env, setup.meta)?;
    }
    // A direct module imports only its entry points.
    let one_each: &[crate::Import] = if direct.is_some() {
        &[]
    } else {
        &setup.meta.imports
    };
    let keep = imports_each(&env, &setup.host, &live, one_each, 0)?;
    let imports = Object::new();
    Reflect::set(&imports, &words::HOST_MODULE.into(), &env).map_err(|error| js_err(&error))?;
    let inst = WebAssembly::Instance::new(&module, &imports).map_err(|error| js_err(&error))?;
    let exports = inst.exports();
    let memory = Reflect::get(&exports, &words::MEMORY.into()).map_err(|error| js_err(&error))?;
    let memory = memory
        .dyn_into::<WebAssembly::Memory>()
        .map_err(|error| js_err(&error))?;
    let live_now = Live {
        exports,
        memory,
        links: None,
    };
    if let Some(id) = direct {
        live_now.set_global(0, words::HOST_CTX, id as i32);
    }
    *live.borrow_mut() = Some(live_now);
    Ok(Box::new(Browser {
        live,
        errors,
        _imports: keep,
        direct,
    }))
}

// A script built as units: its memory, table and globals made here, its own
// unit instantiated now and every other before the first call that needs one.
fn instantiate_linked(setup: &Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    let layout = unit_layout(setup.meta, setup.linked);
    let memory_descriptor = Object::new();
    set(
        &memory_descriptor,
        "initial",
        &JsValue::from(layout.heap.div_ceil(65536)),
    )?;
    if let Some(bytes) = setup.memory_bytes {
        let pages = (bytes as u64).div_ceil(65536) as u32;
        set(&memory_descriptor, "maximum", &JsValue::from(pages))?;
    }
    let memory = WebAssembly::Memory::new(&memory_descriptor).map_err(|error| js_err(&error))?;
    let table_descriptor = Object::new();
    set(&table_descriptor, "element", &"anyfunc".into())?;
    set(&table_descriptor, "initial", &JsValue::from(1))?;
    let table = WebAssembly::Table::new(&table_descriptor).map_err(|error| js_err(&error))?;
    let globals = shared_starts(&layout)
        .into_iter()
        .map(mutable_global)
        .collect::<Result<Vec<_>, String>>()?;
    let live: Rc<RefCell<Option<Live>>> = Rc::new(RefCell::new(None));
    let errors = Rc::new(RefCell::new(None));
    let unit = |compiled: Option<Arc<Compiled>>, base: u32| LinkUnit {
        compiled,
        base,
        link_base: 0,
        links: vec![],
        exports: None,
        direct: None,
    };
    let mut units = vec![unit(None, layout.bases[0])];
    let mut by_key = FxHashMap::default();
    by_key.insert(setup.key.to_string(), 0);
    for ((key, compiled), base) in setup.linked.iter().zip(&layout.bases[1..]) {
        by_key.insert(key.clone(), units.len());
        units.push(unit(Some(compiled.clone()), *base));
    }
    let links = Rc::new(Links {
        host: setup.host.clone(),
        live: live.clone(),
        errors: errors.clone(),
        memory: memory.clone(),
        table,
        globals,
        pending: Cell::new(true),
        closures: RefCell::default(),
        units: RefCell::new(units),
        by_key: RefCell::new(by_key),
    });
    links.make(0, setup.wasm, setup.meta)?;
    let exports = links.units.borrow()[0]
        .exports
        .clone()
        .ok_or("the script's own unit is not made")?;
    *live.borrow_mut() = Some(Live {
        exports,
        memory,
        links: Some(links.clone()),
    });
    Ok(Box::new(Browser {
        live,
        errors,
        _imports: vec![],
        direct: None,
    }))
}

struct Browser {
    live: Rc<RefCell<Option<Live>>>,
    errors: Rc<RefCell<Option<String>>>,
    _imports: Vec<Import>,
    /// The id a direct module passes the engine's entry points.
    direct: Option<usize>,
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(id) = self.direct {
            DIRECT.with(|direct| direct.borrow_mut()[id] = None);
        }
        // A unit's closures, and what its entry points reach, hold the rest.
        if let Some(links) = self.live.borrow_mut().take().and_then(|live| live.links) {
            for unit in links.units.borrow().iter() {
                if let Some(id) = unit.direct {
                    DIRECT.with(|direct| direct.borrow_mut()[id] = None);
                }
            }
        }
    }
}

/// What a direct module's calls into the engine reach it by: its host state
/// and memory, under the id the module passes, the unit that passed it, and
/// why a call failed.
struct Direct {
    host: Shared,
    live: Rc<RefCell<Option<Live>>>,
    unit: usize,
    errors: Rc<RefCell<Option<String>>>,
}

thread_local! {
    static DIRECT: RefCell<Vec<Option<Rc<Direct>>>> = const { RefCell::new(Vec::new()) };
}

fn register(direct: Direct) -> usize {
    DIRECT.with(|table| {
        let mut table = table.borrow_mut();
        table.push(Some(Rc::new(direct)));
        table.len() - 1
    })
}

fn direct_of(ctx: i32) -> Option<Rc<Direct>> {
    DIRECT.with(|t| t.borrow().get(ctx as usize).cloned().flatten())
}

// A direct module's entry points are the engine's own exports, so a call from
// the script stays inside WebAssembly: no JavaScript, no arrays, no BigInts.
fn link_direct(env: &Object, meta: &crate::Meta) -> Result<(), String> {
    let engine = wasm_bindgen::exports();
    let mut names: Vec<String> = direct_arities(meta)
        .into_iter()
        .map(|n| format!("{}{n}", words::DIRECT_CALL))
        .collect();
    names.push(words::DIRECT_RESULT.into());
    for name in names {
        let ours = format!("__balaur{name}");
        let f = Reflect::get(&engine, &ours.as_str().into()).map_err(|error| js_err(&error))?;
        if !f.is_function() {
            return Err(format!("this engine exports no `{ours}`"));
        }
        Reflect::set(env, &name.as_str().into(), &f).map_err(|error| js_err(&error))?;
    }
    Ok(())
}

// A failed call cannot unwind through the script: it says why in `error` and
// traps, and `Browser::call` reads it. Nothing is held when it traps.
fn enter(ctx: i32, index: i32, bits: &[u64]) -> i64 {
    match direct_step(ctx, index, bits) {
        Ok(v) => v,
        Err(()) => core::arch::wasm32::unreachable(),
    }
}

fn direct_step(ctx: i32, index: i32, bits: &[u64]) -> Result<i64, ()> {
    let d = direct_of(ctx).ok_or(())?;
    let out = match d.live.borrow().as_ref() {
        Some(live) => {
            let mut view = Ctx { live, unit: d.unit };
            direct_call(&d.host, &mut view, d.unit, index as usize, bits)
        }
        None => Err("the module is not ready".into()),
    };
    out.map(u64::cast_signed).map_err(|e| {
        *d.errors.borrow_mut() = Some(e);
    })
}

macro_rules! entries {
    ($($name:ident($($a:ident),*);)*) => {$(
        // Exported by its symbol, whatever its visibility.
        #[unsafe(no_mangle)]
        extern "C" fn $name(ctx: i32, index: i32 $(, $a: i64)*) -> i64 {
            enter(ctx, index, &[$($a.cast_unsigned()),*])
        }
    )*};
}

entries! {
    __balaur__bs_call0();
    __balaur__bs_call1(a0);
    __balaur__bs_call2(a0, a1);
    __balaur__bs_call3(a0, a1, a2);
    __balaur__bs_call4(a0, a1, a2, a3);
    __balaur__bs_call5(a0, a1, a2, a3, a4);
    __balaur__bs_call6(a0, a1, a2, a3, a4, a5);
    __balaur__bs_call7(a0, a1, a2, a3, a4, a5, a6);
    __balaur__bs_call8(a0, a1, a2, a3, a4, a5, a6, a7);
    __balaur__bs_call9(a0, a1, a2, a3, a4, a5, a6, a7, a8);
    __balaur__bs_call10(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9);
    __balaur__bs_call11(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);
    __balaur__bs_call12(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11);
    __balaur__bs_call13(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
    __balaur__bs_call14(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13);
    __balaur__bs_call15(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
    __balaur__bs_call16(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);
}

#[unsafe(no_mangle)]
extern "C" fn __balaur__bs_result(ctx: i32, k: i32) -> i64 {
    direct_of(ctx).map_or(0, |d| direct_result(&d.host, k as usize).cast_signed())
}

impl Browser {
    fn with<T>(&self, f: impl FnOnce(&Live) -> T, otherwise: T) -> T {
        self.live.borrow().as_ref().map_or(otherwise, f)
    }
}

impl Wasm for Browser {
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        self.call_in(0, name, args, results)
    }
    fn call_in(
        &mut self,
        unit: usize,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Vec<Raw>, String> {
        let guard = self.live.borrow();
        let func = guard
            .as_ref()
            .ok_or("the module is not ready")?
            .export(unit, name)?;
        drop(guard);
        apply(&func, args, results).map_err(|trap| {
            // A direct call that failed left its reason before trapping.
            self.errors.borrow_mut().take().unwrap_or(trap)
        })
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.has_func_in(0, name)
    }
    fn has_func_in(&mut self, unit: usize, name: &str) -> bool {
        self.with(|live| live.export(unit, name).is_ok(), false)
    }
    fn link(&mut self, key: &str, unit: Arc<Compiled>) -> Result<usize, String> {
        let links = self.with(|live| live.links.clone(), None);
        match links {
            Some(links) => links.link(key, unit),
            None => Err(format!("{key}: this script is not built as units")),
        }
    }
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        self.with(|live| live.read(at, len), vec![0; len])
    }
    fn memory_len(&self) -> usize {
        self.with(|live| live.bytes().length() as usize, 0)
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        self.with(|live| live.write(at, bytes), ());
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        self.with(|live| live.global(0, name), None)
    }
    fn set_global(&mut self, name: &str, value: i32) {
        self.with(|live| live.set_global(0, name, value), ());
    }
    fn global_in(&mut self, unit: usize, name: &str) -> Option<i32> {
        self.with(|live| live.own_global(unit, name), None)
    }
    fn make(&mut self, _unit: usize) -> Result<(), String> {
        match self.with(|live| live.links.clone(), None) {
            Some(links) => links.make_pending(),
            None => Ok(()),
        }
    }
    // The page's engine has no fuel; a runaway script is the tab's to stop.
    fn set_fuel(&mut self, _fuel: u64) {}
}
