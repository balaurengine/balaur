// Between the seam's neutral values and the script's typed ones.

use balaur_lang::Ty;
use balaur_lang::runtime::Value as L;
use balaur_script::Value as N;

const VEC: [(&str, &[&str]); 3] = [
    ("Vec2", &["x", "y"]),
    ("Vec3", &["x", "y", "z"]),
    ("Color", &["r", "g", "b", "a"]),
];

pub(crate) fn to_neutral(v: &L) -> N {
    match v {
        L::Void => N::Nil,
        L::Int(x) => N::Int(*x),
        L::Float(x) => N::Num(*x),
        L::Bool(b) => N::Bool(*b),
        L::Node(x) => N::Node(x.cast_unsigned()),
        L::Str(s) => N::Str(s.clone()),
        L::Array(items) => N::List(items.iter().map(to_neutral).collect()),
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
                _ => N::Map(
                    fields
                        .iter()
                        .map(|(k, v)| (k.clone(), to_neutral(v)))
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
        N::List(items) => items
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

/// `v` as a value of `ty`; a field it does not carry stays at its default.
pub(crate) fn from_neutral(v: &N, ty: &Ty) -> L {
    match (ty, v) {
        (Ty::Float, N::Num(x)) => L::Float(*x),
        (Ty::Float, N::Int(x)) => L::Float(*x as f64),
        (Ty::Int, N::Int(x)) => L::Int(*x),
        (Ty::Int, N::Num(x)) => L::Int(*x as i64),
        (Ty::Bool, N::Bool(b)) => L::Bool(*b),
        (Ty::Str, N::Str(s)) => L::Str(s.clone()),
        (Ty::Node, N::Node(x)) => L::Node(x.cast_signed()),
        (Ty::Array(et), N::List(items)) => {
            L::Array(items.iter().map(|i| from_neutral(i, et)).collect())
        }
        (Ty::Struct(name), v) => {
            if let Some((_, fields)) = VEC.iter().find(|(n, _)| n == name)
                && let Some(xs) = numbers(v)
            {
                let values = fields
                    .iter()
                    .zip(xs.into_iter().chain(std::iter::repeat(0.0)))
                    .map(|(f, x)| ((*f).to_string(), L::Float(x)))
                    .collect();
                return L::Struct(name.clone(), values);
            }
            match v {
                N::Map(fields) => L::Struct(
                    name.clone(),
                    fields
                        .iter()
                        .map(|(k, v)| (k.clone(), from_neutral(v, &Ty::Void)))
                        .collect(),
                ),
                _ => L::Void,
            }
        }
        _ => L::Void,
    }
}
