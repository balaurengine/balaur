// Blocks, statements and control flow: `match` and `if let` become `if`
// chains over `is_some()` and `==`, `for (i, x) in xs.iter().enumerate()`
// an index loop, tuple destructuring a temporary and its fields.

use crate::cx::{ATOM, Cx, Ex, Named, UNWRAP};
use crate::expr::indent;
use crate::project::name_hint;
use crate::ty::Ty;
use rune::ast;
use rune::ast::Spanned;
use std::fmt::Write as _;

/// A name a pattern binds, and the text that reads it.
pub(crate) struct Bind {
    pub name: String,
    pub value: String,
    pub ty: Ty,
}

/// A block's text with `first` as its first line.
fn prepend(block: &str, first: Option<&str>) -> String {
    let Some(first) = first else {
        return block.to_string();
    };
    if block == "{}" {
        return format!("{{\n{}\n}}", indent(first));
    }
    block.replacen("{\n", &format!("{{\n{}\n", indent(first)), 1)
}

/// What a pattern tests, if anything, and the names it binds; `None` when
/// it cannot be read at all.
type Test = Option<(Option<String>, Vec<Bind>)>;

fn wrap(lines: &[String]) -> String {
    let body: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
    if body.is_empty() {
        return "{}".into();
    }
    let joined: Vec<String> = body.iter().map(|s| (*s).clone()).collect();
    format!("{{\n{}\n}}", indent(&joined.join("\n")))
}

fn diverges(e: &ast::Expr) -> bool {
    match e {
        ast::Expr::Return(_) | ast::Expr::Break(_) | ast::Expr::Continue(_) => true,
        ast::Expr::Block(b) => b.block.statements.last().is_some_and(|s| match s {
            ast::Stmt::Expr(e) => diverges(e),
            ast::Stmt::Semi(s) => diverges(&s.expr),
            _ => false,
        }),
        _ => false,
    }
}

pub(crate) fn block_diverges(b: &ast::Block) -> bool {
    b.statements.last().is_some_and(|s| match s {
        ast::Stmt::Expr(e) => diverges(e),
        ast::Stmt::Semi(s) => diverges(&s.expr),
        _ => false,
    })
}

/// A plain name or a field chain: safe to write twice.
fn is_place_text(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        && !s.starts_with(|c: char| c.is_ascii_digit())
}

pub(crate) fn is_place(e: &ast::Expr) -> bool {
    match e {
        ast::Expr::Path(_) => true,
        ast::Expr::FieldAccess(f) => is_place(&f.expr),
        ast::Expr::Index(i) => is_place(&i.target),
        ast::Expr::Group(g) => is_place(&g.expr),
        _ => false,
    }
}

