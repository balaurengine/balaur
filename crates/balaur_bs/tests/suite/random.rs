// Random well-typed programs, run on every engine and on an interpreter that
// is the language's meaning in code (PLAN-script-tests §8). `BS_RANDOM_*`.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};
use std::collections::HashMap;
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Debug)]
enum T {
    Int,
    Float,
    Bool,
    Str,
    Ints,
    /// `struct Point { x: int, y: int }`.
    Point,
    /// `enum Shade { Dark, Lit(int) }`.
    Shade,
    /// `{str: int}`.
    Ages,
    /// `int?`.
    Maybe,
}

/// What every program declares before its functions.
const DECLARATIONS: &str = "struct Point { x: int, y: int }\nenum Shade { Dark, Lit(int) }\n";

/// The keys a map literal and a map write may name.
const KEYS: [&str; 3] = ["a", "b", "c"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Eq,
    Ne,
    And,
    Or,
}

impl Op {
    fn text(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Rem => "%",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::And => "&&",
            Op::Or => "||",
        }
    }
}

#[derive(Clone, Debug)]
enum E {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Var(String),
    Bin(Op, Box<E>, Box<E>),
    Neg(Box<E>),
    Not(Box<E>),
    If(Box<E>, Box<E>, Box<E>),
    Call(String, Vec<E>),
    Len(String),
    /// `xs[((i % xs.len()) + xs.len()) % xs.len()]`: always in range.
    At(String, Box<E>),
    StrLen(Box<E>),
    /// `"a{n}b{flag}"`: text and variables.
    Interp(Vec<(String, String)>),
    Array(Vec<E>),
    /// `Point { x: .., y: .. }`.
    Point(Box<E>, Box<E>),
    /// `p.x` or `p.y` of a variable.
    Field(String, &'static str),
    Dark,
    Lit(Box<E>),
    /// `match s { Shade::Dark => .., Shade::Lit(n) => .. }`, `n` an int in the
    /// second arm.
    Match(String, Box<E>, String, Box<E>),
    /// `{"a": .., "b": ..}`, each key once.
    Ages(Vec<(&'static str, E)>),
    /// `m["k"].or(..)`.
    Get(String, &'static str, Box<E>),
    /// `m.len()`.
    Count(String),
    Nil,
    /// `o.or(..)`.
    Or(String, Box<E>),
}

#[derive(Clone, Debug)]
enum S {
    Let(String, E),
    Set(String, E),
    Push(String, E),
    SetAt(String, E, E),
    If(E, Vec<S>, Vec<S>),
    For(String, i64, Vec<S>),
    While(String, i64, Vec<S>),
    /// `let n: T = e;`, for the kinds whose literal takes its type from where
    /// it goes.
    LetTyped(String, T, E),
    /// `p.x = e;`
    SetField(String, &'static str, E),
    /// `m["k"] = e;`
    SetKey(String, &'static str, E),
}

struct Func {
    name: String,
    params: Vec<(String, T)>,
    ret: T,
    body: Vec<S>,
    tail: E,
}

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
struct Scope {
    vars: Vec<(String, T)>,
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

struct Gen {
    rng: Rng,
    names: usize,
    /// The helpers already made: what `main` and later helpers may call.
    funcs: Vec<(String, Vec<T>, T)>,
}

impl Gen {
    fn name(&mut self, stem: &str) -> String {
        self.names += 1;
        format!("{stem}{}", self.names)
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

    fn expr(&mut self, t: T, s: &Scope, depth: u32) -> E {
        let leaf = depth == 0 || self.rng.chance(4);
        let vars = s.of(t);
        if leaf {
            if !vars.is_empty() && self.rng.chance(2) {
                return E::Var(
                    vars[usize::try_from(self.rng.below(vars.len() as u64)).unwrap()].clone(),
                );
            }
            return match t {
                T::Int => E::Int(self.int_lit()),
                T::Float => E::Float(self.float_lit()),
                T::Bool => E::Bool(self.rng.chance(2)),
                T::Str => E::Str(
                    ["", "a", "bc", "x y"][usize::try_from(self.rng.below(4)).unwrap()].into(),
                ),
                T::Ints => E::Array(
                    (0..=self.rng.below(3))
                        .map(|_| E::Int(self.int_lit()))
                        .collect(),
                ),
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
            _ => self.composite(t, s, d),
        }
    }

    /// A value of one of the four held kinds. A struct and a map are always a
    /// literal, never a variable, so no two names hold one.
    fn composite(&mut self, t: T, s: &Scope, d: u32) -> E {
        let held = s.of(t);
        let named = !held.is_empty() && matches!(t, T::Shade | T::Maybe) && self.rng.chance(2);
        if named {
            return E::Var(
                held[usize::try_from(self.rng.below(held.len() as u64)).unwrap()].clone(),
            );
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

    /// An int read out of a held value in scope, or `None` when none is.
    fn read_held(&mut self, s: &Scope, d: u32) -> Option<E> {
        let kinds: Vec<T> = [T::Point, T::Shade, T::Ages, T::Maybe]
            .into_iter()
            .filter(|t| !s.of(*t).is_empty())
            .collect();
        let t = *kinds.get(usize::try_from(self.rng.below(kinds.len().max(1) as u64)).unwrap())?;
        let names = s.of(t);
        let v = names[usize::try_from(self.rng.below(names.len() as u64)).unwrap()].clone();
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
                let k = KEYS[usize::try_from(self.rng.below(3)).unwrap()];
                E::Get(v, k, Box::new(self.expr(T::Int, s, d)))
            }
            _ => E::Or(v, Box::new(self.expr(T::Int, s, d))),
        })
    }

    fn int_expr(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(11) {
            9 | 10 => self
                .read_held(s, d)
                .unwrap_or_else(|| E::Int(self.int_lit())),
            0..=2 => {
                let op = [Op::Add, Op::Sub, Op::Mul][usize::try_from(self.rng.below(3)).unwrap()];
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
                let arrays = s.of(T::Ints);
                if arrays.is_empty() {
                    return E::StrLen(Box::new(self.expr(T::Str, s, d)));
                }
                let a =
                    arrays[usize::try_from(self.rng.below(arrays.len() as u64)).unwrap()].clone();
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
        match self.rng.below(6) {
            0..=2 => {
                let op = [Op::Add, Op::Sub, Op::Mul, Op::Div]
                    [usize::try_from(self.rng.below(4)).unwrap()];
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
            _ => self
                .call(T::Float, s, d)
                .unwrap_or_else(|| E::Float(self.float_lit())),
        }
    }

    fn bool_expr(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(5) {
            0 | 1 => {
                let op =
                    [Op::Lt, Op::Le, Op::Eq, Op::Ne][usize::try_from(self.rng.below(4)).unwrap()];
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
            _ => E::Bin(
                Op::Eq,
                Box::new(self.expr(T::Str, s, d)),
                Box::new(self.expr(T::Str, s, d)),
            ),
        }
    }

    fn str_expr(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(3) {
            0 => E::Bin(
                Op::Add,
                Box::new(self.expr(T::Str, s, d)),
                Box::new(self.expr(T::Str, s, d)),
            ),
            1 => {
                let mut pieces = Vec::new();
                for _ in 0..=self.rng.below(2) {
                    let shown = [T::Int, T::Bool]
                        .iter()
                        .flat_map(|t| s.of(*t))
                        .collect::<Vec<_>>();
                    let Some(v) = shown
                        .get(usize::try_from(self.rng.below(shown.len().max(1) as u64)).unwrap())
                    else {
                        break;
                    };
                    pieces.push((
                        ["", "n=", " "][usize::try_from(self.rng.below(3)).unwrap()].to_string(),
                        v.clone(),
                    ));
                }
                if pieces.is_empty() {
                    E::Str("q".into())
                } else {
                    E::Interp(pieces)
                }
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
        let fitting: Vec<_> = self
            .funcs
            .iter()
            .filter(|(_, _, r)| *r == ret)
            .cloned()
            .collect();
        let (name, params, _) = fitting
            .get(usize::try_from(self.rng.below(fitting.len().max(1) as u64)).unwrap())?
            .clone();
        let args = params.iter().map(|p| self.expr(*p, s, d)).collect();
        Some(E::Call(name, args))
    }

    fn stmts(&mut self, s: &mut Scope, n: u64, depth: u32) -> Vec<S> {
        let mut out = Vec::new();
        for _ in 0..n {
            let ints = s.of(T::Int);
            let arrays = s.of(T::Ints);
            match self.rng.below(10) {
                0 | 1 => {
                    let t = [
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
                    ][usize::try_from(self.rng.below(10)).unwrap()];
                    let e = self.expr(t, s, 3);
                    let n = self.name("v");
                    s.vars.push((n.clone(), t));
                    if matches!(t, T::Point | T::Shade | T::Ages | T::Maybe) {
                        out.push(S::LetTyped(n, t, e));
                    } else {
                        out.push(S::Let(n, e));
                    }
                }
                7 if !s.of(T::Point).is_empty() => {
                    let points = s.of(T::Point);
                    let p = points[usize::try_from(self.rng.below(points.len() as u64)).unwrap()]
                        .clone();
                    let field = if self.rng.chance(2) { "x" } else { "y" };
                    out.push(S::SetField(p, field, self.expr(T::Int, s, 2)));
                }
                8 if !s.of(T::Ages).is_empty() => {
                    let maps = s.of(T::Ages);
                    let m =
                        maps[usize::try_from(self.rng.below(maps.len() as u64)).unwrap()].clone();
                    let k = KEYS[usize::try_from(self.rng.below(3)).unwrap()];
                    out.push(S::SetKey(m, k, self.expr(T::Int, s, 2)));
                }
                2 if ints.iter().any(|n| !n.starts_with('i')) => {
                    // A loop's own counter is never written: only `let`s and parameters.
                    let ints: Vec<_> = ints.into_iter().filter(|n| !n.starts_with('i')).collect();
                    let v =
                        ints[usize::try_from(self.rng.below(ints.len() as u64)).unwrap()].clone();
                    out.push(S::Set(v, self.expr(T::Int, s, 3)));
                }
                3 if !arrays.is_empty() => {
                    let a = arrays[usize::try_from(self.rng.below(arrays.len() as u64)).unwrap()]
                        .clone();
                    if self.rng.chance(2) {
                        out.push(S::Push(a, self.expr(T::Int, s, 2)));
                    } else {
                        let at = self.expr(T::Int, s, 2);
                        out.push(S::SetAt(a, at, self.expr(T::Int, s, 2)));
                    }
                }
                4 if depth > 0 => {
                    let c = self.expr(T::Bool, s, 2);
                    let mut inner = s.clone();
                    let n = self.rng.below(3) + 1;
                    let a = self.stmts(&mut inner, n, depth - 1);
                    let mut inner = s.clone();
                    let n = self.rng.below(3);
                    let b = self.stmts(&mut inner, n, depth - 1);
                    out.push(S::If(c, a, b));
                }
                5 if depth > 0 => {
                    let i = self.name("i");
                    let mut inner = s.clone();
                    inner.vars.push((i.clone(), T::Int));
                    let n = self.rng.below(3) + 1;
                    let body = self.stmts(&mut inner, n, depth - 1);
                    out.push(S::For(i, i64::try_from(self.rng.below(5)).unwrap(), body));
                }
                6 if depth > 0 => {
                    let c = self.name("c");
                    let mut inner = s.clone();
                    let n = self.rng.below(3) + 1;
                    let body = self.stmts(&mut inner, n, depth - 1);
                    out.push(S::While(c, i64::try_from(self.rng.below(4)).unwrap(), body));
                }
                _ => {}
            }
        }
        out
    }

    fn func(&mut self, name: String, params: Vec<(String, T)>, ret: T) -> Func {
        let mut s = Scope {
            vars: params.clone(),
        };
        let n = self.rng.below(5) + 1;
        let body = self.stmts(&mut s, n, 2);
        let tail = fold(self.expr(ret, &s, 4), ret, &s);
        Func {
            name,
            params,
            ret,
            body,
            tail,
        }
    }

    fn program(&mut self) -> Vec<Func> {
        let mut funcs = Vec::new();
        for _ in 0..self.rng.below(3) {
            let ret = if self.rng.chance(3) { T::Float } else { T::Int };
            let params: Vec<(String, T)> = (0..=self.rng.below(2))
                .map(|_| {
                    let t = if self.rng.chance(3) { T::Float } else { T::Int };
                    (self.name("p"), t)
                })
                .collect();
            let name = self.name("h");
            let f = self.func(name.clone(), params.clone(), ret);
            self.funcs
                .push((name, params.iter().map(|(_, t)| *t).collect(), ret));
            funcs.push(f);
        }
        let ret = [T::Int, T::Float, T::Bool, T::Str][usize::try_from(self.rng.below(4)).unwrap()];
        funcs.push(self.func("main".into(), vec![], ret));
        funcs
    }
}

// The answer with every variable in scope folded in, so a wrong value
// anywhere shows in what the function returns.
fn fold(tail: E, ret: T, s: &Scope) -> E {
    let add = |a: E, b: E| E::Bin(Op::Add, Box::new(a), Box::new(b));
    let mut out = tail;
    for (name, t) in &s.vars {
        out = match (ret, t) {
            (T::Int, T::Int) | (T::Float, T::Int | T::Float) | (T::Str, T::Str) => {
                add(out, E::Var(name.clone()))
            }
            (T::Int | T::Float, T::Ints) => add(
                add(out, E::Len(name.clone())),
                E::At(name.clone(), Box::new(E::Int(0))),
            ),
            (T::Int | T::Float, T::Str) => add(out, E::StrLen(Box::new(E::Var(name.clone())))),
            (T::Str, T::Int | T::Bool) => add(out, E::Interp(vec![("/".into(), name.clone())])),
            (T::Bool, T::Bool) => E::Bin(Op::Ne, Box::new(out), Box::new(E::Var(name.clone()))),
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
            _ => out,
        };
    }
    out
}

fn ty(t: T) -> &'static str {
    match t {
        T::Int => "int",
        T::Float => "float",
        T::Bool => "bool",
        T::Str => "str",
        T::Ints => "[int]",
        T::Point => "Point",
        T::Shade => "Shade",
        T::Ages => "{str: int}",
        T::Maybe => "int?",
    }
}

fn show_expr(e: &E) -> String {
    match e {
        E::Int(i) if *i == i64::MIN => "(-9223372036854775807 - 1)".into(),
        E::Int(i) if *i < 0 => format!("({i})"),
        E::Int(i) => i.to_string(),
        E::Float(f) => {
            let text = format!("{f:?}");
            let text = if text.contains(['.', 'e', 'E']) {
                text
            } else {
                format!("{text}.0")
            };
            if f.is_sign_negative() {
                format!("({text})")
            } else {
                text
            }
        }
        E::Bool(b) => b.to_string(),
        E::Str(s) => format!("\"{s}\""),
        E::Var(v) => v.clone(),
        E::Bin(op, a, b) => format!("({} {} {})", show_expr(a), op.text(), show_expr(b)),
        E::Neg(a) => format!("(-{})", show_expr(a)),
        E::Not(a) => format!("(!{})", show_expr(a)),
        E::If(c, a, b) => format!(
            "(if {} {{ {} }} else {{ {} }})",
            show_expr(c),
            show_expr(a),
            show_expr(b)
        ),
        E::Call(f, args) => format!(
            "{f}({})",
            args.iter().map(show_expr).collect::<Vec<_>>().join(", ")
        ),
        E::Len(a) => format!("{a}.len()"),
        E::At(a, i) => format!(
            "{a}[((({}) % {a}.len()) + {a}.len()) % {a}.len()]",
            show_expr(i)
        ),
        E::StrLen(s) => format!("({}).len()", show_expr(s)),
        E::Interp(pieces) => {
            let mut out = String::from("\"");
            for (text, v) in pieces {
                let _ = write!(out, "{text}{{{v}}}");
            }
            out.push('"');
            out
        }
        E::Array(items) => format!(
            "[{}]",
            items.iter().map(show_expr).collect::<Vec<_>>().join(", ")
        ),
        E::Point(x, y) => format!("Point {{ x: {}, y: {} }}", show_expr(x), show_expr(y)),
        E::Field(v, f) => format!("{v}.{f}"),
        E::Dark => "Shade::Dark".into(),
        E::Lit(e) => format!("Shade::Lit({})", show_expr(e)),
        E::Match(v, dark, n, lit) => format!(
            "(match {v} {{ Shade::Dark => {}, Shade::Lit({n}) => {} }})",
            show_expr(dark),
            show_expr(lit)
        ),
        E::Ages(pairs) => format!(
            "{{{}}}",
            pairs
                .iter()
                .map(|(k, e)| format!("\"{k}\": {}", show_expr(e)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        E::Get(v, k, d) => format!("{v}[\"{k}\"].or({})", show_expr(d)),
        E::Count(v) => format!("{v}.len()"),
        E::Nil => "nil".into(),
        E::Or(v, d) => format!("{v}.or({})", show_expr(d)),
    }
}

fn show_stmts(stmts: &[S], indent: usize, out: &mut String) {
    let pad = "    ".repeat(indent);
    for st in stmts {
        match st {
            S::Let(n, e) => {
                let _ = writeln!(out, "{pad}let {n} = {};", show_expr(e));
            }
            S::Set(n, e) => {
                let _ = writeln!(out, "{pad}{n} = {};", show_expr(e));
            }
            S::Push(n, e) => {
                let _ = writeln!(out, "{pad}{n}.push({});", show_expr(e));
            }
            S::SetAt(n, i, e) => {
                let _ = writeln!(
                    out,
                    "{pad}{n}[((({}) % {n}.len()) + {n}.len()) % {n}.len()] = {};",
                    show_expr(i),
                    show_expr(e)
                );
            }
            S::If(c, a, b) => {
                let _ = writeln!(out, "{pad}if {} {{", show_expr(c));
                show_stmts(a, indent + 1, out);
                let _ = writeln!(out, "{pad}}} else {{");
                show_stmts(b, indent + 1, out);
                let _ = writeln!(out, "{pad}}}");
            }
            S::For(i, k, body) => {
                let _ = writeln!(out, "{pad}for {i} in 0..{k} {{");
                show_stmts(body, indent + 1, out);
                let _ = writeln!(out, "{pad}}}");
            }
            S::While(c, k, body) => {
                let _ = writeln!(out, "{pad}let {c} = 0;\n{pad}while {c} < {k} {{");
                show_stmts(body, indent + 1, out);
                let _ = writeln!(out, "{pad}    {c} += 1;\n{pad}}}");
            }
            S::LetTyped(n, t, e) => {
                let _ = writeln!(out, "{pad}let {n}: {} = {};", ty(*t), show_expr(e));
            }
            S::SetField(v, f, e) => {
                let _ = writeln!(out, "{pad}{v}.{f} = {};", show_expr(e));
            }
            S::SetKey(v, k, e) => {
                let _ = writeln!(out, "{pad}{v}[\"{k}\"] = {};", show_expr(e));
            }
        }
    }
}

fn source(funcs: &[Func]) -> String {
    let mut out = String::from(DECLARATIONS);
    for f in funcs {
        let params: Vec<String> = f
            .params
            .iter()
            .map(|(n, t)| format!("{n}: {}", ty(*t)))
            .collect();
        let public = if f.name == "main" { "pub " } else { "" };
        let _ = writeln!(
            out,
            "{public}fn {}({}) -> {} {{",
            f.name,
            params.join(", "),
            ty(f.ret)
        );
        show_stmts(&f.body, 1, &mut out);
        let _ = writeln!(out, "    {}\n}}", show_expr(&f.tail));
    }
    out
}

mod interp;

use interp::{Interp, to_value};

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        _ => a == b,
    }
}

fn engines() -> Vec<(&'static str, EngineKind)> {
    vec![
        #[cfg(feature = "wasmi")]
        ("wasmi", EngineKind::Wasmi),
        #[cfg(all(
            feature = "wasmtime",
            not(target_arch = "wasm32"),
            not(target_os = "ios")
        ))]
        ("wasmtime", EngineKind::Wasmtime),
        #[cfg(all(feature = "c", feature = "native"))]
        ("native", EngineKind::Native),
    ]
}

#[test]
fn random_programs_answer_as_the_reference_interpreter_does() {
    let seed: u64 = std::env::var("BS_RANDOM_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x0bad_5eed);
    let count: u64 = std::env::var("BS_RANDOM_PROGRAMS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let show = std::env::var_os("BS_RANDOM_SHOW").is_some();
    let mut failures = Vec::new();
    let mut answered = 0;
    // Every program made and compiled first, so native code is one C build of
    // all of them rather than a compiler run each.
    let mut made = Vec::new();
    for n in 0..count {
        let mut g = Gen {
            rng: Rng(seed ^ (n + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)),
            names: 0,
            funcs: Vec::new(),
        };
        let funcs = g.program();
        let src = source(&funcs);
        let want = Interp {
            funcs: &funcs,
            scopes: vec![HashMap::new()],
        }
        .body(funcs.last().expect("main"));
        if want.is_ok() {
            answered += 1;
        }
        if show {
            eprintln!("program {n}: {:?}\n{src}", want.as_ref().map(to_value));
        }
        match compile(&src, &Options::default()) {
            Ok(compiled) => made.push((n, src, want, compiled)),
            Err(e) => failures.push(format!(
                "program {n} (seed {seed}) does not compile: {e}\n{src}"
            )),
        }
    }
    #[cfg(all(feature = "c", feature = "native"))]
    super::native::build(&made.iter().map(|(.., c)| c).collect::<Vec<_>>());
    for (n, src, want, compiled) in &made {
        for (name, engine) in engines() {
            let limits = Limits {
                engine,
                ..Limits::default()
            };
            let mut rt = Runtime::new(compiled, HostFns::default(), limits).unwrap();
            let got = rt.call("main", &[]);
            let agrees = match (want, &got) {
                (Ok(w), Ok(g)) => same(&to_value(w), g),
                (Err(_), Err(_)) => true,
                _ => false,
            };
            if !agrees {
                failures.push(format!(
                    "program {n} (seed {seed}) on {name}: the interpreter gave {:?}, the engine {got:?}\n{src}",
                    want.as_ref().map(to_value)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} disagreements:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // A generator whose programs mostly trap would agree while testing little.
    assert!(
        answered * 10 >= count * 7,
        "only {answered} of {count} programs gave a value"
    );
}
