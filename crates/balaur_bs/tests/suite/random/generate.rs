//! The generator: a well-typed program from a seed, every value folded into
//! what its function answers so a wrong one anywhere shows.

use super::{ANY_KEYS, E, Func, KEYS, Op, Param, S, T};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, in_: u64) -> bool {
        self.below(in_) == 0
    }
}

/// What a generator may name: the variables in scope, by type.
#[derive(Clone, Default)]
pub(super) struct Scope {
    pub(super) vars: Vec<(String, T)>,
}

impl Scope {
    fn of(&self, t: T) -> Vec<String> {
        self.vars
            .iter()
            .filter(|(_, u)| *u == t)
            .map(|(n, _)| n.clone())
            .collect()
    }
}

/// A helper already made: what `main` and later helpers may call.
struct Sig {
    name: String,
    params: Vec<(T, bool)>,
    ret: T,
}

pub(super) struct Gen {
    rng: Rng,
    names: usize,
    funcs: Vec<Sig>,
}

/// The kinds a string interpolates.
const SHOWN: [T; 9] = [
    T::Int,
    T::Bool,
    T::Float,
    T::Str,
    T::Ints,
    T::Point,
    T::Ages,
    T::Maybe,
    T::Any,
];

/// What a typed `let` may read out of an `any`.
const UNBOXED: [T; 8] = [
    T::Int,
    T::Float,
    T::Bool,
    T::Str,
    T::Ints,
    T::Point,
    T::Ages,
    T::Maybe,
];

fn plain(t: T) -> bool {
    matches!(t, T::Int | T::Float | T::Bool | T::Str)
}

/// What a helper takes and gives.
const PASSED: [T; 6] = [T::Int, T::Int, T::Float, T::Point, T::Ages, T::Any];

impl Gen {
    pub(super) fn new(seed: u64) -> Gen {
        Gen {
            rng: Rng(seed),
            names: 0,
            funcs: Vec::new(),
        }
    }

    fn name(&mut self, stem: &str) -> String {
        self.names += 1;
        format!("{stem}{}", self.names)
    }

    fn pick<X: Clone>(&mut self, xs: &[X]) -> X {
        xs[usize::try_from(self.rng.below(xs.len() as u64)).unwrap()].clone()
    }

    fn some<X: Clone>(&mut self, xs: &[X]) -> Option<X> {
        (!xs.is_empty()).then(|| self.pick(xs))
    }

    fn key(&mut self) -> E {
        E::Str(self.pick(&ANY_KEYS).into())
    }

    /// An index into an `any` list: mostly in or near its range.
    fn small(&mut self, s: &Scope, d: u32) -> E {
        if self.rng.chance(2) {
            E::Int(i64::try_from(self.rng.below(4)).unwrap() - 1)
        } else {
            self.expr(T::Int, s, d)
        }
    }

    fn int_lit(&mut self) -> i64 {
        match self.rng.below(10) {
            0 => i64::MAX - i64::try_from(self.rng.below(3)).unwrap(),
            1 => i64::MIN + i64::try_from(self.rng.below(3)).unwrap(),
            2 => i64::try_from(self.rng.below(1_000_000)).unwrap() - 500_000,
            _ => i64::try_from(self.rng.below(20)).unwrap() - 5,
        }
    }

    fn float_lit(&mut self) -> f64 {
        let whole = f64::from(u32::try_from(self.rng.below(2000)).unwrap()) - 1000.0;
        match self.rng.below(6) {
            0 => 0.0,
            1 => -0.0,
            2 => whole / 7.0,
            3 => whole * 1e10,
            _ => whole / 4.0,
        }
    }

    pub(super) fn expr(&mut self, t: T, s: &Scope, depth: u32) -> E {
        let leaf = depth == 0 || self.rng.chance(4);
        if leaf {
            if self.rng.chance(2)
                && let Some(v) = self.some(&s.of(t))
            {
                return E::Var(v);
            }
            return match t {
                T::Int => E::Int(self.int_lit()),
                T::Float => E::Float(self.float_lit()),
                T::Bool => E::Bool(self.rng.chance(2)),
                T::Str => E::Str(self.pick(&["", "a", "bc", "x y"]).into()),
                T::Ints => E::Array(
                    (0..=self.rng.below(3))
                        .map(|_| E::Int(self.int_lit()))
                        .collect(),
                ),
                T::Any if self.rng.chance(2) => E::NilAny,
                T::Any => E::MapAny,
                _ => self.composite(t, s, 0),
            };
        }
        let d = depth - 1;
        match t {
            T::Int => self.int_expr(s, d),
            T::Float => self.float_expr(s, d),
            T::Bool => self.bool_expr(s, d),
            T::Str => self.str_expr(s, d),
            T::Ints => self.ints_expr(s, d),
            T::Any => self.any_expr(s, d),
            _ => self.composite(t, s, d),
        }
    }

