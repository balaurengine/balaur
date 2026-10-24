// The methods a script calls on a `str`, answered by the host: the compiler
// imports one function per method, and the runtime runs it here.

use crate::Ty;
use crate::runtime::Value;

#[derive(Clone, Copy)]
pub(crate) enum K {
    Bool,
    Int,
    Float,
    Str,
    Strs,
}

pub(crate) struct Method {
    pub name: &'static str,
    /// After the string it is called on.
    pub params: &'static [K],
    pub ret: K,
}

const fn m(name: &'static str, params: &'static [K], ret: K) -> Method {
    Method { name, params, ret }
}

pub(crate) const METHODS: &[Method] = &[
    m("contains", &[K::Str], K::Bool),
    m("starts_with", &[K::Str], K::Bool),
    m("ends_with", &[K::Str], K::Bool),
    m("find", &[K::Str], K::Int),
    m("slice", &[K::Int, K::Int], K::Str),
    m("to_upper", &[], K::Str),
    m("to_lower", &[], K::Str),
    m("trim", &[], K::Str),
    m("replace", &[K::Str, K::Str], K::Str),
    m("split", &[K::Str], K::Strs),
    m("split_once", &[K::Str], K::Strs),
    m("chars", &[], K::Strs),
    m("parse_int", &[], K::Int),
    m("parse_float", &[], K::Float),
];

pub(crate) fn method(name: &str) -> Option<&'static Method> {
    METHODS.iter().find(|m| m.name == name)
}

pub(crate) fn ty(k: K) -> Ty {
    match k {
        K::Bool => Ty::Bool,
        K::Int => Ty::Int,
        K::Float => Ty::Float,
        K::Str => Ty::Str,
        K::Strs => Ty::Array(Box::new(Ty::Str)),
    }
}

/// The method's parameters as the import takes them: the string first.
pub(crate) fn params(m: &Method) -> Vec<Ty> {
    std::iter::once(Ty::Str)
        .chain(m.params.iter().map(|k| ty(*k)))
        .collect()
}

fn text(v: Option<&Value>) -> &str {
    match v {
        Some(Value::Str(s)) => s,
        _ => "",
    }
}

// A byte offset clamped into the string and moved back onto a character.
fn boundary(s: &str, at: i64) -> usize {
    let mut at = usize::try_from(at.max(0)).unwrap_or(0).min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `recv.name(args)`. Offsets are bytes, as `len` counts them; a number that
/// does not parse is 0.
pub(crate) fn run(name: &str, recv: &str, args: &[Value]) -> Value {
    let int = |i: usize| match args.get(i) {
        Some(Value::Int(x)) => *x,
        _ => 0,
    };
    let arg = |i: usize| text(args.get(i));
    match name {
        "contains" => Value::Bool(recv.contains(arg(0))),
        "starts_with" => Value::Bool(recv.starts_with(arg(0))),
        "ends_with" => Value::Bool(recv.ends_with(arg(0))),
        "find" => Value::Int(recv.find(arg(0)).map_or(-1, |i| i as i64)),
        "slice" => {
            let (a, b) = (boundary(recv, int(0)), boundary(recv, int(1)));
            Value::Str(recv.get(a..b.max(a)).unwrap_or_default().to_string())
        }
        "to_upper" => Value::Str(recv.to_uppercase()),
        "to_lower" => Value::Str(recv.to_lowercase()),
        "trim" => Value::Str(recv.trim().to_string()),
        "replace" if arg(0).is_empty() => Value::Str(recv.to_string()),
        "replace" => Value::Str(recv.replace(arg(0), arg(1))),
        // An empty separator splits into characters.
        "split" if arg(0).is_empty() => {
            Value::Array(recv.chars().map(|c| Value::Str(c.to_string())).collect())
        }
        "split" => Value::Array(
            recv.split(arg(0))
                .map(|p| Value::Str(p.to_string()))
                .collect(),
        ),
        // Before and after the first separator, or nothing when there is none.
        "split_once" => Value::Array(recv.split_once(arg(0)).map_or_else(Vec::new, |(a, b)| {
            vec![Value::Str(a.to_string()), Value::Str(b.to_string())]
        })),
        "chars" => Value::Array(recv.chars().map(|c| Value::Str(c.to_string())).collect()),
        "parse_int" => Value::Int(recv.trim().parse().unwrap_or(0)),
        "parse_float" => Value::Float(recv.trim().parse().unwrap_or(0.0)),
        _ => Value::Void,
    }
}
