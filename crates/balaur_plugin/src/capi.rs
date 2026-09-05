//! The C ABI an extension written in another language binds against.
//!
//! The Rust extension path next door needs both sides built by the same rustc,
//! because Rust has no stable ABI. That rules out every language that is not
//! Rust, and it also rules out Rust built by a different compiler. This module
//! is the version without that constraint: an extension exports four C
//! symbols, receives a table of function pointers, and speaks only in
//! `#[repr(C)]` types. Odin, Zig, C and C++ can all write one.
//!
//! # The shape
//!
//! The extension exports:
//!
//! ```c
//! uint32_t    balaur_extension_abi(void);      // must equal BALAUR_ABI_VERSION
//! const char* balaur_extension_name(void);     // static, NUL-terminated
//! const char* balaur_extension_version(void);  // static, NUL-terminated
//! int32_t     balaur_extension_declare(const BalaurApi*, BalaurRegistry*);
//! ```
//!
//! The host hands `declare` a [`BalaurApi`] of function pointers rather than
//! expecting the extension to resolve symbols back into the executable, which
//! is unreliable across platforms and linkers.
//!
//! # The ownership rule, which is the whole design
//!
//! **No allocation ever crosses this boundary.** A [`BalaurValue`] is a
//! borrowed view that is valid only for the duration of the call it appears
//! in. Arguments point at memory the host owns; a value the extension writes
//! into `out` points at memory the extension owns, and the host copies it
//! before returning. Constants are copied at registration time.
//!
//! So there is no `balaur_value_free`, no ownership transfer, and no question
//! about which allocator released what. It is the same discipline
//! `Value::Callback` already uses for script functions, applied to everything.
//!
//! # Errors and panics
//!
//! A function returns `0` for success. On anything else the host raises an
//! error to the calling script; if the extension also wrote a string into
//! `out`, that string becomes the message.
//!
//! Every entry point here catches unwinding, because a Rust panic crossing
//! into C is undefined behaviour. The reverse is the extension's
//! responsibility: a C function that unwinds into Rust is equally undefined.

use std::ffi::c_void;
#[cfg(feature = "dylib")]
use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::{Result, bail};
use balaur_core::Engine;
use balaur_script::{Bindings, Value};

use crate::{Manifest, Plugin, Registry};

/// Bumped whenever anything in this file changes shape.
///
/// Checked before the host calls anything else in the library, so a stale
/// extension is refused with a message rather than misreading memory.
pub const BALAUR_ABI_VERSION: u32 = 1;

// Only the loader looks for these, and only it needs them named.
#[cfg(feature = "dylib")]
pub(crate) const C_ABI_SYMBOL: &[u8] = b"balaur_extension_abi";
#[cfg(feature = "dylib")]
pub(crate) const C_NAME_SYMBOL: &[u8] = b"balaur_extension_name";
#[cfg(feature = "dylib")]
pub(crate) const C_VERSION_SYMBOL: &[u8] = b"balaur_extension_version";
#[cfg(feature = "dylib")]
pub(crate) const C_DECLARE_SYMBOL: &[u8] = b"balaur_extension_declare";

pub const BALAUR_NIL: u32 = 0;
pub const BALAUR_BOOL: u32 = 1;
pub const BALAUR_INT: u32 = 2;
pub const BALAUR_NUM: u32 = 3;
pub const BALAUR_STR: u32 = 4;
pub const BALAUR_VEC2: u32 = 5;
pub const BALAUR_VEC3: u32 = 6;
pub const BALAUR_COLOR: u32 = 7;
pub const BALAUR_NODE: u32 = 8;
pub const BALAUR_CALLBACK: u32 = 9;
pub const BALAUR_LIST: u32 = 10;
pub const BALAUR_MAP: u32 = 11;
pub const BALAUR_MANY: u32 = 12;
/// Arbitrary bytes, in the `string` arm of the payload. A new tag rather than
/// a layout change, so an extension built against version 1 still reads every
/// value it already knew.
pub const BALAUR_BYTES: u32 = 13;

