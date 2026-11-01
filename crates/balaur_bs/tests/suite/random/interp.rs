//! The reference interpreter: what a random program answers, worked out
//! without the compiler, so a disagreement names a bug in one of the two.
//! It types every expression as the compiler does, so a conversion into or
//! out of an `any` happens where the compiler puts one.

use super::anys::{self, A};
use super::{E, Func, Op, S, T};
use balaur_bs::runtime::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub(super) enum V {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Ints(Rc<RefCell<Vec<i64>>>),
    /// A value: binding one copies it.
    Point([i64; 2]),
    /// `None` is `Shade::Dark`.
    Shade(Option<i64>),
    /// Keys in the order first set; every name holding it shares it.
    Ages(Rc<RefCell<Vec<(String, i64)>>>),
    Maybe(Option<i64>),
    Any(A),
}

impl V {
    fn kind(&self) -> T {
        match self {
            V::Int(_) => T::Int,
            V::Float(_) => T::Float,
            V::Bool(_) => T::Bool,
            V::Str(_) => T::Str,
            V::Ints(_) => T::Ints,
            V::Point(_) => T::Point,
            V::Shade(_) => T::Shade,
            V::Ages(_) => T::Ages,
            V::Maybe(_) => T::Maybe,
            V::Any(_) => T::Any,
        }
    }
}

#[derive(Debug)]
pub(super) enum Stop {
    /// The program traps, as every engine must.
    Trap,
    /// Outside what the interpreter models: a generator bug, or a shrink
    /// that cut too much.
    Bad(String),
}

pub(super) type Run<X> = Result<X, Stop>;

fn bad<X>(what: impl Into<String>) -> Run<X> {
    Err(Stop::Bad(what.into()))
}

/// What `main` answers. With `mul_adds` an int `*` adds: an interpreter
/// wrong on purpose, for testing the shrink.
pub(super) fn answer(funcs: &[Func], mul_adds: bool) -> Run<V> {
    let Some(main) = funcs.last().filter(|f| f.name == "main") else {
        return bad("no main");
    };
    Interp {
        funcs,
        scopes: vec![HashMap::new()],
        mul_adds,
    }
    .body(main)
}

pub(super) fn to_value(v: &V) -> Run<Value> {
    Ok(match v {
        V::Int(i) => Value::Int(*i),
        V::Float(f) => Value::Float(if f.is_nan() { f64::NAN } else { *f }),
        V::Bool(b) => Value::Bool(*b),
        V::Str(s) => Value::Str(s.clone()),
        v => return bad(format!("main answers {v:?}")),
    })
}

fn plain(t: T) -> bool {
    matches!(t, T::Int | T::Float | T::Bool | T::Str)
}

/// The type an `if` takes from its branches: an int beside a float is
/// widened, an `any` beside a plain value is read as that value's kind.
fn join(a: T, b: T) -> Option<T> {
    match (a, b) {
        _ if a == b => Some(a),
        (T::Int, T::Float) | (T::Float, T::Int) => Some(T::Float),
        (T::Any, p) | (p, T::Any) if plain(p) => Some(p),
        _ => None,
    }
}

fn binary_type(op: Op, a: T, b: T) -> Option<T> {
    if matches!(op, Op::And | Op::Or | Op::Eq | Op::Ne | Op::Lt | Op::Le) {
        return Some(T::Bool);
    }
    let (a, b) = match (a, b) {
        (T::Any, p) | (p, T::Any) if plain(p) => (p, p),
        (T::Any, T::Any) => (T::Float, T::Float),
        x => x,
    };
    match (a, b) {
        (T::Int, T::Int) => Some(T::Int),
        (T::Int | T::Float, T::Int | T::Float) => Some(T::Float),
        (T::Str, T::Str) if op == Op::Add => Some(T::Str),
        _ => None,
    }
}

fn method_type(name: &str) -> Option<T> {
    Some(match name {
        "as_int" | "len" => T::Int,
        "as_float" => T::Float,
        "as_bool" | "is_nil" | "has" | "equals" | "push" | "insert" => T::Bool,
        "as_str" | "kind" | "text" => T::Str,
        "remove" | "sorted" => T::Any,
        _ => return None,
    })
}

