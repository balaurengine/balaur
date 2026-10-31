//! A failing program made smaller one edit at a time, each edit kept while
//! the failure holds: a helper dropped, a statement dropped or opened up, an
//! expression replaced by a part of itself or by its type's zero.

use super::interp::type_of;
use super::{E, Func, S, T};

/// The smallest program edits reach from `funcs` while `still` holds,
/// trying at most `budget` of them.
pub(super) fn shrink(
    funcs: &[Func],
    still: &mut dyn FnMut(&[Func]) -> bool,
    budget: usize,
) -> Vec<Func> {
    let mut best = funcs.to_vec();
    let mut least = weight(&best);
    let mut tries = 0;
    loop {
        let mut progressed = false;
        let mut site = 0;
        while site < sites(&best) {
            let mut option = 0;
            let mut kept = false;
            while let Some(smaller) = edit(&best, site, option) {
                option += 1;
                let w = weight(&smaller);
                if w >= least {
                    continue;
                }
                if tries == budget {
                    return best;
                }
                tries += 1;
                if still(&smaller) {
                    (best, least, kept, progressed) = (smaller, w, true, true);
                    break;
                }
            }
            // A kept edit leaves this site holding something new to try.
            if !kept {
                site += 1;
            }
        }
        if !progressed {
            return best;
        }
    }
}

/// Each helper's parameters, as (helper, parameter) pairs.
fn params(funcs: &[Func]) -> Vec<(usize, usize)> {
    let helpers = &funcs[..funcs.len() - 1];
    (0..helpers.len())
        .flat_map(|f| (0..helpers[f].params.len()).map(move |p| (f, p)))
        .collect()
}

/// How many places `edit` can change: each helper, each helper's
/// parameter, then each statement and expression in order.
fn sites(funcs: &[Func]) -> usize {
    let mut walk = Walk::new(funcs, None);
    for f in &mut funcs.to_vec() {
        walk.func(f);
    }
    funcs.len() - 1 + params(funcs).len() + walk.site
}

/// `funcs` with edit `option` made at `site`, or `None` past its last.
fn edit(funcs: &[Func], site: usize, option: usize) -> Option<Vec<Func>> {
    let helpers = funcs.len() - 1;
    if site < helpers {
        return (option == 0).then(|| drop_helper(funcs, site));
    }
    let params = params(funcs);
    if let Some(&(f, p)) = params.get(site - helpers) {
        return (option == 0).then(|| drop_param(funcs, f, p));
    }
    let mut out = funcs.to_vec();
    let mut walk = Walk::new(funcs, Some((site - helpers - params.len(), option)));
    for f in &mut out {
        walk.func(f);
        if walk.done {
            break;
        }
    }
    walk.applied.then_some(out)
}

// A helper gone, every call to it its result type's zero.
fn drop_helper(funcs: &[Func], i: usize) -> Vec<Func> {
    let mut out = funcs.to_vec();
    let gone = out.remove(i);
    let zero = zero(gone.ret);
    let mut replace = |e: &mut E| {
        if matches!(e, E::Call(f, _) if *f == gone.name) {
            *e = zero.clone();
        }
    };
    for f in &mut out {
        for st in &mut f.body {
            stmt_exprs(st, &mut replace);
        }
        every_expr(&mut f.tail, &mut replace);
    }
    out
}

// A parameter gone from a helper and from every call to it; where the body
// read it, its type's zero.
fn drop_param(funcs: &[Func], f: usize, p: usize) -> Vec<Func> {
    let mut out = funcs.to_vec();
    let gone = out[f].params.remove(p);
    let name = out[f].name.clone();
    let zero = zero(gone.t);
    let mut read = |e: &mut E| {
        if matches!(e, E::Var(v) if *v == gone.name) {
            *e = zero.clone();
        }
    };
    for st in &mut out[f].body {
        stmt_exprs(st, &mut read);
    }
    every_expr(&mut out[f].tail, &mut read);
    let mut call = |e: &mut E| {
        if let E::Call(g, args) = e
            && *g == name
            && p < args.len()
        {
            args.remove(p);
        }
    };
    for g in &mut out {
        for st in &mut g.body {
            stmt_exprs(st, &mut call);
        }
        every_expr(&mut g.tail, &mut call);
    }
    out
}

