// A module run as native code an export built ahead of time (`crate::c`), when
// a loaded build has its hash; any other goes to the engine it would have had.

use super::engine::{Args, ImportCtx, Raw, Setup, Shared, Wasm, copy_from, read_from};
use super::host::dispatch;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::rc::Rc;
use std::sync::Mutex;

const ABI: u32 = 3;
const HOST_TRAP: i32 = 6;

/// `bs_host` in the C prelude: the build links against nothing, so its
/// memory and its stack's extent come from here.
#[repr(C)]
struct HostVtable {
    import: unsafe extern "C" fn(*mut c_void, u32, u32, *const u64, *mut u64) -> i32,
    resize: unsafe extern "C" fn(*mut c_void, u64, u64) -> *mut c_void,
    stack_bottom: unsafe extern "C" fn() -> usize,
}

const BLOCK_ALIGN: usize = 16;

// A block of `n` bytes, as both its allocation and its release name it.
fn block(n: u64) -> Option<std::alloc::Layout> {
    std::alloc::Layout::from_size_align(usize::try_from(n.max(1)).ok()?, BLOCK_ALIGN).ok()
}

unsafe extern "C" fn resize(ptr: *mut c_void, old: u64, new: u64) -> *mut c_void {
    let (Some(was), Some(now)) = (block(old), block(new)) else {
        return std::ptr::null_mut();
    };
    // SAFETY: a non-null `ptr` is a block this function handed out at `old` bytes.
    unsafe {
        if ptr.is_null() {
            return std::alloc::alloc_zeroed(now).cast();
        }
        if new == 0 {
            std::alloc::dealloc(ptr.cast(), was);
            return std::ptr::null_mut();
        }
        let grown = std::alloc::realloc(ptr.cast(), was, now.size());
        if !grown.is_null() && now.size() > was.size() {
            grown
                .add(was.size())
                .write_bytes(0, now.size() - was.size());
        }
        grown.cast()
    }
}

#[cfg(target_vendor = "apple")]
unsafe extern "C" fn stack_bottom() -> usize {
    unsafe extern "C" {
        fn pthread_self() -> *mut c_void;
        fn pthread_get_stackaddr_np(thread: *mut c_void) -> *mut c_void;
        fn pthread_get_stacksize_np(thread: *mut c_void) -> usize;
    }
    // SAFETY: libSystem's own answers about the calling thread.
    unsafe {
        let t = pthread_self();
        pthread_get_stackaddr_np(t) as usize - pthread_get_stacksize_np(t)
    }
}

// Elsewhere the module takes its floor from where the host first called in.
#[cfg(not(target_vendor = "apple"))]
unsafe extern "C" fn stack_bottom() -> usize {
    0
}

#[repr(C)]
struct Inst {
    _opaque: [u8; 0],
}

/// `bs_unit` in the C prelude: a unit's memory, table and link bases, and
/// the place it has in its script's [`Ctx::units`].
#[repr(C)]
struct CUnit {
    base: [u32; 3],
    number: u32,
}

/// `bs_slot`: a `fn` table slot.
#[repr(C)]
struct Slot {
    func: *const c_void,
    unit: *mut CUnit,
    tag: u64,
}

/// `bs_ref`: an export as a link slot takes it.
#[repr(C)]
struct Ref {
    func: *const c_void,
    tag: u64,
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
    call: unsafe extern "C" fn(*mut Inst, *mut CUnit, u32, *const u64, *mut u64) -> i32,
    destroy: unsafe extern "C" fn(*mut Inst),
    memory: unsafe extern "C" fn(*mut Inst, *mut u64) -> *mut u8,
    globals: unsafe extern "C" fn(*mut Inst) -> *mut u64,
    unit: u32,
    refs: *const Ref,
    make: Option<unsafe extern "C" fn(*mut Inst, *mut CUnit) -> i32>,
    link_instance: unsafe extern "C" fn(*const HostVtable, *mut c_void, u64, u64) -> *mut Inst,
    table: unsafe extern "C" fn(*mut Inst, u32, *mut u64) -> *mut Slot,
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

static HOST: HostVtable = HostVtable {
    import,
    resize,
    stack_bottom,
};

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
        Kind::F64 => Raw::F64(super::values::float(bits)),
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
    /// A whole module's, or the script's own unit's: the instance's memory,
    /// globals, table and end are the same in every module of a build.
    module: &'static Module,
    inst: *mut Inst,
    tables: Rc<Tables>,
    error: Option<String>,
    /// A script built as units: each unit, its own first; empty for a whole module.
    units: Vec<Unit>,
    by_key: HashMap<String, usize>,
    /// Link slots whose unit is not linked yet: the slot, the file, the export.
    waiting: Vec<(u64, String, String)>,
}

