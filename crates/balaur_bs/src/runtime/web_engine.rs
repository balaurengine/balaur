// The browser's own engine: the page compiles the module natively, so a web
// build runs scripts at the speed wasmtime gives a desktop one.

use super::engine::{Args, ImportCtx, Raw, Setup, Shared, Wasm};
use super::host::{direct_arities, direct_call, direct_result, dispatch};
use crate::{Slot, words};
use js_sys::{Array, BigInt, Function, Object, Reflect, Uint8Array, WebAssembly};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

struct Live {
    exports: Object,
    memory: WebAssembly::Memory,
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
    fn global(&self, name: &str) -> Option<i32> {
        let g = Reflect::get(&self.exports, &name.into()).ok()?;
        Reflect::get(&g, &"value".into())
            .ok()?
            .as_f64()
            .map(|v| v as i32)
    }
    fn set_global(&self, name: &str, value: i32) {
        if let Ok(g) = Reflect::get(&self.exports, &name.into()) {
            let _ = Reflect::set(&g, &"value".into(), &JsValue::from(value));
        }
    }
    fn export(&self, name: &str) -> Result<Function, String> {
        let f = Reflect::get(&self.exports, &name.into()).map_err(|e| js_err(&e))?;
        f.dyn_into().map_err(|_| format!("no export `{name}`"))
    }
}

// Re-entrant from an import: the page's engine lets a host function call back in.
fn apply(f: &Function, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
    let a: Array = args.iter().map(|r| to_js(*r)).collect();
    let out = f.apply(&JsValue::NULL, &a).map_err(|e| js_err(&e))?;
    Ok(match results {
        0 => vec![],
        1 => vec![from_js(&out, None)],
        _ => Array::from(&out)
            .iter()
            .map(|v| from_js(&v, None))
            .collect(),
    })
}

struct Ctx<'a>(&'a Live);

impl ImportCtx for Ctx<'_> {
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        self.0.read(at, len)
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        self.0.write(at, bytes);
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        self.0.global(name)
    }
    fn set_global(&mut self, name: &str, value: i32) {
        self.0.set_global(name, value);
    }
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        apply(&self.0.export(name)?, args, results)
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.0.export(name).is_ok()
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

#[allow(
    clippy::needless_pass_by_value,
    reason = "every engine is handed its setup the same way"
)]
pub(crate) fn instantiate(setup: Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    // The page's engine meters no fuel, and the module owns its memory, so
    // neither limit holds here: a runaway script stalls the tab.
    let _ = (setup.fuel, setup.memory_bytes);
    let module =
        WebAssembly::Module::new(&Uint8Array::from(setup.wasm).into()).map_err(|e| js_err(&e))?;
    let live: Rc<RefCell<Option<Live>>> = Rc::new(RefCell::new(None));
    let env = Object::new();
    let mut keep = vec![];
    let direct = setup.meta.direct.then(|| {
        DIRECT.with(|t| {
            let mut t = t.borrow_mut();
            t.push(Some(Rc::new(Direct {
                host: setup.host.clone(),
                live: live.clone(),
                error: RefCell::new(None),
            })));
            t.len() - 1
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
        let (host, live, params) = (setup.host.clone(), live.clone(), imp.params.clone());
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
            let out =
                dispatch(&host, &mut Ctx(l), 0, index, &args).map_err(|e| JsValue::from_str(&e))?;
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
            .map_err(|e| js_err(&e))?;
        Reflect::set(&env, &imp.name.as_str().into(), &wrapped).map_err(|e| js_err(&e))?;
        keep.push(f);
    }
    let imports = Object::new();
    Reflect::set(&imports, &words::HOST_MODULE.into(), &env).map_err(|e| js_err(&e))?;
    let inst = WebAssembly::Instance::new(&module, &imports).map_err(|e| js_err(&e))?;
    let exports = inst.exports();
    let memory = Reflect::get(&exports, &words::MEMORY.into()).map_err(|e| js_err(&e))?;
    let memory = memory
        .dyn_into::<WebAssembly::Memory>()
        .map_err(|e| js_err(&e))?;
    let live_now = Live { exports, memory };
    if let Some(id) = direct {
        live_now.set_global(words::HOST_CTX, id as i32);
    }
    *live.borrow_mut() = Some(live_now);
    Ok(Box::new(Browser {
        live,
        _imports: keep,
        direct,
    }))
}

struct Browser {
    live: Rc<RefCell<Option<Live>>>,
    _imports: Vec<Import>,
    /// The id a direct module passes the engine's entry points.
    direct: Option<usize>,
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(id) = self.direct {
            DIRECT.with(|t| t.borrow_mut()[id] = None);
        }
    }
}

/// What a direct module's calls into the engine reach it by: its host state
/// and memory, under the id the module passes, and why a call failed.
struct Direct {
    host: Shared,
    live: Rc<RefCell<Option<Live>>>,
    error: RefCell<Option<String>>,
}

thread_local! {
    static DIRECT: RefCell<Vec<Option<Rc<Direct>>>> = const { RefCell::new(Vec::new()) };
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
        let f = Reflect::get(&engine, &ours.as_str().into()).map_err(|e| js_err(&e))?;
        if !f.is_function() {
            return Err(format!("this engine exports no `{ours}`"));
        }
        Reflect::set(env, &name.as_str().into(), &f).map_err(|e| js_err(&e))?;
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
        Some(l) => direct_call(&d.host, &mut Ctx(l), index as usize, bits),
        None => Err("the module is not ready".into()),
    };
    out.map(u64::cast_signed).map_err(|e| {
        *d.error.borrow_mut() = Some(e);
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
        let guard = self.live.borrow();
        let f = guard
            .as_ref()
            .ok_or("the module is not ready")?
            .export(name)?;
        drop(guard);
        apply(&f, args, results).map_err(|trap| {
            // A direct call that failed left its reason before trapping.
            self.direct
                .and_then(|id| direct_of(id as i32))
                .and_then(|d| d.error.borrow_mut().take())
                .unwrap_or(trap)
        })
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.with(
            |l| Reflect::get(&l.exports, &name.into()).is_ok_and(|f| f.is_function()),
            false,
        )
    }
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        self.with(|l| l.read(at, len), vec![0; len])
    }
    fn memory_len(&self) -> usize {
        self.with(|l| l.bytes().length() as usize, 0)
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        self.with(|l| l.write(at, bytes), ());
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        self.with(|l| l.global(name), None)
    }
    fn set_global(&mut self, name: &str, value: i32) {
        self.with(|l| l.set_global(name, value), ());
    }
    // The page's engine has no fuel; a runaway script is the tab's to stop.
    fn set_fuel(&mut self, _fuel: u64) {}
}