    /// A Point, a Shade, a map or an optional: named, or a literal.
    fn composite(&mut self, t: T, s: &Scope, d: u32) -> E {
        if self.rng.chance(3)
            && let Some(e) = self.named(t, s, d)
        {
            return e;
        }
        match t {
            T::Point => E::Point(
                Box::new(self.expr(T::Int, s, d)),
                Box::new(self.expr(T::Int, s, d)),
            ),
            T::Shade if self.rng.chance(3) => E::Dark,
            T::Shade => E::Lit(Box::new(self.expr(T::Int, s, d))),
            T::Ages => {
                let mut keys = KEYS.to_vec();
                let mut pairs = Vec::new();
                for _ in 0..self.rng.below(4) {
                    let k =
                        keys.remove(usize::try_from(self.rng.below(keys.len() as u64)).unwrap());
                    pairs.push((k, self.expr(T::Int, s, d)));
                }
                E::Ages(pairs)
            }
            T::Maybe if self.rng.chance(3) => E::Nil,
            T::Maybe => self.expr(T::Int, s, d),
            _ => unreachable!("a held kind"),
        }
    }

    /// A held value that is no literal: a variable, a call, or an `if`
    /// between two of those, for where a map literal has no type to take.
    fn named(&mut self, t: T, s: &Scope, d: u32) -> Option<E> {
        let passed = matches!(t, T::Point | T::Ages);
        match self.rng.below(4) {
            0 if passed && d > 0 => {
                let c = self.expr(T::Bool, s, d - 1);
                let a = self.named(t, s, d - 1)?;
                let b = self.named(t, s, d - 1)?;
                Some(E::If(Box::new(c), Box::new(a), Box::new(b)))
            }
            1 if passed => self
                .call(t, s, d)
                .or_else(|| self.some(&s.of(t)).map(E::Var)),
            _ => match self.some(&s.of(t)) {
                Some(v) => Some(E::Var(v)),
                None if passed => self.call(t, s, d),
                None => None,
            },
        }
    }

    /// An expression whose type is `any`.
    fn any_expr(&mut self, s: &Scope, d: u32) -> E {
        let held = s.of(T::Any);
        let leaf = |g: &mut Gen| match g.some(&held) {
            Some(v) if !g.rng.chance(4) => E::Var(v),
            _ if g.rng.chance(2) => E::NilAny,
            _ => E::MapAny,
        };
        if d == 0 || self.rng.chance(4) {
            return leaf(self);
        }
        let d = d - 1;
        match self.rng.below(9) {
            0 | 1 => {
                let r = Box::new(self.any_expr(s, d));
                let k = self.pick(&ANY_KEYS);
                match self.rng.below(3) {
                    0 => E::Method(r, "get", vec![E::Str(k.into())]),
                    1 => E::Index(r, Box::new(E::Str(k.into()))),
                    _ => E::Dot(r, k),
                }
            }
            2 => {
                let r = Box::new(self.any_expr(s, d));
                let at = self.small(s, d);
                if self.rng.chance(2) {
                    E::Method(r, "at", vec![at])
                } else {
                    E::Index(r, Box::new(at))
                }
            }
            3 => {
                let r = Box::new(self.any_expr(s, d));
                let k = self.key();
                E::Method(r, "with", vec![k, self.any_arg(s, d)])
            }
            4 => {
                let r = Box::new(self.any_expr(s, d));
                let k = if self.rng.chance(2) {
                    self.key()
                } else {
                    self.small(s, d)
                };
                E::Method(r, "remove", vec![k])
            }
            5 => self.call(T::Any, s, d).unwrap_or_else(|| leaf(self)),
            6 => E::If(
                Box::new(self.expr(T::Bool, s, d)),
                Box::new(self.any_expr(s, d)),
                Box::new(self.any_expr(s, d)),
            ),
            7 => E::Method(Box::new(self.any_expr(s, d)), "sorted", vec![]),
            _ => leaf(self),
        }
    }

