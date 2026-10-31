//! What an `any` holds and what its methods do, as `src/prelude/any.bs` and
//! `codegen/convert.rs` define them: a map or a list is one cell every holder
//! shares, a scalar a cell nothing changes.

use super::T;
use super::interp::{Run, Stop, V};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub(super) enum A {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Rc<RefCell<Vec<A>>>),
    /// Keys in the order first set.
    Map(Rc<RefCell<Vec<(String, A)>>>),
}

fn list(items: Vec<A>) -> A {
    A::List(Rc::new(RefCell::new(items)))
}

fn map(pairs: Vec<(String, A)>) -> A {
    A::Map(Rc::new(RefCell::new(pairs)))
}

/// A typed value in a new cell; an `any` is its own cell.
pub(super) fn boxed(v: &V) -> Run<A> {
    Ok(match v {
        V::Int(i) | V::Maybe(Some(i)) => A::Int(*i),
        V::Float(f) => A::Float(*f),
        V::Bool(b) => A::Bool(*b),
        V::Str(s) => A::Str(s.clone()),
        V::Ints(xs) => list(xs.borrow().iter().map(|i| A::Int(*i)).collect()),
        V::Point([x, y]) => map(vec![("x".into(), A::Int(*x)), ("y".into(), A::Int(*y))]),
        V::Ages(m) => map(m
            .borrow()
            .iter()
            .map(|(k, n)| (k.clone(), A::Int(*n)))
            .collect()),
        V::Maybe(None) => A::Nil,
        V::Any(a) => a.clone(),
        V::Shade(_) => return Err(Stop::Bad("a Shade boxed".into())),
    })
}

/// An `any` read as a `t`: a scalar by its `as_` method, anything else
/// rebuilt from what the cell holds, a missing part its type's zero.
pub(super) fn unboxed(a: &A, t: T) -> Run<V> {
    Ok(match t {
        T::Int => V::Int(as_int(a)),
        T::Float => V::Float(as_float(a)),
        T::Bool => V::Bool(as_bool(a)),
        T::Str => V::Str(as_str(a)),
        T::Point => V::Point([as_int(&get(a, "x")), as_int(&get(a, "y"))]),
        // Walked as `for i in 0..a.len() { a.at(i) }`, so a map or a string
        // gives as many zeros as it has keys or bytes.
        T::Ints => V::Ints(Rc::new(RefCell::new(
            (0..len(a)).map(|i| as_int(&at(a, i))).collect(),
        ))),
        T::Ages => V::Ages(Rc::new(RefCell::new(
            keys(a)
                .into_iter()
                .map(|k| {
                    let n = as_int(&get(a, &k));
                    (k, n)
                })
                .collect(),
        ))),
        T::Maybe if matches!(a, A::Nil) => V::Maybe(None),
        T::Maybe => V::Maybe(Some(as_int(a))),
        T::Any => V::Any(a.clone()),
        T::Shade => return Err(Stop::Bad("a Shade unboxed".into())),
    })
}

pub(super) fn kind(a: &A) -> &'static str {
    match a {
        A::Nil => "nil",
        A::Bool(_) => "bool",
        A::Int(_) => "int",
        A::Float(_) => "float",
        A::Str(_) => "str",
        A::List(_) => "list",
        A::Map(_) => "map",
    }
}

pub(super) fn as_int(a: &A) -> i64 {
    match a {
        A::Int(i) => *i,
        A::Float(f) => *f as i64,
        A::Bool(true) => 1,
        _ => 0,
    }
}

pub(super) fn as_float(a: &A) -> f64 {
    match a {
        A::Float(f) => *f,
        A::Int(i) => *i as f64,
        _ => 0.0,
    }
}

pub(super) fn as_bool(a: &A) -> bool {
    matches!(a, A::Bool(true))
}

pub(super) fn as_str(a: &A) -> String {
    match a {
        A::Str(s) => s.clone(),
        _ => String::new(),
    }
}

pub(super) fn len(a: &A) -> i64 {
    let n = match a {
        A::List(xs) => xs.borrow().len(),
        A::Map(m) => m.borrow().len(),
        A::Str(s) => s.len(),
        _ => 0,
    };
    i64::try_from(n).unwrap()
}