/// UTF-8 bytes, not NUL-terminated. Borrowed for the length of the call.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BalaurStr {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BalaurSlice {
    pub items: *const BalaurValue,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BalaurMapRef {
    pub items: *const BalaurEntry,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BalaurEntry {
    pub key: BalaurStr,
    pub value: BalaurValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union BalaurPayload {
    pub boolean: bool,
    pub integer: i64,
    pub number: f64,
    /// Node entity bits, or a callback id.
    pub bits: u64,
    /// `vec2` uses the first two lanes, `vec3` the first three, `color` all.
    pub vector: [f32; 4],
    pub string: BalaurStr,
    pub list: BalaurSlice,
    pub map: BalaurMapRef,
}

/// The neutral value, mirrored for C.
///
/// `kind` is one of the `BALAUR_*` constants and decides which arm of
/// `payload` is live. Reading any other arm is undefined, exactly as in C.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BalaurValue {
    pub kind: u32,
    pub reserved: u32,
    pub payload: BalaurPayload,
}

impl BalaurValue {
    #[must_use]
    pub const fn nil() -> Self {
        Self {
            kind: BALAUR_NIL,
            reserved: 0,
            payload: BalaurPayload { bits: 0 },
        }
    }
}

impl BalaurStr {
    fn borrowed(text: &str) -> Self {
        Self::borrowed_bytes(text.as_bytes())
    }

    fn borrowed_bytes(bytes: &[u8]) -> Self {
        Self {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    /// # Safety
    /// The pointer must be valid for `len` bytes for as long as the returned
    /// reference is used. The lifetime is unbounded, so callers must keep it
    /// inside the call that produced it.
    unsafe fn as_str<'a>(self) -> Option<&'a str> {
        std::str::from_utf8(unsafe { self.as_bytes() }).ok()
    }

    /// # Safety
    /// Same as [`Self::as_str`]: valid for `len` bytes for the returned
    /// reference's use.
    unsafe fn as_bytes<'a>(self) -> &'a [u8] {
        if self.ptr.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// Opaque handle to the registration a plugin is in the middle of.
pub struct BalaurRegistry {
    _private: [u8; 0],
}

/// Opaque handle to a named binding group.
pub struct BalaurModule {
    _private: [u8; 0],
}

/// What an extension implements and hands to `balaur_module_function`.
///
/// Returns `0` on success. `args` is borrowed for the length of the call, and
/// anything written to `out` must stay alive until it returns.
pub type BalaurFn = unsafe extern "C" fn(
    user: *mut c_void,
    args: *const BalaurValue,
    argc: usize,
    out: *mut BalaurValue,
) -> i32;

/// The host functions an extension may call, passed to `declare`.
///
/// A table rather than exported symbols: an extension resolving symbols back
/// into the host executable depends on the host being linked with its dynamic
/// symbols exported, which is neither the default nor portable.
#[repr(C)]
pub struct BalaurApi {
    pub abi_version: u32,
    pub reserved: u32,
    /// Open a named binding group. Null on failure.
    pub module_open: unsafe extern "C" fn(*mut BalaurRegistry, BalaurStr) -> *mut BalaurModule,
    pub module_function: unsafe extern "C" fn(*mut BalaurModule, BalaurStr, BalaurFn, *mut c_void),
    pub module_constant: unsafe extern "C" fn(*mut BalaurModule, BalaurStr, *const BalaurValue),
    /// Finish a group. Every opened module must be closed exactly once.
    pub module_close: unsafe extern "C" fn(*mut BalaurModule),
    /// Levels: 0 trace, 1 debug, 2 info, 3 warn, 4 error.
    pub log: unsafe extern "C" fn(u32, BalaurStr),
}

#[must_use]
pub fn host_api() -> BalaurApi {
    BalaurApi {
        abi_version: BALAUR_ABI_VERSION,
        reserved: 0,
        module_open,
        module_function,
        module_constant,
        module_close,
        log,
    }
}

/// A module handle is a double-boxed `Bindings`, so the pointer C holds is
/// thin. The inner box is the trait object.
type ModuleHandle = Box<dyn Bindings<Engine>>;

unsafe extern "C" fn module_open(
    registry: *mut BalaurRegistry,
    name: BalaurStr,
) -> *mut BalaurModule {
    let opened = catch_unwind(AssertUnwindSafe(|| {
        if registry.is_null() {
            return None;
        }
        let registry = unsafe { &mut *registry.cast::<Registry<'_>>() };
        let name = unsafe { name.as_str() }?;
        let bindings = registry.script_module(name).ok()?;
        Some(Box::into_raw(Box::new(bindings)).cast::<BalaurModule>())
    }));
    opened.ok().flatten().unwrap_or(std::ptr::null_mut())
}

unsafe extern "C" fn module_function(
    module: *mut BalaurModule,
    name: BalaurStr,
    function: BalaurFn,
    user: *mut c_void,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if module.is_null() {
            return;
        }
        let module = unsafe { &mut *module.cast::<ModuleHandle>() };
        let Some(name) = (unsafe { name.as_str() }) else {
            return;
        };
        let user = UserData(user);
        module.function_raw(
            name,
            Box::new(move |_engine, args| invoke(function, user, args)),
        );
    }));
}

unsafe extern "C" fn module_constant(
    module: *mut BalaurModule,
    name: BalaurStr,
    value: *const BalaurValue,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if module.is_null() || value.is_null() {
            return;
        }
        let module = unsafe { &mut *module.cast::<ModuleHandle>() };
        let Some(name) = (unsafe { name.as_str() }) else {
            return;
        };
        // Copied here, which is what makes the borrowed representation safe:
        // the extension's memory is not referenced after this returns.
        let Ok(owned) = (unsafe { from_c(&*value) }) else {
            return;
        };
        module.constant(name, owned);
    }));
}