    /// Where an `any` is wanted: one, or a typed value boxed into one.
    fn any_arg(&mut self, s: &Scope, d: u32) -> E {
        if self.rng.chance(2) {
            self.any_expr(s, d)
        } else {
            self.boxed(s, d)
        }
    }

    /// A typed value where an `any` goes: the cell it is boxed into is new,
    /// so no name holds it yet.
    fn boxed(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(8) {
            0 => self.expr(T::Int, s, d),
            1 => self.expr(T::Float, s, d),
            2 => self.expr(T::Bool, s, d),
            3 => self.expr(T::Str, s, d),
            4 => self.expr(T::Ints, s, d),
            5 => self.expr(T::Point, s, d),
            6 => self
                .named(T::Ages, s, d)
                .unwrap_or_else(|| self.expr(T::Int, s, d)),
            _ => self.some(&s.of(T::Maybe)).map_or(E::NilAny, E::Var),
        }
    }

    /// What goes into an `any`'s map or list: only a new cell, so no map or
    /// list ever holds itself.
    fn fresh(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(6) {
            0 => E::NilAny,
            1 => E::MapAny,
            2 => {
                let r = Box::new(self.any_expr(s, d));
                let k = self.key();
                E::Method(r, "with", vec![k, self.any_arg(s, d)])
            }
            _ => self.boxed(s, d),
        }
    }

    /// An `any` variable, or a key of one: what a write changes in place.
    fn any_place(&mut self, s: &Scope) -> Option<E> {
        let v = Box::new(E::Var(self.some(&s.of(T::Any))?));
        let k = self.pick(&ANY_KEYS);
        Some(match self.rng.below(5) {
            0 => E::Method(v, "get", vec![E::Str(k.into())]),
            1 => E::Dot(v, k),
            _ => *v,
        })
    }

    /// A method that changes the `any` it is called on; with `answers`, one
    /// that answers a bool.
    fn change(&mut self, r: E, s: &Scope, d: u32, answers: bool) -> E {
        let r = Box::new(r);
        match self.rng.below(if answers { 4 } else { 5 }) {
            0 => {
                let k = self.key();
                E::Method(r, "set", vec![k, self.fresh(s, d)])
            }
            1 => E::Method(r, "push", vec![self.fresh(s, d)]),
            2 => {
                let at = self.small(s, d);
                E::Method(r, "put", vec![at, self.fresh(s, d)])
            }
            3 => {
                let at = self.small(s, d);
                E::Method(r, "insert", vec![at, self.fresh(s, d)])
            }
            _ => {
                let k = self.key();
                E::Method(r, "remove", vec![k])
            }
        }
    }

    /// An int read out of a held value in scope, or `None` when none is.
    fn read_held(&mut self, s: &Scope, d: u32) -> Option<E> {
        let kinds: Vec<T> = [T::Point, T::Shade, T::Ages, T::Maybe]
            .into_iter()
            .filter(|t| !s.of(*t).is_empty())
            .collect();
        let t = self.some(&kinds)?;
        let v = self.pick(&s.of(t));
        Some(match t {
            T::Point => E::Field(v, if self.rng.chance(2) { "x" } else { "y" }),
            T::Shade => {
                let n = self.name("m");
                let mut inner = s.clone();
                inner.vars.push((n.clone(), T::Int));
                let dark = self.expr(T::Int, s, d);
                let lit = self.expr(T::Int, &inner, d);
                E::Match(v, Box::new(dark), n, Box::new(lit))
            }
            T::Ages if self.rng.chance(3) => E::Count(v),
            T::Ages => {
                let k = self.pick(&KEYS);
                E::Get(v, k, Box::new(self.expr(T::Int, s, d)))
            }
            _ => E::Or(v, Box::new(self.expr(T::Int, s, d))),
        })
    }

    /// Whether to read an `any` here: always when one is in scope, now and
    /// then from `nil()` or `any_map()` when none is.
    fn reads_any(&mut self, s: &Scope) -> bool {
        !s.of(T::Any).is_empty() || self.rng.chance(8)
    }