impl Cx<'_> {
    pub(crate) fn block(&mut self, b: &ast::Block, value: bool) -> (String, Ty) {
        self.block_with(b, value, vec![])
    }

    /// A block with names bound at its top.
    pub(crate) fn block_with(
        &mut self,
        b: &ast::Block,
        value: bool,
        binds: Vec<Bind>,
    ) -> (String, Ty) {
        self.push_scope();
        let mut lines = vec![];
        for bd in binds {
            self.learn_local(&bd.name, &bd.ty);
            let out = self.bind(&bd.name, false);
            lines.push(format!("let {out} = {};", bd.value));
        }
        let n = b.statements.len();
        let mut t = Ty::Void;
        for (i, st) in b.statements.iter().enumerate() {
            let tail = value && i + 1 == n && matches!(st, ast::Stmt::Expr(e) if !diverges(e));
            lines.extend(self.comments_before(st.span().range().start));
            let (text, ty) = self.stmt(st, tail);
            if tail {
                t = ty;
            }
            lines.push(text);
            lines.extend(self.narrow_after(st));
        }
        lines.extend(self.comments_before(b.close.span.range().start));
        self.pop_scope();
        (wrap(&lines), t)
    }

    /// One statement with its notes and hoisted lines above it.
    pub(crate) fn stmt(&mut self, st: &ast::Stmt, tail: bool) -> (String, Ty) {
        let saved_pre = std::mem::take(&mut self.pre);
        let saved_notes = std::mem::take(&mut self.notes);
        let (text, t) = match st {
            ast::Stmt::Local(l) => (self.local(l), Ty::Void),
            ast::Stmt::Semi(s) => (self.expr_stmt(&s.expr), Ty::Void),
            ast::Stmt::Expr(e) if tail => {
                let saved = self.hint.take();
                self.hint = self.tail_hint.clone();
                let ex = self.expr(e);
                self.hint = saved;
                (ex.s, ex.t)
            }
            ast::Stmt::Expr(e) => (self.expr_stmt(e), Ty::Void),
            other => {
                self.flag(
                    other.span(),
                    "item",
                    "an item inside a function: move it to the top level",
                );
                (
                    format!(
                        "// {}",
                        self.text(other.span()).lines().next().unwrap_or("")
                    ),
                    Ty::Void,
                )
            }
        };
        let mut lines = std::mem::replace(&mut self.notes, saved_notes);
        lines.extend(std::mem::replace(&mut self.pre, saved_pre));
        lines.push(text);
        (lines.join("\n"), t)
    }

    pub(crate) fn expr_stmt(&mut self, e: &ast::Expr) -> String {
        let ex = self.expr(e);
        let s = ex.s;
        let block_like = (s.starts_with("if ")
            || s.starts_with("for ")
            || s.starts_with("while ")
            || s.starts_with("loop ")
            || s.starts_with('{'))
            && s.ends_with('}');
        if block_like || s.ends_with(';') || s.is_empty() {
            s
        } else {
            format!("{s};")
        }
    }

    fn local(&mut self, l: &ast::Local) -> String {
        match &l.pat {
            ast::Pat::Path(pp) if pp.path.rest.is_empty() => {
                let name = self.text(pp.path.span()).trim().to_string();
                if let Some(text) = self.let_or_exit(&name, &l.expr) {
                    return text;
                }
                let saved = self.hint.replace(name_hint(&name));
                let ex = self.expr(&l.expr);
                self.hint = saved;
                self.learn_local(&name, &ex.t);
                let alias = matches!(&ex.t, Ty::Shape(_))
                    || matches!(&ex.t, Ty::Opt(t) if matches!(**t, Ty::Shape(_)));
                let alias = alias && is_place(&l.expr);
                let out = self.bind(&name, alias);
                let vt = self.local_ty(&name);
                self.let_text(&out, &ex, &vt)
            }
            ast::Pat::Ignore(_) => {
                let ex = self.expr(&l.expr);
                format!("{};", ex.s)
            }
            // `let (ok, v) = script::attempt(|| e)`: dropped, `e` runs as it is.
            ast::Pat::Tuple(t) if t.path.is_none() && self.attempted(&l.expr).is_some() => {
                self.attempt_let(t, &l.expr)
            }
            ast::Pat::Tuple(t) if t.path.is_none() => {
                let ex = self.expr(&l.expr);
                let mut names = vec![];
                for (p, _) in &t.items {
                    names.push(match p {
                        ast::Pat::Path(pp) => Some(self.text(pp.path.span()).trim().to_string()),
                        ast::Pat::Ignore(_) => None,
                        other => {
                            self.flag(other.span(), "pattern", "a nested pattern in a `let`");
                            None
                        }
                    });
                }
                let base: Vec<&str> = names.iter().flatten().map(String::as_str).collect();
                let tmp = if base.is_empty() {
                    self.fresh("pair")
                } else {
                    format!("{}_{}", base.join("_"), self.tmp + 1)
                };
                self.tmp += 1;
                let mut lines = vec![format!("let {tmp} = {};", ex.s)];
                for (i, n) in names.iter().enumerate() {
                    let Some(n) = n else { continue };
                    let (value, t) = match &ex.t {
                        Ty::Shape(id) => {
                            let t = self
                                .p
                                .shapes
                                .field(*id, &format!("_{i}"))
                                .unwrap_or(Ty::Unknown);
                            (format!("{tmp}._{i}"), self.p.norm(&t))
                        }
                        _ => (format!("{tmp}.at({i})"), Ty::Any),
                    };
                    self.learn_local(n, &t);
                    let out = self.bind(n, false);
                    lines.push(format!("let {out} = {value};"));
                }
                lines.join("\n")
            }
            other => {
                self.flag(
                    other.span(),
                    "pattern",
                    "a destructuring `let` the translator does not lower",
                );
                let ex = self.expr(&l.expr);
                let mut names = vec![];
                crate::closure::pat_names(self, other, &mut names);
                for n in &names {
                    self.bind(n, false);
                }
                format!("let {} = {};", self.text(other.span()), ex.s)
            }
        }
    }

    /// `let name = <ex>;`, with the variable's type when the value alone
    /// would not give it.
    fn let_text(&mut self, out: &str, ex: &Ex, vt: &Ty) -> String {
        let init = self.coerce(ex, vt);
        let et = self.p.norm(&ex.t);
        let annotate = vt.known()
            && (et != *vt || matches!(et, Ty::Nil | Ty::Unknown) || contains_unknown(&et));
        if annotate && !matches!(vt, Ty::Unknown) {
            let t = self.ty_str(vt);
            if init.starts_with(&format!("{ex}.as_", ex = ex.at(ATOM))) && et == Ty::Any {
                return format!("let {out} = {init};");
            }
            format!("let {out}: {t} = {init};")
        } else {
            format!("let {out} = {init};")
        }
    }

    /// `let x = match opt { Some(v) => v, None => return }` (or the `if let`
    /// form): a check that leaves, then the unwrapped value.
    fn let_or_exit(&mut self, name: &str, e: &ast::Expr) -> Option<String> {
        let (scrut, bound, exit): (&ast::Expr, String, &ast::Expr) = match e {
            ast::Expr::Match(m) if m.branches.len() == 2 => {
                let mut some = None;
                let mut exit = None;
                for (b, _) in &m.branches {
                    if b.condition.is_some() {
                        return None;
                    }
                    match &b.pat {
                        ast::Pat::Tuple(t)
                            if t.path
                                .as_ref()
                                .is_some_and(|p| self.text(p.span()).trim() == "Some")
                                && t.items.len() == 1 =>
                        {
                            let ast::Pat::Path(inner) = &t.items.first()?.0 else {
                                return None;
                            };
                            let v = self.text(inner.path.span()).trim().to_string();
                            if self.text(b.body.span()).trim() != v {
                                return None;
                            }
                            some = Some(v);
                        }
                        ast::Pat::Path(p) if self.text(p.path.span()).trim() == "None" => {
                            if !diverges(&b.body) {
                                return None;
                            }
                            exit = Some(&b.body);
                        }
                        ast::Pat::Ignore(_) => {
                            if !diverges(&b.body) {
                                return None;
                            }
                            exit = Some(&b.body);
                        }
                        _ => return None,
                    }
                }
                (&*m.expr, some?, exit?)
            }
            _ => return None,
        };
        let s = self.expr(scrut);
        let place = if is_place_text(&s.s) && is_place(scrut) {
            s.s.clone()
        } else {
            let tmp = format!("{name}_or");
            self.pre.push(format!("let {tmp} = {};", s.s));
            tmp
        };
        let check = Self::nil_check(&Ex::atom(place.clone(), s.t.clone()), false);
        let exit_text = self.diverge_stmt(exit);
        let inner = if s.t == Ty::Any || !s.t.known() {
            s.t.clone()
        } else {
            s.t.inner()
        };
        let unwrap = if s.t == Ty::Any || !s.t.known() {
            place.clone()
        } else {
            format!("{place}.{UNWRAP}()")
        };
        let _ = bound;
        self.learn_local(name, &inner);
        let out = self.bind(name, false);
        let vt = self.local_ty(name);
        let decl = self.let_text(&out, &Ex::atom(unwrap, inner), &vt);
        Some(format!("if {check} {}\n{decl}", wrap(&[exit_text])))
    }

    /// A diverging arm body as statements.
    fn diverge_stmt(&mut self, e: &ast::Expr) -> String {
        if let ast::Expr::Block(b) = e {
            let (text, _) = self.block(&b.block, false);
            // Unwrapped: the caller wraps it again.
            return text
                .trim_start_matches('{')
                .trim_end_matches('}')
                .trim()
                .lines()
                .map(|l| l.strip_prefix("    ").unwrap_or(l))
                .collect::<Vec<_>>()
                .join("\n");
        }
        self.expr_stmt(e)
    }

    pub(crate) fn return_expr(&mut self, r: &ast::ExprReturn) -> Ex {
        match &r.expr {
            None => {
                self.learn_ret(&Ty::Void);
                let ret = self.cur().map_or(Ty::Void, |s| self.p.norm(&s.ret));
                if matches!(ret, Ty::Void | Ty::Unknown) {
                    Ex::new("return", Ty::Void, 0)
                } else {
                    Ex::new("return nil", Ty::Void, 0)
                }
            }
            Some(e) => {
                let saved = self.hint.take();
                if let Some(s) = self.cur() {
                    self.hint = Some(fn_hint(&self.p.modules[self.m].key, &s.name));
                }
                let ex = self.expr(e);
                self.hint = saved;
                self.learn_ret(&ex.t);
                let ret = self.cur().map_or(Ty::Unknown, |s| self.p.norm(&s.ret));
                Ex::new(
                    format!("return {}", self.coerce_at(&ex, &ret, r.span())),
                    Ty::Void,
                    0,
                )
            }
        }
    }

    pub(crate) fn cond(&mut self, c: &ast::Condition) -> (String, Vec<Bind>) {
        match c {
            ast::Condition::Expr(e) => {
                let ex = self.expr(e);
                if self.rec() && !ex.t.known() {
                    self.learn_place(e, &Ty::Bool);
                }
                (self.coerce(&ex, &Ty::Bool), vec![])
            }
            ast::Condition::ExprLet(l) => {
                let s = self.expr(&l.expr);
                let s = self.hoist_scrutinee(s, &l.expr);
                match self.pattern(&l.pat, &s) {
                    Some((cond, binds)) => (cond.unwrap_or_else(|| "true".into()), binds),
                    None => ("false".into(), vec![]),
                }
            }
            _ => ("false".into(), vec![]),
        }
    }

    /// The scrutinee as something safe to read more than once.
    fn hoist_scrutinee(&mut self, s: Ex, e: &ast::Expr) -> Ex {
        if is_place_text(&s.s) && is_place(e) {
            return s;
        }
        if self.no_hoist > 0 {
            self.flag(
                e.span(),
                "hoist",
                "a value matched inside a `while` condition: bind it before the loop",
            );
            return s;
        }
        let name = self.fresh("matched");
        self.pre.push(format!("let {name} = {};", s.s));
        Ex::atom(name, s.t)
    }

    /// What a pattern tests and binds, against `s`.
    fn pattern(&mut self, pat: &ast::Pat, s: &Ex) -> Test {
        match pat {
            ast::Pat::Ignore(_) => Some((None, vec![])),
            ast::Pat::Path(pp) => self.pattern_path(pp, s),
            ast::Pat::Lit(l) => {
                let v = self.expr(&l.expr);
                let lhs = if s.t == Ty::Any || !s.t.known() {
                    self.coerce(s, &v.t)
                } else {
                    s.at(4)
                };
                Some((Some(format!("{lhs} == {}", v.at(4))), vec![]))
            }
            ast::Pat::Tuple(t)
                if t.path
                    .as_ref()
                    .is_some_and(|p| self.text(p.span()).trim() == "Some")
                    && t.items.len() == 1 =>
            {
                self.pattern_some(&t.items.first()?.0, s)
            }
            ast::Pat::Tuple(t) if t.path.is_none() => self.pattern_tuple(t, s),
            other => {
                let first = self
                    .text(other.span())
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                self.flag(
                    other.span(),
                    "pattern",
                    format!("the pattern `{first}`: .bs has no destructuring"),
                );
                // Its names still exist, so their uses are not flagged again.
                let mut names = vec![];
                crate::closure::pat_names(self, other, &mut names);
                let binds = names
                    .into_iter()
                    .map(|n| Bind {
                        name: n,
                        value: "nil".into(),
                        ty: Ty::Any,
                    })
                    .collect();
                Some((Some("false".into()), binds))
            }
        }
    }

    /// `None`, a const, or a name the value binds to.
    fn pattern_path(&mut self, pp: &ast::PatPath, s: &Ex) -> Test {
        let text = self.text(pp.path.span()).trim().to_string();
        if text == "None" {
            return Some((Some(Self::nil_check(s, false)), vec![]));
        }
        match self.resolve(&pp.path) {
            Named::Const { .. } | Named::Engine(_) => {
                let path = rune::alloc::clone::TryClone::try_clone(&pp.path).ok()?;
                let c = self.expr(&ast::Expr::Path(path));
                let lhs = if s.t == Ty::Any {
                    self.coerce(s, &c.t)
                } else {
                    s.at(4)
                };
                Some((Some(format!("{lhs} == {}", c.s)), vec![]))
            }
            _ if text.starts_with(|c: char| c.is_lowercase() || c == '_')
                && pp.path.rest.is_empty() =>
            {
                Some((
                    None,
                    vec![Bind {
                        name: text,
                        value: s.s.clone(),
                        ty: s.t.clone(),
                    }],
                ))
            }
            _ => {
                self.flag(pp.span(), "pattern", format!("the pattern `{text}`"));
                Some((Some("false".into()), vec![]))
            }
        }
    }

    /// `Some(p)`: the value holds one, and `p` matches what it holds.
    fn pattern_some(&mut self, inner_pat: &ast::Pat, s: &Ex) -> Test {
        let check = Self::nil_check(s, true);
        let dynamic = s.t == Ty::Any || !s.t.known();
        let inner_t = if dynamic { s.t.clone() } else { s.t.inner() };
        let inner_v = if dynamic {
            s.s.clone()
        } else {
            format!("{}.{UNWRAP}()", s.at(ATOM))
        };
        let (c, b) = self.pattern(inner_pat, &Ex::atom(inner_v, inner_t))?;
        let cond = match c {
            Some(c) if check.contains(' ') => format!("({check}) && {c}"),
            Some(c) => format!("{check} && {c}"),
            None => check,
        };
        Some((Some(cond), b))
    }

    /// `(a, b)` against a tuple's fields, each by its own pattern.
    fn pattern_tuple(&mut self, t: &ast::PatTuple, s: &Ex) -> Test {
        let mut conds = vec![];
        let mut binds = vec![];
        for (i, (p, _)) in t.items.iter().enumerate() {
            let sub = match &s.t {
                Ty::Shape(id) => {
                    let ft = self
                        .p
                        .shapes
                        .field(*id, &format!("_{i}"))
                        .unwrap_or(Ty::Unknown);
                    Ex::atom(format!("{}._{i}", s.at(ATOM)), self.p.norm(&ft))
                }
                _ => Ex::atom(format!("{}.at({i})", s.at(ATOM)), Ty::Any),
            };
            let (c, b) = self.pattern(p, &sub)?;
            if let Some(c) = c {
                conds.push(if c.contains(" && ") || c.contains(" || ") {
                    format!("({c})")
                } else {
                    c
                });
            }
            binds.extend(b);
        }
        Some(((!conds.is_empty()).then(|| conds.join(" && ")), binds))
    }

    /// The locals an `if` tests for nil, when its condition is only that:
    /// `util::is_nil(a) || b.is_none()`.
    fn nil_tested(&self, e: &ast::Expr, out: &mut Vec<String>) -> bool {
        let local = |e: &ast::Expr, cx: &Self| -> Option<String> {
            match e {
                ast::Expr::Path(p) if p.rest.is_empty() => {
                    Some(cx.text(p.span()).trim().to_string())
                }
                _ => None,
            }
        };
        match e {
            ast::Expr::Binary(b) if matches!(b.op, ast::BinOp::Or(_)) => {
                self.nil_tested(&b.lhs, out) && self.nil_tested(&b.rhs, out)
            }
            ast::Expr::Binary(b)
                if matches!(b.op, ast::BinOp::Is(_))
                    && self.text(b.rhs.span()).trim() == "Tuple" =>
            {
                local(&b.lhs, self).map(|n| out.push(n)).is_some()
            }
            ast::Expr::Group(g) => self.nil_tested(&g.expr, out),
            // `!(x is Object)`, `x is not Object`: a test that it holds something.
            ast::Expr::Unary(u) if matches!(u.op, ast::UnOp::Not(_)) => {
                let inner = match &*u.expr {
                    ast::Expr::Group(g) => &*g.expr,
                    e => e,
                };
                match inner {
                    ast::Expr::Binary(b)
                        if matches!(b.op, ast::BinOp::Is(_))
                            && self.text(b.rhs.span()).trim() != "Tuple" =>
                    {
                        local(&b.lhs, self).map(|n| out.push(n)).is_some()
                    }
                    _ => false,
                }
            }
            ast::Expr::Binary(b)
                if matches!(b.op, ast::BinOp::IsNot(_))
                    && self.text(b.rhs.span()).trim() != "Tuple" =>
            {
                local(&b.lhs, self).map(|n| out.push(n)).is_some()
            }
            ast::Expr::Call(c) => {
                let callee = self.text(c.expr.span()).trim().to_string();
                if callee.ends_with("is_nil") && c.args.len() == 1 {
                    return c
                        .args
                        .first()
                        .and_then(|a| local(&a.0, self))
                        .map(|n| out.push(n))
                        .is_some();
                }
                if let ast::Expr::FieldAccess(f) = &*c.expr
                    && c.args.is_empty()
                    && self.field_name(&f.expr_field).as_deref() == Some("is_none")
                {
                    return local(&f.expr, self).map(|n| out.push(n)).is_some();
                }
                false
            }
            _ => false,
        }
    }

    /// After `if is_nil(x) { return; }`, `x` is what it holds.
    fn narrow_after(&mut self, st: &ast::Stmt) -> Vec<String> {
        let e = match st {
            ast::Stmt::Expr(e) => e,
            ast::Stmt::Semi(s) => &s.expr,
            _ => return vec![],
        };
        let ast::Expr::If(i) = e else { return vec![] };
        if !i.expr_else_ifs.is_empty() || i.expr_else.is_some() || !block_diverges(&i.block) {
            return vec![];
        }
        let ast::Condition::Expr(c) = &*i.condition else {
            return vec![];
        };
        let mut names = vec![];
        if !self.nil_tested(c, &mut names) {
            return vec![];
        }
        let mut lines = vec![];
        for n in names {
            let Some(v) = self.lookup(&n).cloned() else {
                continue;
            };
            let t = self.local_ty(&n);
            if let Ty::Opt(inner) = t {
                let inner = *inner;
                lines.push(format!("let {} = {}.{UNWRAP}();", v.out, v.out));
                self.bind_typed(&n, &v.out, inner);
                if let Some(last) = self.scopes.last_mut().and_then(|s| s.last_mut()) {
                    last.alias = v.alias;
                }
            }
        }
        lines
    }

    pub(crate) fn if_expr(&mut self, i: &ast::ExprIf) -> Ex {
        let (c, binds) = self.cond(&i.condition);
        let (b, bt) = self.block_with(&i.block, true, binds);
        let bt = if block_diverges(&i.block) {
            Ty::Unknown
        } else {
            bt
        };
        let (rest, rt, has_else) = self.else_chain(&i.expr_else_ifs, i.expr_else.as_ref());
        let mut text = format!("if {c} {b}");
        if let Some(r) = rest {
            text.push_str(" else ");
            text.push_str(&r);
        }
        // Unknown stays unknown: a later round may type the branches.
        let t = if has_else {
            self.p.shapes.join(&bt, &rt)
        } else {
            Ty::Void
        };
        // A one-line Rune `if` stays one line.
        if !self.text(i.span()).contains('\n')
            && let Some(one) = one_line(&text)
        {
            return Ex::new(one, t, 0);
        }
        Ex::new(text, t, 0)
    }

    /// What follows an `else`. An `else if` whose condition needs lines
    /// before it becomes `else { lines; if .. }`, so they run only there.
    fn else_chain(
        &mut self,
        eifs: &[ast::ExprElseIf],
        els: Option<&ast::ExprElse>,
    ) -> (Option<String>, Ty, bool) {
        let Some((first, more)) = eifs.split_first() else {
            return match els {
                Some(e) => {
                    let (b, t) = self.block(&e.block, true);
                    let t = if block_diverges(&e.block) {
                        Ty::Unknown
                    } else {
                        t
                    };
                    (Some(b), t, true)
                }
                None => (None, Ty::Void, false),
            };
        };
        let saved = std::mem::take(&mut self.pre);
        let (c, binds) = self.cond(&first.condition);
        let pre = std::mem::replace(&mut self.pre, saved);
        let (b, bt) = self.block_with(&first.block, true, binds);
        let bt = if block_diverges(&first.block) {
            Ty::Unknown
        } else {
            bt
        };
        let (rest, rt, has_else) = self.else_chain(more, els);
        let mut chain = format!("if {c} {b}");
        if let Some(r) = rest {
            chain.push_str(" else ");
            chain.push_str(&r);
        }
        let t = if has_else {
            self.p.shapes.join(&bt, &rt)
        } else {
            Ty::Void
        };
        if pre.is_empty() {
            return (Some(chain), t, has_else);
        }
        let mut lines = pre;
        lines.push(chain);
        (Some(wrap(&lines)), t, has_else)
    }

    pub(crate) fn match_expr(&mut self, m: &ast::ExprMatch) -> Ex {
        let s = self.expr(&m.expr);
        let s = self.hoist_scrutinee(s, &m.expr);
        let mut arms: Vec<(Option<String>, String)> = vec![];
        let mut t = Ty::Unknown;
        let mut saw_some = false;
        let mut saw_none = false;
        let n = m.branches.len();
        for (k, (b, _)) in m.branches.iter().enumerate() {
            let before = self.comments_before(b.span().range().start);
            let Some((mut cond, binds)) = self.pattern(&b.pat, &s) else {
                continue;
            };
            let ptext = self.text(b.pat.span()).trim().to_string();
            saw_some |= ptext.starts_with("Some");
            saw_none |= ptext == "None";
            if let Some((_, g)) = &b.condition {
                // The guard reads the bound names as what they unwrap to.
                self.push_scope();
                for bd in &binds {
                    self.learn_local(&bd.name, &bd.ty);
                    let v = crate::cx::Var {
                        name: bd.name.clone(),
                        out: bd.value.clone(),
                        alias: false,
                        ty: None,
                    };
                    self.scopes
                        .last_mut()
                        .expect("the guard's scope was just pushed")
                        .push(v);
                }
                let g = self.expr(g);
                self.pop_scope();
                let g = self.coerce(&g, &Ty::Bool);
                cond = Some(match cond {
                    Some(c) => format!("{c} && ({g})"),
                    None => g,
                });
            }
            // An option matched both ways: the last arm is the `else`.
            if k + 1 == n && saw_some && saw_none && b.condition.is_none() {
                cond = None;
            }
            let (body, bt) = match &b.body {
                ast::Expr::Block(blk) if blk.async_token.is_none() && blk.label.is_none() => {
                    self.block_with(&blk.block, true, binds)
                }
                e => {
                    self.push_scope();
                    let mut lines = vec![];
                    for bd in binds {
                        self.learn_local(&bd.name, &bd.ty);
                        let out = self.bind(&bd.name, false);
                        lines.push(format!("let {out} = {};", bd.value));
                    }
                    let saved_pre = std::mem::take(&mut self.pre);
                    let saved_notes = std::mem::take(&mut self.notes);
                    let ex = self.expr(e);
                    let pre = std::mem::replace(&mut self.pre, saved_pre);
                    // An arm's notes go inside it, beside what they are about.
                    lines.extend(std::mem::replace(&mut self.notes, saved_notes));
                    lines.extend(pre);
                    let diverging = diverges(e) || matches!(ex.t, Ty::Void);
                    if diverging {
                        lines.push(
                            if ex.s.ends_with('}')
                                && (ex.s.starts_with("if ")
                                    || ex.s.starts_with("for ")
                                    || ex.s.starts_with("while "))
                            {
                                ex.s.clone()
                            } else if ex.s.is_empty() || ex.s == "nil" {
                                String::new()
                            } else {
                                format!("{};", ex.s)
                            },
                        );
                    } else {
                        lines.push(ex.s.clone());
                    }
                    self.pop_scope();
                    (wrap(&lines), if diverging { Ty::Void } else { ex.t })
                }
            };
            if !diverges(&b.body) {
                t = self.p.shapes.join(&t, &bt);
            }
            let body = if before.is_empty() {
                body
            } else {
                body.replacen("{\n", &format!("{{\n{}\n", indent(&before.join("\n"))), 1)
            };
            arms.push((cond, body));
            if arms.last().is_some_and(|a| a.0.is_none()) {
                break;
            }
        }
        if arms.is_empty() {
            return Ex::new("", Ty::Void, 0);
        }
        let mut out = String::new();
        for (k, (cond, body)) in arms.iter().enumerate() {
            let _ = match (k, cond) {
                (0, Some(c)) => write!(out, "if {c} {body}"),
                (0, None) => write!(out, "if true {body}"),
                (_, Some(c)) => write!(out, " else if {c} {body}"),
                (_, None) => write!(out, " else {body}"),
            };
        }
        let has_else = arms.iter().any(|a| a.0.is_none());
        let t = if has_else { t } else { Ty::Void };
        Ex::new(out, t, 0)
    }

    pub(crate) fn block_expr(&mut self, b: &ast::ExprBlock) -> Ex {
        if b.async_token.is_some() {
            self.flag(
                b.span(),
                "async-block",
                "an `async` block: make it an `async fn`",
            );
        }
        if b.label.is_some() {
            self.flag(b.span(), "label", "a labelled block");
        }
        let (text, t) = self.block(&b.block, true);
        Ex::new(format!("if true {text}"), t, 0)
    }

    pub(crate) fn while_expr(&mut self, w: &ast::ExprWhile) -> Ex {
        if w.label.is_some() {
            self.flag(w.span(), "label", "a labelled loop: .bs has no labels");
        }
        self.no_hoist += 1;
        let (c, binds) = self.cond(&w.condition);
        self.no_hoist -= 1;
        if !binds.is_empty() || matches!(&*w.condition, ast::Condition::ExprLet(_)) {
            self.flag(
                w.span(),
                "while-let",
                "`while let`: loop, then break when the value is none",
            );
        }
        let (b, _) = self.block_with(&w.body, false, binds);
        Ex::new(format!("while {c} {b}"), Ty::Void, 0)
    }

    pub(crate) fn loop_expr(&mut self, l: &ast::ExprLoop) -> Ex {
        if l.label.is_some() {
            self.flag(l.span(), "label", "a labelled loop: .bs has no labels");
        }
        let (b, _) = self.block(&l.body, false);
        Ex::new(format!("loop {b}"), Ty::Void, 0)
    }

    pub(crate) fn for_expr(&mut self, f: &ast::ExprFor) -> Ex {
        if f.label.is_some() {
            self.flag(f.span(), "label", "a labelled loop: .bs has no labels");
        }
        if let ast::Expr::Range(r) = &*f.iter {
            return self.for_range(f, r);
        }
        let (base, links) = self.chain(&f.iter);
        let names: Vec<&str> = links.iter().map(|l| l.name.as_str()).collect();
        let enumerate = names.last() == Some(&"enumerate")
            && names
                .iter()
                .all(|n| matches!(*n, "iter" | "enumerate" | "into_iter"));
        if enumerate
            && let ast::Pat::Tuple(t) = &f.binding
            && t.items.len() == 2
        {
            return self.for_enumerate(f, base, t);
        }
        if names == ["chars"] && matches!(f.binding, ast::Pat::Path(_)) {
            return self.for_chars(f, base);
        }
        if names.iter().any(|n| {
            matches!(
                *n,
                "rev" | "skip" | "take" | "zip" | "chars" | "windows" | "chunks"
            )
        }) {
            let chain = names.join("().");
            self.flag(
                f.span(),
                "iter",
                format!("a loop over `{chain}`: write it as an index loop"),
            );
        }
        let items = self.expr(&f.iter);
        if let ast::Pat::Tuple(t) = &f.binding
            && t.items.len() == 2
        {
            return self.for_entries(f, items, t);
        }
        let elem = match &items.t {
            Ty::Array(t) => (**t).clone(),
            Ty::Any => Ty::Any,
            Ty::Str => {
                self.flag(f.span(), "string-loop", "looping a string's characters");
                Ty::Str
            }
            _ => Ty::Unknown,
        };
        if self.rec() && !items.t.known() {
            self.learn_place(&f.iter, &Ty::Array(Box::new(Ty::Unknown)));
        }
        self.push_scope();
        let var = match &f.binding {
            ast::Pat::Path(_) | ast::Pat::Ignore(_) => self.loop_var(&f.binding, &elem),
            other => {
                self.flag(other.span(), "pattern", "a destructuring loop variable");
                let mut names = vec![];
                crate::closure::pat_names(self, other, &mut names);
                for n in &names {
                    self.bind(n, false);
                }
                self.fresh("entry")
            }
        };
        let (b, _) = self.block(&f.body, false);
        self.pop_scope();
        Ex::new(format!("for {var} in {} {b}", items.s), Ty::Void, 0)
    }

    /// `for i in a..b`, and `a..=b` as `a..b + 1`.
    fn for_range(&mut self, f: &ast::ExprFor, r: &ast::ExprRange) -> Ex {
        let from = r.start.as_ref().map(|s| self.expr(s));
        let to = r.end.as_ref().map(|s| self.expr(s));
        let (Some(from), Some(to)) = (from, to) else {
            self.flag(f.span(), "range", "an open range in a `for`");
            return Ex::new(
                format!("// {}", self.text(f.span()).lines().next().unwrap_or("")),
                Ty::Void,
                0,
            );
        };
        let to_s = if matches!(r.limits, ast::ExprRangeLimits::Closed(_)) {
            format!("{} + 1", to.at(4))
        } else {
            self.coerce(&to, &Ty::Int)
        };
        if self.rec() {
            for (ex, e) in [(&from, &r.start), (&to, &r.end)] {
                if let (false, Some(e)) = (ex.t.known(), e) {
                    self.learn_place(e, &Ty::Int);
                }
            }
        }
        self.push_scope();
        let var = self.loop_var(&f.binding, &Ty::Int);
        let (b, _) = self.block(&f.body, false);
        self.pop_scope();
        Ex::new(
            format!("for {var} in {}..{to_s} {b}", self.coerce(&from, &Ty::Int)),
            Ty::Void,
            0,
        )
    }

    /// `for (i, x) in xs.iter().enumerate()`: an index loop.
    fn for_enumerate(&mut self, f: &ast::ExprFor, base: &ast::Expr, t: &ast::PatTuple) -> Ex {
        let (Some((ip, _)), Some((xp, _))) = (t.items.first(), t.items.last()) else {
            return Ex::new("", Ty::Void, 0);
        };
        let items = self.expr(base);
        let items = self.hoist_items(items, base);
        self.push_scope();
        let idx = self.loop_var(ip, &Ty::Int);
        let elem = items.t.elem();
        let elem_name = self.elem_binding(xp, &elem);
        let (b, _) = self.block(&f.body, false);
        self.pop_scope();
        let first = elem_name.map(|n| format!("let {n} = {}[{idx}];", items.at(ATOM)));
        if items.t == Ty::Any {
            self.note(
                f.span(),
                "any-loop",
                "looping an `any` list by index: its elements are `any`",
            );
        }
        Ex::new(
            format!(
                "for {idx} in 0..{}.len() {}",
                items.at(ATOM),
                prepend(&b, first.as_deref())
            ),
            Ty::Void,
            0,
        )
    }

    /// `for c in s.chars()`: an index loop over one-byte slices.
    fn for_chars(&mut self, f: &ast::ExprFor, base: &ast::Expr) -> Ex {
        let s = self.expr(base);
        let s = self.hoist_items(s, base);
        let text = if s.t == Ty::Any {
            format!("{}.as_str()", s.at(ATOM))
        } else {
            s.at(ATOM)
        };
        self.flag(
            f.span(),
            "chars",
            "`chars()` as one-byte slices: right for ASCII, not for other text",
        );
        self.push_scope();
        let c = self.loop_var(&f.binding, &Ty::Str);
        let i = self.fresh("at");
        let (b, _) = self.block(&f.body, false);
        self.pop_scope();
        let first = format!("let {c} = {text}.slice({i}, {i} + 1);");
        Ex::new(
            format!("for {i} in 0..{text}.len() {}", prepend(&b, Some(&first))),
            Ty::Void,
            0,
        )
    }

    /// `for (k, v) in map`: its keys, and each value by its key.
    fn for_entries(&mut self, f: &ast::ExprFor, items: Ex, t: &ast::PatTuple) -> Ex {
        let (Some((kp, _)), Some((vp, _))) = (t.items.first(), t.items.last()) else {
            return Ex::new("", Ty::Void, 0);
        };
        if let Ty::Shape(id) = items.t {
            self.dynamic_use(id, f.span(), "its entries are looped over");
        }
        let items = self.hoist_items(items, &f.iter);
        self.push_scope();
        let k = self.loop_var(kp, &Ty::Str);
        let v = self.elem_binding(vp, &Ty::Any);
        let (b, _) = self.block(&f.body, false);
        self.pop_scope();
        let first = v.map(|v| format!("let {v} = {}.get({k});", items.at(ATOM)));
        Ex::new(
            format!(
                "for {k} in {}.keys() {}",
                items.at(ATOM),
                prepend(&b, first.as_deref())
            ),
            Ty::Void,
            0,
        )
    }

    fn hoist_items(&mut self, items: Ex, e: &ast::Expr) -> Ex {
        if is_place_text(&items.s) && is_place(e) {
            return items;
        }
        let n = self.fresh("items");
        self.pre.push(format!("let {n} = {};", items.s));
        Ex::atom(n, items.t)
    }

    fn loop_var(&mut self, p: &ast::Pat, t: &Ty) -> String {
        if let ast::Pat::Path(pp) = p {
            let n = self.text(pp.path.span()).trim().to_string();
            self.learn_local(&n, t);
            self.bind(&n, false)
        } else {
            let n = self.fresh("_i");
            self.bind(&n, false)
        }
    }

    fn elem_binding(&mut self, p: &ast::Pat, t: &Ty) -> Option<String> {
        match p {
            ast::Pat::Path(pp) => {
                let n = self.text(pp.path.span()).trim().to_string();
                self.learn_local(&n, t);
                Some(self.bind(&n, false))
            }
            ast::Pat::Ignore(_) => None,
            other => {
                self.flag(other.span(), "pattern", "a destructured element");
                None
            }
        }
    }
}

