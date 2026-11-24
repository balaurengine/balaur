//! Script values to Valve's C types and back, for the generated bindings: an
//! int is any integer (a 64-bit handle as its bits), a steam id an int or its
//! decimal text, bytes a list of ints, a struct a map of its record's fields.

use std::ffi::{CStr, CString, c_char};

use anyhow::{Result, anyhow, bail};
use balaur_script::Value;

use crate::sdk::types::{CGameID, CSteamID, Plain, zeroed};

/// A C value as a script reads it.
pub(crate) trait ToScript {
    fn to_script(&self) -> Value;
}

/// A C value from what a script passed; `what` names it in an error.
pub(crate) trait FromScript: Sized {
    fn from_script(value: &Value, what: &str) -> Result<Self>;
}

macro_rules! small_ints {
    ($($t:ty),*) => {$(
        impl ToScript for $t {
            fn to_script(&self) -> Value {
                Value::Int(i64::from(*self))
            }
        }
        impl FromScript for $t {
            fn from_script(value: &Value, what: &str) -> Result<Self> {
                // Wrapped as C converts, so a flag mask past `i32::MAX` keeps its bits.
                #[allow(clippy::cast_possible_wrap, reason = "C's conversion")]
                Ok(int_of(value, what)? as $t)
            }
        }
    )*};
}

small_ints!(i8, u8, i16, u16, i32, u32, i64);

impl ToScript for u64 {
    #[allow(clippy::cast_possible_wrap, reason = "a handle crosses as its bits")]
    fn to_script(&self) -> Value {
        Value::Int(*self as i64)
    }
}

impl FromScript for u64 {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        match value {
            Value::Str(text) => text
                .parse()
                .map_err(|_| anyhow!("{what} should be a steam id, got {text:?}")),
            other => Ok(int_of(other, what)? as Self),
        }
    }
}

impl ToScript for isize {
    fn to_script(&self) -> Value {
        Value::Int(*self as i64)
    }
}

impl FromScript for isize {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        #[allow(clippy::cast_possible_truncation, reason = "C's conversion")]
        Ok(int_of(value, what)? as Self)
    }
}

impl ToScript for bool {
    fn to_script(&self) -> Value {
        Value::Bool(*self)
    }
}

impl FromScript for bool {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        match value {
            Value::Bool(flag) => Ok(*flag),
            other => bail!("{what} should be a bool, got {}", other.type_name()),
        }
    }
}

impl ToScript for f32 {
    fn to_script(&self) -> Value {
        Value::Num(f64::from(*self))
    }
}

impl ToScript for f64 {
    fn to_script(&self) -> Value {
        Value::Num(*self)
    }
}

impl FromScript for f64 {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        match value {
            Value::Num(n) => Ok(*n),
            #[allow(
                clippy::cast_precision_loss,
                reason = "a whole number passed as a float"
            )]
            Value::Int(n) => Ok(*n as Self),
            other => bail!("{what} should be a number, got {}", other.type_name()),
        }
    }
}

impl FromScript for f32 {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        Ok(f64::from_script(value, what)? as Self)
    }
}

impl ToScript for CSteamID {
    fn to_script(&self) -> Value {
        { self.0 }.to_script()
    }
}

impl FromScript for CSteamID {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        Ok(Self(u64::from_script(value, what)?))
    }
}

impl ToScript for CGameID {
    fn to_script(&self) -> Value {
        { self.0 }.to_script()
    }
}

impl FromScript for CGameID {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        Ok(Self(u64::from_script(value, what)?))
    }
}

impl<T: FromScript> FromScript for Option<T> {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        match value {
            Value::Nil => Ok(None),
            other => T::from_script(other, what).map(Some),
        }
    }
}

impl<T: FromScript> FromScript for Vec<T> {
    fn from_script(value: &Value, what: &str) -> Result<Self> {
        let Value::List(items) = value else {
            bail!("{what} should be a list, got {}", value.type_name());
        };
        items
            .iter()
            .enumerate()
            .map(|(at, item)| T::from_script(item, &format!("{what}[{at}]")))
            .collect()
    }
}

fn int_of(value: &Value, what: &str) -> Result<i64> {
    match value {
        Value::Int(n) => Ok(*n),
        #[allow(clippy::cast_possible_truncation, reason = "checked whole first")]
        Value::Num(n) if n.fract() == 0.0 => Ok(*n as i64),
        other => bail!("{what} should be an int, got {}", other.type_name()),
    }
}

