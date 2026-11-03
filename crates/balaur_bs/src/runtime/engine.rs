// What the runtime needs from a WebAssembly engine, so wasmi, wasmtime and a
// browser's own engine all run the same module through the same host calls.

use super::host::HostState;
use crate::Meta;
use std::cell::RefCell;
use std::rc::Rc;

/// A host call's results: nearly always one slot or none, so kept inline.
pub(crate) type Raws = smallvec::SmallVec<[Raw; 4]>;
/// A host call's arguments, inline for the common counts.
pub(crate) type Args = smallvec::SmallVec<[Raw; 8]>;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Raw {
    I32(i32),
    I64(i64),
    F64(f64),
}

impl Raw {
    pub(crate) fn i32(self) -> i32 {
        match self {
            Raw::I32(x) => x,
            Raw::I64(x) => x as i32,
            Raw::F64(x) => x as i32,
        }
    }
    pub(crate) fn bytes(self) -> [u8; 8] {
        match self {
            Raw::I32(x) => i64::from(x).to_le_bytes(),
            Raw::I64(x) => x.to_le_bytes(),
            Raw::F64(x) => x.to_le_bytes(),
        }
    }
}

/// An instantiated module.
pub(crate) trait Wasm {
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String>;
    fn has_func(&mut self, name: &str) -> bool;
    /// [`Wasm::call`] on unit `unit` of a script built as units, made now if
    /// this is its first call; unit 0 is the script's own.
    fn call_in(
        &mut self,
        unit: usize,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Vec<Raw>, String> {
        if unit != 0 {
            return Err("this script is not built as units".into());
        }
        self.call(name, args, results)
    }
    /// Whether unit `unit` exports `name`.
    fn has_func_in(&mut self, unit: usize, name: &str) -> bool {
        unit == 0 && self.has_func(name)
    }
    /// The unit `key` as a unit of this script, added unless it is one: its
    /// index. Its static data takes a block of the heap, since the space
    /// before the heap was laid out for the units known when it was made.
    fn link(&mut self, key: &str, _unit: std::sync::Arc<crate::Compiled>) -> Result<usize, String> {
        Err(format!("{key}: this script is not built as units"))
    }
    fn read(&self, at: usize, len: usize) -> Vec<u8>;
    /// `out.len()` bytes at `at` into `out`, zeros past the end: the read a
    /// word of the module's memory wants, allocating nothing.
    fn read_into(&self, at: usize, out: &mut [u8]) {
        out.copy_from_slice(&self.read(at, out.len()));
    }
    fn memory_len(&self) -> usize;
    fn write(&mut self, at: usize, bytes: &[u8]);
    fn global(&mut self, name: &str) -> Option<i32>;
    fn set_global(&mut self, name: &str, value: i32);
    /// Global `name` of unit `unit`'s own, once it is made.
    fn global_in(&mut self, unit: usize, name: &str) -> Option<i32> {
        if unit == 0 { self.global(name) } else { None }
    }
    /// Unit `unit` made now, unless it is.
    fn make(&mut self, _unit: usize) -> Result<(), String> {
        Ok(())
    }
    fn set_fuel(&mut self, fuel: u64);
    /// What the engine's fuel has left, on one that meters it.
    fn fuel(&mut self) -> Option<u64> {
        None
    }
    /// Whether a call can stop partway and carry on later: what a debugger
    /// needs to hold a script at a line while the frame loop runs.
    fn can_park(&self) -> bool {
        false
    }
    /// [`Wasm::call_in`], except that a debugger stop parks the call for
    /// [`Wasm::resume`] instead of answering it.
    fn call_resumable(
        &mut self,
        unit: usize,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Outcome, String> {
        self.call_in(unit, name, args, results).map(Outcome::Done)
    }
    fn resume(&mut self) -> Result<Outcome, String> {
        Err("this engine cannot carry on a parked call".into())
    }
    /// Whether a native build runs it.
    fn native(&self) -> bool {
        false
    }
}

/// How a resumable call came back.
pub(crate) enum Outcome {
    Done(Vec<Raw>),
    #[cfg_attr(
        not(feature = "wasmi"),
        allow(dead_code, reason = "only wasmi parks a call")
    )]
    Parked,
}

