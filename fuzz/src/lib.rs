//! What more than one target does.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Compiled, Ty};

fn zero(ty: &Ty) -> Option<Value> {
    Some(match ty {
        Ty::Int => Value::Int(0),
        Ty::Float => Value::Float(0.0),
        Ty::Bool => Value::Bool(false),
        Ty::Str => Value::Str(String::new()),
        Ty::Node => Value::Node(0),
        Ty::Void | Ty::Any | Ty::Optional(_) => Value::Void,
        Ty::Array(_) => Value::Array(Vec::new()),
        Ty::Map(..) => Value::Map(Vec::new()),
        Ty::Struct(_) | Ty::Fn(..) => return None,
    })
}

/// Call each public function a unit has, with zero arguments, under the fuel
/// and memory limits: each traps or answers, and none brings the host down.
pub fn call_every_public(c: &Compiled) {
    let limits = Limits {
        fuel: Some(1_000_000),
        memory_bytes: Some(64 << 20),
        engine: EngineKind::Wasmi,
        skip_native: false,
    };
    let Ok(mut rt) = Runtime::new(c, HostFns::default(), limits) else {
        return;
    };
    for f in &c.meta.fns {
        if !f.public || f.is_async || f.owner.is_some() {
            continue;
        }
        let Some(args) = f.params.iter().map(zero).collect::<Option<Vec<_>>>() else {
            continue;
        };
        let _ = rt.call(&f.key, &args);
    }
}

/// `data` up to its first zero byte as text, and the bytes after it.
pub fn text_then_bytes(data: &[u8]) -> Option<(&str, &[u8])> {
    let cut = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let text = std::str::from_utf8(&data[..cut]).ok()?;
    Some((text, data.get(cut + 1..).unwrap_or_default()))
}

/// `bytes` with `patches` written over it past `keep`: each five bytes are a
/// little-endian position, wrapped to the length, and the byte to put there.
pub fn patched(mut bytes: Vec<u8>, keep: usize, patches: &[u8]) -> Vec<u8> {
    let Some(span) = bytes.len().checked_sub(keep).filter(|&s| s > 0) else {
        return bytes;
    };
    for patch in patches.chunks_exact(5) {
        let at = u32::from_le_bytes([patch[0], patch[1], patch[2], patch[3]]) as usize;
        bytes[keep + at % span] = patch[4];
    }
    bytes
}