/// `if c {\n    a\n} else {\n    b\n}` as `if c { a } else { b }`, when
/// every block holds one line and the whole fits.
fn one_line(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len().is_multiple_of(2) {
        return None;
    }
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i % 2 == 0 {
            if !(l.ends_with('{') || i == lines.len() - 1) {
                return None;
            }
            if i == lines.len() - 1 && l.trim() != "}" {
                return None;
            }
            if i > 0 && !l.starts_with('}') {
                return None;
            }
            out.push_str(l.trim());
            out.push(' ');
        } else {
            let body = l.strip_prefix("    ")?;
            if body.starts_with(' ') || body.starts_with("//") || body.is_empty() {
                return None;
            }
            out.push_str(body);
            out.push(' ');
        }
    }
    let out = out.trim_end().to_string();
    (out.len() <= 100).then_some(out)
}

fn contains_unknown(t: &Ty) -> bool {
    match t {
        Ty::Unknown | Ty::Nil => true,
        Ty::Array(t) | Ty::Opt(t) => contains_unknown(t),
        _ => false,
    }
}

/// A struct name for what function `f` hands back.
pub(crate) fn fn_hint(module: &str, f: &str) -> (u8, String) {
    let stem = module.rsplit("::").next().unwrap_or(module);
    match f {
        "state" | "new" | "make" | "create" | "default" | "fresh_state" | "init_state" => {
            (2, crate::source::pascal(&format!("{stem}_state")))
        }
        f => {
            let f = f
                .trim_start_matches("make_")
                .trim_start_matches("new_")
                .trim_start_matches("build_")
                .trim_start_matches("read_")
                .trim_start_matches("get_");
            (2, crate::source::pascal(f))
        }
    }
}