    fn int_expr(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(13) {
            9 | 10 => self
                .read_held(s, d)
                .unwrap_or_else(|| E::Int(self.int_lit())),
            11 if self.reads_any(s) => {
                let r = Box::new(self.any_expr(s, d));
                E::Method(r, if self.rng.chance(2) { "as_int" } else { "len" }, vec![])
            }
            12 if self.reads_any(s) => {
                // An `any` beside an int is read as one.
                let op = self.pick(&[Op::Add, Op::Sub, Op::Mul]);
                let (r, n) = (self.any_expr(s, d), self.expr(T::Int, s, d));
                let (a, b) = if self.rng.chance(2) { (r, n) } else { (n, r) };
                E::Bin(op, Box::new(a), Box::new(b))
            }
            0..=2 => {
                let op = self.pick(&[Op::Add, Op::Sub, Op::Mul]);
                E::Bin(
                    op,
                    Box::new(self.expr(T::Int, s, d)),
                    Box::new(self.expr(T::Int, s, d)),
                )
            }
            3 => {
                // A divisor in 2..=18, or now and then any int, which may trap.
                let op = if self.rng.chance(2) { Op::Div } else { Op::Rem };
                let by = self.expr(T::Int, s, d);
                let by = if self.rng.chance(8) {
                    by
                } else {
                    E::Bin(
                        Op::Add,
                        Box::new(E::Bin(Op::Rem, Box::new(by), Box::new(E::Int(9)))),
                        Box::new(E::Int(10)),
                    )
                };
                E::Bin(op, Box::new(self.expr(T::Int, s, d)), Box::new(by))
            }
            4 => E::Neg(Box::new(self.expr(T::Int, s, d))),
            5 => E::If(
                Box::new(self.expr(T::Bool, s, d)),
                Box::new(self.expr(T::Int, s, d)),
                Box::new(self.expr(T::Int, s, d)),
            ),
            6 => {
                let Some(a) = self.some(&s.of(T::Ints)) else {
                    return E::StrLen(Box::new(self.expr(T::Str, s, d)));
                };
                if self.rng.chance(2) {
                    E::Len(a)
                } else {
                    E::At(a, Box::new(self.expr(T::Int, s, d)))
                }
            }
            7 => self
                .call(T::Int, s, d)
                .unwrap_or_else(|| E::Int(self.int_lit())),
            _ => E::StrLen(Box::new(self.expr(T::Str, s, d))),
        }
    }

    fn float_expr(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(7) {
            0..=2 => {
                let op = self.pick(&[Op::Add, Op::Sub, Op::Mul, Op::Div]);
                // Now and then an int on one side: it is widened.
                let (a, b) = match self.rng.below(4) {
                    0 => (self.expr(T::Int, s, d), self.expr(T::Float, s, d)),
                    1 => (self.expr(T::Float, s, d), self.expr(T::Int, s, d)),
                    _ => (self.expr(T::Float, s, d), self.expr(T::Float, s, d)),
                };
                E::Bin(op, Box::new(a), Box::new(b))
            }
            3 => E::Neg(Box::new(self.expr(T::Float, s, d))),
            4 => E::If(
                Box::new(self.expr(T::Bool, s, d)),
                Box::new(self.expr(T::Float, s, d)),
                Box::new(self.expr(T::Float, s, d)),
            ),
            5 if self.reads_any(s) => {
                // Read as a float: by `as_float`, negated, or beside a float
                // or another `any`.
                let r = self.any_expr(s, d);
                match self.rng.below(4) {
                    0 => E::Method(Box::new(r), "as_float", vec![]),
                    1 => E::Neg(Box::new(r)),
                    2 => {
                        let op = self.pick(&[Op::Add, Op::Sub, Op::Mul, Op::Div]);
                        E::Bin(op, Box::new(r), Box::new(self.expr(T::Float, s, d)))
                    }
                    _ => {
                        let op = self.pick(&[Op::Add, Op::Sub, Op::Mul, Op::Div]);
                        E::Bin(op, Box::new(r), Box::new(self.any_expr(s, d)))
                    }
                }
            }
            _ => self
                .call(T::Float, s, d)
                .unwrap_or_else(|| E::Float(self.float_lit())),
        }
    }