fn stmt_exprs(st: &mut S, f: &mut dyn FnMut(&mut E)) {
    match st {
        S::Let(_, e)
        | S::LetTyped(_, _, e)
        | S::Set(_, e)
        | S::Push(_, e)
        | S::SetField(_, _, e)
        | S::SetKey(_, _, e)
        | S::Do(e) => every_expr(e, f),
        S::SetAt(_, a, b) | S::Put(a, b) => {
            every_expr(a, f);
            every_expr(b, f);
        }
        S::If(c, a, b) => {
            every_expr(c, f);
            for st in a.iter_mut().chain(b) {
                stmt_exprs(st, f);
            }
        }
        S::For(_, _, body) | S::While(_, _, body) => {
            for st in body {
                stmt_exprs(st, f);
            }
        }
        S::Each(_, r, body) => {
            every_expr(r, f);
            for st in body {
                stmt_exprs(st, f);
            }
        }
    }
}

// `f` on `e`, then on what is left of its parts.
fn every_expr(e: &mut E, f: &mut dyn FnMut(&mut E)) {
    f(e);
    for c in parts_mut(e) {
        every_expr(c, f);
    }
}

fn parts_mut(e: &mut E) -> Vec<&mut E> {
    match e {
        E::Bin(_, a, b) | E::Point(a, b) | E::Index(a, b) | E::Match(_, a, _, b) => vec![a, b],
        E::Neg(a)
        | E::Not(a)
        | E::At(_, a)
        | E::StrLen(a)
        | E::Lit(a)
        | E::Get(_, _, a)
        | E::Or(_, a)
        | E::Dot(a, _) => vec![a],
        E::If(c, a, b) => vec![c, a, b],
        E::Call(_, args) | E::Array(args) => args.iter_mut().collect(),
        E::Ages(pairs) => pairs.iter_mut().map(|(_, e)| e).collect(),
        E::Method(r, _, args) => std::iter::once(&mut **r).chain(args.iter_mut()).collect(),
        _ => Vec::new(),
    }
}

// A match's second arm is left out: it may name what the pattern binds.
fn parts(e: &E) -> Vec<&E> {
    match e {
        E::Bin(_, a, b) | E::Point(a, b) | E::Index(a, b) => vec![a, b],
        E::Match(_, a, _, _)
        | E::Neg(a)
        | E::Not(a)
        | E::At(_, a)
        | E::StrLen(a)
        | E::Lit(a)
        | E::Get(_, _, a)
        | E::Or(_, a)
        | E::Dot(a, _) => vec![a],
        E::If(c, a, b) => vec![c, a, b],
        E::Call(_, args) | E::Array(args) => args.iter().collect(),
        E::Ages(pairs) => pairs.iter().map(|(_, e)| e).collect(),
        E::Method(r, _, args) => std::iter::once(&**r).chain(args).collect(),
        _ => Vec::new(),
    }
}

/// The simplest value of `t`.
fn zero(t: T) -> E {
    match t {
        T::Int => E::Int(0),
        T::Float => E::Float(0.0),
        T::Bool => E::Bool(false),
        T::Str => E::Str(String::new()),
        T::Ints => E::Array(vec![E::Int(0)]),
        T::Point => E::Point(Box::new(E::Int(0)), Box::new(E::Int(0))),
        T::Shade => E::Dark,
        T::Ages => E::Ages(Vec::new()),
        T::Maybe => E::Nil,
        T::Any => E::NilAny,
    }
}

/// How big a program is: every edit makes it smaller, so a shrink ends.
fn weight(funcs: &[Func]) -> usize {
    funcs
        .iter()
        .map(|f| 4 + f.params.len() + block_weight(&f.body) + expr_weight(&f.tail))
        .sum()
}

