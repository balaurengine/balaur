// A module run as native code an export built ahead of time (`crate::c`), when
// a loaded build has its hash; any other goes to the engine it would have had.

use super::engine::{Args, ImportCtx, Raw, Setup, Shared, Wasm, read_from};
use super::host::dispatch;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::rc::Rc;
use std::sync::Mutex;

const ABI: u32 = 1;
const HOST_TRAP: i32 = 6;

#[repr(C)]
struct HostVtable {
    import: unsafe extern "C" fn(*mut c_void, u32, *const u64, *mut u64) -> i32,
}

#[repr(C)]
struct Inst {
    _opaque: [u8; 0],
}

/// `bs_module` in the C prelude.
#[repr(C)]
pub(crate) struct Module {
    abi: u32,
    nfuncs: u32,
    func_names: *const *const c_char,
    func_results: *const *const c_char,
    nglobals: u32,
    global_names: *const *const c_char,
    global_index: *const u32,
    nimports: u32,
    import_params: *const *const c_char,
    import_results: *const *const c_char,
    instantiate: unsafe extern "C" fn(*const HostVtable, *mut c_void, u64) -> *mut Inst,
    call: unsafe extern "C" fn(*mut Inst, u32, *const u64, *mut u64) -> i32,
    destroy: unsafe extern "C" fn(*mut Inst),
    memory: unsafe extern "C" fn(*mut Inst, *mut u64) -> *mut u8,
    globals: unsafe extern "C" fn(*mut Inst) -> *mut u64,
}

#[repr(C)]
struct Entry {
    hash: *const c_char,
    module: *const Module,
}