/// A call's arguments, read in order.
pub(crate) struct Args<'a> {
    values: &'a [Value],
    at: usize,
    call: &'static str,
}

impl<'a> Args<'a> {
    pub(crate) const fn new(values: &'a [Value], call: &'static str) -> Self {
        Self {
            values,
            at: 0,
            call,
        }
    }

    fn next(&mut self) -> &'a Value {
        let value = self.values.get(self.at).unwrap_or(&Value::Nil);
        self.at += 1;
        value
    }

    pub(crate) fn take<T: FromScript>(&mut self, name: &str) -> Result<T> {
        let value = self.next();
        T::from_script(value, &format!("{}'s `{name}`", self.call))
    }

    /// A string for Valve, or nil for its null.
    pub(crate) fn string(&mut self, name: &str) -> Result<Option<CString>> {
        match self.next() {
            Value::Nil => Ok(None),
            Value::Str(text) => CString::new(text.as_str())
                .map(Some)
                .map_err(|_| anyhow!("{}'s `{name}` holds a NUL", self.call)),
            other => bail!(
                "{}'s `{name}` should be a string, got {}",
                self.call,
                other.type_name()
            ),
        }
    }

    pub(crate) fn bytes(&mut self, name: &str) -> Result<Vec<u8>> {
        let value = self.next();
        balaur_core::handler::bytes_of(value).map_err(|e| anyhow!("{}'s `{name}`: {e}", self.call))
    }

    /// Lists Valve reads with one count between them are as long as each other.
    pub(crate) fn same_len(&self, lens: &[usize]) -> Result<()> {
        if lens.windows(2).any(|pair| pair[0] != pair[1]) {
            bail!(
                "{}'s lists share one count, so they are as long as each other: {lens:?}",
                self.call
            );
        }
        Ok(())
    }

    pub(crate) fn done(&self) -> Result<()> {
        if self.values.len() > self.at {
            bail!(
                "{} takes {} arguments, got {}",
                self.call,
                self.at,
                self.values.len()
            );
        }
        Ok(())
    }
}

/// A record's fields, for building the struct it stands for. A field the map
/// leaves out is zero.
pub(crate) struct Fields<'a> {
    pairs: &'a [(String, Value)],
    what: &'a str,
}

impl<'a> Fields<'a> {
    pub(crate) fn of(value: &'a Value, what: &'a str) -> Result<Self> {
        match value {
            Value::Map(pairs) => Ok(Self { pairs, what }),
            other => bail!("{what} should be a map, got {}", other.type_name()),
        }
    }

    fn get(&self, name: &str) -> Option<&'a Value> {
        self.pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
            .filter(|value| !matches!(value, Value::Nil))
    }

    fn what(&self, name: &str) -> String {
        format!("{}.{name}", self.what)
    }

    pub(crate) fn take<T: FromScript + Plain>(&self, name: &str) -> Result<T> {
        self.get(name).map_or_else(
            || Ok(zeroed()),
            |value| T::from_script(value, &self.what(name)),
        )
    }

    pub(crate) fn chars<const N: usize>(&self, name: &str) -> Result<[c_char; N]> {
        let mut out = [0; N];
        match self.get(name) {
            None => {}
            Some(Value::Str(text)) => {
                // The last byte stays the NUL Valve reads to.
                for (slot, byte) in out.iter_mut().zip(text.bytes().take(N.saturating_sub(1))) {
                    *slot = c_char::from_ne_bytes([byte]);
                }
            }
            Some(other) => bail!(
                "{} should be a string, got {}",
                self.what(name),
                other.type_name()
            ),
        }
        Ok(out)
    }

    pub(crate) fn byte_array<const N: usize>(&self, name: &str) -> Result<[u8; N]> {
        let mut out = [0; N];
        if let Some(value) = self.get(name) {
            let held = balaur_core::handler::bytes_of(value)?;
            for (slot, byte) in out.iter_mut().zip(held) {
                *slot = byte;
            }
        }
        Ok(out)
    }

    pub(crate) fn array<T: FromScript + Plain, const N: usize>(
        &self,
        name: &str,
    ) -> Result<[T; N]> {
        let mut out = [zeroed::<T>(); N];
        if let Some(value) = self.get(name) {
            let held: Vec<T> = Vec::from_script(value, &self.what(name))?;
            for (slot, item) in out.iter_mut().zip(held) {
                *slot = item;
            }
        }
        Ok(out)
    }
}

