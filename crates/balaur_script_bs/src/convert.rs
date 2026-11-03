// Between the seam's neutral values and the script's typed ones.

use crate::api::Structs;
use balaur_bs::runtime::{CallArgs, Value as L};
use balaur_bs::{Meta, Ty};
use balaur_script::Value as N;
use smol_str::SmolStr;

/// What a value crossing here may be declared as: the records a script's
/// module knows, or the engine's own.
pub(crate) trait Records {
    fn fields_of(&self, name: &str) -> Option<&[(String, Ty)]>;
}

impl Records for Meta {
    fn fields_of(&self, name: &str) -> Option<&[(String, Ty)]> {
        self.fields(name)
    }
}

impl Records for Structs {
    fn fields_of(&self, name: &str) -> Option<&[(String, Ty)]> {
        self.get(name).map(Vec::as_slice)
    }
}

const VEC: [(&str, &[&str]); 3] = [
    ("Vec2", &["x", "y"]),
    ("Vec3", &["x", "y", "z"]),
    ("Color", &["r", "g", "b", "a"]),
];

/// The argument at `index` as the seam's value, a vector read from its slots.
pub(crate) fn neutral_at(args: &dyn CallArgs, index: usize) -> N {
    match args.vector(index) {
        Some(([x, y, ..], 2)) => N::Vec2([x, y]),
        Some(([x, y, z, _], 3)) => N::Vec3([x, y, z]),
        Some((color, 4)) => N::Color(color),
        _ => to_neutral(&args.value(index)),
    }
}

pub(crate) fn to_neutral(v: &L) -> N {
    match v {
        L::Vector(numbers) => N::List(numbers.iter().map(|n| N::Num(*n)).collect()),
        L::Void => N::Nil,
        L::Int(x) => N::Int(*x),
        L::Float(x) => N::Num(*x),
        L::Bool(b) => N::Bool(*b),
        L::Node(x) => N::Node(x.cast_unsigned()),
        L::Str(s) => N::Str(s.clone()),
        L::Array(items) => N::List(items.iter().map(to_neutral).collect()),
        L::Map(entries) => N::Map(
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), to_neutral(v)))
                .collect(),
        ),
        L::Struct(name, fields) => {
            let f = |i: usize| match fields.get(i).map(|(_, v)| v) {
                Some(L::Float(x)) => *x as f32,
                Some(L::Int(x)) => *x as f32,
                _ => 0.0,
            };
            match name.as_str() {
                "Vec2" => N::Vec2([f(0), f(1)]),
                "Vec3" => N::Vec3([f(0), f(1), f(2)]),
                "Color" => N::Color([f(0), f(1), f(2), f(3)]),
                // A record's nil field is one it leaves out, as a table would.
                _ => N::Map(
                    fields
                        .iter()
                        .filter(|(_, v)| !matches!(v, L::Void))
                        .map(|(k, v)| (k.to_string(), to_neutral(v)))
                        .collect(),
                ),
            }
        }
    }
}

