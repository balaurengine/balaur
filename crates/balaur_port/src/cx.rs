// The walker: `learn` rounds over a function's AST record types and fields,
// the last writes `.bs`, so what is written follows what was inferred.

use crate::project::{Project, Sig};
use crate::source::safe_ident;
use crate::ty::Ty;
use rune::ast;
use rune::ast::Spanned;

/// An emitted expression: its text, its type, how tightly it binds.
#[derive(Clone, Debug)]
pub(crate) struct Ex {
    pub s: String,
    pub t: Ty,
    pub prec: u8,
}

/// How `.bs` reads the value out of a `T?`: one place to change if the
/// compiler lands another spelling.
pub(crate) const UNWRAP: &str = "unwrap";

pub(crate) const ATOM: u8 = 10;
pub(crate) const UNARY: u8 = 9;

impl Ex {
    pub(crate) fn new(s: impl Into<String>, t: Ty, prec: u8) -> Self {
        Self {
            s: s.into(),
            t,
            prec,
        }
    }
    pub(crate) fn atom(s: impl Into<String>, t: Ty) -> Self {
        Self::new(s, t, ATOM)
    }
    /// The text, in parentheses when it binds looser than `min`.
    pub(crate) fn at(&self, min: u8) -> String {
        if self.prec < min {
            format!("({})", self.s)
        } else {
            self.s.clone()
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Var {
    /// The Rune name.
    pub name: String,
    /// What `.bs` calls it.
    pub out: String,
    /// Bound from a read of a struct place: `.bs` copies it, Rune shared it.
    pub alias: bool,
    /// This binding's own type, where it differs from the name's elsewhere
    /// in the function: an option unwrapped after a check.
    pub ty: Option<Ty>,
}

/// What a path names.
#[derive(Clone, Debug)]
pub(crate) enum Named {
    Local(Var),
    /// The script's `this`, inside one of its methods.
    This,
    Fn(usize),
    Const {
        module: usize,
        name: String,
    },
    Engine(String),
    None,
    Unknown(String),
}

pub(crate) struct Cx<'p> {
    pub p: &'p mut Project,
    pub m: usize,
    pub learn: bool,
    pub sig: Option<usize>,
    pub scopes: Vec<Vec<Var>>,
    /// Lines to put before the statement being written.
    pub pre: Vec<String>,
    /// Notes for the statement being written, each a `MARK` comment.
    pub notes: Vec<String>,
    /// Functions lifted out of the one being written, written after it.
    pub lifted: Vec<(bool, String)>,
    /// A struct name for a literal in a function's tail.
    pub tail_hint: Option<(u8, String)>,
    pub tmp: usize,
    /// A name for the struct of an object literal, from where it sits.
    pub hint: Option<(u8, String)>,
    /// The literal being walked is an argument the engine takes as a table.
    pub engine_arg: bool,
    /// The function being walked is a method of the file's struct.
    pub method: bool,
    /// Inside a `while` condition: nothing can be hoisted out of it.
    pub no_hoist: u32,
}

impl<'p> Cx<'p> {
    pub(crate) fn new(p: &'p mut Project, m: usize, learn: bool) -> Self {
        Self {
            p,
            m,
            learn,
            sig: None,
            scopes: vec![],
            pre: vec![],
            notes: vec![],
            lifted: vec![],
            tail_hint: None,
            tmp: 0,
            hint: None,
            engine_arg: false,
            method: false,
            no_hoist: 0,
        }
    }

    pub(crate) fn text(&self, span: ast::Span) -> &str {
        let r = span.range();
        self.p.modules[self.m].src.get(r).unwrap_or("")
    }

    pub(crate) fn ident(&self, i: &ast::Ident) -> String {
        self.text(i.span).to_string()
    }

    pub(crate) fn line(&self, span: ast::Span) -> u32 {
        self.p.modules[self.m].lines.line(span.range().start)
    }

    /// Something the walker could not map: a note in the output and a line
    /// in the report. Only the writing round records it.
    pub(crate) fn flag(&mut self, span: ast::Span, kind: &'static str, msg: impl Into<String>) {
        if self.learn || self.p.peeking > 0 {
            return;
        }
        let msg = msg.into();
        let line = self.line(span);
        let note = format!("// {}: {msg}", crate::MARK);
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
        self.p.modules[self.m].flag(line, kind, msg);
    }

    /// Something worth knowing that the output already handles.
    pub(crate) fn note(&mut self, span: ast::Span, kind: &'static str, msg: impl Into<String>) {
        if self.learn || self.p.peeking > 0 {
            return;
        }
        let line = self.line(span);
        self.p.modules[self.m].note(line, kind, msg.into());
    }

    pub(crate) fn fresh(&mut self, base: &str) -> String {
        self.tmp += 1;
        format!("{base}{}", self.tmp)
    }

    // Scopes.

    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(vec![]);
    }
    pub(crate) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub(crate) fn bind(&mut self, name: &str, alias: bool) -> String {
        let out = if name == "this" && self.method {
            "self".to_string()
        } else {
            safe_ident(name)
        };
        if self.scopes.is_empty() {
            self.push_scope();
        }
        self.scopes
            .last_mut()
            .expect("a scope was just pushed if there was none")
            .push(Var {
                name: name.to_string(),
                out: out.clone(),
                alias,
                ty: None,
            });
        out
    }

    /// `name` from here on is `out`, of type `ty`.
    pub(crate) fn bind_typed(&mut self, name: &str, out: &str, ty: Ty) {
        if self.scopes.is_empty() {
            self.push_scope();
        }
        self.scopes
            .last_mut()
            .expect("a scope was just pushed if there was none")
            .push(Var {
                name: name.to_string(),
                out: out.to_string(),
                alias: false,
                ty: Some(ty),
            });
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<&Var> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|v| v.name == name)
    }

