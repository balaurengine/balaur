// The types the translator infers. Rune has none, so every one here is a
// guess from literals, operators, call sites and field uses, joined until
// nothing moves. A guess that cannot be made is `any`, which `.bs` has.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Ty {
    /// Nothing seen yet.
    Unknown,
    /// No value: a statement, `return;`.
    Void,
    /// `None` or `()` before anything says what it is the absence of.
    Nil,
    Int,
    Float,
    Bool,
    Str,
    Node,
    Any,
    Array(Box<Ty>),
    Opt(Box<Ty>),
    /// An object literal's struct, by shape id (see `Shapes`).
    Shape(usize),
    /// A function reference, by signature id.
    Fn(usize),
    /// An engine value type the docs name: `Vec3`, `Color`.
    Named(&'static str),
}

impl Ty {
    pub(crate) fn opt(t: Ty) -> Ty {
        match t {
            Ty::Opt(_) | Ty::Any | Ty::Nil => t,
            Ty::Unknown | Ty::Void => Ty::Nil,
            t => Ty::Opt(Box::new(t)),
        }
    }

    /// What `.unwrap()` hands back.
    pub(crate) fn inner(&self) -> Ty {
        match self {
            Ty::Opt(t) => (**t).clone(),
            Ty::Nil => Ty::Unknown,
            t => t.clone(),
        }
    }

    pub(crate) fn elem(&self) -> Ty {
        match self {
            Ty::Array(t) => (**t).clone(),
            Ty::Str => Ty::Str,
            Ty::Any => Ty::Any,
            _ => Ty::Unknown,
        }
    }

    pub(crate) fn is_num(&self) -> bool {
        matches!(self, Ty::Int | Ty::Float)
    }

    pub(crate) fn known(&self) -> bool {
        !matches!(self, Ty::Unknown)
    }
}

/// A struct inferred from object literals and field uses. Shapes merge
/// (union-find) when one value is seen holding two of them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Shape {
    /// Names offered by where the literal sits, strongest first.
    pub hints: Vec<(u8, String)>,
    pub fields: BTreeMap<String, Ty>,
    /// The order fields were first seen in, for the declaration.
    pub order: Vec<String>,
    /// Why this is `any` rather than a struct, when it is.
    pub free: Option<String>,
    /// Read or written by computed key: `any` unless a field is ever named.
    pub dynamic: Option<String>,
    /// Module index of the first literal, or of the first use.
    pub home: Option<(usize, u32)>,
    /// A tuple: fields are `_0`, `_1`…
    pub tuple: bool,
    /// The file's own `pub struct`, holding what `this` held.
    pub script: bool,
    /// Fields with a default, `exports()` in a script.
    pub defaults: BTreeMap<String, String>,
    pub literals: usize,
    /// Literal sites by module: (count, first offset, most keys in one). The
    /// struct is declared where its fullest literal is.
    pub sites: BTreeMap<usize, (usize, u32, usize)>,
    /// Keys each literal names, by site: a wide struct whose literals name
    /// few of its fields is a property bag, kept as `any`.
    pub site_keys: BTreeMap<(usize, u32), usize>,
    /// Times an `any` was handed where this struct was wanted: data the
    /// engine hands over (a document) that a literal also builds.
    pub any_fed: std::collections::BTreeSet<(usize, usize)>,
    /// Sites where a value of this struct is handed on as an `any`.
    pub to_any: std::collections::BTreeSet<(usize, usize)>,
    /// Fields read or written by name (`x.f`), not only in literals.
    pub accessed: std::collections::BTreeSet<String>,
    /// Fields some write gave an `any`: read out as the field's type there,
    /// and `any` when nothing else typed them.
    pub any_fields: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Shapes {
    parent: Vec<usize>,
    pub data: Vec<Shape>,
    pub changed: bool,
}

impl Shapes {
    pub(crate) fn add(&mut self, s: Shape) -> usize {
        self.parent.push(self.parent.len());
        self.data.push(s);
        self.changed = true;
        self.data.len() - 1
    }

    pub(crate) fn find(&self, mut id: usize) -> usize {
        while self.parent[id] != id {
            id = self.parent[id];
        }
        id
    }

    pub(crate) fn get(&self, id: usize) -> &Shape {
        &self.data[self.find(id)]
    }

    pub(crate) fn get_mut(&mut self, id: usize) -> &mut Shape {
        let r = self.find(id);
        &mut self.data[r]
    }

    pub(crate) fn hint(&mut self, id: usize, strength: u8, name: &str) {
        if name.is_empty() {
            return;
        }
        let s = self.get_mut(id);
        if !s.hints.iter().any(|(_, n)| n == name) {
            s.hints.push((strength, name.to_string()));
        }
    }

    /// Used by computed key: free-form data, unless a field is ever named.
    pub(crate) fn make_free(&mut self, id: usize, why: &str) {
        let s = self.get_mut(id);
        if s.dynamic.is_none() && !s.script {
            s.dynamic = Some(why.to_string());
            self.changed = true;
        }
    }

    /// Free-form whatever its fields: a widget spec the engine reads by key.
    pub(crate) fn force_free(&mut self, id: usize, why: &str) {
        let s = self.get_mut(id);
        if s.free.is_none() && !s.script {
            s.free = Some(why.to_string());
            self.changed = true;
        }
    }

    /// Free-form: forced, or used by computed key with no field ever used
    /// by name.
    pub(crate) fn is_free(&self, id: usize) -> bool {
        let s = self.get(id);
        s.free.is_some() || (s.dynamic.is_some() && (s.fields.is_empty() || s.accessed.is_empty()))
    }