fn numbers(v: &N) -> Option<Vec<f64>> {
    match v {
        N::Vec2(a) => Some(a.iter().map(|x| f64::from(*x)).collect()),
        N::Vec3(a) => Some(a.iter().map(|x| f64::from(*x)).collect()),
        N::Color(a) => Some(a.iter().map(|x| f64::from(*x)).collect()),
        N::Transform2d(a) => Some(a.iter().map(|x| f64::from(*x)).collect()),
        N::Transform3d(a) => Some(a.iter().map(|x| f64::from(*x)).collect()),
        N::List(items) | N::Many(items) => items
            .iter()
            .map(|i| match i {
                N::Num(x) => Some(*x),
                N::Int(x) => Some(*x as f64),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// `v` as what an `any` holds: a vector or a transform is a list of its numbers.
pub(crate) fn dynamic(v: &N) -> L {
    match v {
        N::Bool(b) => L::Bool(*b),
        N::Int(x) => L::Int(*x),
        N::Num(x) => L::Float(*x),
        N::Str(s) => L::Str(s.clone()),
        N::Node(x) => L::Node(x.cast_signed()),
        N::List(items) | N::Many(items) => L::Array(items.iter().map(dynamic).collect()),
        N::Bytes(b) => L::Array(b.iter().map(|x| L::Int(i64::from(*x))).collect()),
        N::Map(entries) => L::Map(
            entries
                .iter()
                .map(|(k, v)| (SmolStr::from(k.as_str()), dynamic(v)))
                .collect(),
        ),
        _ => numbers(v).map_or(L::Void, |xs| {
            L::Array(xs.into_iter().map(L::Float).collect())
        }),
    }
}

/// `v` as a value of `ty`; a field it does not carry stays at its default.
pub(crate) fn from_neutral<R: Records>(v: &N, ty: &Ty, records: &R) -> L {
    match (ty, v) {
        (Ty::Any, v) => dynamic(v),
        (Ty::Optional(_), N::Nil) => L::Void,
        (Ty::Optional(t), v) => from_neutral(v, t, records),
        (Ty::Float, N::Num(x)) => L::Float(*x),
        (Ty::Float, N::Int(x)) => L::Float(*x as f64),
        (Ty::Int, N::Int(x)) => L::Int(*x),
        (Ty::Int, N::Num(x)) => L::Int(*x as i64),
        (Ty::Bool, N::Bool(b)) => L::Bool(*b),
        (Ty::Str, N::Str(s)) => L::Str(s.clone()),
        (Ty::Node, N::Node(x)) => L::Node(x.cast_signed()),
        (Ty::Array(_), N::Nil) => L::Array(vec![]),
        (Ty::Array(et), N::List(items) | N::Many(items)) => {
            L::Array(items.iter().map(|i| from_neutral(i, et, records)).collect())
        }
        (Ty::Struct(name), v) => {
            if let Some((_, fields)) = VEC.iter().find(|(n, _)| n == name)
                && let Some(xs) = numbers(v)
            {
                let values = fields
                    .iter()
                    .zip(xs.into_iter().chain(std::iter::repeat(0.0)))
                    .map(|(f, x)| (SmolStr::new(f), L::Float(x)))
                    .collect();
                return L::Struct(SmolStr::new(name), values);
            }
            if let Some(fields) = records.fields_of(name) {
                return record(name, fields, v, records);
            }
            match v {
                N::Map(_) => L::Struct(SmolStr::new(name), vec![]),
                _ => L::Void,
            }
        }
        _ => L::Void,
    }
}

// A table as the record it is declared to be, field by field; one it does
// not carry is that field's default.
fn record<R: Records>(name: &str, fields: &[(String, Ty)], v: &N, records: &R) -> L {
    let entries: &[(String, N)] = match v {
        N::Map(entries) => entries,
        _ => &[],
    };
    let values = fields
        .iter()
        .map(|(f, t)| {
            let got = entries
                .iter()
                .find(|(k, _)| k == f)
                .map_or(&N::Nil, |(_, v)| v);
            (SmolStr::new(f), from_neutral(got, t, records))
        })
        .collect();
    L::Struct(SmolStr::new(name), values)
}

/// Every argument of a call as the seam's values, for an engine function
/// that takes them all: a node operation, a raw binding.
pub(crate) fn neutral(args: &dyn CallArgs) -> smallvec::SmallVec<[N; 4]> {
    (0..args.len())
        .map(|index| neutral_at(args, index))
        .collect()
}

/// A script's arguments as a typed binding reads them: a number, a flag, a
/// node or a string straight from the call, anything else converted when it
/// is asked for.
pub(crate) struct Seam<'a>(pub &'a dyn CallArgs);

impl balaur_script::Args for Seam<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn int(&self, index: usize) -> Option<i64> {
        self.0.int(index)
    }
    fn num(&self, index: usize) -> Option<f64> {
        self.0.float(index)
    }
    fn flag(&self, index: usize) -> Option<bool> {
        self.0.bool(index)
    }
    fn node(&self, index: usize) -> Option<u64> {
        self.0.node(index).map(i64::cast_unsigned)
    }
    fn text(&self, index: usize) -> Option<&str> {
        self.0.text(index)
    }
    fn is_nil(&self, index: usize) -> bool {
        self.0.is_nil(index)
    }
    fn value(&self, index: usize) -> N {
        neutral_at(self.0, index)
    }
}
