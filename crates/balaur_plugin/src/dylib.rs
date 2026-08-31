//! Loading an extension from a shared library.
//!
//! Everything that crosses before the check is `#[repr(C)]` and fixed-size:
//! reading a `String` out of a library built by another compiler is already
//! the undefined behaviour the check exists to prevent. Only once the tag
//! agrees does anything Rust-shaped cross.

use crate::Fingerprint;

#[cfg(feature = "dylib")]
pub(crate) const TAG_SYMBOL: &[u8] = b"balaur_plugin_abi";
#[cfg(feature = "dylib")]
pub(crate) const CREATE_SYMBOL: &[u8] = b"balaur_plugin_create";
const FIELD: usize = 48;

/// The build a library was compiled for, in a shape both sides agree on
/// without allocating or dereferencing anything.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AbiTag {
    pub rustc: [u8; FIELD],
    pub engine: [u8; FIELD],
    pub registry_abi: u32,
}

impl AbiTag {
    #[must_use]
    pub fn current() -> Self {
        let current = Fingerprint::current();
        Self {
            rustc: fixed(&current.rustc),
            engine: fixed(&current.engine),
            registry_abi: current.registry_abi,
        }
    }

    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            rustc: unfixed(&self.rustc),
            engine: unfixed(&self.engine),
            registry_abi: self.registry_abi,
        }
    }
}

fn fixed(text: &str) -> [u8; FIELD] {
    let mut out = [0u8; FIELD];
    let bytes = text.as_bytes();
    let take = bytes.len().min(FIELD);
    out[..take].copy_from_slice(&bytes[..take]);
    out
}

fn unfixed(bytes: &[u8; FIELD]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(FIELD);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[must_use]
pub const fn library_suffix() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// Emit the two symbols `load_extension` looks for.
///
/// The type must implement `Default`; an extension is constructed by the host,
/// not by its own `main`.
#[macro_export]
macro_rules! export_plugin {
    ($plugin:ty) => {
        #[no_mangle]
        pub extern "C" fn balaur_plugin_abi() -> $crate::AbiTag {
            $crate::AbiTag::current()
        }

        #[no_mangle]
        pub extern "C" fn balaur_plugin_create() -> *mut ::std::boxed::Box<dyn $crate::Plugin> {
            let plugin: ::std::boxed::Box<dyn $crate::Plugin> =
                ::std::boxed::Box::new(<$plugin as ::std::default::Default>::default());
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(plugin))
        }
    };
}