/// The pointer Valve reads a string from, null for nil.
pub(crate) fn cstr(text: Option<&CStr>) -> *const c_char {
    text.map_or(std::ptr::null(), CStr::as_ptr)
}

/// A pointer to an optional struct, null for nil.
pub(crate) fn opt_ptr<T>(value: Option<&T>) -> *const T {
    value.map_or(std::ptr::null(), std::ptr::from_ref)
}

/// The text in a buffer Valve filled, up to its NUL.
pub(crate) fn chars(buffer: &[c_char]) -> Value {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let bytes: Vec<u8> = buffer[..end].iter().map(|&c| c.to_ne_bytes()[0]).collect();
    Value::Str(String::from_utf8_lossy(&bytes).into_owned())
}

/// Bytes as a script holds them: a list of ints.
pub(crate) fn bytes(buffer: &[u8]) -> Value {
    Value::List(buffer.iter().map(|&b| Value::Int(i64::from(b))).collect())
}

pub(crate) fn list<T: ToScript>(items: &[T]) -> Value {
    Value::List(items.iter().map(ToScript::to_script).collect())
}

/// The string Valve's pointer names, or nil for null.
///
/// # Safety
/// `text` is null or a NUL-terminated string Steam keeps alive for the call.
pub(crate) unsafe fn c_str_ptr(text: *const c_char) -> Value {
    if text.is_null() {
        return Value::Nil;
    }
    // SAFETY: the caller's promise.
    let text = unsafe { CStr::from_ptr(text) };
    Value::Str(text.to_string_lossy().into_owned())
}

/// The struct Valve's pointer names, or nil for null.
///
/// # Safety
/// `value` is null or points at a `T` Steam keeps alive for the call.
pub(crate) unsafe fn ptr_value<T: ToScript + Plain>(value: *const T) -> Value {
    if value.is_null() {
        return Value::Nil;
    }
    // SAFETY: the caller's promise; packed structs read unaligned.
    unsafe { value.read_unaligned() }.to_script()
}

/// A `T` from the bytes Steam handed a callback, or None when they are too few.
pub(crate) fn read<T: Plain>(data: &[u8]) -> Option<T> {
    if data.len() < size_of::<T>() {
        return None;
    }
    // SAFETY: `T` is plain data and the bytes are enough; read unaligned.
    Some(unsafe { data.as_ptr().cast::<T>().read_unaligned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_past_i64_crosses_as_its_bits_and_back() {
        let value = u64::MAX.to_script();
        assert_eq!(value, Value::Int(-1));
        assert_eq!(u64::from_script(&value, "h").unwrap(), u64::MAX);
    }

    #[test]
    fn a_steam_id_reads_from_text_or_an_int() {
        let id = 76_561_197_960_287_930_u64;
        assert_eq!(
            u64::from_script(&Value::Str(id.to_string()), "id").unwrap(),
            id
        );
        assert_eq!(u64::from_script(&id.to_script(), "id").unwrap(), id);
    }

    #[test]
    fn a_filled_buffer_reads_to_its_nul() {
        let buffer: Vec<c_char> = b"Ana\0junk"
            .iter()
            .map(|&b| c_char::from_ne_bytes([b]))
            .collect();
        assert_eq!(chars(&buffer), Value::Str("Ana".into()));
    }

    #[test]
    fn a_string_field_keeps_its_last_byte_for_the_nul() {
        let map = Value::Map(vec![("name".into(), Value::Str("abcdef".into()))]);
        let fields = Fields::of(&map, "x").unwrap();
        let out: [c_char; 4] = fields.chars("name").unwrap();
        assert_eq!(out.map(|c| c.to_ne_bytes()[0]), *b"abc\0");
    }

    #[test]
    fn lists_sharing_a_count_must_match() {
        let args = Args::new(&[], "steam_inventory::generate_items");
        assert!(args.same_len(&[2, 2]).is_ok());
        assert!(args.same_len(&[2, 3]).is_err());
    }
}