/// A loaded build's modules by hash. The library stays loaded for the life of
/// the process, so what it points at does too.
struct Loaded(Vec<(String, &'static Module)>);

// SAFETY: the modules are immutable data in a library that is never unloaded.
unsafe impl Send for Loaded {}

static LOADED: Mutex<Loaded> = Mutex::new(Loaded(Vec::new()));

static HOST: HostVtable = HostVtable { import };

/// Opens the native build at `path` and adds its modules; how many it has.
pub(crate) fn load(path: &Path) -> Result<usize, String> {
    // SAFETY: the library is one an export built from this runtime's own C
    // translation; loading it runs no initialisers of its own.
    let lib = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
    let lib: &'static libloading::Library = Box::leak(Box::new(lib));
    // SAFETY: `balaur_bs_modules` has this signature in every build `crate::c` writes.
    let list =
        unsafe { lib.get::<unsafe extern "C" fn(*mut u32) -> *const Entry>(b"balaur_bs_modules") }
            .map_err(|e| e.to_string())?;
    let mut count = 0u32;
    // SAFETY: as above; the table it returns is static data of `count` entries.
    let entries = unsafe {
        let at = list(&raw mut count);
        std::slice::from_raw_parts(at, count as usize)
    };
    let mut loaded = LOADED.lock().map_err(|e| e.to_string())?;
    for e in entries {
        // SAFETY: each entry names a static module and a NUL-terminated hash.
        let (hash, module) = unsafe { (CStr::from_ptr(e.hash), &*e.module) };
        if module.abi != ABI {
            return Err(format!(
                "a native build for runtime ABI {}, not {ABI}",
                module.abi
            ));
        }
        loaded.0.push((hash.to_string_lossy().into_owned(), module));
    }
    Ok(entries.len())
}

/// The native build of `wasm`, when a loaded one has it.
/// Whether any native build is loaded.
pub(crate) fn any() -> bool {
    LOADED.lock().is_ok_and(|l| !l.0.is_empty())
}

pub(crate) fn find(wasm: &[u8]) -> Option<&'static Module> {
    let hash = crate::module_hash(wasm);
    let loaded = LOADED.lock().ok()?;
    loaded.0.iter().find(|(h, _)| *h == hash).map(|(_, m)| *m)
}

#[derive(Clone, Copy)]
enum Kind {
    I32,
    I64,
    F64,
}

fn kinds(codes: *const c_char) -> Vec<Kind> {
    // SAFETY: every code string in a descriptor is NUL-terminated static data.
    let codes = unsafe { CStr::from_ptr(codes) }.to_bytes();
    codes
        .iter()
        .map(|c| match c {
            b'l' => Kind::I64,
            b'd' => Kind::F64,
            _ => Kind::I32,
        })
        .collect()
}

fn names(list: *const *const c_char, n: u32) -> Vec<String> {
    (0..n as usize)
        .map(|i| {
            // SAFETY: `list` holds `n` NUL-terminated static strings.
            unsafe { CStr::from_ptr(*list.add(i)) }
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn raw(kind: Kind, bits: u64) -> Raw {
    match kind {
        Kind::I32 => Raw::I32(bits as u32 as i32),
        Kind::I64 => Raw::I64(bits.cast_signed()),
        Kind::F64 => Raw::F64(f64::from_bits(bits)),
    }
}

fn bits(r: Raw) -> u64 {
    match r {
        Raw::I32(x) => u64::from(x.cast_unsigned()),
        Raw::I64(x) => x.cast_unsigned(),
        Raw::F64(x) => x.to_bits(),
    }
}

/// What a module's exports and imports are, read once from its descriptor.
struct Tables {
    funcs: HashMap<String, (u32, Vec<Kind>)>,
    globals: HashMap<String, u32>,
    imports: Vec<(Vec<Kind>, Vec<Kind>)>,
}

/// What the C instance hands every import back: the runtime's host state,
/// and where a failed host function leaves its message.
struct Ctx {
    host: Shared,
    module: &'static Module,
    inst: *mut Inst,
    tables: Rc<Tables>,
    error: Option<String>,
}

struct Native {
    ctx: *mut Ctx,
}

pub(crate) fn instantiate(
    module: &'static Module,
    setup: Setup<'_>,
) -> Result<Box<dyn Wasm>, String> {
    let n = |c: u32| c as usize;
    let fnames = names(module.func_names, module.nfuncs);
    let funcs = fnames
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            // SAFETY: `func_results` has `nfuncs` entries.
            let results = kinds(unsafe { *module.func_results.add(i) });
            (name, (i as u32, results))
        })
        .collect();
    let globals = names(module.global_names, module.nglobals)
        .into_iter()
        .enumerate()
        // SAFETY: `global_index` has `nglobals` entries.
        .map(|(i, name)| (name, unsafe { *module.global_index.add(i) }))
        .collect();
    let imports = (0..n(module.nimports))
        .map(|i| {
            // SAFETY: both lists have `nimports` entries.
            unsafe {
                (
                    kinds(*module.import_params.add(i)),
                    kinds(*module.import_results.add(i)),
                )
            }
        })
        .collect();
    let ctx = Box::into_raw(Box::new(Ctx {
        host: setup.host,
        module,
        inst: std::ptr::null_mut(),
        tables: Rc::new(Tables {
            funcs,
            globals,
            imports,
        }),
        error: None,
    }));
    let max = setup.memory_bytes.map_or(u64::MAX, |b| b as u64);
    // SAFETY: `ctx` outlives the instance, which `Native::drop` frees first.
    let inst = unsafe { (module.instantiate)(&raw const HOST, ctx.cast(), max) };
    if inst.is_null() {
        // SAFETY: `ctx` came from `Box::into_raw` above and nothing else holds it.
        drop(unsafe { Box::from_raw(ctx) });
        return Err("the native module could not get its memory".into());
    }
    // SAFETY: `ctx` is live and nothing else is touching it yet.
    unsafe { (*ctx).inst = inst };
    Ok(Box::new(Native { ctx }))
}

impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: the instance and `ctx` were made together in `instantiate`
        // and are freed together here, once.
        unsafe {
            ((*self.ctx).module.destroy)((*self.ctx).inst);
            drop(Box::from_raw(self.ctx));
        }
    }
}

// Every access goes through the raw pointer for as long as one statement, so
// a host function the module calls, which reaches the same `Ctx`, never
// overlaps a reference held here.
fn view(ctx: *mut Ctx) -> View {
    View(ctx)
}

struct View(*mut Ctx);

impl View {
    fn parts(&self) -> (&'static Module, *mut Inst, Rc<Tables>) {
        // SAFETY: the `Ctx` is live while its instance runs or is held.
        unsafe { ((*self.0).module, (*self.0).inst, (*self.0).tables.clone()) }
    }

    fn memory(&self) -> &[u8] {
        let (module, inst, _) = self.parts();
        let mut len = 0u64;
        // SAFETY: the instance is live; the slice is read before anything
        // else can run in the module and move its memory.
        unsafe {
            let at = (module.memory)(inst, &raw mut len);
            std::slice::from_raw_parts(at, len as usize)
        }
    }