/// What the compiler types `e` as, given each variable's type and each
/// function's result.
pub(super) fn type_of(
    e: &E,
    var: &dyn Fn(&str) -> Option<T>,
    ret: &dyn Fn(&str) -> Option<T>,
) -> Option<T> {
    let ty = |e: &E| type_of(e, var, ret);
    Some(match e {
        E::Int(_)
        | E::Len(_)
        | E::At(..)
        | E::StrLen(_)
        | E::Field(..)
        | E::Get(..)
        | E::Count(_)
        | E::Or(..) => T::Int,
        E::Float(_) => T::Float,
        E::Bool(_) | E::Not(_) => T::Bool,
        E::Str(_) | E::Interp(_) => T::Str,
        E::Var(v) => var(v)?,
        E::Bin(op, a, b) => binary_type(*op, ty(a)?, ty(b)?)?,
        E::Neg(a) => match ty(a)? {
            T::Int => T::Int,
            T::Float | T::Any => T::Float,
            _ => return None,
        },
        E::If(_, a, b) => join(ty(a)?, ty(b)?)?,
        E::Match(_, dark, n, lit) => {
            let bound = |x: &str| if x == n { Some(T::Int) } else { var(x) };
            join(ty(dark)?, type_of(lit, &bound, ret)?)?
        }
        E::Call(f, _) => ret(f)?,
        E::Array(_) => T::Ints,
        E::Point(..) => T::Point,
        E::Dark | E::Lit(_) => T::Shade,
        E::Ages(_) => T::Ages,
        E::Nil => T::Maybe,
        E::Method(_, name, _) => method_type(name)?,
        E::Index(..) | E::NilAny | E::MapAny => T::Any,
    })
}

/// `v` where a `t` goes, converted as the compiler converts it.
fn coerce(v: V, t: T) -> Run<V> {
    if v.kind() == t {
        return Ok(v);
    }
    Ok(match (v, t) {
        (V::Int(i), T::Float) => V::Float(i as f64),
        (V::Int(i), T::Maybe) => V::Maybe(Some(i)),
        (v, T::Any) => V::Any(anys::boxed(&v)?),
        (V::Any(a), t) => anys::unboxed(&a, t)?,
        (v, t) => return bad(format!("{v:?} where {t:?} goes")),
    })
}

/// A value as `"{v}"` writes it.
fn shown(v: &V) -> Run<String> {
    Ok(match v {
        V::Int(i) => i.to_string(),
        V::Float(f) => format!("{f:?}"),
        V::Bool(b) => b.to_string(),
        V::Str(s) => s.clone(),
        V::Shade(_) => return bad("a Shade interpolated"),
        v => anys::text(&anys::boxed(v)?),
    })
}

fn index(len: usize, i: i64) -> Run<usize> {
    if len == 0 {
        return Err(Stop::Trap);
    }
    let n = i64::try_from(len).unwrap();
    Ok(usize::try_from(i.wrapping_rem(n).wrapping_add(n).wrapping_rem(n)).unwrap())
}

struct Interp<'a> {
    funcs: &'a [Func],
    scopes: Vec<HashMap<String, V>>,
    mul_adds: bool,
}

