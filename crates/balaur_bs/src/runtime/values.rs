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
    /// What an `any` holds when it is a map: keys in the order they came.
    Map(Vec<(String, Value)>),
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

/// The float in `bits`, as a module hands it out. A NaN's sign and payload
/// are the hardware's, so the host only ever sees the canonical NaN.
pub(crate) fn float(bits: u64) -> f64 {
    const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;
    let x = f64::from_bits(bits);
    if x.is_nan() {
        f64::from_bits(CANONICAL_NAN)
    } else {
        x
    }
}

/// Field and element layout, as the compiler lays them out.
pub(crate) fn slots(meta: &Meta, ty: &Ty) -> Vec<(crate::Slot, Option<Ty>)> {
    use crate::Slot;
    match ty {
        Ty::Int | Ty::Node => vec![(Slot::I64, None)],
        Ty::Float => vec![(Slot::F64, None)],
        Ty::Bool | Ty::Fn(..) => vec![(Slot::I32, None)],
        Ty::Str | Ty::Array(_) | Ty::Any => vec![(Slot::I32, Some(ty.clone()))],
        Ty::Void => vec![],
        Ty::Struct(s) if meta.is_class(s) => vec![(Slot::I32, Some(ty.clone()))],
        Ty::Struct(s) => meta
            .fields(s)
            .unwrap_or(&[])
            .iter()
            .flat_map(|(_, t)| slots(meta, t))
            .collect(),
        Ty::Optional(t) => {
            let mut v = slots(meta, t);
            v.push((Slot::I32, None));
            v
        }
        Ty::Map(..) => unreachable!("a map type is made its class before code is made"),
    }
}

/// How many bytes a class object of `s` takes: its header, then its fields.
pub(crate) fn object_size(meta: &Meta, s: &str) -> usize {
    OBJ_HEADER
        + 8 * meta
            .fields(s)
            .unwrap_or(&[])
            .iter()
            .map(|(_, t)| width(meta, t))
            .sum::<usize>()
}

/// Where a class object's fields start, after its count.
pub(crate) const OBJ_HEADER: usize = 8;

/// How many slots a value of `ty` takes, without listing them: what a host
/// call counts on every call.
pub(crate) fn width(meta: &Meta, ty: &Ty) -> usize {
    match ty {
        Ty::Void => 0,
        Ty::Optional(t) => width(meta, t) + 1,
        Ty::Struct(s) if meta.is_class(s) => 1,
        Ty::Struct(s) => meta
            .fields(s)
            .unwrap_or(&[])
            .iter()
            .map(|(_, t)| width(meta, t))
            .sum(),
        _ => 1,
    }
}

pub(crate) fn has_heap(meta: &Meta, ty: &Ty) -> bool {
    match ty {
        Ty::Str | Ty::Array(_) | Ty::Any => true,
        Ty::Optional(t) => has_heap(meta, t),
        Ty::Struct(s) if meta.is_class(s) => true,
        Ty::Struct(s) => meta
            .fields(s)
            .unwrap_or(&[])
            .iter()
            .any(|(_, t)| has_heap(meta, t)),
        _ => false,
    }
}

/// A value whose slots start at `at` in memory.
pub(crate) fn read_value(meta: &Meta, m: &(impl Reader + ?Sized), ty: &Ty, at: usize) -> Value {
    match ty {
        Ty::Int => Value::Int(rd64(m, at) as i64),
        Ty::Node => Value::Node(rd64(m, at) as i64),
        Ty::Float => Value::Float(float(rd64(m, at))),
        Ty::Bool => Value::Bool(rd32(m, at) != 0),
        // A `fn` shows as its table slot.
        Ty::Fn(..) => Value::Int(i64::from(rd32(m, at))),
        Ty::Str => read_str(m, rd32(m, at) as usize),
        Ty::Array(et) => read_array(meta, m, et, rd32(m, at) as usize),
        Ty::Any => read_any(meta, m, rd32(m, at) as usize),
        Ty::Void => Value::Void,
        // Nil reads as nothing; the flag sits after the value's own slots.
        Ty::Optional(t) => {
            if rd32(m, at + 8 * width(meta, t)) == 0 {
                Value::Void
            } else {
                read_value(meta, m, t, at)
            }
        }
        Ty::Struct(s) if meta.is_class(s) => read_object(meta, m, s, rd32(m, at) as usize),
        Ty::Map(..) => unreachable!("a map type is made its class before code is made"),
        Ty::Struct(s) => {
            let mut off = at;
            let fields = meta.fields(s).unwrap_or(&[]);
            let values = fields
                .iter()
                .map(|(n, t)| {
                    let v = read_value(meta, m, t, off);
                    off += 8 * width(meta, t);
                    (n.clone(), v)
                })
                .collect();
            Value::Struct(s.clone(), values)
        }
    }
}

thread_local! {
    /// The objects being read, outermost first: scripts that hold each other
    /// make a cycle, and an object met again inside itself reads as nil.
    static READING: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(vec![]) };
}

/// The fields of the class object at `p`, as a struct; a null one is nothing.
pub(crate) fn read_object(meta: &Meta, m: &(impl Reader + ?Sized), s: &str, p: usize) -> Value {
    if p == 0 || READING.with(|reading| reading.borrow().contains(&p)) {
        return Value::Void;
    }
    READING.with(|reading| reading.borrow_mut().push(p));
    let mut off = p + OBJ_HEADER;
    let fields = meta.fields(s).unwrap_or(&[]);
    let values = fields
        .iter()
        .map(|(n, t)| {
            let v = read_value(meta, m, t, off);
            off += 8 * width(meta, t);
            (n.clone(), v)
        })
        .collect();
    READING.with(|reading| reading.borrow_mut().pop());
    Value::Struct(s.to_string(), values)
}

