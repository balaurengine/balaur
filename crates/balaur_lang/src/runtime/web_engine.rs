// The browser's own engine: the page compiles the module natively, so a web
// build runs scripts at the speed wasmtime gives a desktop one.

use super::engine::{ImportCtx, Raw, Setup, Wasm};
use super::host::dispatch;
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
        _ => Raw::F64(v.as_f64().unwrap_or(0.0)),
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
    // A host function is variadic from JavaScript's side; this hands the Rust
    // closure its arguments as one array.
    let shim = Function::new_with_args(
        "f",
        "return function(){ return f(Array.prototype.slice.call(arguments)); }",
    );
    let mut keep = vec![];
    for (index, imp) in setup.meta.imports.iter().enumerate() {
        let (host, live, params) = (setup.host.clone(), live.clone(), imp.params.clone());
        let f: Import = Closure::new(move |a: Array| {
            let args: Vec<Raw> = params
                .iter()
                .enumerate()
                .map(|(i, s)| from_js(&a.get(i as u32), Some(*s)))
                .collect();
            let guard = live.borrow();
            let l = guard
                .as_ref()
                .ok_or_else(|| JsValue::from_str("the module is not ready"))?;
            let out =
                dispatch(&host, &mut Ctx(l), index, &args).map_err(|e| JsValue::from_str(&e))?;
            Ok(match out.len() {
                0 => JsValue::UNDEFINED,
                1 => to_js(out[0]),
                _ => out.into_iter().map(to_js).collect::<Array>().into(),
            })
        });
        let wrapped = shim
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
    *live.borrow_mut() = Some(Live { exports, memory });
    Ok(Box::new(Browser {
        live,
        _imports: keep,
    }))
}

struct Browser {
    live: Rc<RefCell<Option<Live>>>,
    _imports: Vec<Import>,
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
        apply(&f, args, results)
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
