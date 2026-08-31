//! Opening a shared library and taking the plugin out of it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::dylib::{AbiTag, CREATE_SYMBOL, TAG_SYMBOL};
use crate::{library_suffix, Fingerprint, Manifest, Plugin};

/// A plugin and the library it came from.
///
/// The library is dropped last: unloading it while its code is still reachable
/// would leave the plugin's vtable pointing into unmapped memory.
pub struct Extension {
    plugin: Box<dyn Plugin>,
    path: PathBuf,
    _library: libloading::Library,
}

impl Extension {
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        self.plugin.manifest()
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn plugin_mut(&mut self) -> &mut dyn Plugin {
        self.plugin.as_mut()
    }
}

/// Open a shared library and take the plugin out of it.
///
/// # Errors
/// If the library will not open, lacks either symbol, or was built against a
/// different compiler, engine version or registry.
///
/// # Safety
/// Loading any shared library runs its initialisers. This one additionally
/// trusts the two symbols to have the signatures `export_plugin!` gives them,
/// which the tag check is what makes reasonable.
pub unsafe fn load_extension(path: &Path) -> Result<Extension> {
    let library = unsafe { libloading::Library::new(path) }
        .with_context(|| format!("opening {}", path.display()))?;

    let tag_fn: libloading::Symbol<'_, unsafe extern "C" fn() -> AbiTag> =
        unsafe { library.get(TAG_SYMBOL) }.map_err(|_| {
            anyhow!(
                "{} is not a balaur extension: no {} symbol",
                path.display(),
                String::from_utf8_lossy(TAG_SYMBOL)
            )
        })?;
    refuse_mismatch(path, &unsafe { tag_fn() }.fingerprint())?;

    let create: libloading::Symbol<'_, unsafe extern "C" fn() -> *mut Box<dyn Plugin>> =
        unsafe { library.get(CREATE_SYMBOL) }
            .map_err(|_| anyhow!("{} declares an abi tag but no constructor", path.display()))?;
    let raw = unsafe { create() };
    if raw.is_null() {
        bail!("{} returned no plugin", path.display());
    }
    let plugin = *unsafe { Box::from_raw(raw) };

    Ok(Extension {
        plugin,
        path: path.to_path_buf(),
        _library: library,
    })
}

/// Refuse a library built for a different compiler, engine or registry.
///
/// Separate from `load_extension` so the decision can be tested: on macOS a
/// library whose bytes were edited is killed by the loader before any of this
/// runs, so a tampered file cannot stand in for an honestly mismatched build.
///
/// # Errors
/// If anything in the fingerprint disagrees.
pub fn refuse_mismatch(path: &Path, theirs: &Fingerprint) -> Result<()> {
    let differences = Fingerprint::current().differences(theirs);
    if differences.is_empty() {
        return Ok(());
    }
    bail!("cannot load {}: {}", path.display(), differences.join("; "))
}

/// Every extension in `dir`, in dependency order.
///
/// Discovery is sorted and the order comes from the manifests, because a
/// directory listing is not deterministic and load order decides registration
/// order.
///
/// # Errors
/// If a library fails to load, requires something absent, or the requirements
/// form a cycle.
///
/// # Safety
/// Each library is opened, which runs its initialisers.
pub unsafe fn load_extensions_in(dir: &Path) -> Result<Vec<Extension>> {
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(OsStr::to_str) == Some(library_suffix()))
            .collect(),
        Err(_) => return Ok(Vec::new()),
    };
    paths.sort();

    let mut loaded: Vec<Extension> = Vec::with_capacity(paths.len());
    for path in &paths {
        loaded.push(unsafe { load_extension(path) }?);
    }

    let manifests: Vec<Manifest> = loaded.iter().map(|e| e.manifest().clone()).collect();
    let order = crate::load_order(&manifests)?;
    let mut ordered = Vec::with_capacity(loaded.len());
    for name in order {
        let at = loaded
            .iter()
            .position(|e| e.manifest().name == name)
            .ok_or_else(|| anyhow!("`{name}` was ordered but is not loaded"))?;
        ordered.push(loaded.remove(at));
    }
    Ok(ordered)
}
