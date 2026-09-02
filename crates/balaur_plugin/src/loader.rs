//! Opening a shared library and taking the plugin out of it.

use std::ffi::OsStr;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::capi::{
    static_text, BalaurApi, BalaurRegistry, CExtension, BALAUR_ABI_VERSION, C_ABI_SYMBOL,
    C_DECLARE_SYMBOL, C_NAME_SYMBOL, C_VERSION_SYMBOL,
};
use crate::dylib::{AbiTag, CREATE_SYMBOL, TAG_SYMBOL};
use crate::{library_suffix, Fingerprint, Manifest, Plugin};

/// A plugin and the library it came from.
///
/// The library is never unloaded. Its closures and vtables outlive this
/// value in the engine and the script host's thread-locals, and Linux really
/// unmaps a closed library where macOS only pretends to.
pub struct Extension {
    plugin: Box<dyn Plugin>,
    path: PathBuf,
    _library: ManuallyDrop<libloading::Library>,
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
/// Two kinds of library are accepted, distinguished by which symbols they
/// export. A Rust extension exports `balaur_plugin_abi` and hands over a
/// `Box<dyn Plugin>`, which requires the identical compiler on both sides. A
/// C extension exports `balaur_extension_abi` and speaks only `#[repr(C)]`,
/// which any language can produce -- see [`crate::capi`].
///
/// # Errors
/// If the library will not open, exports neither entry point, or disagrees
/// about the build (Rust) or the ABI version (C).
///
/// # Safety
/// Loading any shared library runs its initialisers. This one additionally
/// trusts the symbols to have the signatures their macro or header gives
/// them, which the version check is what makes reasonable.
pub unsafe fn load_extension(path: &Path) -> Result<Extension> {
    let library = unsafe { libloading::Library::new(path) }
        .with_context(|| format!("opening {}", path.display()))?;

    let plugin = if let Ok(tag_fn) =
        unsafe { library.get::<unsafe extern "C" fn() -> AbiTag>(TAG_SYMBOL) }
    {
        unsafe { rust_plugin(path, &library, &tag_fn) }?
    } else if let Ok(abi_fn) = unsafe { library.get::<unsafe extern "C" fn() -> u32>(C_ABI_SYMBOL) }
    {
        unsafe { c_plugin(path, &library, &abi_fn) }?
    } else {
        bail!(
            "{} is not a balaur extension: it exports neither {} nor {}",
            path.display(),
            String::from_utf8_lossy(TAG_SYMBOL),
            String::from_utf8_lossy(C_ABI_SYMBOL),
        );
    };

    Ok(Extension {
        plugin,
        path: path.to_path_buf(),
        _library: ManuallyDrop::new(library),
    })
}

/// The Rust path: refuse the build before anything Rust-shaped crosses.
unsafe fn rust_plugin(
    path: &Path,
    library: &libloading::Library,
    tag_fn: &unsafe extern "C" fn() -> AbiTag,
) -> Result<Box<dyn Plugin>> {
    refuse_mismatch(path, &unsafe { tag_fn() }.fingerprint())?;

    let create: libloading::Symbol<'_, unsafe extern "C" fn() -> *mut Box<dyn Plugin>> =
        unsafe { library.get(CREATE_SYMBOL) }
            .map_err(|_| anyhow!("{} declares an abi tag but no constructor", path.display()))?;
    let raw = unsafe { create() };
    if raw.is_null() {
        bail!("{} returned no plugin", path.display());
    }
    Ok(*unsafe { Box::from_raw(raw) })
}

/// The C path: one version number, then four symbols and no Rust types.
unsafe fn c_plugin(
    path: &Path,
    library: &libloading::Library,
    abi_fn: &unsafe extern "C" fn() -> u32,
) -> Result<Box<dyn Plugin>> {
    let declared = unsafe { abi_fn() };
    if declared != BALAUR_ABI_VERSION {
        bail!(
            "cannot load {}: built against balaur c abi {declared}, host speaks {}",
            path.display(),
            BALAUR_ABI_VERSION
        );
    }

    let name = unsafe { c_string(library, path, C_NAME_SYMBOL) }?;
    let version = unsafe { c_string(library, path, C_VERSION_SYMBOL) }?;

    let declare: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const BalaurApi, *mut BalaurRegistry) -> i32,
    > = unsafe { library.get(C_DECLARE_SYMBOL) }.map_err(|_| {
        anyhow!(
            "{} declares a c abi version but no {}",
            path.display(),
            String::from_utf8_lossy(C_DECLARE_SYMBOL)
        )
    })?;

    Ok(Box::new(unsafe {
        CExtension::new(&name, &version, *declare)
    }))
}

unsafe fn c_string(library: &libloading::Library, path: &Path, symbol: &[u8]) -> Result<String> {
    let getter: libloading::Symbol<'_, unsafe extern "C" fn() -> *const std::ffi::c_char> =
        unsafe { library.get(symbol) }.map_err(|_| {
            anyhow!(
                "{} is missing {}",
                path.display(),
                String::from_utf8_lossy(symbol)
            )
        })?;
    unsafe { static_text(getter()) }.ok_or_else(|| {
        anyhow!(
            "{} returned an unreadable {}",
            path.display(),
            String::from_utf8_lossy(symbol)
        )
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