/// One unit of a script run as native code.
struct Unit {
    module: &'static Module,
    tables: Rc<Tables>,
    /// Its bases, which its table slots point at for as long as the instance lives.
    c: *mut CUnit,
    /// What the host numbers its engine calls by.
    numbered: usize,
}

impl Drop for Unit {
    fn drop(&mut self) {
        // SAFETY: `c` came from `Box::into_raw` in `make_unit`, and the
        // instance whose slots point at it is destroyed before its `Ctx`.
        drop(unsafe { Box::from_raw(self.c) });
    }
}

struct Native {
    ctx: *mut Ctx,
}

impl Tables {
    /// A module's exports and imports, read from its descriptor.
    fn of(module: &'static Module) -> Self {
        let funcs = names(module.func_names, module.nfuncs)
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
        let imports = (0..module.nimports as usize)
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
        Tables {
            funcs,
            globals,
            imports,
        }
    }
}

fn context(host: Shared, module: &'static Module) -> *mut Ctx {
    Box::into_raw(Box::new(Ctx {
        host,
        module,
        inst: std::ptr::null_mut(),
        tables: Rc::new(Tables::of(module)),
        error: None,
        units: vec![],
        by_key: HashMap::new(),
        waiting: vec![],
    }))
}

pub(crate) fn instantiate(
    module: &'static Module,
    setup: Setup<'_>,
) -> Result<Box<dyn Wasm>, String> {
    let ctx = context(setup.host, module);
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

/// Whether a loaded build has the script's own unit and every unit it uses.
pub(crate) fn has_units(setup: &Setup<'_>) -> bool {
    find(setup.wasm).is_some() && setup.linked.iter().all(|(_, c)| find(&c.wasm).is_some())
}

/// A script built as units, run as native code: one instance its units
/// share, each unit made now, its own first.
pub(crate) fn instantiate_linked(setup: Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    let root = find(setup.wasm).ok_or("no loaded native build has this unit")?;
    let unit = setup.meta.unit.as_ref().ok_or("not a unit")?;
    let (bases, heap) = super::engine::unit_layout(setup.meta, setup.linked);
    let host = setup.host.clone();
    let ctx = context(setup.host, root);
    let max = setup.memory_bytes.map_or(u64::MAX, |b| b as u64);
    // SAFETY: `ctx` outlives the instance, which `Native::drop` frees first.
    let inst = unsafe { (root.link_instance)(&raw const HOST, ctx.cast(), u64::from(heap), max) };
    if inst.is_null() {
        // SAFETY: `ctx` came from `Box::into_raw` above and nothing else holds it.
        drop(unsafe { Box::from_raw(ctx) });
        return Err("the native script could not get its memory".into());
    }
    // SAFETY: `ctx` is live and nothing else is touching it yet.
    unsafe { (*ctx).inst = inst };
    let native = Native { ctx };
    make_unit(ctx, setup.key, root, unit, bases[0], 0)?;
    for ((key, compiled), base) in setup.linked.iter().zip(&bases[1..]) {
        let module = find(&compiled.wasm).ok_or_else(|| format!("{key}: no native build"))?;
        let unit = compiled
            .meta
            .unit
            .as_ref()
            .ok_or_else(|| format!("{key}: not a unit"))?;
        let numbered = host.borrow_mut().bind_unit(compiled.clone())?;
        make_unit(ctx, key, module, unit, *base, numbered)?;
    }
    Ok(Box::new(native))
}

/// `unit` as unit `key` of the script, added unless it is one: its index. Its
/// static data takes a block of the heap, since the space before the heap was
/// laid out for the units known when the instance was made.
fn link_unit(
    ctx: *mut Ctx,
    key: &str,
    compiled: &std::sync::Arc<crate::Compiled>,
) -> Result<usize, String> {
    // SAFETY: the `Ctx` is live; each read ends with its statement.
    let (linked, known, host) = unsafe {
        (
            !(*ctx).units.is_empty(),
            (*ctx).by_key.get(key).copied(),
            (*ctx).host.clone(),
        )
    };
    if !linked {
        return Err(format!("{key}: this script is not built as units"));
    }
    if let Some(index) = known {
        return Ok(index);
    }
    let module =
        find(&compiled.wasm).ok_or_else(|| format!("{key}: no native build of this unit"))?;
    let unit = compiled
        .meta
        .unit
        .as_ref()
        .ok_or_else(|| format!("{key}: not a unit"))?;
    let base = if unit.data_len == 0 {
        0
    } else {
        let mut v = view(ctx);
        let size = Raw::I32((unit.data_len + 15) as i32);
        let block = v.call_in(0, crate::words::ALLOC, &[size], 1)?;
        let block = block.first().map_or(0, |b| b.i32() as u32);
        // Held as long as the instance, so no object a leak count watches.
        if let Some(live) = v.global(crate::words::LIVE) {
            v.set_global(crate::words::LIVE, live - 1);
        }
        block.next_multiple_of(16)
    };
    let numbered = host.borrow_mut().bind_unit(compiled.clone())?;
    make_unit(ctx, key, module, unit, base, numbered)
}

/// Unit `key` made at `memory_base`: its table slots and link slots added to
/// the table, its static data and own `fn` values placed, each link slot
/// filled where its unit is linked, and the slots that waited for this one.
fn make_unit(
    ctx: *mut Ctx,
    key: &str,
    module: &'static Module,
    unit: &crate::Unit,
    memory_base: u32,
    numbered: usize,
) -> Result<usize, String> {
    let e = |what: &str| format!("{key}: {what}");
    let make = module
        .make
        .ok_or_else(|| e("not a unit of a native build"))?;
    // SAFETY: the `Ctx` is live; nothing else touches it while this runs.
    let (root, inst, index) = unsafe { ((*ctx).module, (*ctx).inst, (*ctx).units.len()) };
    let grow = unit.refs + unit.links.len() as u32;
    let mut len = 0u64;
    // SAFETY: a live instance; the table grows by `grow` empty slots.
    if unsafe { (root.table)(inst, grow, &raw mut len) }.is_null() {
        return Err(e("the table could not grow"));
    }
    let table_base = u32::try_from(len).map_err(|_| e("the table is too long"))? - grow;
    let c = Box::into_raw(Box::new(CUnit {
        base: [memory_base, table_base, table_base + unit.refs],
        number: index as u32,
    }));
    // SAFETY: a live instance, and `c` lives as long as it (`Unit::drop`).
    let code = unsafe { make(inst, c) };
    let held = Unit {
        module,
        tables: Rc::new(Tables::of(module)),
        c,
        numbered,
    };
    if code != 0 {
        return Err(e(&message(code, None)));
    }
    // SAFETY: as above.
    unsafe {
        (*ctx).units.push(held);
        (*ctx).by_key.insert(key.to_string(), index);
    }
    let link_base = u64::from(table_base + unit.refs);
    for (k, link) in unit.links.iter().enumerate() {
        let slot = link_base + k as u64;
        // SAFETY: as above.
        let target = unsafe { (*ctx).by_key.get(&link.file).copied() };
        match target {
            Some(target) => fill(ctx, slot, target, &link.export)?,
            // SAFETY: as above.
            None => unsafe {
                (*ctx)
                    .waiting
                    .push((slot, link.file.clone(), link.export.clone()));
            },
        }
    }
    // SAFETY: as above.
    let waited: Vec<(u64, String, String)> = unsafe {
        let (now, later) = std::mem::take(&mut (*ctx).waiting)
            .into_iter()
            .partition(|(_, file, _)| file == key);
        (*ctx).waiting = later;
        now
    };
    for (slot, _, export) in waited {
        fill(ctx, slot, index, &export)?;
    }
    Ok(index)
}

/// Table slot `slot`: unit `target`'s export `export`, run as that unit.
fn fill(ctx: *mut Ctx, slot: u64, target: usize, export: &str) -> Result<(), String> {
    // SAFETY: the `Ctx` is live; nothing else touches it while this runs.
    let (root, inst, module, tables, c) = unsafe {
        let units = &(*ctx).units;
        let u = &units[target];
        ((*ctx).module, (*ctx).inst, u.module, u.tables.clone(), u.c)
    };
    let (at, _) = tables
        .funcs
        .get(export)
        .ok_or_else(|| format!("no `{}` to link", export.trim_start_matches('~')))?;
    // SAFETY: `refs` has one entry per export, and `at` is one.
    let r = unsafe { &*module.refs.add(*at as usize) };
    let mut len = 0u64;
    // SAFETY: a live instance; growing by 0 only reads the table.
    let table = unsafe { (root.table)(inst, 0, &raw mut len) };
    if slot >= len {
        return Err("a link slot past the table's end".into());
    }
    // SAFETY: `slot` is inside the table.
    unsafe {
        *table.add(slot as usize) = Slot {
            func: r.func,
            unit: c,
            tag: r.tag,
        };
    }
    Ok(())
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
        // A script built as units shares its globals by name.
        let index = if self.linked() {
            crate::words::UNIT_GLOBALS.iter().position(|g| *g == name)?
        } else {
            *tables.globals.get(name)? as usize
        };
        // SAFETY: the index is one of the instance's globals.
        Some(unsafe { (module.globals)(inst).add(index) })
    }

    fn linked(&self) -> bool {
        // SAFETY: the `Ctx` is live; the read ends with the statement.
        unsafe { !(*self.0).units.is_empty() }
    }

    /// Unit `unit`'s module, its bases and its exports; a whole module's
    /// own for unit 0.
    fn unit(&self, unit: usize) -> Result<(&'static Module, *mut CUnit, Rc<Tables>), String> {
        // SAFETY: the `Ctx` is live; each read ends with its statement.
        unsafe {
            let units = &(*self.0).units;
            if let Some(u) = units.get(unit) {
                return Ok((u.module, u.c, u.tables.clone()));
            }
            if unit == 0 && (*self.0).units.is_empty() {
                return Ok((
                    (*self.0).module,
                    std::ptr::null_mut(),
                    (*self.0).tables.clone(),
                ));
            }
        }
        Err("this script is not built as units".into())
    }
}

