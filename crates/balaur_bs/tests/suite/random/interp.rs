//! The reference interpreter: what a random program answers, worked out
//! without the compiler, so a disagreement names a bug in one of the two.

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
    Point(Rc<RefCell<[i64; 2]>>),
    /// `None` is `Shade::Dark`.
    Shade(Option<i64>),
    /// Keys in the order first set.
    Ages(Rc<RefCell<Vec<(&'static str, i64)>>>),
    Maybe(Option<i64>),
}

pub(super) type Run<T> = Result<T, &'static str>;

pub(super) struct Interp<'a> {
    pub(super) funcs: &'a [Func],
    pub(super) scopes: Vec<HashMap<String, V>>,
}

impl Interp<'_> {
    fn get(&self, n: &str) -> V {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(n))
            .cloned()
            .expect("a variable in scope")
    }
    fn set(&mut self, n: &str, v: V) {
        let scope = self
            .scopes
            .iter_mut()
            .rev()
            .find(|s| s.contains_key(n))
            .expect("a variable in scope");
        scope.insert(n.into(), v);
    }
    fn int(&mut self, e: &E) -> Run<i64> {
        match self.eval(e)? {
            V::Int(i) => Ok(i),
            v => panic!("an int, got {v:?}"),
        }
    }
    fn float(&mut self, e: &E) -> Run<f64> {
        match self.eval(e)? {
            V::Float(f) => Ok(f),
            V::Int(i) => Ok(i as f64),
            v => panic!("a float, got {v:?}"),
        }
    }
    fn index(len: usize, i: i64) -> usize {
        let n = i64::try_from(len).unwrap();
        usize::try_from(i.wrapping_rem(n).wrapping_add(n).wrapping_rem(n)).unwrap()
    }
    fn is_float(&self, e: &E) -> bool {
        match e {
            E::Float(_) => true,
            E::Var(v) => matches!(self.get(v), V::Float(_)),
            E::Bin(op, a, b) => {
                matches!(op, Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Rem)
                    && (self.is_float(a) || self.is_float(b))
            }
            E::Neg(a) => self.is_float(a),
            E::If(_, a, b) => self.is_float(a) || self.is_float(b),
            E::Call(f, _) => self.funcs.iter().any(|g| g.name == *f && g.ret == T::Float),
            _ => false,
        }
    }

    fn eval(&mut self, e: &E) -> Run<V> {
        Ok(match e {
            E::Int(i) => V::Int(*i),
            E::Float(f) => V::Float(*f),
            E::Bool(b) => V::Bool(*b),
            E::Str(s) => V::Str(s.clone()),
            E::Var(v) => self.get(v),
            E::Neg(a) => match self.eval(a)? {
                V::Int(i) => V::Int(i.wrapping_neg()),
                V::Float(f) => V::Float(-f),
                v => panic!("negated {v:?}"),
            },
            E::Not(a) => match self.eval(a)? {
                V::Bool(b) => V::Bool(!b),
                v => panic!("not {v:?}"),
            },
            E::Bin(Op::And, a, b) => match self.eval(a)? {
                V::Bool(false) => V::Bool(false),
                _ => self.eval(b)?,
            },
            E::Bin(Op::Or, a, b) => match self.eval(a)? {
                V::Bool(true) => V::Bool(true),
                _ => self.eval(b)?,
            },
            E::Bin(op, a, b) => self.binary(*op, a, b)?,
            E::If(c, a, b) => {
                let pick = matches!(self.eval(c)?, V::Bool(true));
                let either_float = self.is_float(a) || self.is_float(b);
                let v = self.eval(if pick { a } else { b })?;
                match v {
                    V::Int(i) if either_float => V::Float(i as f64),
                    v => v,
                }
            }
            E::Call(f, args) => {
                let func = self.funcs.iter().find(|g| g.name == *f).expect("a helper");
                let mut frame = HashMap::new();
                for ((p, t), a) in func.params.iter().zip(args) {
                    let v = if *t == T::Float {
                        V::Float(self.float(a)?)
                    } else {
                        self.eval(a)?
                    };
                    frame.insert(p.clone(), v);
                }
                let saved = std::mem::replace(&mut self.scopes, vec![frame]);
                let out = self.body(func);
                self.scopes = saved;
                out?
            }
            E::Len(a) => match self.get(a) {
                V::Ints(xs) => V::Int(i64::try_from(xs.borrow().len()).unwrap()),
                v => panic!("len of {v:?}"),
            },
            E::At(a, i) => {
                let i = self.int(i)?;
                match self.get(a) {
                    V::Ints(xs) => {
                        let xs = xs.borrow();
                        V::Int(xs[Self::index(xs.len(), i)])
                    }
                    v => panic!("index into {v:?}"),
                }
            }
            E::StrLen(s) => match self.eval(s)? {
                V::Str(s) => V::Int(i64::try_from(s.len()).unwrap()),
                v => panic!("len of {v:?}"),
            },
            E::Interp(pieces) => {
                let mut out = String::new();
                for (text, v) in pieces {
                    out.push_str(text);
                    match self.get(v) {
                        V::Int(i) => out.push_str(&i.to_string()),
                        V::Bool(b) => out.push_str(&b.to_string()),
                        v => panic!("interpolated {v:?}"),
                    }
                }
                V::Str(out)
            }
            E::Array(items) => {
                let mut xs = Vec::new();
                for i in items {
                    xs.push(self.int(i)?);
                }
                V::Ints(Rc::new(RefCell::new(xs)))
            }
            E::Point(..)
            | E::Field(..)
            | E::Dark
            | E::Lit(_)
            | E::Match(..)
            | E::Ages(_)
            | E::Get(..)
            | E::Count(_)
            | E::Nil
            | E::Or(..) => self.held(e)?,
        })
    }

    /// A struct, an enum, a map or an optional, made or read.
    fn held(&mut self, e: &E) -> Run<V> {
        Ok(match e {
            E::Point(x, y) => {
                let x = self.int(x)?;
                let y = self.int(y)?;
                V::Point(Rc::new(RefCell::new([x, y])))
            }
            E::Field(v, f) => match self.get(v) {
                V::Point(p) => V::Int(p.borrow()[usize::from(*f == "y")]),
                v => panic!("a field of {v:?}"),
            },
            E::Dark => V::Shade(None),
            E::Lit(e) => V::Shade(Some(self.int(e)?)),
            E::Match(v, dark, n, lit) => match self.get(v) {
                V::Shade(None) => self.eval(dark)?,
                V::Shade(Some(k)) => {
                    self.scopes.push(HashMap::from([(n.clone(), V::Int(k))]));
                    let out = self.eval(lit);
                    self.scopes.pop();
                    out?
                }
                v => panic!("matched {v:?}"),
            },
            E::Ages(pairs) => {
                let mut held = Vec::new();
                for (k, e) in pairs {
                    held.push((*k, self.int(e)?));
                }
                V::Ages(Rc::new(RefCell::new(held)))
            }
            E::Get(v, k, d) => {
                let found = match self.get(v) {
                    V::Ages(m) => m.borrow().iter().find(|(key, _)| key == k).map(|(_, n)| *n),
                    v => panic!("a key of {v:?}"),
                };
                // The default is an argument, worked out whether or not it is used.
                let d = self.int(d)?;
                V::Int(found.unwrap_or(d))
            }
            E::Count(v) => match self.get(v) {
                V::Ages(m) => V::Int(i64::try_from(m.borrow().len()).unwrap()),
                v => panic!("len of {v:?}"),
            },
            E::Nil => V::Maybe(None),
            E::Or(v, d) => {
                let held = match self.get(v) {
                    V::Maybe(o) => o,
                    v => panic!("or on {v:?}"),
                };
                let d = self.int(d)?;
                V::Int(held.unwrap_or(d))
            }
            _ => unreachable!("a held kind"),
        })
    }

    fn binary(&mut self, op: Op, a: &E, b: &E) -> Run<V> {
        if self.is_float(a) || self.is_float(b) {
            let (x, y) = (self.float(a)?, self.float(b)?);
            return Ok(match op {
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
                _ => panic!("{op:?} on floats"),
            });
        }
        Ok(match (self.eval(a)?, self.eval(b)?) {
            (V::Int(x), V::Int(y)) => match op {
                Op::Add => V::Int(x.wrapping_add(y)),
                Op::Sub => V::Int(x.wrapping_sub(y)),
                Op::Mul => V::Int(x.wrapping_mul(y)),
                Op::Div => V::Int(x.checked_div(y).ok_or("trap")?),
                Op::Rem if y == 0 => return Err("trap"),
                Op::Rem => V::Int(x.wrapping_rem(y)),
                Op::Lt => V::Bool(x < y),
                Op::Le => V::Bool(x <= y),
                Op::Eq => V::Bool(x == y),
                Op::Ne => V::Bool(x != y),
                _ => panic!("{op:?} on ints"),
            },
            (V::Bool(x), V::Bool(y)) => match op {
                Op::Eq => V::Bool(x == y),
                Op::Ne => V::Bool(x != y),
                _ => panic!("{op:?} on bools"),
            },
            (V::Str(x), V::Str(y)) => match op {
                Op::Add => V::Str(x + &y),
                Op::Eq => V::Bool(x == y),
                _ => panic!("{op:?} on strings"),
            },
            (x, y) => panic!("{op:?} on {x:?} and {y:?}"),
        })
    }

    pub(super) fn body(&mut self, f: &Func) -> Run<V> {
        self.run(&f.body)?;
        let v = self.eval(&f.tail)?;
        Ok(match v {
            V::Int(i) if f.ret == T::Float => V::Float(i as f64),
            v => v,
        })
    }

    fn run(&mut self, stmts: &[S]) -> Run<()> {
        for st in stmts {
            match st {
                S::Let(n, e) => {
                    let v = self.eval(e)?;
                    self.scopes.last_mut().unwrap().insert(n.clone(), v);
                }
                S::LetTyped(n, t, e) => {
                    let v = match (t, self.eval(e)?) {
                        (T::Maybe, V::Int(i)) => V::Maybe(Some(i)),
                        (_, v) => v,
                    };
                    self.scopes.last_mut().unwrap().insert(n.clone(), v);
                }
                S::SetField(p, f, e) => {
                    let v = self.int(e)?;
                    if let V::Point(held) = self.get(p) {
                        held.borrow_mut()[usize::from(*f == "y")] = v;
                    }
                }
                S::SetKey(m, k, e) => {
                    let v = self.int(e)?;
                    if let V::Ages(held) = self.get(m) {
                        let mut held = held.borrow_mut();
                        match held.iter_mut().find(|(key, _)| key == k) {
                            Some(slot) => slot.1 = v,
                            None => held.push((*k, v)),
                        }
                    }
                }
                S::Set(n, e) => {
                    let v = self.eval(e)?;
                    self.set(n, v);
                }
                S::Push(n, e) => {
                    let v = self.int(e)?;
                    if let V::Ints(xs) = self.get(n) {
                        xs.borrow_mut().push(v);
                    }
                }
                S::SetAt(n, i, e) => {
                    let i = self.int(i)?;
                    let v = self.int(e)?;
                    if let V::Ints(xs) = self.get(n) {
                        let len = xs.borrow().len();
                        xs.borrow_mut()[Self::index(len, i)] = v;
                    }
                }
                S::If(c, a, b) => {
                    let pick = matches!(self.eval(c)?, V::Bool(true));
                    self.scopes.push(HashMap::new());
                    let r = self.run(if pick { a } else { b });
                    self.scopes.pop();
                    r?;
                }
                S::For(i, k, body) => {
                    for n in 0..*k {
                        self.scopes.push(HashMap::from([(i.clone(), V::Int(n))]));
                        let r = self.run(body);
                        self.scopes.pop();
                        r?;
                    }
                }
                S::While(c, k, body) => {
                    self.scopes.last_mut().unwrap().insert(c.clone(), V::Int(0));
                    loop {
                        let V::Int(n) = self.get(c) else {
                            unreachable!()
                        };
                        if n >= *k {
                            break;
                        }
                        self.scopes.push(HashMap::new());
                        let r = self.run(body);
                        self.scopes.pop();
                        r?;
                        let V::Int(n) = self.get(c) else {
                            unreachable!()
                        };
                        self.set(c, V::Int(n + 1));
                    }
                }
            }
        }
        Ok(())
    }
}

pub(super) fn to_value(v: &V) -> Value {
    match v {
        V::Int(i) => Value::Int(*i),
        V::Float(f) => Value::Float(if f.is_nan() { f64::NAN } else { *f }),
        V::Bool(b) => Value::Bool(*b),
        V::Str(s) => Value::Str(s.clone()),
        V::Ints(xs) => Value::Array(xs.borrow().iter().map(|i| Value::Int(*i)).collect()),
        V::Point(_) | V::Shade(_) | V::Ages(_) | V::Maybe(_) => {
            unreachable!("main answers an int, a float, a bool or a str")
        }
    }
}