    fn bool_expr(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(7) {
            0 | 1 => {
                let op = self.pick(&[Op::Lt, Op::Le, Op::Eq, Op::Ne]);
                let side = if self.rng.chance(3) { T::Float } else { T::Int };
                E::Bin(
                    op,
                    Box::new(self.expr(side, s, d)),
                    Box::new(self.expr(side, s, d)),
                )
            }
            2 => {
                let op = if self.rng.chance(2) { Op::And } else { Op::Or };
                E::Bin(
                    op,
                    Box::new(self.expr(T::Bool, s, d)),
                    Box::new(self.expr(T::Bool, s, d)),
                )
            }
            3 => E::Not(Box::new(self.expr(T::Bool, s, d))),
            4 if self.reads_any(s) => self.any_bool(s, d),
            5 => self.equal_held(s, d),
            _ => E::Bin(
                Op::Eq,
                Box::new(self.expr(T::Str, s, d)),
                Box::new(self.expr(T::Str, s, d)),
            ),
        }
    }

    /// A bool asked of an `any`, or got by changing one.
    fn any_bool(&mut self, s: &Scope, d: u32) -> E {
        let r = self.any_expr(s, d);
        let eq = self.pick(&[Op::Eq, Op::Ne]);
        match self.rng.below(9) {
            0 => E::Method(Box::new(r), self.pick(&["as_bool", "is_nil"]), vec![]),
            1 => E::Method(Box::new(r), "has", vec![self.key()]),
            2 => E::Method(Box::new(r), "equals", vec![self.any_arg(s, d)]),
            3 => {
                let t = self.pick(&[T::Int, T::Float, T::Bool, T::Str]);
                let plain = self.expr(t, s, d);
                let (a, b) = if self.rng.chance(2) {
                    (r, plain)
                } else {
                    (plain, r)
                };
                E::Bin(eq, Box::new(a), Box::new(b))
            }
            4 => E::Bin(eq, Box::new(r), Box::new(self.any_expr(s, d))),
            5 => E::Not(Box::new(r)),
            6 => {
                let op = self.pick(&[Op::Lt, Op::Le]);
                E::Bin(op, Box::new(r), Box::new(self.expr(T::Int, s, d)))
            }
            _ => self.change(r, s, d, true),
        }
    }

    /// Two Points, maps or arrays compared: as values, by what they hold.
    fn equal_held(&mut self, s: &Scope, d: u32) -> E {
        let op = self.pick(&[Op::Eq, Op::Ne]);
        let t = self.pick(&[T::Point, T::Ages, T::Ints]);
        let pair = if t == T::Ages {
            self.named(T::Ages, s, d).zip(self.named(T::Ages, s, d))
        } else {
            Some((self.expr(t, s, d), self.expr(t, s, d)))
        };
        let Some((a, b)) = pair else {
            return E::Bool(self.rng.chance(2));
        };
        E::Bin(op, Box::new(a), Box::new(b))
    }

    fn str_expr(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(4) {
            0 => E::Bin(
                Op::Add,
                Box::new(self.expr(T::Str, s, d)),
                Box::new(self.expr(T::Str, s, d)),
            ),
            1 => {
                let shown: Vec<String> = SHOWN.iter().flat_map(|t| s.of(*t)).collect();
                let mut pieces = Vec::new();
                for _ in 0..=self.rng.below(2) {
                    let Some(v) = self.some(&shown) else {
                        break;
                    };
                    let text = self.pick(&["", "n=", " "]).to_string();
                    pieces.push((text, v));
                }
                if pieces.is_empty() {
                    E::Str("q".into())
                } else {
                    E::Interp(pieces)
                }
            }
            2 if self.reads_any(s) => {
                let r = Box::new(self.any_expr(s, d));
                E::Method(r, self.pick(&["as_str", "kind", "text"]), vec![])
            }
            _ => E::If(
                Box::new(self.expr(T::Bool, s, d)),
                Box::new(self.expr(T::Str, s, d)),
                Box::new(self.expr(T::Str, s, d)),
            ),
        }
    }

    fn ints_expr(&mut self, s: &Scope, d: u32) -> E {
        E::Array(
            (0..=self.rng.below(3))
                .map(|_| self.expr(T::Int, s, d))
                .collect(),
        )
    }