unsafe extern "C" fn module_close(module: *mut BalaurModule) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !module.is_null() {
            drop(unsafe { Box::from_raw(module.cast::<ModuleHandle>()) });
        }
    }));
}

unsafe extern "C" fn log(level: u32, message: BalaurStr) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(message) = (unsafe { message.as_str() }) else {
            return;
        };
        match level {
            0 => tracing::trace!(target: "extension", "{message}"),
            1 => tracing::debug!(target: "extension", "{message}"),
            3 => tracing::warn!(target: "extension", "{message}"),
            4 => tracing::error!(target: "extension", "{message}"),
            _ => tracing::info!(target: "extension", "{message}"),
        }
    }));
}

/// A raw pointer the host never dereferences, only hands back. Wrapped so the
/// closure that captures it is expressible.
#[derive(Clone, Copy)]
struct UserData(*mut c_void);

/// Call into the extension, converting in and copying out.
fn invoke(function: BalaurFn, user: UserData, args: &[Value]) -> Result<Value> {
    let mut arena = Arena::default();
    let mut borrowed = Vec::with_capacity(args.len());
    for arg in args {
        borrowed.push(to_c(arg, &mut arena));
    }

    let mut out = BalaurValue::nil();
    let status = unsafe { function(user.0, borrowed.as_ptr(), borrowed.len(), &raw mut out) };

    // Copied before returning, while the extension's memory is still alive.
    let returned = unsafe { from_c(&out) };
    if status != 0 {
        let detail = match &returned {
            Ok(Value::Str(message)) if !message.is_empty() => message.clone(),
            _ => format!("returned status {status}"),
        };
        bail!("extension function failed: {detail}");
    }
    returned
}

/// Backing store for the borrowed views handed to an extension.
///
/// `Box<[T]>` keeps its contents at a fixed address when the box itself moves
/// into the vector, so a pointer taken before the push stays valid.
#[derive(Default)]
struct Arena {
    lists: Vec<Box<[BalaurValue]>>,
    maps: Vec<Box<[BalaurEntry]>>,
}

fn to_c(value: &Value, arena: &mut Arena) -> BalaurValue {
    let (kind, payload) = match value {
        Value::Nil => (BALAUR_NIL, BalaurPayload { bits: 0 }),
        Value::Bool(b) => (BALAUR_BOOL, BalaurPayload { boolean: *b }),
        Value::Int(i) => (BALAUR_INT, BalaurPayload { integer: *i }),
        Value::Num(n) => (BALAUR_NUM, BalaurPayload { number: *n }),
        Value::Str(s) => (
            BALAUR_STR,
            BalaurPayload {
                string: BalaurStr::borrowed(s),
            },
        ),
        Value::Bytes(bytes) => (
            BALAUR_BYTES,
            BalaurPayload {
                string: BalaurStr::borrowed_bytes(bytes),
            },
        ),
        Value::Vec2([x, y]) => (
            BALAUR_VEC2,
            BalaurPayload {
                vector: [*x, *y, 0.0, 0.0],
            },
        ),
        Value::Vec3([x, y, z]) => (
            BALAUR_VEC3,
            BalaurPayload {
                vector: [*x, *y, *z, 0.0],
            },
        ),
        Value::Color(rgba) => (BALAUR_COLOR, BalaurPayload { vector: *rgba }),
        Value::Node(bits) => (BALAUR_NODE, BalaurPayload { bits: *bits }),
        Value::Callback(id) => (BALAUR_CALLBACK, BalaurPayload { bits: id.0 }),
        Value::List(items) => (BALAUR_LIST, list_payload(items, arena)),
        Value::Many(items) => (BALAUR_MANY, list_payload(items, arena)),
        Value::Map(pairs) => {
            let mut entries = Vec::with_capacity(pairs.len());
            for (key, value) in pairs {
                entries.push(BalaurEntry {
                    key: BalaurStr::borrowed(key),
                    value: to_c(value, arena),
                });
            }
            let boxed = entries.into_boxed_slice();
            let (items, len) = (boxed.as_ptr(), boxed.len());
            arena.maps.push(boxed);
            (
                BALAUR_MAP,
                BalaurPayload {
                    map: BalaurMapRef { items, len },
                },
            )
        }
    };
    BalaurValue {
        kind,
        reserved: 0,
        payload,
    }
}

