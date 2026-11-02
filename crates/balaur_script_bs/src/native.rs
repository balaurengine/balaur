// A game's `.bs` scripts as native code: an export writes their C, and the
// backend loads the library built from it, when there is one, at start.

#[cfg(feature = "c")]
use crate::host::{BsHost, key_of};
#[cfg(feature = "c")]
use anyhow::{Context, Result, anyhow};

/// The `.bs` files among `scripts` as one C file, each file's unit compiled
/// as this game's runtime compiles it, and how many there are; `None` when
/// there are none or the engine runs no `.bs`.
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
    // The instance a project's units share starts from a unit of its own.
    let mut modules = vec![host.project_unit()?.wasm];
    for (rel, bytes) in scripts.iter().filter(|(rel, _)| BsHost::claims(rel)) {
        let source = std::str::from_utf8(bytes).with_context(|| format!("{rel} is not text"))?;
        modules.push(host.unit_of(&key_of(rel), source)?.wasm);
    }
    if modules.len() == 1 {
        return Ok(None);
    }
    let wasm: Vec<&[u8]> = modules.iter().map(Vec::as_slice).collect();
    let c = balaur_bs::c::translate(&wasm).map_err(|e| anyhow!("translating to C: {e}"))?;
    Ok(Some((c, modules.len() - 1)))
}

/// Opens a native build at `path`, as a game finds one beside it at start;
/// how many modules it holds.
///
/// # Errors
/// A library that does not open, or one built for another runtime.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn load_native(path: &std::path::Path) -> anyhow::Result<usize> {
    balaur_bs::runtime::load_native(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

pub(crate) fn has_native(wasm: &[u8]) -> bool {
    #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
    return balaur_bs::runtime::has_native(wasm);
    #[cfg(not(all(feature = "native", not(target_arch = "wasm32"))))]
    {
        let _ = wasm;
        false
    }
}

/// Loads the native build beside the executable, once a process, when there
/// is one; whether any is loaded.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub(crate) fn load_beside_executable() -> bool {
    use balaur_core::standalone::{native_scripts_beside, native_scripts_file};
    static LOADED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LOADED.get_or_init(|| {
        let os = std::env::consts::OS;
        // Android opens it by name from the app's own libraries.
        let path = if os == "android" {
            std::path::PathBuf::from(native_scripts_file(os))
        } else {
            let Some(path) = std::env::current_exe()
                .ok()
                .map(|exe| native_scripts_beside(&exe, os))
            else {
                return false;
            };
            if !path.is_file() {
                return false;
            }
            path
        };
        match balaur_bs::runtime::load_native(&path) {
            Ok(n) => {
                tracing::info!("the scripts run as native code: {n} module(s)");
                true
            }
            Err(e) if os == "android" && e.contains("not found") => false,
            Err(e) => {
                tracing::error!("{}: {e}; scripts run interpreted", path.display());
                false
            }
        }
    })
}