    /// Every name in scope, innermost last: what a closure may capture.
    pub(crate) fn in_scope(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for s in &self.scopes {
            for v in s {
                if !out.contains(&v.name) {
                    out.push(v.name.clone());
                }
            }
        }
        out
    }

    // Types.

    pub(crate) fn cur(&self) -> Option<&Sig> {
        self.sig.map(|s| &self.p.sigs[s])
    }

    /// A local's type: everything assigned to it anywhere in the function.
    pub(crate) fn local_ty(&self, name: &str) -> Ty {
        if let Some(t) = self.lookup(name).and_then(|v| v.ty.clone()) {
            return self.p.norm(&t);
        }
        let Some(sig) = self.cur() else {
            return Ty::Unknown;
        };
        if name == "this"
            && let Some(s) = self.p.modules[self.m].script
        {
            return Ty::Shape(s);
        }
        if let Some(p) = sig.params.iter().find(|p| p.name == name) {
            // Only ever handed `any`: an `any`.
            if !p.ty.known() && p.saw_any {
                return Ty::Any;
            }
            return self.p.norm(&p.ty);
        }
        sig.locals.get(name).map_or(Ty::Unknown, |t| self.p.norm(t))
    }

    /// `name` holds a `t` too.
    pub(crate) fn learn_local(&mut self, name: &str, t: &Ty) {
        if !self.rec() || !t.known() {
            return;
        }
        let Some(sig) = self.sig else { return };
        if name == "this" {
            return;
        }
        if let Some(i) = self.p.sigs[sig].params.iter().position(|p| p.name == name) {
            let old = self.p.sigs[sig].params[i].ty.clone();
            let j = self.p.shapes.join(&old, t);
            if j != old {
                self.p.sigs[sig].params[i].ty = j;
                self.p.changed = true;
            }
            return;
        }
        let old = self.p.sigs[sig]
            .locals
            .get(name)
            .cloned()
            .unwrap_or(Ty::Unknown);
        let j = self.p.shapes.join(&old, t);
        if j != old {
            self.p.sigs[sig].locals.insert(name.to_string(), j);
            self.p.changed = true;
        }
    }

