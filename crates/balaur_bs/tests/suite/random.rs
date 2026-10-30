// Random well-typed programs, run on every engine and on a small interpreter
// that is the language's meaning in code: the engines share one compiler, so
// only this catches it writing the wrong code. `BS_RANDOM_SEED`, `_PROGRAMS`.

use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Debug)]
enum T {
    Int,
    Float,
    Bool,
    Str,
    Ints,
}

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
            };
        }
        let d = depth - 1;
        match t {
            T::Int => self.int_expr(s, d),
            T::Float => self.float_expr(s, d),
            T::Bool => self.bool_expr(s, d),
            T::Str => self.str_expr(s, d),
            T::Ints => self.ints_expr(s, d),
        }
    }

    fn int_expr(&mut self, s: &Scope, d: u32) -> E {
        match self.rng.below(9) {
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
            match self.rng.below(8) {
                0 | 1 => {
                    let t = [T::Int, T::Int, T::Float, T::Bool, T::Str, T::Ints]
                        [usize::try_from(self.rng.below(6)).unwrap()];
                    let e = self.expr(t, s, 3);
                    let n = self.name("v");
                    s.vars.push((n.clone(), t));
                    out.push(S::Let(n, e));
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
        }
    }
}

fn source(funcs: &[Func]) -> String {
    let mut out = String::new();
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

// The interpreter.

#[derive(Clone, Debug)]
enum V {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Ints(Rc<RefCell<Vec<i64>>>),
}

type Run<T> = Result<T, &'static str>;

struct Interp<'a> {
    funcs: &'a [Func],
    scopes: Vec<HashMap<String, V>>,
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

    fn body(&mut self, f: &Func) -> Run<V> {
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

fn to_value(v: &V) -> Value {
    match v {
        V::Int(i) => Value::Int(*i),
        V::Float(f) => Value::Float(if f.is_nan() { f64::NAN } else { *f }),
        V::Bool(b) => Value::Bool(*b),
        V::Str(s) => Value::Str(s.clone()),
        V::Ints(xs) => Value::Array(xs.borrow().iter().map(|i| Value::Int(*i)).collect()),
    }
}

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
        let compiled = match compile(&src, &Options::default()) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!(
                    "program {n} (seed {seed}) does not compile: {e}\n{src}"
                ));
                continue;
            }
        };
        for (name, engine) in engines() {
            let limits = Limits {
                engine,
                ..Limits::default()
            };
            let mut rt = Runtime::new(&compiled, HostFns::default(), limits).unwrap();
            let got = rt.call("main", &[]);
            let agrees = match (&want, &got) {
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