fn block_weight(body: &[S]) -> usize {
    body.iter().map(stmt_weight).sum()
}

fn stmt_weight(st: &S) -> usize {
    3 + match st {
        S::Let(_, e)
        | S::LetTyped(_, _, e)
        | S::Set(_, e)
        | S::Push(_, e)
        | S::SetField(_, _, e)
        | S::SetKey(_, _, e)
        | S::Do(e) => expr_weight(e),
        S::SetAt(_, a, b) | S::Put(a, b) => expr_weight(a) + expr_weight(b),
        S::If(c, a, b) => expr_weight(c) + block_weight(a) + block_weight(b),
        S::For(_, k, body) | S::While(_, k, body) => {
            usize::try_from(*k).unwrap_or(0) + block_weight(body)
        }
        S::Each(_, r, body) => expr_weight(r) + block_weight(body),
    }
}

fn expr_weight(e: &E) -> usize {
    let own = match e {
        E::Int(i) => usize::from(*i != 0),
        E::Float(f) => usize::from(f.to_bits() != 0),
        E::Bool(b) => usize::from(*b),
        E::Str(s) => s.len(),
        E::Interp(pieces) => pieces.iter().map(|(text, _)| 2 + text.len()).sum(),
        // A read of a variable outweighs a literal, so a shrink trades one
        // for the other and the variable can go.
        E::MapAny
        | E::Var(_)
        | E::Len(_)
        | E::Count(_)
        | E::Field(..)
        | E::At(..)
        | E::Get(..)
        | E::Or(..) => 1,
        E::Match(_, _, _, lit) => 1 + expr_weight(lit),
        _ => 0,
    };
    2 + own + parts(e).into_iter().map(expr_weight).sum::<usize>()
}

/// What a statement may become, each a run of statements in its place.
fn stmt_options(st: &S) -> Vec<Vec<S>> {
    let mut out = vec![Vec::new()];
    match st {
        S::If(_, a, b) => {
            out.push(a.clone());
            out.push(b.clone());
        }
        S::For(i, k, body) => {
            let mut once = vec![S::Let(i.clone(), E::Int(0))];
            once.extend(body.iter().cloned());
            out.push(once);
            if *k > 1 {
                out.push(vec![S::For(i.clone(), 1, body.clone())]);
            }
        }
        S::While(c, k, body) => {
            out.push(body.clone());
            if *k > 1 {
                out.push(vec![S::While(c.clone(), 1, body.clone())]);
            }
        }
        S::Each(x, _, body) => {
            let mut once = vec![S::LetTyped(x.clone(), T::Any, E::NilAny)];
            once.extend(body.iter().cloned());
            out.push(once);
        }
        _ => {}
    }
    out
}

/// A walk over every statement and expression in order, counting the places
/// it passes; at the target place it makes the edit asked for and stops.
struct Walk {
    target: Option<(usize, usize)>,
    site: usize,
    done: bool,
    applied: bool,
    rets: Vec<(String, T)>,
    env: Vec<(String, T)>,
}

impl Walk {
    fn new(funcs: &[Func], target: Option<(usize, usize)>) -> Walk {
        Walk {
            target,
            site: 0,
            done: false,
            applied: false,
            rets: funcs.iter().map(|f| (f.name.clone(), f.ret)).collect(),
            env: Vec::new(),
        }
    }

    fn type_of(&self, e: &E) -> Option<T> {
        let find = |list: &[(String, T)], n: &str| {
            list.iter().rev().find(|(m, _)| m == n).map(|(_, t)| *t)
        };
        type_of(e, &|n| find(&self.env, n), &|f| find(&self.rets, f))
    }