    /// Records that the place `e` (a name, a field, an element) holds `t`.
    pub(crate) fn learn_place(&mut self, e: &ast::Expr, t: &Ty) {
        if !self.rec() || !t.known() {
            return;
        }
        match e {
            ast::Expr::Path(p) if p.rest.is_empty() => {
                if let ast::PathSegment::Ident(i) = &p.first {
                    let name = self.ident(i);
                    if self.lookup(&name).is_some() {
                        self.learn_local(&name, t);
                    }
                }
            }
            ast::Expr::FieldAccess(f) => {
                let Some(field) = self.field_name(&f.expr_field) else {
                    return;
                };
                let base = self.peek_ty(&f.expr);
                // An option written to a field was checked first (Rune would
                // have failed on it otherwise): the field holds what is inside.
                let t = &match t {
                    Ty::Opt(inner) => (**inner).clone(),
                    t => t.clone(),
                };
                if let Ty::Shape(id) = &base {
                    self.p.shapes.get_mut(*id).accessed.insert(field.clone());
                }
                match base {
                    Ty::Shape(id) => self.p.shapes.set_field(id, &field, t.clone()),
                    Ty::Opt(inner) if matches!(*inner, Ty::Shape(_)) => {
                        if let Ty::Shape(id) = *inner {
                            self.p.shapes.set_field(id, &field, t.clone());
                        }
                    }
                    Ty::Unknown => {
                        // A field written on something untyped: a struct of
                        // its own until a call site says otherwise.
                        let id = self.implicit_shape(&f.expr);
                        if let Some(id) = id {
                            self.p.shapes.set_field(id, &field, t.clone());
                        }
                    }
                    _ => {}
                }
            }
            ast::Expr::Index(ix) => {
                let base = self.peek_ty(&ix.target);
                match base {
                    Ty::Array(_) | Ty::Unknown => {
                        if !matches!(self.peek_ty(&ix.index), Ty::Str) {
                            self.learn_place(&ix.target, &Ty::Array(Box::new(t.clone())));
                        }
                    }
                    Ty::Shape(id) => {
                        if let Some(k) = self.lit_str(&ix.index) {
                            self.p.shapes.set_field(id, &k, t.clone());
                        } else {
                            self.p.shapes.make_free(id, "written under a computed key");
                        }
                    }
                    _ => {}
                }
            }
            ast::Expr::Group(g) => self.learn_place(&g.expr, t),
            _ => {}
        }
    }

    /// A struct for an untyped name whose fields are used, so the uses have
    /// somewhere to go; a call site's argument merges into it later.
    pub(crate) fn implicit_shape(&mut self, e: &ast::Expr) -> Option<usize> {
        if !self.rec() {
            return None;
        }
        let name = match e {
            ast::Expr::Path(p) if p.rest.is_empty() => match &p.first {
                ast::PathSegment::Ident(i) => self.ident(i),
                _ => return None,
            },
            _ => return None,
        };
        self.lookup(&name)?;
        let id = self.p.shapes.add(crate::ty::Shape {
            home: Some((self.m, e.span().range().start as u32)),
            ..Default::default()
        });
        let hint = crate::project::name_hint(&name);
        // `S` is the editor's state wherever it is passed; other names are guesses.
        let strength = if name == "S" {
            hint.0
        } else {
            hint.0.saturating_sub(1)
        };
        self.p.shapes.hint(id, strength, &hint.1);
        self.learn_local(&name, &Ty::Shape(id));
        Some(id)
    }

    /// The type of `e` without writing or recording anything.
    pub(crate) fn peek_ty(&mut self, e: &ast::Expr) -> Ty {
        let saved = (
            std::mem::take(&mut self.pre),
            std::mem::take(&mut self.notes),
            std::mem::take(&mut self.lifted),
            self.tmp,
            self.hint.take(),
        );
        self.p.peeking += 1;
        let t = self.expr(e).t;
        self.p.peeking -= 1;
        self.pre = saved.0;
        self.notes = saved.1;
        self.lifted = saved.2;
        self.tmp = saved.3;
        self.hint = saved.4;
        t
    }

    pub(crate) fn field_name(&self, f: &ast::ExprField) -> Option<String> {
        match f {
            ast::ExprField::Path(p) if p.rest.is_empty() => match &p.first {
                ast::PathSegment::Ident(i) => Some(self.ident(i)),
                _ => None,
            },
            ast::ExprField::LitNumber(n) => Some(format!("_{}", self.text(n.span))),
            _ => None,
        }
    }

    /// The text of a plain string literal, if `e` is one.
    pub(crate) fn lit_str(&self, e: &ast::Expr) -> Option<String> {
        if let ast::Expr::Lit(l) = e
            && let ast::Lit::Str(s) = &l.lit
        {
            let raw = self.text(s.span);
            let body = raw.strip_prefix('"')?.strip_suffix('"')?;
            if !body.contains('\\') {
                return Some(body.to_string());
            }
        }
        None
    }

    /// A parameter handed on where an `any` is taken: an `any` itself,
    /// unless something else types it.
    pub(crate) fn flows_into_any(&mut self, e: &ast::Expr) {
        if !self.rec() {
            return;
        }
        let ast::Expr::Path(p) = e else { return };
        if !p.rest.is_empty() {
            return;
        }
        let name = self.text(p.span()).trim().to_string();
        let Some(sig) = self.sig else { return };
        if let Some(param) = self.p.sigs[sig].params.iter_mut().find(|q| q.name == name)
            && !param.saw_any
        {
            param.saw_any = true;
            self.p.changed = true;
        }
    }