pub(super) fn get(a: &A, key: &str) -> A {
    match a {
        A::Map(m) => m
            .borrow()
            .iter()
            .find(|(k, _)| k == key)
            .map_or(A::Nil, |(_, v)| v.clone()),
        _ => A::Nil,
    }
}

pub(super) fn at(a: &A, index: i64) -> A {
    match a {
        A::List(xs) => usize::try_from(index)
            .ok()
            .and_then(|i| xs.borrow().get(i).cloned())
            .unwrap_or(A::Nil),
        _ => A::Nil,
    }
}

pub(super) fn has(a: &A, key: &str) -> bool {
    matches!(a, A::Map(m) if m.borrow().iter().any(|(k, _)| k == key))
}

fn keys(a: &A) -> Vec<String> {
    match a {
        A::Map(m) => m.borrow().iter().map(|(k, _)| k.clone()).collect(),
        _ => Vec::new(),
    }
}

/// A string as it is; anything else written out, its strings quoted.
pub(super) fn text(a: &A) -> String {
    match a {
        A::Str(s) => s.clone(),
        _ => quoted(a),
    }
}

fn quoted(a: &A) -> String {
    match a {
        A::Nil => "nil".into(),
        A::Bool(b) => b.to_string(),
        A::Int(i) => i.to_string(),
        A::Float(f) => format!("{f:?}"),
        A::Str(s) => escaped(s),
        A::List(xs) => format!(
            "[{}]",
            xs.borrow()
                .iter()
                .map(quoted)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        A::Map(m) => format!(
            "{{{}}}",
            m.borrow()
                .iter()
                .map(|(k, v)| format!("{}: {}", escaped(k), quoted(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn escaped(raw: &str) -> String {
    let mut out = vec![b'"'];
    for b in raw.bytes() {
        match b {
            b'"' => out.extend(b"\\\""),
            b'\\' => out.extend(b"\\\\"),
            b'\n' => out.extend(b"\\n"),
            b'\r' => out.extend(b"\\r"),
            b'\t' => out.extend(b"\\t"),
            0 => out.extend(b"\\0"),
            b if b < 32 || b == 127 => out.extend(format!("\\u{{{b:x}}}").bytes()),
            b => out.push(b),
        }
    }
    out.push(b'"');
    String::from_utf8(out).expect("bytes of a str and ASCII escapes")
}

/// A map or a list made again, all the way down.
fn copy(a: &A) -> A {
    match a {
        A::List(xs) => list(xs.borrow().iter().map(copy).collect()),
        A::Map(m) => map(m
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), copy(v)))
            .collect()),
        _ => a.clone(),
    }
}

/// A new map: this one's entries copied, with `key` set to a copy of `item`.
pub(super) fn with(a: &A, key: &str, item: &A) -> A {
    let mut pairs = match a {
        A::Map(m) => m
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), copy(v)))
            .collect(),
        _ => Vec::new(),
    };
    match pairs.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = copy(item),
        None => pairs.push((key.into(), copy(item))),
    }
    map(pairs)
}

pub(super) fn set(a: &A, key: &str, item: A) -> bool {
    let A::Map(m) = a else { return false };
    let mut m = m.borrow_mut();
    match m.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = item,
        None => m.push((key.into(), item)),
    }
    true
}

pub(super) fn push(a: &A, item: A) -> bool {
    let A::List(xs) = a else { return false };
    xs.borrow_mut().push(item);
    true
}

pub(super) fn put(a: &A, index: i64, item: A) -> bool {
    let A::List(xs) = a else { return false };
    let mut xs = xs.borrow_mut();
    match usize::try_from(index).ok().filter(|i| *i < xs.len()) {
        Some(i) => {
            xs[i] = item;
            true
        }
        None => false,
    }
}

pub(super) fn insert(a: &A, index: i64, item: A) -> bool {
    let A::List(xs) = a else { return false };
    let mut xs = xs.borrow_mut();
    match usize::try_from(index).ok().filter(|i| *i <= xs.len()) {
        Some(i) => {
            xs.insert(i, item);
            true
        }
        None => false,
    }
}