    /// Passes one place with `options` edits: the one to make, when this is
    /// the target place and it has that many.
    fn hit(&mut self, options: usize) -> Option<usize> {
        let here = self.site;
        self.site += 1;
        let (site, option) = self.target?;
        if site != here {
            return None;
        }
        self.done = true;
        self.applied = option < options;
        self.applied.then_some(option)
    }

    fn func(&mut self, f: &mut Func) {
        self.env = f.params.iter().map(|p| (p.name.clone(), p.t)).collect();
        self.block(&mut f.body);
        self.expr(&mut f.tail);
    }

    fn block(&mut self, body: &mut Vec<S>) {
        let mut i = 0;
        while i < body.len() && !self.done {
            let options = stmt_options(&body[i]);
            if let Some(o) = self.hit(options.len()) {
                body.splice(i..=i, options[o].clone());
                return;
            }
            self.stmt(&mut body[i]);
            i += 1;
        }
    }

    fn scoped(&mut self, bound: Option<(String, T)>, body: &mut Vec<S>) {
        let mark = self.env.len();
        self.env.extend(bound);
        self.block(body);
        self.env.truncate(mark);
    }

    fn stmt(&mut self, st: &mut S) {
        match st {
            S::Let(n, e) => {
                self.expr(e);
                if let Some(t) = self.type_of(e) {
                    self.env.push((n.clone(), t));
                }
            }
            S::LetTyped(n, t, e) => {
                self.expr(e);
                self.env.push((n.clone(), *t));
            }
            S::Set(_, e) | S::Push(_, e) | S::SetField(_, _, e) | S::SetKey(_, _, e) | S::Do(e) => {
                self.expr(e);
            }
            S::SetAt(_, i, e) => {
                self.expr(i);
                self.expr(e);
            }
            S::Put(target, e) => {
                // The key written stays a key: only what it is written on and by.
                for part in parts_mut(target) {
                    self.expr(part);
                }
                self.expr(e);
            }
            S::If(c, a, b) => {
                self.expr(c);
                self.scoped(None, a);
                self.scoped(None, b);
            }
            S::For(i, _, body) => self.scoped(Some((i.clone(), T::Int)), body),
            S::While(_, _, body) => self.scoped(None, body),
            S::Each(x, r, body) => {
                self.expr(r);
                self.scoped(Some((x.clone(), T::Any)), body);
            }
        }
    }

    fn expr(&mut self, e: &mut E) {
        if self.done {
            return;
        }
        let options = self.expr_options(e);
        if let Some(o) = self.hit(options.len()) {
            *e = options[o].clone();
            return;
        }
        if let E::Match(_, dark, n, lit) = e {
            self.expr(dark);
            self.env.push((n.clone(), T::Int));
            self.expr(lit);
            self.env.pop();
            return;
        }
        for part in parts_mut(e) {
            self.expr(part);
        }
    }

    /// What an expression may become: its type's zero, a part of itself of
    /// the same type, or a list, map or string with one piece fewer.
    fn expr_options(&self, e: &E) -> Vec<E> {
        let Some(t) = self.type_of(e) else {
            return Vec::new();
        };
        let mut out: Vec<E> = Some(zero(t)).filter(|z| z != e).into_iter().collect();
        out.extend(
            parts(e)
                .into_iter()
                .filter(|p| self.type_of(p) == Some(t))
                .cloned(),
        );
        match e {
            E::Array(items) => {
                for i in 0..items.len() {
                    let mut fewer = items.clone();
                    fewer.remove(i);
                    out.push(E::Array(fewer));
                }
            }
            E::Ages(pairs) => {
                for i in 0..pairs.len() {
                    let mut fewer = pairs.clone();
                    fewer.remove(i);
                    out.push(E::Ages(fewer));
                }
            }
            E::Interp(pieces) => {
                for i in 0..pieces.len() {
                    let mut fewer = pieces.clone();
                    if pieces.len() > 1 {
                        fewer.remove(i);
                    } else {
                        fewer[i].0.clear();
                    }
                    out.push(E::Interp(fewer));
                }
            }
            _ => {}
        }
        out
    }
}