/// The module, as a host function sees it while it runs.
pub(crate) trait ImportCtx {
    fn read(&self, at: usize, len: usize) -> Vec<u8>;
    /// [`Wasm::read_into`], as a host function sees the module.
    fn read_into(&self, at: usize, out: &mut [u8]) {
        out.copy_from_slice(&self.read(at, out.len()));
    }
    fn write(&mut self, at: usize, bytes: &[u8]);
    fn global(&mut self, name: &str) -> Option<i32>;
    fn set_global(&mut self, name: &str, value: i32);
    /// [`Wasm::global_in`], as a host function sees the module.
    fn global_in(&mut self, unit: usize, name: &str) -> Option<i32> {
        if unit == 0 { self.global(name) } else { None }
    }
    /// [`Wasm::make`], as a host function sees the module.
    fn make(&mut self, _unit: usize) -> Result<(), String> {
        Ok(())
    }
    /// One of the module's own exports, called from inside a host function:
    /// how a host function allocates the string or array it hands back.
    fn call(&mut self, name: &str, args: &[Raw], results: usize) -> Result<Vec<Raw>, String>;
    fn has_func(&mut self, name: &str) -> bool;
    /// [`ImportCtx::call`] on unit `unit` of a script built as units: a host
    /// function reaching another script of the same instance.
    fn call_in(
        &mut self,
        unit: usize,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Vec<Raw>, String> {
        if unit != 0 {
            return Err("this script is not built as units".into());
        }
        self.call(name, args, results)
    }
    fn has_func_in(&mut self, unit: usize, name: &str) -> bool {
        unit == 0 && self.has_func(name)
    }
    /// [`Wasm::link`] from inside a host function: a script loaded while
    /// another runs.
    fn link(&mut self, key: &str, _unit: std::sync::Arc<crate::Compiled>) -> Result<usize, String> {
        Err(format!("{key}: this script is not built as units"))
    }
    /// `size` bytes from the module's own allocator, for the host to fill:
    /// what a string or an array it hands back is written into.
    fn alloc(&mut self, size: usize) -> Result<usize, String> {
        let returned = self.call(crate::words::ALLOC, &[Raw::I32(size as i32)], 1)?;
        Ok(returned
            .first()
            .map_or(0, |pointer| pointer.i32() as u32 as usize))
    }
}

/// `data[at..]` into `out`, or zeros where `data` ends first.
pub(crate) fn copy_from(data: &[u8], at: usize, out: &mut [u8]) {
    match data.get(at..at + out.len()) {
        Some(bytes) => out.copy_from_slice(bytes),
        None => out.fill(0),
    }
}

/// Reads past the end of memory come back as zeros, as an unwritten slot would.
#[cfg(any(feature = "wasmi", native, jit))]
pub(crate) fn read_from(data: &[u8], at: usize, len: usize) -> Vec<u8> {
    data.get(at..at + len)
        .map_or_else(|| vec![0; len], <[u8]>::to_vec)
}

pub(crate) type Shared = Rc<RefCell<HostState>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EngineKind {
    /// The best this build has: wasmtime when compiled in, the browser's own
    /// engine on the web, wasmi otherwise.
    #[default]
    Auto,
    #[cfg(feature = "wasmi")]
    Wasmi,
    #[cfg(jit)]
    Wasmtime,
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    Browser,
    /// The native build an export made, loaded with
    /// [`load_native`](super::load_native); an error when it lacks the module.
    #[cfg(native)]
    Native,
}

pub(crate) struct Setup<'a> {
    pub wasm: &'a [u8],
    pub meta: &'a Meta,
    pub host: Shared,
    pub fuel: Option<u64>,
    pub memory_bytes: Option<usize>,
    /// A unit's file key and the units of the modules it uses.
    #[cfg_attr(
        not(any(
            jit,
            native,
            feature = "wasmi",
            all(target_arch = "wasm32", feature = "web")
        )),
        allow(dead_code, reason = "only an engine that links units reads it")
    )]
    pub key: &'a str,
    #[cfg_attr(
        not(any(
            jit,
            native,
            feature = "wasmi",
            all(target_arch = "wasm32", feature = "web")
        )),
        allow(dead_code, reason = "only an engine that links units reads them")
    )]
    pub linked: &'a [(String, std::sync::Arc<crate::Compiled>)],
    /// Whether a loaded native build may run it ([`super::Limits::skip_native`]).
    #[cfg_attr(
        not(native),
        allow(dead_code, reason = "only a build that loads native code reads it")
    )]
    pub native: bool,
}

/// Where a linked script keeps things in its memory.
#[cfg(any(
    jit,
    native,
    feature = "wasmi",
    all(target_arch = "wasm32", feature = "web")
))]
pub(crate) struct Layout {
    /// Each unit's static data, its own first, then `linked`'s in order.
    pub bases: Vec<u32>,
    /// The shadow stack its debug units share, untouched without one.
    pub stack: u32,
    /// Where its heap starts, after them all.
    pub heap: u32,
}

#[cfg(any(
    jit,
    native,
    feature = "wasmi",
    all(target_arch = "wasm32", feature = "web")
))]
pub(crate) fn unit_layout(
    meta: &Meta,
    linked: &[(String, std::sync::Arc<crate::Compiled>)],
) -> Layout {
    let lens = std::iter::once(meta)
        .chain(linked.iter().map(|(_, c)| &c.meta))
        .map(|m| m.unit.as_ref().map_or(0, |u| u.data_len));
    let mut at = crate::codegen::DATA_START;
    let mut bases = vec![];
    for len in lens {
        bases.push(at);
        at = (at + len).next_multiple_of(16);
    }
    Layout {
        bases,
        stack: at,
        heap: at + crate::codegen::SHADOW_STACK,
    }
}