fn list_payload(items: &[Value], arena: &mut Arena) -> BalaurPayload {
    let mut children = Vec::with_capacity(items.len());
    for item in items {
        children.push(to_c(item, arena));
    }
    let boxed = children.into_boxed_slice();
    let (ptr, len) = (boxed.as_ptr(), boxed.len());
    arena.lists.push(boxed);
    BalaurPayload {
        list: BalaurSlice { items: ptr, len },
    }
}

/// Deep-copy a value out of the extension's memory.
///
/// # Safety
/// `value` must point at a well-formed `BalaurValue` whose pointers are valid
/// for their stated lengths.
unsafe fn from_c(value: &BalaurValue) -> Result<Value> {
    let payload = &value.payload;
    Ok(match value.kind {
        BALAUR_NIL => Value::Nil,
        BALAUR_BOOL => Value::Bool(unsafe { payload.boolean }),
        BALAUR_INT => Value::Int(unsafe { payload.integer }),
        BALAUR_NUM => Value::Num(unsafe { payload.number }),
        BALAUR_STR => {
            let text = unsafe { payload.string.as_str() }
                .ok_or_else(|| anyhow::anyhow!("extension returned a string that is not UTF-8"))?;
            Value::Str(text.to_string())
        }
        BALAUR_BYTES => Value::Bytes(unsafe { payload.string.as_bytes() }.to_vec()),
        BALAUR_VEC2 => {
            let v = unsafe { payload.vector };
            Value::Vec2([v[0], v[1]])
        }
        BALAUR_VEC3 => {
            let v = unsafe { payload.vector };
            Value::Vec3([v[0], v[1], v[2]])
        }
        BALAUR_COLOR => Value::Color(unsafe { payload.vector }),
        BALAUR_NODE => Value::Node(unsafe { payload.bits }),
        BALAUR_CALLBACK => Value::Callback(balaur_script::CallbackId(unsafe { payload.bits })),
        BALAUR_LIST => Value::List(unsafe { copy_list(payload.list) }?),
        BALAUR_MANY => Value::Many(unsafe { copy_list(payload.list) }?),
        BALAUR_MAP => {
            let map = unsafe { payload.map };
            let mut pairs = Vec::with_capacity(map.len);
            for index in 0..map.len {
                let entry = unsafe { &*map.items.add(index) };
                let key = unsafe { entry.key.as_str() }
                    .ok_or_else(|| anyhow::anyhow!("extension returned a key that is not UTF-8"))?;
                pairs.push((key.to_string(), unsafe { from_c(&entry.value) }?));
            }
            Value::Map(pairs)
        }
        other => bail!("extension returned value kind {other}, which this engine does not know"),
    })
}

/// # Safety
/// `slice` must be valid for its stated length.
unsafe fn copy_list(slice: BalaurSlice) -> Result<Vec<Value>> {
    let mut out = Vec::with_capacity(slice.len);
    for index in 0..slice.len {
        out.push(unsafe { from_c(&*slice.items.add(index)) }?);
    }
    Ok(out)
}

/// A library speaking the C ABI, presented as an ordinary [`Plugin`].
///
/// Everything downstream -- load order, the registry, the script modules it
/// declares -- is unchanged. Only the boundary differs.
pub struct CExtension {
    manifest: Manifest,
    declare: unsafe extern "C" fn(*const BalaurApi, *mut BalaurRegistry) -> i32,
}

impl CExtension {
    /// # Safety
    /// `declare` must be the `balaur_extension_declare` symbol of a library
    /// whose `balaur_extension_abi` already returned [`BALAUR_ABI_VERSION`].
    #[must_use]
    pub unsafe fn new(
        name: &str,
        version: &str,
        declare: unsafe extern "C" fn(*const BalaurApi, *mut BalaurRegistry) -> i32,
    ) -> Self {
        // The manifest carries the host's own compiler fingerprint: a C
        // extension has none, and the ABI version check stands in for it.
        Self {
            manifest: Manifest::new(name, version),
            declare,
        }
    }
}

impl Plugin for CExtension {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn declare(&mut self, registry: &mut Registry<'_>) -> Result<()> {
        let api = host_api();
        let handle: *mut Registry<'_> = registry;
        let status = unsafe { (self.declare)(&raw const api, handle.cast::<BalaurRegistry>()) };
        if status != 0 {
            bail!(
                "extension `{}` failed to declare itself (status {status})",
                self.manifest.name
            );
        }
        Ok(())
    }
}

/// Read a `const char*` the extension owns for the life of the library.
///
/// # Safety
/// `text` must be a valid NUL-terminated string, or null.
#[cfg(feature = "dylib")]
pub(crate) unsafe fn static_text(text: *const c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(text) }
        .to_str()
        .ok()
        .map(ToString::to_string)
}