    fn call(&mut self, ret: T, s: &Scope, d: u32) -> Option<E> {
        let points = !s.of(T::Point).is_empty();
        let fitting: Vec<usize> = (0..self.funcs.len())
            .filter(|&i| {
                let f = &self.funcs[i];
                f.ret == ret && (points || f.params.iter().all(|(_, by_ref)| !by_ref))
            })
            .collect();
        let i = self.some(&fitting)?;
        self.call_to(i, s, d)
    }

    /// A call to helper `i`, or `None` when it changes a Point and none is
    /// in scope.
    fn call_to(&mut self, i: usize, s: &Scope, d: u32) -> Option<E> {
        let points = s.of(T::Point);
        let params = self.funcs[i].params.clone();
        if points.is_empty() && params.iter().any(|(_, by_ref)| *by_ref) {
            return None;
        }
        let mut args = Vec::new();
        for (t, by_ref) in params {
            args.push(if by_ref {
                E::Var(self.pick(&points))
            } else if t == T::Any {
                self.any_arg(s, d)
            } else if self.rng.chance(8) {
                // Read out of an `any` as the parameter's type.
                self.any_expr(s, d)
            } else {
                self.expr(t, s, d)
            });
        }
        Some(E::Call(self.funcs[i].name.clone(), args))
    }

    fn stmts(&mut self, s: &mut Scope, n: u64, depth: u32) -> Vec<S> {
        let mut out = Vec::new();
        for _ in 0..n {
            let k = self.rng.below(15);
            match k {
                0 | 1 | 9 | 12 => self.binding(k, s, &mut out),
                2 | 3 | 7 | 8 | 10 | 11 => self.write(k, s, &mut out),
                4 | 5 | 6 | 13 if depth > 0 => self.nested(k, s, depth - 1, &mut out),
                _ => {}
            }
        }
        out
    }

    /// A new variable: a value of any kind, one read out of an `any`, or a
    /// helper's answer.
    fn binding(&mut self, k: u64, s: &mut Scope, out: &mut Vec<S>) {
        match k {
            12 if !self.funcs.is_empty() => {
                let i = usize::try_from(self.rng.below(self.funcs.len() as u64)).unwrap();
                out.extend(self.bind_call(i, s));
            }
            0 | 1 => {
                let t = self.pick(&[
                    T::Int,
                    T::Int,
                    T::Float,
                    T::Bool,
                    T::Str,
                    T::Ints,
                    T::Point,
                    T::Shade,
                    T::Ages,
                    T::Maybe,
                    T::Any,
                    T::Any,
                ]);
                let e = if t == T::Any {
                    self.any_arg(s, 3)
                } else {
                    self.expr(t, s, 3)
                };
                let n = self.name("v");
                s.vars.push((n.clone(), t));
                if matches!(t, T::Point | T::Shade | T::Ages | T::Maybe | T::Any) {
                    out.push(S::LetTyped(n, t, e));
                } else {
                    out.push(S::Let(n, e));
                }
            }
            9 if !s.of(T::Any).is_empty() => {
                // A value read out of an `any` by the type it is bound to.
                let t = self.pick(&UNBOXED);
                let e = self.any_expr(s, 3);
                let n = self.name("v");
                out.push(S::LetTyped(n.clone(), t, e));
                if t == T::Ints {
                    // Never empty, so an index into it stays in range.
                    out.push(S::Push(n.clone(), self.expr(T::Int, s, 2)));
                }
                s.vars.push((n, t));
            }
            _ => {}
        }
    }