/// Where a global every unit of a linked script shares sits among them, as
/// an engine makes them: [`crate::words::UNIT_GLOBALS`], then
/// [`crate::words::COUNT_GLOBALS`].
#[cfg(any(jit, feature = "wasmi", all(target_arch = "wasm32", feature = "web")))]
pub(crate) fn shared_global(name: &str) -> Option<usize> {
    shared_globals().position(|global| global == name)
}

#[cfg(any(jit, feature = "wasmi", all(target_arch = "wasm32", feature = "web")))]
fn shared_globals() -> impl Iterator<Item = &'static str> {
    use crate::words::{COUNT_GLOBALS, UNIT_GLOBALS};
    UNIT_GLOBALS.into_iter().chain(COUNT_GLOBALS)
}

/// What each shared global starts at, in [`shared_global`]'s order.
#[cfg(any(jit, feature = "wasmi", all(target_arch = "wasm32", feature = "web")))]
pub(crate) fn shared_starts(layout: &Layout) -> Vec<i32> {
    use crate::words::{BUMP, DBG_BASE, DBG_SP, FUEL};
    shared_globals()
        .map(|name| match name {
            BUMP => layout.heap as i32,
            DBG_SP | DBG_BASE => layout.stack as i32,
            FUEL => i32::MAX,
            _ => 0,
        })
        .collect()
}

/// Where a unit's own parts are in the script it joined.
#[cfg(any(jit, feature = "wasmi", all(target_arch = "wasm32", feature = "web")))]
pub(crate) struct Bases {
    /// Its index in the instance.
    pub index: u32,
    /// Its static data, its own `fn` table slots and its link slots.
    pub memory: u32,
    pub table: u32,
    pub link: u32,
    /// What a direct unit passes the engine's entry points.
    pub host_ctx: u32,
}

/// The value of a global that is a unit's own: `None` for one its script's
/// units share.
#[cfg(any(jit, feature = "wasmi", all(target_arch = "wasm32", feature = "web")))]
pub(crate) fn own_global(name: &str, bases: &Bases, unit: &crate::Unit) -> Option<u32> {
    use crate::words;
    match name {
        words::MEMORY_BASE => Some(bases.memory),
        words::TABLE_BASE => Some(bases.table),
        words::LINK_BASE => Some(bases.link),
        words::DBG_BREAKS => Some(bases.memory + unit.breaks_at),
        words::PROF => Some(bases.memory + unit.rows_at),
        words::DBG_UNIT => Some(bases.index),
        words::HOST_CTX => Some(bases.host_ctx),
        _ => None,
    }
}

#[cfg_attr(
    not(any(native, all(target_arch = "wasm32", feature = "web"))),
    allow(
        clippy::needless_pass_by_value,
        reason = "the native and browser engines take the setup itself"
    )
)]
pub(crate) fn instantiate(kind: EngineKind, setup: Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    // A unit links with the units it uses: natively when a loaded build has
    // every one, else on the engine asked for.
    #[cfg(native)]
    if setup.native
        && setup.meta.unit.is_some()
        && matches!(kind, EngineKind::Auto | EngineKind::Native)
    {
        if super::native_engine::has_units(&setup) {
            return super::native_engine::instantiate_linked(setup);
        }
        if kind == EngineKind::Native {
            return Err("no loaded native build has every unit of this script".into());
        }
    }
    // A native build beats every engine, when one has this module.
    #[cfg(native)]
    if setup.native
        && setup.meta.unit.is_none()
        && matches!(kind, EngineKind::Auto | EngineKind::Native)
    {
        if let Some(module) = super::native_engine::find(setup.wasm) {
            return super::native_engine::instantiate(module, setup);
        }
        if kind == EngineKind::Native {
            return Err("no loaded native build has this module".into());
        }
    }
    match kind {
        #[cfg(jit)]
        EngineKind::Wasmtime | EngineKind::Auto => super::wasmtime_engine::instantiate(&setup),
        #[cfg(all(target_arch = "wasm32", feature = "web"))]
        EngineKind::Browser => super::web_engine::instantiate(setup),
        #[cfg(all(target_arch = "wasm32", feature = "web", not(jit)))]
        EngineKind::Auto => super::web_engine::instantiate(setup),
        #[cfg(feature = "wasmi")]
        EngineKind::Wasmi => super::wasmi_engine::instantiate(&setup),
        #[allow(
            unreachable_patterns,
            reason = "Auto lands here only when no faster engine is compiled in"
        )]
        #[cfg(feature = "wasmi")]
        EngineKind::Auto => super::wasmi_engine::instantiate(&setup),
        #[allow(
            unreachable_patterns,
            reason = "only a build without any engine reaches this"
        )]
        _ => {
            let _ = setup;
            Err("this build has no WebAssembly engine: enable `wasmi`, `wasmtime` or `web`".into())
        }
    }
}
