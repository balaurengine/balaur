//! Valve's `steam_api`, opened at run time from beside the game's executable.

use std::path::{Path, PathBuf};

/// The library's name as Valve ships it for this platform.
pub const FILE: &str = if cfg!(target_os = "windows") {
    "steam_api64.dll"
} else if cfg!(target_os = "macos") {
    "libsteam_api.dylib"
} else {
    "libsteam_api.so"
};

/// An open `steam_api`. Its symbols live as long as it does.
pub struct Library {
    #[cfg(not(target_family = "wasm"))]
    inner: Inner,
}

#[cfg(not(target_family = "wasm"))]
enum Inner {
    Opened(libloading::Library),
    /// A test's functions by symbol name, NUL and all, in place of Valve's.
    #[cfg(test)]
    Fake(std::collections::BTreeMap<&'static [u8], usize>),
}

impl Library {
    /// Open the library at `path`.
    ///
    /// # Safety
    /// Opening runs the library's initialisers: `path` is Valve's `steam_api`.
    #[cfg(not(target_family = "wasm"))]
    pub unsafe fn open(path: &Path) -> Result<Self, String> {
        // SAFETY: the caller names Valve's library.
        let inner = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
        Ok(Self {
            inner: Inner::Opened(inner),
        })
    }

    /// A library of a test's own functions, each under the symbol it stands
    /// in for (`b"SteamAPI_InitFlat\0"`).
    #[cfg(all(test, not(target_family = "wasm")))]
    pub(crate) fn fake(symbols: std::collections::BTreeMap<&'static [u8], usize>) -> Self {
        Self {
            inner: Inner::Fake(symbols),
        }
    }

    /// A browser has no dlopen.
    ///
    /// # Safety
    /// None needed; it never opens anything.
    #[cfg(target_family = "wasm")]
    pub unsafe fn open(_path: &Path) -> Result<Self, String> {
        Err("a browser opens no library".to_string())
    }

    /// The symbol `name` (NUL-terminated), or None where the library lacks it.
    ///
    /// # Safety
    /// `T` is the symbol's own function pointer type.
    #[cfg(not(target_family = "wasm"))]
    pub(crate) unsafe fn sym<T: Copy>(&self, name: &[u8]) -> Option<T> {
        match &self.inner {
            // SAFETY: the caller spells the symbol's type.
            Inner::Opened(library) => unsafe { library.get::<T>(name).ok().map(|symbol| *symbol) },
            #[cfg(test)]
            Inner::Fake(symbols) => symbols.get(name).map(|&address| {
                assert_eq!(size_of::<T>(), size_of::<usize>(), "a function pointer");
                // SAFETY: the test put a function of the symbol's own type there.
                unsafe { std::mem::transmute_copy::<usize, T>(&address) }
            }),
        }
    }

    /// # Safety
    /// None needed; there is no library.
    #[cfg(target_family = "wasm")]
    pub(crate) unsafe fn sym<T: Copy>(&self, _name: &[u8]) -> Option<T> {
        None
    }
}

/// Where a game's `steam_api` sits: beside its executable, which in a macOS
/// `.app` is `Contents/MacOS`. None on a phone or in a browser, which Valve
/// ships no library for.
pub fn beside_executable() -> Option<PathBuf> {
    if !cfg!(desktop) {
        return None;
    }
    Some(std::env::current_exe().ok()?.parent()?.join(FILE))
}
