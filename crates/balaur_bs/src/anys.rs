// An `any` holds a value whose kind is known only when the script runs: what
// the engine hands over for an http reply, a gamend record or an event. It is
// a refcounted cell in the module's memory, and nil is the pointer 0.

use crate::Ty;

/// rc, tag, then 8 bytes of payload: the value, or one or two pointers.
pub(crate) const CELL: i32 = 16;

pub(crate) const BOOL: i32 = 1;
pub(crate) const INT: i32 = 2;
pub(crate) const FLOAT: i32 = 3;
pub(crate) const STR: i32 = 4;
/// The payload points at an `[any]`.
pub(crate) const LIST: i32 = 5;
/// The payload points at a `[str]` of keys, then an `[any]` of values.
pub(crate) const MAP: i32 = 6;
pub(crate) const NODE: i32 = 7;

#[derive(Clone, Copy)]
pub(crate) enum A {
    Bool,
    Int,
    Float,
    Str,
    Strs,
    Node,
    Any,
}

pub(crate) struct Method {
    pub name: &'static str,
    /// After the `any` it is called on.
    pub params: &'static [A],
    pub ret: A,
}

const fn m(name: &'static str, params: &'static [A], ret: A) -> Method {
    Method { name, params, ret }
}

pub(crate) const METHODS: &[Method] = &[
    m("kind", &[], A::Str),
    m("is_nil", &[], A::Bool),
    m("as_int", &[], A::Int),
    m("as_float", &[], A::Float),
    m("as_bool", &[], A::Bool),
    m("as_str", &[], A::Str),
    m("as_node", &[], A::Node),
    m("len", &[], A::Int),
    m("get", &[A::Str], A::Any),
    m("at", &[A::Int], A::Any),
    m("has", &[A::Str], A::Bool),
    m("keys", &[], A::Strs),
    m("text", &[], A::Str),
    // A new map: this one's entries, with `key` set. Nil counts as empty.
    m("with", &[A::Str, A::Any], A::Any),
    // This map changed in place, which every holder of it sees; false for nil
    // or a value that is not a map.
    m("set", &[A::Str, A::Any], A::Bool),
    // The same for a list: one more element at its end.
    m("push", &[A::Any], A::Bool),
    // A map's entry by key, or a list's element by index, taken out in place
    // and handed back; nil when there is none.
    m("remove", &[A::Any], A::Any),
    // The same value: maps by their entries, lists by their elements.
    m("equals", &[A::Any], A::Bool),
    // A new list of this one's elements in order: numbers by value, strings
    // by their text.
    m("sorted", &[], A::Any),
];

pub(crate) fn method(name: &str) -> Option<&'static Method> {
    METHODS.iter().find(|m| m.name == name)
}

pub(crate) fn ty(a: A) -> Ty {
    match a {
        A::Bool => Ty::Bool,
        A::Int => Ty::Int,
        A::Float => Ty::Float,
        A::Str => Ty::Str,
        A::Strs => Ty::Array(Box::new(Ty::Str)),
        A::Node => Ty::Node,
        A::Any => Ty::Any,
    }
}

/// The method's parameters as the import takes them: the `any` first.
pub(crate) fn params(m: &Method) -> Vec<Ty> {
    std::iter::once(Ty::Any)
        .chain(m.params.iter().map(|a| ty(*a)))
        .collect()
}
