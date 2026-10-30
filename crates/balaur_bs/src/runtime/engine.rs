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
    fn memory_len(&self) -> usize;
    fn write(&mut self, at: usize, bytes: &[u8]);
    fn global(&mut self, name: &str) -> Option<i32>;
    fn set_global(&mut self, name: &str, value: i32);
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
    /// [`Wasm::call`], except that a debugger stop parks the call for
    /// [`Wasm::resume`] instead of answering it.
    fn call_resumable(
        &mut self,
        name: &str,
        args: &[Raw],
        results: usize,
    ) -> Result<Outcome, String> {
        self.call(name, args, results).map(Outcome::Done)
    }
    fn resume(&mut self) -> Result<Outcome, String> {
        Err("this engine cannot carry on a parked call".into())
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
    fn write(&mut self, at: usize, bytes: &[u8]);
    fn global(&mut self, name: &str) -> Option<i32>;
    fn set_global(&mut self, name: &str, value: i32);
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
    pub key: &'a str,
    pub linked: &'a [(String, std::sync::Arc<crate::Compiled>)],
}

/// Where each unit of a linked script puts its static data, its own first,
/// then `linked`'s in order; and where its heap starts after them all.
pub(crate) fn unit_layout(
    meta: &Meta,
    linked: &[(String, std::sync::Arc<crate::Compiled>)],
) -> (Vec<u32>, u32) {
    let lens = std::iter::once(meta)
        .chain(linked.iter().map(|(_, c)| &c.meta))
        .map(|m| m.unit.as_ref().map_or(0, |u| u.data_len));
    let mut at = crate::codegen::DATA_START;
    let mut bases = vec![];
    for len in lens {
        bases.push(at);
        at = (at + len).next_multiple_of(16);
    }
    (bases, at)
}

#[cfg_attr(
    not(any(native, all(target_arch = "wasm32", feature = "web"))),
    allow(
        clippy::needless_pass_by_value,
        reason = "the native and browser engines take the setup itself"
    )
)]
pub(crate) fn instantiate(kind: EngineKind, setup: Setup<'_>) -> Result<Box<dyn Wasm>, String> {
    // A unit links with the units it uses, which wasmtime and wasmi do.
    if setup.meta.unit.is_some() {
        return match kind {
            #[cfg(jit)]
            EngineKind::Wasmtime | EngineKind::Auto => super::wasmtime_engine::instantiate(&setup),
            #[cfg(feature = "wasmi")]
            EngineKind::Wasmi => super::wasmi_engine::instantiate(&setup),
            #[allow(
                unreachable_patterns,
                reason = "Auto lands here only when no faster engine is compiled in"
            )]
            #[cfg(feature = "wasmi")]
            EngineKind::Auto => super::wasmi_engine::instantiate(&setup),
            #[allow(unreachable_patterns, reason = "every engine but these two")]
            _ => Err("a script built as units runs on wasmtime or wasmi".into()),
        };
    }
    // A native build beats every engine, when one has this module.
    #[cfg(native)]
    if matches!(kind, EngineKind::Auto | EngineKind::Native) {
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