/// A list's element by an int, or a map's entry by a str, taken out.
pub(super) fn remove(a: &A, key: &A) -> A {
    match (a, key) {
        (A::List(xs), A::Int(i)) => {
            let mut xs = xs.borrow_mut();
            match usize::try_from(*i).ok().filter(|i| *i < xs.len()) {
                Some(i) => xs.remove(i),
                None => A::Nil,
            }
        }
        (A::Map(m), A::Str(k)) => {
            let mut m = m.borrow_mut();
            match m.iter().position(|(key, _)| key == k) {
                Some(i) => m.remove(i).1,
                None => A::Nil,
            }
        }
        _ => A::Nil,
    }
}

/// A new list of this one's elements, copied, in order, by the prelude's own
/// merge sort: its order is no total one across kinds, so a library sort
/// could place them otherwise.
pub(super) fn sorted(a: &A) -> A {
    let items = match a {
        A::List(xs) => xs.borrow().iter().map(copy).collect(),
        _ => Vec::new(),
    };
    list(merge_sort(items))
}

fn merge_sort(mut items: Vec<A>) -> Vec<A> {
    if items.len() < 2 {
        return items;
    }
    let right = merge_sort(items.split_off(items.len() / 2));
    let left = merge_sort(items);
    let mut out = Vec::with_capacity(left.len() + right.len());
    let (mut l, mut r) = (left.into_iter().peekable(), right.into_iter().peekable());
    loop {
        let left_first = match (l.peek(), r.peek()) {
            (None, None) => return out,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(x), Some(y)) => order(x, y) <= 0,
        };
        out.extend(if left_first { l.next() } else { r.next() });
    }
}

// Numbers by value, -0.0 before 0.0 and NaN last; two strings by their
// bytes; anything else by how it is written.
fn order(a: &A, b: &A) -> i64 {
    match (a, b) {
        (A::Int(_) | A::Float(_), A::Int(_) | A::Float(_)) => {
            let (x, y) = (as_float(a), as_float(b));
            match (x.is_nan(), y.is_nan()) {
                _ if x < y => -1,
                _ if x > y => 1,
                (true, true) => 0,
                (true, false) => 1,
                (false, true) => -1,
                _ if x == 0.0 => i64::from(y.is_sign_negative()) - i64::from(x.is_sign_negative()),
                _ => 0,
            }
        }
        (A::Str(x), A::Str(y)) => order_bytes(x, y),
        _ => order_bytes(&quoted(a), &quoted(b)),
    }
}

fn order_bytes(a: &str, b: &str) -> i64 {
    a.bytes().zip(b.bytes()).find(|(x, y)| x != y).map_or_else(
        || i64::try_from(a.len()).unwrap() - i64::try_from(b.len()).unwrap(),
        |(x, y)| i64::from(x) - i64::from(y),
    )
}

/// Numbers by number whatever their kind, maps by key whatever their order,
/// lists by their elements, nil only to nil.
pub(super) fn same(a: &A, b: &A) -> bool {
    // An int against a float: equal only to the same float, so not to -0.0.
    let whole = |i: i64, f: f64| {
        #[allow(clippy::float_cmp, reason = "the language compares exactly")]
        let equal = i as f64 == f;
        equal && !(f == 0.0 && f.is_sign_negative())
    };
    match (a, b) {
        (A::Int(i), A::Float(f)) | (A::Float(f), A::Int(i)) => whole(*i, *f),
        (A::Nil, A::Nil) => true,
        (A::Bool(x), A::Bool(y)) => x == y,
        (A::Int(x), A::Int(y)) => x == y,
        #[allow(clippy::float_cmp, reason = "the language compares exactly")]
        (A::Float(x), A::Float(y)) => x == y,
        (A::Str(x), A::Str(y)) => x == y,
        (A::List(x), A::List(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| same(a, b))
        }
        (A::Map(x), A::Map(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            x.len() == y.len()
                && x.iter().all(|(k, v)| {
                    y.iter()
                        .find(|(l, _)| l == k)
                        .is_some_and(|(_, w)| same(v, w))
                })
        }
        _ => false,
    }
}
