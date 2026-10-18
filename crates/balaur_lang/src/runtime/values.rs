// Script values as the host sees them, read from and written to the slots and
// memory the compiler lays out: one 8-byte slot per scalar.

use super::engine::Raw;
use crate::{Meta, Ty};

/// A script value as the host sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Void,
    Int(i64),
    Float(f64),
    Bool(bool),
    Node(i64),
    Str(String),
    Array(Vec<Value>),
    /// The struct's name, then its fields in declaration order.
    Struct(String, Vec<(String, Value)>),
}

impl Value {
    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Struct(_, fields) => fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// Something module memory can be read from: a live instance, or a copy.
pub(crate) trait Reader {
    fn bytes(&self, at: usize, len: usize) -> Vec<u8>;
}

impl Reader for [u8] {
    fn bytes(&self, at: usize, len: usize) -> Vec<u8> {
        self.get(at..at + len)
            .map_or_else(|| vec![0; len], <[u8]>::to_vec)
    }
}

pub(crate) fn rd32(r: &(impl Reader + ?Sized), at: usize) -> u32 {
    u32::from_le_bytes(
        r.bytes(at, 4)
            .try_into()
            .expect("bytes(at, 4) is four bytes"),
    )
}
pub(crate) fn rd64(r: &(impl Reader + ?Sized), at: usize) -> u64 {
    u64::from_le_bytes(
        r.bytes(at, 8)
            .try_into()
            .expect("bytes(at, 8) is eight bytes"),
    )
}

/// Field and element layout, as the compiler lays them out.
pub(crate) fn slots(meta: &Meta, ty: &Ty) -> Vec<(crate::Slot, Option<Ty>)> {
    use crate::Slot;
    match ty {
        Ty::Int | Ty::Node => vec![(Slot::I64, None)],
        Ty::Float => vec![(Slot::F64, None)],
        Ty::Bool => vec![(Slot::I32, None)],
        Ty::Str | Ty::Array(_) => vec![(Slot::I32, Some(ty.clone()))],
        Ty::Void => vec![],
        Ty::Struct(s) => meta
            .fields(s)
            .unwrap_or(&[])
            .iter()
            .flat_map(|(_, t)| slots(meta, t))
            .collect(),
    }
}

pub(crate) fn has_heap(meta: &Meta, ty: &Ty) -> bool {
    slots(meta, ty).iter().any(|(_, h)| h.is_some())
}

/// A value whose slots start at `at` in memory.
pub(crate) fn read_value(meta: &Meta, m: &(impl Reader + ?Sized), ty: &Ty, at: usize) -> Value {
    match ty {
        Ty::Int => Value::Int(rd64(m, at) as i64),
        Ty::Node => Value::Node(rd64(m, at) as i64),
        Ty::Float => Value::Float(f64::from_bits(rd64(m, at))),
        Ty::Bool => Value::Bool(rd32(m, at) != 0),
        Ty::Str => read_str(m, rd32(m, at) as usize),
        Ty::Array(et) => read_array(meta, m, et, rd32(m, at) as usize),
        Ty::Void => Value::Void,
        Ty::Struct(s) => {
            let mut off = at;
            let fields = meta.fields(s).unwrap_or(&[]);
            let values = fields
                .iter()
                .map(|(n, t)| {
                    let v = read_value(meta, m, t, off);
                    off += 8 * slots(meta, t).len();
                    (n.clone(), v)
                })
                .collect();
            Value::Struct(s.clone(), values)
        }
    }
}

pub(crate) fn read_str(m: &(impl Reader + ?Sized), p: usize) -> Value {
    if p == 0 {
        return Value::Str(String::new());
    }
    let len = rd32(m, p + 4) as usize;
    Value::Str(String::from_utf8_lossy(&m.bytes(p + 8, len)).into_owned())
}

fn read_array(meta: &Meta, m: &(impl Reader + ?Sized), et: &Ty, p: usize) -> Value {
    if p == 0 {
        return Value::Array(vec![]);
    }
    let (len, data) = (rd32(m, p + 4) as usize, rd32(m, p + 12) as usize);
    let esz = 8 * slots(meta, et).len();
    Value::Array(
        (0..len)
            .map(|i| read_value(meta, m, et, data + i * esz))
            .collect(),
    )
}

/// A value held in slots outside memory: scalars from the slots themselves,
/// strings and arrays through `m`.
pub(crate) fn decode(meta: &Meta, m: &(impl Reader + ?Sized), ty: &Ty, raw: &[Raw]) -> Value {
    let scalar = |r: Raw| match r {
        Raw::I32(x) => x as u64,
        Raw::I64(x) => x as u64,
        Raw::F64(x) => x.to_bits(),
    };
    match ty {
        Ty::Int => Value::Int(scalar(raw[0]) as i64),
        Ty::Node => Value::Node(scalar(raw[0]) as i64),
        Ty::Float => match raw[0] {
            Raw::F64(x) => Value::Float(x),
            r => Value::Float(f64::from_bits(scalar(r))),
        },
        Ty::Bool => Value::Bool(raw[0].i32() != 0),
        Ty::Str => read_str(m, raw[0].i32() as u32 as usize),
        Ty::Array(et) => read_array(meta, m, et, raw[0].i32() as u32 as usize),
        Ty::Void => Value::Void,
        Ty::Struct(s) => {
            let mut at = 0;
            let fields = meta.fields(s).unwrap_or(&[]);
            let values = fields
                .iter()
                .map(|(n, t)| {
                    let k = slots(meta, t).len();
                    let v = decode(meta, m, t, &raw[at..at + k]);
                    at += k;
                    (n.clone(), v)
                })
                .collect();
            Value::Struct(s.clone(), values)
        }
    }
}

/// The slots of a value with no strings or arrays in it, for a host function's result.
pub(crate) fn encode_plain(meta: &Meta, ty: &Ty, v: &Value) -> Result<Vec<Raw>, String> {
    Ok(match (ty, v) {
        (Ty::Void, _) => vec![],
        (Ty::Int, Value::Int(x)) | (Ty::Node, Value::Node(x) | Value::Int(x)) => vec![Raw::I64(*x)],
        (Ty::Float, Value::Float(x)) => vec![Raw::F64(*x)],
        (Ty::Float, Value::Int(x)) => vec![Raw::F64(*x as f64)],
        (Ty::Bool, Value::Bool(b)) => vec![Raw::I32(i32::from(*b))],
        (Ty::Struct(s), Value::Struct(_, fields)) => {
            let mut out = vec![];
            for (n, t) in meta.fields(s).unwrap_or(&[]) {
                let fv = fields
                    .iter()
                    .find(|(k, _)| k == n)
                    .map_or(&Value::Void, |(_, v)| v);
                out.extend(encode_plain(meta, t, fv)?);
            }
            out
        }
        (Ty::Int | Ty::Node | Ty::Float | Ty::Bool | Ty::Struct(_), Value::Void) => zero(meta, ty),
        _ => return Err(format!("{v:?} is not a {}", crate::show(ty))),
    })
}

pub(crate) fn zero(meta: &Meta, ty: &Ty) -> Vec<Raw> {
    slots(meta, ty)
        .into_iter()
        .map(|(s, _)| match s {
            crate::Slot::I64 => Raw::I64(0),
            crate::Slot::F64 => Raw::F64(0.0),
            crate::Slot::I32 => Raw::I32(0),
        })
        .collect()
}