    pub(crate) fn free_reason(&self, id: usize) -> String {
        let s = self.get(id);
        s.free
            .clone()
            .or_else(|| s.dynamic.clone())
            .unwrap_or_default()
    }

    pub(crate) fn field(&self, id: usize, name: &str) -> Option<Ty> {
        self.get(id).fields.get(name).cloned()
    }

    /// `name` holds a `t` too.
    pub(crate) fn set_field(&mut self, id: usize, name: &str, t: Ty) {
        let r = self.find(id);
        if t == Ty::Any {
            let s = &mut self.data[r];
            if !s.any_fields.iter().any(|f| f == name) {
                s.any_fields.push(name.to_string());
                self.changed = true;
            }
            if !s.order.iter().any(|f| f == name) {
                s.order.push(name.to_string());
                s.fields.insert(name.to_string(), Ty::Unknown);
            }
            return;
        }
        let old = self.data[r].fields.get(name).cloned();
        let joined = match &old {
            Some(o) => self.join(o, &t),
            None => t,
        };
        let r = self.find(r);
        let s = &mut self.data[r];
        if !s.order.iter().any(|f| f == name) {
            s.order.push(name.to_string());
        }
        if old.as_ref() != Some(&joined) {
            // A field's type changing is progress only when it is a real one.
            if old.is_none() || joined.known() {
                self.changed = true;
            }
            s.fields.insert(name.to_string(), joined);
        }
    }

    pub(crate) fn union(&mut self, a: usize, b: usize) -> usize {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return a;
        }
        // Keep the script struct, and otherwise the older shape, as the root.
        let (root, gone) = if self.data[b].script || (!self.data[a].script && b < a) {
            (b, a)
        } else {
            (a, b)
        };
        self.parent[gone] = root;
        self.changed = true;
        let moved = std::mem::take(&mut self.data[gone]);
        // A literal's field order is the declaration's, ahead of guesses.
        if moved.literals > 0 && self.data[root].literals == 0 {
            let mut order = moved.order.clone();
            order.extend(
                self.data[root]
                    .order
                    .iter()
                    .filter(|f| !moved.order.contains(f))
                    .cloned(),
            );
            self.data[root].order = order;
        }
        {
            let r = &mut self.data[root];
            for h in moved.hints {
                if !r.hints.iter().any(|(_, n)| *n == h.1) {
                    r.hints.push(h);
                }
            }
            if r.free.is_none() && !r.script {
                r.free = moved.free;
            }
            if r.dynamic.is_none() {
                r.dynamic = moved.dynamic;
            }
            // A struct lives where a literal builds it, not where a use guessed it.
            r.home = match (r.home, moved.home) {
                (Some(x), Some(y)) if (r.literals > 0) == (moved.literals > 0) => Some(x.min(y)),
                (Some(x), Some(y)) => Some(if r.literals > 0 { x } else { y }),
                (x, y) => x.or(y),
            };
            r.tuple &= moved.tuple;
            r.literals += moved.literals;
            r.any_fed.extend(moved.any_fed);
            r.to_any.extend(moved.to_any);
            for (m, (n, at, keys)) in moved.sites {
                let e = r.sites.entry(m).or_insert((0, at, 0));
                e.0 += n;
                e.1 = e.1.min(at);
                e.2 = e.2.max(keys);
            }
            r.site_keys.extend(moved.site_keys);
            r.accessed.extend(moved.accessed);
            for f in moved.any_fields {
                if !r.any_fields.contains(&f) {
                    r.any_fields.push(f);
                }
            }
            for (k, v) in moved.defaults {
                r.defaults.entry(k).or_insert(v);
            }
        }
        for f in moved.order {
            let t = moved.fields.get(&f).cloned().unwrap_or(Ty::Unknown);
            self.set_field(root, &f, t);
        }
        self.find(root)
    }

    /// The least type holding both.
    pub(crate) fn join(&mut self, a: &Ty, b: &Ty) -> Ty {
        use Ty::{Any, Array, Float, Fn, Int, Nil, Opt, Shape, Unknown, Void};
        match (a, b) {
            (Unknown, t) | (t, Unknown) => t.clone(),
            (x, y) if x == y => match x {
                Shape(i) => Shape(self.find(*i)),
                _ => x.clone(),
            },
            (Any, _) | (_, Any) => Any,
            (Void | Nil, t) | (t, Void | Nil) => Ty::opt(t.clone()),
            (Opt(x), Opt(y)) => {
                let j = self.join(x, y);
                Ty::opt(j)
            }
            (Opt(x), t) | (t, Opt(x)) => {
                let j = self.join(x, t);
                Ty::opt(j)
            }
            (Int, Float) | (Float, Int) => Float,
            (Array(x), Array(y)) => Array(Box::new(self.join(x, y))),
            (Shape(x), Shape(y)) => {
                let (x, y) = (*x, *y);
                // A tuple and an object are not one struct.
                if self.get(x).tuple != self.get(y).tuple {
                    return Any;
                }
                Shape(self.union(x, y))
            }
            (Fn(x), Fn(_)) => Fn(*x),
            _ => Any,
        }
    }

    /// Whether a value of `t` holds the shape `id` inside it, which a value
    /// struct cannot do.
    pub(crate) fn contains(&self, t: &Ty, id: usize, depth: u32) -> bool {
        if depth > 8 {
            return true;
        }
        match t {
            Ty::Shape(s) => {
                let s = self.find(*s);
                s == self.find(id)
                    || self.data[s]
                        .fields
                        .values()
                        .any(|f| self.contains(f, id, depth + 1))
            }
            Ty::Opt(t) => self.contains(t, id, depth + 1),
            _ => false,
        }
    }
}