    fn global_slot(&self, name: &str) -> Option<*mut u64> {
        let (module, inst, tables) = self.parts();
        let index = *tables.globals.get(name)?;
        // SAFETY: the index is one of the module's globals.
        Some(unsafe { (module.globals)(inst).add(index as usize) })
    }
}

impl ImportCtx for View {
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        read_from(self.memory(), at, len)
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        let (module, inst, _) = self.parts();
        let mut len = 0u64;
        // SAFETY: as in `memory`, and only inside the memory's length.
        unsafe {
            let base = (module.memory)(inst, &raw mut len);
            if at + bytes.len() <= len as usize {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), base.add(at), bytes.len());
            }
        }
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        // SAFETY: a live global slot.
        self.global_slot(name).map(|g| unsafe { *g } as u32 as i32)
    }
    fn set_global(&mut self, name: &str, value: i32) {
        if let Some(g) = self.global_slot(name) {
            // SAFETY: a live global slot.
            unsafe { *g = u64::from(value.cast_unsigned()) };
        }
    }
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        let (module, inst, tables) = self.parts();
        let (index, kinds) = tables
            .funcs
            .get(name)
            .ok_or_else(|| format!("no export `{name}`"))?;
        let a: Vec<u64> = args.iter().map(|r| bits(*r)).collect();
        let mut r = vec![0u64; kinds.len().max(results)];
        // SAFETY: the instance is live and `r` has room for every result.
        let code = unsafe { (module.call)(inst, *index, a.as_ptr(), r.as_mut_ptr()) };
        if code != 0 {
            // SAFETY: the call has returned, so nothing else holds the `Ctx`.
            let host = unsafe { (*self.0).error.take() };
            return Err(message(code, host));
        }
        Ok(kinds.iter().zip(r).map(|(k, b)| raw(*k, b)).collect())
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.parts().2.funcs.contains_key(name)
    }
}

fn message(code: i32, host: Option<String>) -> String {
    match code {
        1 => "wasm `unreachable` instruction executed".into(),
        2 => "out of bounds memory access".into(),
        3 => "integer divide by zero".into(),
        4 => "integer overflow".into(),
        5 => "call stack exhausted".into(),
        HOST_TRAP => host.unwrap_or_else(|| "a host function failed".into()),
        7 => "invalid conversion to integer".into(),
        8 => "a `fn` value with no function, or not one of its type".into(),
        other => format!("trap {other}"),
    }
}

unsafe extern "C" fn import(
    ctx: *mut c_void,
    index: u32,
    args: *const u64,
    results: *mut u64,
) -> i32 {
    let ctx = ctx.cast::<Ctx>();
    // SAFETY: `ctx` is the `Ctx` this instance was made with; the reference
    // ends with the statement.
    let (host, tables) = unsafe { ((*ctx).host.clone(), (*ctx).tables.clone()) };
    let Some((params, rets)) = tables.imports.get(index as usize) else {
        return HOST_TRAP;
    };
    let a: Args = params
        .iter()
        .enumerate()
        // SAFETY: the module passes one argument per parameter.
        .map(|(i, k)| raw(*k, unsafe { *args.add(i) }))
        .collect();
    match dispatch(&host, &mut view(ctx), 0, index as usize, &a) {
        Ok(out) => {
            for (i, r) in out.iter().take(rets.len()).enumerate() {
                // SAFETY: the module has room for one result per result type.
                unsafe { *results.add(i) = bits(*r) };
            }
            0
        }
        Err(msg) => {
            // SAFETY: the host function has returned; nothing else holds it.
            unsafe { (*ctx).error = Some(msg) };
            HOST_TRAP
        }
    }
}

impl Wasm for Native {
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String> {
        view(self.ctx).call(name, args, results)
    }
    fn has_func(&mut self, name: &str) -> bool {
        view(self.ctx).has_func(name)
    }
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        view(self.ctx).read(at, len)
    }
    fn memory_len(&self) -> usize {
        view(self.ctx).memory().len()
    }
    fn write(&mut self, at: usize, bytes: &[u8]) {
        view(self.ctx).write(at, bytes);
    }
    fn global(&mut self, name: &str) -> Option<i32> {
        view(self.ctx).global(name)
    }
    fn set_global(&mut self, name: &str, value: i32) {
        view(self.ctx).set_global(name, value);
    }
    // Native code counts no fuel; a module built to meter itself still does.
    fn set_fuel(&mut self, _fuel: u64) {}
}
