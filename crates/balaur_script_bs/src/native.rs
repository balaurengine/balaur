// A game's `.bs` scripts as native code: an export writes their C, and the
// backend loads the framework built from it, when there is one, at start.

#[cfg(feature = "c")]
use crate::host::{BsHost, key_of};
#[cfg(feature = "c")]
use anyhow::{Context, Result, anyhow};

/// The framework the native build goes by, in the app's `Frameworks`.
pub const NATIVE_FRAMEWORK: &str = "BalaurScripts";

/// The `.bs` files among `scripts` as one C file, compiled as this game's
/// runtime compiles them, and how many there are; `None` when there are none
/// or the engine runs no `.bs`.
///
/// # Errors
/// A script that is not text or does not compile.
#[cfg(feature = "c")]
pub fn native_source(
    eng: &balaur_core::Engine,
    scripts: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<Option<(String, usize)>> {
    let Some(host) = BsHost::of(eng) else {
        return Ok(None);
    };
    let mut modules = vec![];
    for (rel, bytes) in scripts.iter().filter(|(rel, _)| BsHost::claims(rel)) {
        let source = std::str::from_utf8(bytes).with_context(|| format!("{rel} is not text"))?;
        let (compiled, _) = host.compile_source(&key_of(rel), source)?;
        modules.push(compiled.wasm);
    }
    if modules.is_empty() {
        return Ok(None);
    }
    let wasm: Vec<&[u8]> = modules.iter().map(Vec::as_slice).collect();
    let c = balaur_bs::c::translate(&wasm).map_err(|e| anyhow!("translating to C: {e}"))?;
    Ok(Some((c, modules.len())))
}

/// Opens a native build at `path`, as a game finds one beside it at start;
/// how many modules it holds.
///
/// # Errors
/// A library that does not open, or one built for another runtime.
#[cfg(feature = "native")]
pub fn load_native(path: &std::path::Path) -> anyhow::Result<usize> {
    balaur_bs::runtime::load_native(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// Whether a native build runs this game's scripts: one beside the
/// executable, or one loaded by hand.
pub(crate) fn any_native() -> bool {
    #[cfg(feature = "native")]
    return load_beside_executable() || balaur_bs::runtime::native_loaded();
    #[cfg(not(feature = "native"))]
    false
}

pub(crate) fn has_native(wasm: &[u8]) -> bool {
    #[cfg(feature = "native")]
    return balaur_bs::runtime::has_native(wasm);
    #[cfg(not(feature = "native"))]
    {
        let _ = wasm;
        false
    }
}

/// Loads the native build beside the executable, once a process, when there
/// is one; whether any is loaded.
#[cfg(feature = "native")]
pub(crate) fn load_beside_executable() -> bool {
    static LOADED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LOADED.get_or_init(|| {
        let Some(path) = std::env::current_exe().ok().and_then(|exe| {
            let dir = exe.parent()?.join("Frameworks");
            Some(
                dir.join(format!("{NATIVE_FRAMEWORK}.framework"))
                    .join(NATIVE_FRAMEWORK),
            )
        }) else {
            return false;
        };
        if !path.is_file() {
            return false;
        }
        match balaur_bs::runtime::load_native(&path) {
            Ok(n) => {
                tracing::info!("{n} script(s) run as native code");
                true
            }
            Err(e) => {
                tracing::error!("{}: {e}; scripts run interpreted", path.display());
                false
            }
        }
    })
}