    /// A write: a whole variable, a field, a key, an element, or an `any`
    /// changed in place.
    fn write(&mut self, k: u64, s: &Scope, out: &mut Vec<S>) {
        match k {
            7 if !s.of(T::Point).is_empty() => {
                let p = self.pick(&s.of(T::Point));
                let field = if self.rng.chance(2) { "x" } else { "y" };
                out.push(S::SetField(p, field, self.expr(T::Int, s, 2)));
            }
            8 if !s.of(T::Ages).is_empty() => {
                let m = self.pick(&s.of(T::Ages));
                let k = self.pick(&KEYS);
                out.push(S::SetKey(m, k, self.expr(T::Int, s, 2)));
            }
            2 => {
                // A loop's own counter is never written: only `let`s and parameters.
                let settable: Vec<(String, T)> = s
                    .vars
                    .iter()
                    .filter(|(n, t)| match t {
                        T::Int => !n.starts_with('i'),
                        T::Float | T::Point | T::Ages | T::Any => true,
                        _ => false,
                    })
                    .cloned()
                    .collect();
                if let Some((v, t)) = self.some(&settable) {
                    let e = if t == T::Any {
                        self.any_arg(s, 3)
                    } else {
                        self.expr(t, s, 3)
                    };
                    out.push(S::Set(v, e));
                }
            }
            3 if !s.of(T::Ints).is_empty() => {
                let a = self.pick(&s.of(T::Ints));
                if self.rng.chance(2) {
                    out.push(S::Push(a, self.expr(T::Int, s, 2)));
                } else {
                    let at = self.expr(T::Int, s, 2);
                    out.push(S::SetAt(a, at, self.expr(T::Int, s, 2)));
                }
            }
            10 => {
                if let Some(r) = self.any_place(s) {
                    let target = match self.rng.below(3) {
                        0 => E::Dot(Box::new(r), self.pick(&ANY_KEYS)),
                        1 => E::Index(Box::new(r), Box::new(self.small(s, 2))),
                        _ => E::Index(Box::new(r), Box::new(self.key())),
                    };
                    out.push(S::Put(target, self.fresh(s, 2)));
                }
            }
            11 => {
                if let Some(r) = self.any_place(s) {
                    out.push(S::Do(self.change(r, s, 2, false)));
                }
            }
            _ => {}
        }
    }

    /// An `if`, a loop or a walk of an `any`, its body `depth` deep.
    fn nested(&mut self, k: u64, s: &Scope, depth: u32, out: &mut Vec<S>) {
        match k {
            4 => {
                let c = self.expr(T::Bool, s, 2);
                let mut inner = s.clone();
                let n = self.rng.below(3) + 1;
                let a = self.stmts(&mut inner, n, depth);
                let mut inner = s.clone();
                let n = self.rng.below(3);
                let b = self.stmts(&mut inner, n, depth);
                out.push(S::If(c, a, b));
            }
            5 => {
                let i = self.name("i");
                let mut inner = s.clone();
                inner.vars.push((i.clone(), T::Int));
                let n = self.rng.below(3) + 1;
                let body = self.stmts(&mut inner, n, depth);
                out.push(S::For(i, i64::try_from(self.rng.below(5)).unwrap(), body));
            }
            6 => {
                let c = self.name("c");
                let mut inner = s.clone();
                let n = self.rng.below(3) + 1;
                let body = self.stmts(&mut inner, n, depth);
                out.push(S::While(c, i64::try_from(self.rng.below(4)).unwrap(), body));
            }
            13 if !s.of(T::Any).is_empty() => {
                // A walk of an `any` list, its elements named in the body.
                let r = self.any_place(s).unwrap_or(E::NilAny);
                let x = self.name("e");
                let mut inner = s.clone();
                inner.vars.push((x.clone(), T::Any));
                let n = self.rng.below(3) + 1;
                let body = self.stmts(&mut inner, n, depth);
                out.push(S::Each(x, r, body));
            }
            _ => {}
        }
    }

    /// A helper's answer bound to a new variable, when it can be called.
    fn bind_call(&mut self, i: usize, s: &mut Scope) -> Option<S> {
        let t = self.funcs[i].ret;
        let e = self.call_to(i, s, 2)?;
        let n = self.name("v");
        s.vars.push((n.clone(), t));
        Some(if plain(t) {
            S::Let(n, e)
        } else {
            S::LetTyped(n, t, e)
        })
    }