impl Interp<'_> {
    fn lookup(&self, n: &str) -> Option<&V> {
        self.scopes.iter().rev().find_map(|s| s.get(n))
    }
    fn get(&self, n: &str) -> Run<V> {
        self.lookup(n)
            .cloned()
            .ok_or_else(|| Stop::Bad(format!("`{n}` is not in scope")))
    }
    fn slot(&mut self, n: &str) -> Run<&mut V> {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|s| s.get_mut(n))
            .ok_or_else(|| Stop::Bad(format!("`{n}` is not in scope")))
    }
    fn declare(&mut self, n: &str, v: V) {
        self.scopes.last_mut().unwrap().insert(n.into(), v);
    }
    fn ty(&self, e: &E) -> Run<T> {
        let var = |n: &str| self.lookup(n).map(V::kind);
        let ret = |f: &str| self.funcs.iter().find(|g| g.name == f).map(|g| g.ret);
        type_of(e, &var, &ret).ok_or_else(|| Stop::Bad(format!("no type for {e:?}")))
    }

    fn int(&mut self, e: &E) -> Run<i64> {
        match self.eval(e)? {
            V::Int(i) => Ok(i),
            v => bad(format!("an int, got {v:?}")),
        }
    }
    /// A condition: an `any` is read by `as_bool`.
    fn truth(&mut self, e: &E) -> Run<bool> {
        match coerce(self.eval(e)?, T::Bool)? {
            V::Bool(b) => Ok(b),
            v => bad(format!("a bool, got {v:?}")),
        }
    }
    fn receiver(&mut self, e: &E) -> Run<A> {
        match self.eval(e)? {
            V::Any(a) => Ok(a),
            v => bad(format!("an `any` method of {v:?}")),
        }
    }

    /// `e` where a `t` goes: a map literal, `nil` or `[]` takes its type
    /// from there, anything else is converted.
    fn typed(&mut self, e: &E, t: T) -> Run<V> {
        match (e, t) {
            (E::Ages(pairs), T::Ages) => {
                let mut held = Vec::new();
                for (k, e) in pairs {
                    held.push(((*k).to_string(), self.int(e)?));
                }
                Ok(V::Ages(Rc::new(RefCell::new(held))))
            }
            (E::Nil, T::Maybe) => Ok(V::Maybe(None)),
            (E::Array(items), T::Ints) if items.is_empty() => Ok(V::Ints(Rc::default())),
            _ => coerce(self.eval(e)?, t),
        }
    }

    fn eval(&mut self, e: &E) -> Run<V> {
        Ok(match e {
            E::Int(i) => V::Int(*i),
            E::Float(f) => V::Float(*f),
            E::Bool(b) => V::Bool(*b),
            E::Str(s) => V::Str(s.clone()),
            E::Var(v) => self.get(v)?,
            E::Neg(a) => match self.eval(a)? {
                V::Int(i) => V::Int(i.wrapping_neg()),
                V::Float(f) => V::Float(-f),
                V::Any(a) => V::Float(-anys::as_float(&a)),
                v => return bad(format!("negated {v:?}")),
            },
            E::Not(a) => match self.eval(a)? {
                V::Bool(b) => V::Bool(!b),
                V::Any(a) => V::Bool(!anys::as_bool(&a)),
                v => return bad(format!("not {v:?}")),
            },
            E::Bin(op @ (Op::And | Op::Or), a, b) => {
                let first = self.truth(a)?;
                if first == (*op == Op::Or) {
                    V::Bool(first)
                } else {
                    V::Bool(self.truth(b)?)
                }
            }
            E::Bin(op, a, b) => self.binary(*op, a, b)?,
            E::If(c, a, b) => {
                let Some(t) = join(self.ty(a)?, self.ty(b)?) else {
                    return bad("the branches differ");
                };
                let pick = self.truth(c)?;
                let v = self.eval(if pick { a } else { b })?;
                coerce(v, t)?
            }
            E::Call(f, args) => self.call(f, args)?,
            E::Len(a) => match self.get(a)? {
                V::Ints(xs) => V::Int(i64::try_from(xs.borrow().len()).unwrap()),
                v => return bad(format!("len of {v:?}")),
            },
            E::At(a, i) => {
                let i = self.int(i)?;
                match self.get(a)? {
                    V::Ints(xs) => {
                        let xs = xs.borrow();
                        V::Int(xs[index(xs.len(), i)?])
                    }
                    v => return bad(format!("index into {v:?}")),
                }
            }
            E::StrLen(s) => match self.eval(s)? {
                V::Str(s) => V::Int(i64::try_from(s.len()).unwrap()),
                v => return bad(format!("len of {v:?}")),
            },
            E::Interp(pieces) => {
                let mut out = String::new();
                for (text, v) in pieces {
                    out.push_str(text);
                    out.push_str(&shown(&self.get(v)?)?);
                }
                V::Str(out)
            }
            E::Array(items) if !items.is_empty() => {
                let mut xs = Vec::new();
                for i in items {
                    xs.push(self.int(i)?);
                }
                V::Ints(Rc::new(RefCell::new(xs)))
            }
            E::Array(_) | E::Ages(_) | E::Nil => return bad("a literal with no type to take"),
            _ => self.held(e)?,
        })
    }

    /// A struct, an enum, a map, an optional or an `any`, made or read.
    fn held(&mut self, e: &E) -> Run<V> {
        Ok(match e {
            E::Point(x, y) => V::Point([self.int(x)?, self.int(y)?]),
            E::Field(v, f) => match self.get(v)? {
                V::Point(p) => V::Int(p[usize::from(*f == "y")]),
                v => return bad(format!("a field of {v:?}")),
            },
            E::Dark => V::Shade(None),
            E::Lit(e) => V::Shade(Some(self.int(e)?)),
            E::Match(v, dark, n, lit) => {
                let shade = self.get(v)?;
                self.scopes.push(HashMap::from([(n.clone(), V::Int(0))]));
                let lit_type = self.ty(lit);
                self.scopes.pop();
                let Some(t) = join(self.ty(dark)?, lit_type?) else {
                    return bad("the arms differ");
                };
                let v = match shade {
                    V::Shade(None) => self.eval(dark)?,
                    V::Shade(Some(k)) => {
                        self.scopes.push(HashMap::from([(n.clone(), V::Int(k))]));
                        let out = self.eval(lit);
                        self.scopes.pop();
                        out?
                    }
                    v => return bad(format!("matched {v:?}")),
                };
                coerce(v, t)?
            }
            E::Get(v, k, d) => {
                let found = match self.get(v)? {
                    V::Ages(m) => m.borrow().iter().find(|(key, _)| key == k).map(|(_, n)| *n),
                    v => return bad(format!("a key of {v:?}")),
                };
                // The default is an argument, worked out whether or not it is used.
                let d = self.int(d)?;
                V::Int(found.unwrap_or(d))
            }
            E::Count(v) => match self.get(v)? {
                V::Ages(m) => V::Int(i64::try_from(m.borrow().len()).unwrap()),
                v => return bad(format!("len of {v:?}")),
            },
            E::Or(v, d) => {
                let V::Maybe(held) = self.get(v)? else {
                    return bad("or on no optional");
                };
                let d = self.int(d)?;
                V::Int(held.unwrap_or(d))
            }
            E::Method(r, name, args) => self.method(r, name, args)?,
            E::Index(r, k) => {
                let a = self.receiver(r)?;
                if self.ty(k)? == T::Str {
                    let key = self.text_arg(k)?;
                    V::Any(anys::get(&a, &key))
                } else {
                    let at = self.int_arg(k)?;
                    V::Any(anys::at(&a, at))
                }
            }
            E::NilAny => V::Any(A::Nil),
            E::MapAny => V::Any(A::Map(Rc::default())),
            _ => return bad(format!("not a held kind: {e:?}")),
        })
    }

    fn text_arg(&mut self, e: &E) -> Run<String> {
        match self.typed(e, T::Str)? {
            V::Str(s) => Ok(s),
            v => bad(format!("a str, got {v:?}")),
        }
    }
    fn int_arg(&mut self, e: &E) -> Run<i64> {
        match self.typed(e, T::Int)? {
            V::Int(i) => Ok(i),
            v => bad(format!("an int, got {v:?}")),
        }
    }
    fn any_arg(&mut self, e: &E) -> Run<A> {
        match self.typed(e, T::Any)? {
            V::Any(a) => Ok(a),
            v => bad(format!("an `any`, got {v:?}")),
        }
    }

    /// A method of the prelude's `any`, its receiver then its arguments
    /// worked out in order, each converted to what the method takes.
    fn method(&mut self, r: &E, name: &str, args: &[E]) -> Run<V> {
        let arity = match name {
            "has" | "push" | "remove" | "equals" => 1,
            "insert" => 2,
            _ => 0,
        };
        if args.len() != arity {
            return bad(format!("`{name}` given {} arguments", args.len()));
        }
        let a = self.receiver(r)?;
        Ok(match name {
            "as_int" => V::Int(anys::as_int(&a)),
            "as_float" => V::Float(anys::as_float(&a)),
            "as_bool" => V::Bool(anys::as_bool(&a)),
            "as_str" => V::Str(anys::as_str(&a)),
            "len" => V::Int(anys::len(&a)),
            "kind" => V::Str(anys::kind(&a).into()),
            "is_nil" => V::Bool(matches!(a, A::Nil)),
            "text" => V::Str(anys::text(&a)),
            "sorted" => V::Any(anys::sorted(&a)),
            "has" => V::Bool(anys::has(&a, &self.text_arg(&args[0])?)),
            "push" => V::Bool(anys::push(&a, self.any_arg(&args[0])?)),
            "insert" => {
                let at = self.int_arg(&args[0])?;
                V::Bool(anys::insert(&a, at, self.any_arg(&args[1])?))
            }
            "remove" => V::Any(anys::remove(&a, &self.any_arg(&args[0])?)),
            "equals" => V::Bool(anys::same(&a, &self.any_arg(&args[0])?)),
            _ => return bad(format!("no method `{name}`")),
        })
    }

    fn binary(&mut self, op: Op, a: &E, b: &E) -> Run<V> {
        let (ta, tb) = (self.ty(a)?, self.ty(b)?);
        let (va, vb) = (self.eval(a)?, self.eval(b)?);
        let eq = matches!(op, Op::Eq | Op::Ne);
        // `==` beside an `any`, or between Points, maps, arrays or
        // optionals, compares both boxed, as `any.equals` does.
        let boxes = |t: T| {
            matches!(
                t,
                T::Any | T::Ints | T::Point | T::Ages | T::Maybe | T::Shade
            )
        };
        if eq && (boxes(ta) || boxes(tb)) {
            let same = anys::same(&anys::boxed(&va)?, &anys::boxed(&vb)?);
            return Ok(V::Bool(same == (op == Op::Eq)));
        }
        let (va, vb) = match (ta, tb) {
            (T::Any, p) if plain(p) => (coerce(va, p)?, vb),
            (p, T::Any) if plain(p) => (va, coerce(vb, p)?),
            (T::Any, T::Any) => (coerce(va, T::Float)?, coerce(vb, T::Float)?),
            _ => (va, vb),
        };
        Ok(match (va, vb) {
            (V::Int(x), V::Int(y)) => match op {
                Op::Add => V::Int(x.wrapping_add(y)),
                Op::Sub => V::Int(x.wrapping_sub(y)),
                Op::Mul if self.mul_adds => V::Int(x.wrapping_add(y)),
                Op::Mul => V::Int(x.wrapping_mul(y)),
                Op::Div => V::Int(x.checked_div(y).ok_or(Stop::Trap)?),
                Op::Rem if y == 0 => return Err(Stop::Trap),
                Op::Rem => V::Int(x.wrapping_rem(y)),
                Op::Lt => V::Bool(x < y),
                Op::Le => V::Bool(x <= y),
                Op::Eq => V::Bool(x == y),
                Op::Ne => V::Bool(x != y),
                _ => return bad(format!("{op:?} on ints")),
            },
            (x @ (V::Int(_) | V::Float(_)), y @ (V::Int(_) | V::Float(_))) => {
                let (V::Float(x), V::Float(y)) = (coerce(x, T::Float)?, coerce(y, T::Float)?)
                else {
                    unreachable!("widened")
                };
                match op {
                    Op::Add => V::Float(x + y),
                    Op::Sub => V::Float(x - y),
                    Op::Mul => V::Float(x * y),
                    Op::Div => V::Float(x / y),
                    Op::Lt => V::Bool(x < y),
                    Op::Le => V::Bool(x <= y),
                    #[allow(clippy::float_cmp, reason = "the language compares floats exactly")]
                    Op::Eq => V::Bool(x == y),
                    #[allow(clippy::float_cmp, reason = "the language compares floats exactly")]
                    Op::Ne => V::Bool(x != y),
                    _ => return bad(format!("{op:?} on floats")),
                }
            }
            (V::Bool(x), V::Bool(y)) if eq => V::Bool((x == y) == (op == Op::Eq)),
            (V::Str(x), V::Str(y)) => match op {
                Op::Add => V::Str(x + &y),
                Op::Eq => V::Bool(x == y),
                Op::Ne => V::Bool(x != y),
                _ => return bad(format!("{op:?} on strings")),
            },
            (x, y) => return bad(format!("{op:?} on {x:?} and {y:?}")),
        })
    }

    /// A helper's answer. A `&mut` argument is the caller's variable, copied
    /// in when its turn comes and stored back after the call.
    fn call(&mut self, f: &str, args: &[E]) -> Run<V> {
        let funcs = self.funcs;
        let Some(func) = funcs.iter().find(|g| g.name == f) else {
            return bad(format!("no helper `{f}`"));
        };
        if func.params.len() != args.len() {
            return bad(format!("`{f}` given {} arguments", args.len()));
        }
        let mut frame = HashMap::new();
        let mut back = Vec::new();
        for (p, a) in func.params.iter().zip(args) {
            let v = if p.by_ref {
                let E::Var(caller) = a else {
                    return bad("a `&mut` argument that is no variable");
                };
                back.push((p.name.as_str(), caller.as_str()));
                self.get(caller)?
            } else {
                self.typed(a, p.t)?
            };
            if v.kind() != p.t {
                return bad(format!("{v:?} for a {:?} parameter", p.t));
            }
            frame.insert(p.name.clone(), v);
        }
        let saved = std::mem::replace(&mut self.scopes, vec![frame]);
        let out = self.body(func);
        let stored: Vec<(&str, Option<V>)> = back
            .iter()
            .map(|(p, caller)| (*caller, self.scopes[0].get(*p).cloned()))
            .collect();
        self.scopes = saved;
        let out = out?;
        for (caller, v) in stored {
            *self.slot(caller)? = v.expect("a parameter stays in its frame");
        }
        Ok(out)
    }

    fn body(&mut self, f: &Func) -> Run<V> {
        self.run(&f.body)?;
        let v = self.eval(&f.tail)?;
        coerce(v, f.ret)
    }

    fn block(&mut self, stmts: &[S], scope: HashMap<String, V>) -> Run<()> {
        self.scopes.push(scope);
        let r = self.run(stmts);
        self.scopes.pop();
        r
    }

    fn run(&mut self, stmts: &[S]) -> Run<()> {
        for st in stmts {
            match st {
                S::Let(n, e) => {
                    let v = self.eval(e)?;
                    self.declare(n, v);
                }
                S::LetTyped(n, t, e) => {
                    let v = self.typed(e, *t)?;
                    self.declare(n, v);
                }
                S::Set(n, e) => {
                    let t = self.get(n)?.kind();
                    let v = self.typed(e, t)?;
                    *self.slot(n)? = v;
                }
                S::SetField(p, f, e) => {
                    let v = self.int(e)?;
                    let V::Point(held) = self.slot(p)? else {
                        return bad("a field of no Point");
                    };
                    held[usize::from(*f == "y")] = v;
                }
                S::SetKey(m, k, e) => {
                    let v = self.int(e)?;
                    let V::Ages(held) = self.get(m)? else {
                        return bad("a key of no map");
                    };
                    let mut held = held.borrow_mut();
                    match held.iter_mut().find(|(key, _)| key == k) {
                        Some(slot) => slot.1 = v,
                        None => held.push(((*k).to_string(), v)),
                    }
                }
                S::Put(target, e) => {
                    let (r, key) = match target {
                        E::Index(r, k) => (&**r, (**k).clone()),
                        _ => return bad("a write to no key"),
                    };
                    let a = self.receiver(r)?;
                    if self.ty(&key)? == T::Int {
                        let at = self.int_arg(&key)?;
                        anys::put(&a, at, self.any_arg(e)?);
                    } else {
                        let key = self.text_arg(&key)?;
                        anys::set(&a, &key, self.any_arg(e)?);
                    }
                }
                S::Do(e) => {
                    self.eval(e)?;
                }
                S::Each(x, r, body) => {
                    let a = self.receiver(r)?;
                    for i in 0..anys::len(&a) {
                        let item = V::Any(anys::at(&a, i));
                        self.block(body, HashMap::from([(x.clone(), item)]))?;
                    }
                }
                S::Push(n, e) => {
                    let v = self.int(e)?;
                    let V::Ints(xs) = self.get(n)? else {
                        return bad("a push to no array");
                    };
                    xs.borrow_mut().push(v);
                }
                S::SetAt(n, i, e) => {
                    let i = self.int(i)?;
                    let v = self.int(e)?;
                    let V::Ints(xs) = self.get(n)? else {
                        return bad("an index into no array");
                    };
                    let len = xs.borrow().len();
                    xs.borrow_mut()[index(len, i)?] = v;
                }
                S::If(c, a, b) => {
                    let pick = self.truth(c)?;
                    self.block(if pick { a } else { b }, HashMap::new())?;
                }
                S::For(i, k, body) => {
                    for n in 0..*k {
                        self.block(body, HashMap::from([(i.clone(), V::Int(n))]))?;
                    }
                }
                S::While(c, k, body) => {
                    self.declare(c, V::Int(0));
                    loop {
                        let V::Int(n) = self.get(c)? else {
                            return bad("a counter that is no int");
                        };
                        if n >= *k {
                            break;
                        }
                        self.block(body, HashMap::new())?;
                        *self.slot(c)? = V::Int(n + 1);
                    }
                }
            }
        }
        Ok(())
    }
}