pub(crate) fn read_str(m: &(impl Reader + ?Sized), p: usize) -> Value {
    if p == 0 {
        return Value::Str(String::new());
    }
    let len = rd32(m, p + 4) as usize;
    Value::Str(String::from_utf8_lossy(&m.bytes(p + 8, len)).into_owned())
}

/// What the `any` cell at `p` holds; nil is `Void`.
pub(crate) fn read_any(meta: &Meta, m: &(impl Reader + ?Sized), p: usize) -> Value {
    use crate::anys::{BOOL, FLOAT, INT, LIST, MAP, NODE, STR};
    if p == 0 {
        return Value::Void;
    }
    let at = p + 8;
    match rd32(m, p + 4) as i32 {
        BOOL => Value::Bool(rd32(m, at) != 0),
        INT => Value::Int(rd64(m, at) as i64),
        NODE => Value::Node(rd64(m, at) as i64),
        FLOAT => Value::Float(float(rd64(m, at))),
        STR => read_str(m, rd32(m, at) as usize),
        LIST => read_array(meta, m, &Ty::Any, rd32(m, at) as usize),
        MAP => {
            let keys = read_array(meta, m, &Ty::Str, rd32(m, at) as usize);
            let values = read_array(meta, m, &Ty::Any, rd32(m, at + 4) as usize);
            match (keys, values) {
                (Value::Array(k), Value::Array(v)) => Value::Map(
                    k.into_iter()
                        .zip(v)
                        .map(|(k, v)| match k {
                            Value::Str(k) => (k, v),
                            _ => (String::new(), v),
                        })
                        .collect(),
                ),
                _ => Value::Map(vec![]),
            }
        }
        _ => Value::Void,
    }
}

/// The tag and payload of a cell holding `v`, with whatever the payload
/// points at made by `heap`: `(Str, v)` and `(Array(Any), list)`.
pub(crate) fn any_cell<E>(
    v: &Value,
    mut heap: impl FnMut(&Ty, &Value) -> Result<u32, E>,
) -> Result<Option<(i32, u64)>, E> {
    use crate::anys::{BOOL, FLOAT, INT, LIST, MAP, NODE, STR};
    let list = Ty::Array(Box::new(Ty::Any));
    let map = |heap: &mut dyn FnMut(&Ty, &Value) -> Result<u32, E>, entries: &[(String, Value)]| {
        let keys = entries.iter().map(|(k, _)| Value::Str(k.clone())).collect();
        let values = entries.iter().map(|(_, v)| v.clone()).collect();
        let k = heap(&Ty::Array(Box::new(Ty::Str)), &Value::Array(keys))?;
        let v = heap(&list, &Value::Array(values))?;
        Ok((MAP, u64::from(k) | (u64::from(v) << 32)))
    };
    Ok(Some(match v {
        Value::Void => return Ok(None),
        Value::Bool(b) => (BOOL, u64::from(*b)),
        Value::Int(x) => (INT, *x as u64),
        Value::Node(x) => (NODE, *x as u64),
        Value::Float(x) => (FLOAT, x.to_bits()),
        Value::Str(_) => (STR, u64::from(heap(&Ty::Str, v)?)),
        Value::Array(_) => (LIST, u64::from(heap(&list, v)?)),
        Value::Map(entries) | Value::Struct(_, entries) => map(&mut heap, entries)?,
    }))
}

/// The 16 bytes of a cell: one reference, the tag, the payload.
pub(crate) fn cell_bytes(tag: i32, payload: u64) -> Vec<u8> {
    let mut b = 1i32.to_le_bytes().to_vec();
    b.extend(tag.to_le_bytes());
    b.extend(payload.to_le_bytes());
    b
}

fn read_array(meta: &Meta, m: &(impl Reader + ?Sized), et: &Ty, p: usize) -> Value {
    if p == 0 {
        return Value::Array(vec![]);
    }
    let (len, data) = (rd32(m, p + 4) as usize, rd32(m, p + 12) as usize);
    let esz = 8 * width(meta, et);
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
            r => Value::Float(float(scalar(r))),
        },
        Ty::Bool => Value::Bool(raw[0].i32() != 0),
        Ty::Fn(..) => Value::Int(i64::from(raw[0].i32())),
        Ty::Str => read_str(m, raw[0].i32() as u32 as usize),
        Ty::Array(et) => read_array(meta, m, et, raw[0].i32() as u32 as usize),
        Ty::Any => read_any(meta, m, raw[0].i32() as u32 as usize),
        Ty::Void => Value::Void,
        Ty::Optional(t) => match raw.last() {
            Some(flag) if flag.i32() != 0 => decode(meta, m, t, &raw[..raw.len() - 1]),
            _ => Value::Void,
        },
        Ty::Struct(s) if meta.is_class(s) => read_object(meta, m, s, raw[0].i32() as u32 as usize),
        Ty::Map(..) => unreachable!("a map type is made its class before code is made"),
        Ty::Struct(s) => {
            let mut at = 0;
            let fields = meta.fields(s).unwrap_or(&[]);
            let values = fields
                .iter()
                .map(|(n, t)| {
                    let k = width(meta, t);
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
        (Ty::Fn(..), Value::Int(x)) => vec![Raw::I32(*x as i32)],
        (Ty::Struct(s), _) if meta.is_class(s) => {
            return Err(format!("a {s} is an object the module makes"));
        }
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
        (
            Ty::Int | Ty::Node | Ty::Float | Ty::Bool | Ty::Struct(_) | Ty::Any | Ty::Optional(_),
            Value::Void,
        ) => zero(meta, ty),
        (Ty::Optional(t), v) => {
            let mut out = encode_plain(meta, t, v)?;
            out.push(Raw::I32(1));
            out
        }
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