    /// A struct read or written by computed key: while learning, a hint it
    /// is free-form data; when writing, a flag if it stayed a struct.
    pub(crate) fn dynamic_use(&mut self, id: usize, span: ast::Span, why: &str) {
        if self.rec() {
            self.p.shapes.make_free(id, why);
            return;
        }
        if !self.learn && !self.p.shapes.is_free(id) {
            let name = self.p.shape_name(id);
            self.flag(
                span,
                "computed-key",
                format!("{why} on struct `{name}`, whose fields are fixed"),
            );
        }
    }

    /// Joins `t` into the current function's result.
    pub(crate) fn learn_ret(&mut self, t: &Ty) {
        if !self.learn || self.p.peeking > 0 {
            return;
        }
        let Some(sig) = self.sig else { return };
        // What `fn rows()` hands back is a row, unless something better names it.
        let fn_name = self.p.sigs[sig].name.clone();
        match t {
            Ty::Array(e) => {
                if let Ty::Shape(id) = **e {
                    let n = crate::source::pascal(&crate::source::singular(&fn_name));
                    self.p.shapes.hint(id, 1, &n);
                }
            }
            Ty::Shape(id) => {
                let (_, n) = crate::stmt::fn_hint(&self.p.modules[self.m].key, &fn_name);
                self.p.shapes.hint(*id, 1, &n);
            }
            _ => {}
        }
        let old = self.p.sigs[sig].ret.clone();
        let j = self.p.shapes.join(&old, t);
        if j != old {
            self.p.sigs[sig].ret = j;
            self.p.changed = true;
        }
    }

    /// `.bs` spelling of a type, from inside this module.
    pub(crate) fn ty_str(&self, t: &Ty) -> String {
        self.p.ty_str(t, self.m)
    }

    /// `ex` where a `want` is wanted: an `any` read out as the type.
    /// `coerce`, flagging an `any` where a struct or array is wanted: no
    /// read turns one into the other.
    pub(crate) fn coerce_at(&mut self, ex: &Ex, want: &Ty, span: ast::Span) -> String {
        let w = self.p.norm(want);
        if let (Ty::Shape(id), Ty::Any, true) = (&ex.t, &w, self.rec()) {
            let site = (self.m, span.range().start);
            self.p.shapes.get_mut(*id).to_any.insert(site);
        }
        if let (Ty::Any, Ty::Shape(id), true) = (&ex.t, &w, self.rec())
            && ex.s != "nil"
        {
            let site = (self.m, span.range().start);
            self.p.shapes.get_mut(*id).any_fed.insert(site);
        }
        if ex.t == Ty::Any && ex.s != "nil" && matches!(w, Ty::Shape(_) | Ty::Array(_) | Ty::Fn(_))
        {
            let ws = self.ty_str(&w);
            if !matches!(&w, Ty::Array(e) if **e == Ty::Any) {
                self.flag(
                    span,
                    "any-to-struct",
                    format!("an `any` where a `{ws}` is wanted"),
                );
            }
        }
        self.coerce(ex, want)
    }

    pub(crate) fn coerce(&self, ex: &Ex, want: &Ty) -> String {
        let want = self.p.norm(want);
        if let Ty::Opt(inner) = &ex.t
            && want.known()
            && !matches!(want, Ty::Opt(_) | Ty::Any | Ty::Nil | Ty::Void)
            && ex.s != "nil"
        {
            let inner = Ex::new(
                format!("{}.{UNWRAP}()", ex.at(ATOM)),
                (**inner).clone(),
                ATOM,
            );
            return self.coerce(&inner, &want);
        }
        if ex.t == Ty::Any {
            let m = match want {
                Ty::Int => Some("as_int"),
                Ty::Float => Some("as_float"),
                Ty::Bool => Some("as_bool"),
                Ty::Str => Some("as_str"),
                Ty::Node => Some("as_node"),
                _ => None,
            };
            if let Some(m) = m {
                if ex.s == "nil" {
                    return crate::project::default_of(&want);
                }
                return format!("{}.{m}()", ex.at(ATOM));
            }
        }
        ex.s.clone()
    }
}