    fn func(&mut self, name: String, params: Vec<Param>, ret: T) -> Func {
        let mut s = Scope {
            vars: params.iter().map(|p| (p.name.clone(), p.t)).collect(),
        };
        let n = self.rng.below(5) + 1;
        let mut body = self.stmts(&mut s, n, 2);
        if name == "main" {
            // Every helper called at least once, so none is only compiled.
            for i in 0..self.funcs.len() {
                if s.of(T::Point).is_empty() && self.funcs[i].params.iter().any(|(_, r)| *r) {
                    let p = self.name("v");
                    body.push(S::LetTyped(p.clone(), T::Point, self.expr(T::Point, &s, 2)));
                    s.vars.push((p, T::Point));
                }
                body.extend(self.bind_call(i, &mut s));
            }
        }
        let tail = if matches!(ret, T::Point | T::Ages | T::Any) {
            // The answer bound first, then every variable folded into it.
            let sum = fold(E::Int(0), T::Int, &s);
            let value = if ret == T::Any {
                self.any_arg(&s, 3)
            } else {
                self.expr(ret, &s, 3)
            };
            let r = self.name("r");
            body.push(S::LetTyped(r.clone(), ret, value));
            match ret {
                T::Point => E::Point(
                    Box::new(E::Bin(
                        Op::Add,
                        Box::new(E::Field(r.clone(), "x")),
                        Box::new(sum),
                    )),
                    Box::new(E::Field(r, "y")),
                ),
                T::Ages => {
                    body.push(S::SetKey(r.clone(), "c", sum));
                    E::Var(r)
                }
                _ => {
                    let set = E::Method(
                        Box::new(E::Var(r.clone())),
                        "set",
                        vec![E::Str("c".into()), sum],
                    );
                    body.push(S::Do(set));
                    E::Var(r)
                }
            }
        } else {
            fold(self.expr(ret, &s, 4), ret, &s)
        };
        Func {
            name,
            params,
            ret,
            body,
            tail,
        }
    }

    pub(super) fn program(&mut self) -> Vec<Func> {
        let mut funcs = Vec::new();
        for _ in 0..self.rng.below(4) {
            let ret = self.pick(&PASSED);
            let mut params = Vec::new();
            for _ in 0..=self.rng.below(2) {
                let t = self.pick(&PASSED);
                let by_ref =
                    t == T::Point && params.iter().all(|p: &Param| !p.by_ref) && self.rng.chance(2);
                params.push(Param {
                    name: self.name("p"),
                    t,
                    by_ref,
                });
            }
            let name = self.name("h");
            let f = self.func(name.clone(), params.clone(), ret);
            self.funcs.push(Sig {
                name,
                params: params.iter().map(|p| (p.t, p.by_ref)).collect(),
                ret,
            });
            funcs.push(f);
        }
        let ret = self.pick(&[T::Int, T::Float, T::Bool, T::Str]);
        funcs.push(self.func("main".into(), vec![], ret));
        funcs
    }
}

// The answer with every variable in scope folded in, so a wrong value
// anywhere shows in what the function returns.
pub(super) fn fold(tail: E, ret: T, s: &Scope) -> E {
    let add = |a: E, b: E| E::Bin(Op::Add, Box::new(a), Box::new(b));
    let var = |n: &String| E::Var(n.clone());
    let mut out = tail;
    for (name, t) in &s.vars {
        out = match (ret, t) {
            (T::Int, T::Int) | (T::Float, T::Int | T::Float) | (T::Str, T::Str) => {
                add(out, var(name))
            }
            (T::Int | T::Float, T::Ints) => add(
                add(out, E::Len(name.clone())),
                E::At(name.clone(), Box::new(E::Int(0))),
            ),
            (T::Int | T::Float, T::Str) => add(out, E::StrLen(Box::new(var(name)))),
            (T::Str, t) if *t != T::Shade => add(out, E::Interp(vec![("/".into(), name.clone())])),
            (T::Bool, T::Bool) => E::Bin(Op::Ne, Box::new(out), Box::new(var(name))),
            (T::Int | T::Float, T::Point) => add(
                add(out, E::Field(name.clone(), "x")),
                E::Field(name.clone(), "y"),
            ),
            (T::Int | T::Float, T::Shade) => add(
                out,
                E::Match(
                    name.clone(),
                    Box::new(E::Int(7)),
                    "folded".into(),
                    Box::new(E::Var("folded".into())),
                ),
            ),
            (T::Int | T::Float, T::Ages) => add(
                add(out, E::Count(name.clone())),
                E::Get(name.clone(), "a", Box::new(E::Int(-3))),
            ),
            (T::Int | T::Float, T::Maybe) => add(out, E::Or(name.clone(), Box::new(E::Int(11)))),
            (T::Int | T::Float, T::Any) => add(
                add(
                    out,
                    E::StrLen(Box::new(E::Interp(vec![(String::new(), name.clone())]))),
                ),
                E::Method(Box::new(var(name)), "as_int", vec![]),
            ),
            (T::Bool, T::Any) => E::Bin(
                Op::Ne,
                Box::new(out),
                Box::new(E::Method(Box::new(var(name)), "is_nil", vec![])),
            ),
            _ => out,
        };
    }
    out
}