impl ImportCtx for View {
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        read_from(self.memory(), at, len)
    }
    fn read_into(&self, at: usize, out: &mut [u8]) {
        copy_from(self.memory(), at, out);
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
        self.call_in(0, name, args, results)
    }
    fn has_func(&mut self, name: &str) -> bool {
        self.has_func_in(0, name)
    }
    fn call_in(
        &mut self,
        unit: usize,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Vec<Raw>, String> {
        let (module, c, tables) = self.unit(unit)?;
        let inst = self.parts().1;
        let (index, kinds) = tables
            .funcs
            .get(name)
            .ok_or_else(|| format!("no export `{name}`"))?;
        let a: Vec<u64> = args.iter().map(|r| bits(*r)).collect();
        let mut r = vec![0u64; kinds.len().max(results)];
        // SAFETY: the instance is live, `c` is its unit or null for a whole
        // module, and `r` has room for every result.
        let code = unsafe { (module.call)(inst, c, *index, a.as_ptr(), r.as_mut_ptr()) };
        if code != 0 {
            // SAFETY: the call has returned, so nothing else holds the `Ctx`.
            let host = unsafe { (*self.0).error.take() };
            return Err(message(code, host));
        }
        Ok(kinds.iter().zip(r).map(|(k, b)| raw(*k, b)).collect())
    }
    fn has_func_in(&mut self, unit: usize, name: &str) -> bool {
        self.unit(unit)
            .is_ok_and(|(_, _, tables)| tables.funcs.contains_key(name))
    }
    fn link(&mut self, key: &str, unit: std::sync::Arc<crate::Compiled>) -> Result<usize, String> {
        link_unit(self.0, key, &unit)
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
    unit: u32,
    index: u32,
    args: *const u64,
    results: *mut u64,
) -> i32 {
    let ctx = ctx.cast::<Ctx>();
    // SAFETY: `ctx` is the `Ctx` this instance was made with; the reference
    // ends with the statement.
    let (host, tables, numbered) = unsafe {
        let units = &(*ctx).units;
        match units.get(unit as usize) {
            Some(u) => ((*ctx).host.clone(), u.tables.clone(), u.numbered),
            None => ((*ctx).host.clone(), (*ctx).tables.clone(), 0),
        }
    };
    let Some((params, rets)) = tables.imports.get(index as usize) else {
        return HOST_TRAP;
    };
    let a: Args = params
        .iter()
        .enumerate()
        // SAFETY: the module passes one argument per parameter.
        .map(|(i, k)| raw(*k, unsafe { *args.add(i) }))
        .collect();
    match dispatch(&host, &mut view(ctx), numbered, index as usize, &a) {
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
    fn call_in(
        &mut self,
        unit: usize,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Vec<Raw>, String> {
        view(self.ctx).call_in(unit, name, args, results)
    }
    fn has_func_in(&mut self, unit: usize, name: &str) -> bool {
        view(self.ctx).has_func_in(unit, name)
    }
    fn link(&mut self, key: &str, unit: std::sync::Arc<crate::Compiled>) -> Result<usize, String> {
        link_unit(self.ctx, key, &unit)
    }
    fn read(&self, at: usize, len: usize) -> Vec<u8> {
        view(self.ctx).read(at, len)
    }
    fn read_into(&self, at: usize, out: &mut [u8]) {
        view(self.ctx).read_into(at, out);
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
    fn native(&self) -> bool {
        true
    }
}
