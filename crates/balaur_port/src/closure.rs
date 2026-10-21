// Closures become named functions: an iterator's a looping helper called with
// its captures, any other a `fn` value whose captures are flagged parameters.

use crate::call::Link;
use crate::cx::{Cx, Ex};
use crate::expr::indent;
use crate::project::{Param, Sig};
use crate::ty::Ty;
use rune::ast;
use rune::ast::Spanned;
use std::collections::HashMap;
use std::fmt::Write as _;

/// Names a piece of AST reads and names it binds.
#[derive(Default)]
pub(crate) struct Names {
    pub read: Vec<String>,
    pub bound: Vec<String>,
}

impl Names {
    fn read(&mut self, n: String) {
        if !self.read.contains(&n) {
            self.read.push(n);
        }
    }
}

pub(crate) fn pat_names(cx: &Cx<'_>, p: &ast::Pat, out: &mut Vec<String>) {
    match p {
        ast::Pat::Path(pp) => {
            if pp.path.rest.is_empty()
                && let ast::PathSegment::Ident(i) = &pp.path.first
            {
                let n = cx.ident(i);
                if n != "None" {
                    out.push(n);
                }
            }
        }
        ast::Pat::Tuple(t) => {
            for (p, _) in &t.items {
                pat_names(cx, p, out);
            }
        }
        ast::Pat::Vec(v) => {
            for (p, _) in &v.items {
                pat_names(cx, p, out);
            }
        }
        ast::Pat::Object(o) => {
            for (p, _) in &o.items {
                pat_names(cx, p, out);
            }
        }
        ast::Pat::Binding(b) => pat_names(cx, &b.pat, out),
        _ => {}
    }
}

impl Cx<'_> {
    pub(crate) fn names_expr(&self, e: &ast::Expr, n: &mut Names) {
        use ast::Expr as E;
        match e {
            E::Path(p) => {
                if p.rest.is_empty()
                    && let ast::PathSegment::Ident(i) = &p.first
                {
                    n.read(self.ident(i));
                }
            }
            E::Assign(a) => {
                self.names_expr(&a.lhs, n);
                self.names_expr(&a.rhs, n);
            }
            E::While(_) | E::Loop(_) | E::For(_) | E::Let(_) | E::If(_) | E::Match(_) => {
                self.names_flow(e, n);
            }
            E::Call(c) => {
                self.names_expr(&c.expr, n);
                for (a, _) in &c.args {
                    self.names_expr(a, n);
                }
            }
            E::FieldAccess(f) => self.names_expr(&f.expr, n),
            E::Binary(b) => {
                self.names_expr(&b.lhs, n);
                if !matches!(
                    b.op,
                    ast::BinOp::As(_) | ast::BinOp::Is(_) | ast::BinOp::IsNot(_)
                ) {
                    self.names_expr(&b.rhs, n);
                }
            }
            E::Unary(u) => self.names_expr(&u.expr, n),
            E::Index(i) => {
                self.names_expr(&i.target, n);
                self.names_expr(&i.index, n);
            }
            E::Break(b) => {
                if let Some(e) = &b.expr {
                    self.names_expr(e, n);
                }
            }
            E::Yield(y) => {
                if let Some(e) = &y.expr {
                    self.names_expr(e, n);
                }
            }
            E::Block(b) => self.names_block(&b.block, n),
            E::Return(r) => {
                if let Some(e) = &r.expr {
                    self.names_expr(e, n);
                }
            }
            E::Await(a) => self.names_expr(&a.expr, n),
            E::Try(t) => self.names_expr(&t.expr, n),
            E::Closure(c) => {
                if let ast::ExprClosureArgs::List { args, .. } = &c.args {
                    for (a, _) in args {
                        if let ast::FnArg::Pat(p) = a {
                            pat_names(self, p, &mut n.bound);
                        }
                    }
                }
                self.names_expr(&c.body, n);
            }
            E::Object(o) => {
                for (fa, _) in &o.assignments {
                    match &fa.assign {
                        Some((_, v)) => self.names_expr(v, n),
                        None => {
                            if let ast::ObjectKey::Path(p) = &fa.key {
                                n.read(self.text(p.span()).trim().to_string());
                            }
                        }
                    }
                }
            }
            E::Tuple(t) => {
                for (e, _) in &t.items {
                    self.names_expr(e, n);
                }
            }
            E::Vec(v) => {
                for (e, _) in &v.items {
                    self.names_expr(e, n);
                }
            }
            E::Range(r) => {
                if let Some(s) = &r.start {
                    self.names_expr(s, n);
                }
                if let Some(e) = &r.end {
                    self.names_expr(e, n);
                }
            }
            E::Group(g) => self.names_expr(&g.expr, n),
            E::Empty(g) => self.names_expr(&g.expr, n),
            E::MacroCall(mc) => {
                // Arguments are a token stream; any name in it may be a read,
                // `format!`'s `{name}` included.
                let text = self.text(mc.span()).to_string();
                for word in idents(&text) {
                    n.read(word);
                }
            }
            _ => {}
        }
    }

    /// `names_expr` for control flow, which binds names of its own.
    fn names_flow(&self, e: &ast::Expr, n: &mut Names) {
        use ast::Expr as E;
        match e {
            E::While(w) => {
                self.names_cond(&w.condition, n);
                self.names_block(&w.body, n);
            }
            E::Loop(l) => self.names_block(&l.body, n),
            E::For(f) => {
                self.names_expr(&f.iter, n);
                pat_names(self, &f.binding, &mut n.bound);
                self.names_block(&f.body, n);
            }
            E::Let(l) => {
                self.names_expr(&l.expr, n);
                pat_names(self, &l.pat, &mut n.bound);
            }
            E::If(i) => {
                self.names_cond(&i.condition, n);
                self.names_block(&i.block, n);
                for ei in &i.expr_else_ifs {
                    self.names_cond(&ei.condition, n);
                    self.names_block(&ei.block, n);
                }
                if let Some(e) = &i.expr_else {
                    self.names_block(&e.block, n);
                }
            }
            E::Match(m) => {
                self.names_expr(&m.expr, n);
                for (b, _) in &m.branches {
                    pat_names(self, &b.pat, &mut n.bound);
                    if let Some((_, c)) = &b.condition {
                        self.names_expr(c, n);
                    }
                    self.names_expr(&b.body, n);
                }
            }
            _ => {}
        }
    }

    fn names_cond(&self, c: &ast::Condition, n: &mut Names) {
        match c {
            ast::Condition::Expr(e) => self.names_expr(e, n),
            ast::Condition::ExprLet(l) => {
                self.names_expr(&l.expr, n);
                pat_names(self, &l.pat, &mut n.bound);
            }
            _ => {}
        }
    }

    pub(crate) fn names_block(&self, b: &ast::Block, n: &mut Names) {
        for s in &b.statements {
            match s {
                ast::Stmt::Local(l) => {
                    self.names_expr(&l.expr, n);
                    pat_names(self, &l.pat, &mut n.bound);
                }
                ast::Stmt::Expr(e) => self.names_expr(e, n),
                ast::Stmt::Semi(s) => self.names_expr(&s.expr, n),
                _ => {}
            }
        }
    }

    /// What `exprs` read from the scope around them, in order: captures.
    pub(crate) fn captures(&self, exprs: &[&ast::Expr], params: &[String]) -> Vec<String> {
        let mut n = Names::default();
        for e in exprs {
            self.names_expr(e, &mut n);
        }
        let scope = self.in_scope();
        n.read
            .into_iter()
            .filter(|r| scope.contains(r) && !params.contains(r))
            .filter(|r| !n.bound.contains(r) || scope.contains(r) && !n.bound.contains(r))
            .collect()
    }

    /// Runs `body` as the body of a new function, `base` named, taking
    /// `captures` then `params`; returns its signature and its name. The
    /// text goes into `self.lifted` for the item to write after itself.
    pub(crate) fn lift(
        &mut self,
        at: usize,
        base: &str,
        captures: &[String],
        params: &[(String, Ty)],
        body: &mut dyn FnMut(&mut Self) -> (String, Ty),
    ) -> (usize, String) {
        let key = (self.m, at);
        let this = captures.iter().any(|c| c == "this") && self.method;
        let caps: Vec<String> = captures
            .iter()
            .filter(|c| *c != "this" || !self.method)
            .cloned()
            .collect();
        // Only the editor's state: a callback each caller hands `S`.
        let state = caps == ["S"];
        let sig = if let Some(s) = self.p.lifted.get(&key) {
            *s
        } else {
            let n = self
                .p
                .sigs
                .iter()
                .filter(|s| s.module == self.m && s.lifted)
                .count();
            let mut name = format!("{base}_{n}");
            if self.p.modules[self.m].fns.contains_key(&name) {
                name.push('_');
            }
            let mut ps: Vec<Param> = caps
                .iter()
                .map(|c| Param {
                    name: c.clone(),
                    ty: Ty::Unknown,
                    mutated: false,
                    saw_any: false,
                })
                .collect();
            ps.extend(params.iter().map(|(n, t)| Param {
                name: n.clone(),
                ty: t.clone(),
                mutated: false,
                saw_any: false,
            }));
            self.p.sigs.push(Sig {
                module: self.m,
                name: name.clone(),
                out: name,
                params: ps,
                ret: Ty::Unknown,
                method: this,
                public: false,
                is_async: false,
                locals: HashMap::new(),
                lifted: true,
                captured: if state { 0 } else { caps.len() },
                state,
                poly: None,
            });
            let s = self.p.sigs.len() - 1;
            self.p.lifted.insert(key, s);
            s
        };
        // Captured parameters have the types they have out here.
        let cap_tys: Vec<Ty> = caps.iter().map(|c| self.local_ty(c)).collect();
        if self.rec() {
            for (i, t) in cap_tys.iter().enumerate() {
                let old = self.p.sigs[sig].params[i].ty.clone();
                let j = self.p.shapes.join(&old, t);
                if j != old {
                    self.p.sigs[sig].params[i].ty = j;
                    self.p.changed = true;
                }
            }
            for (i, (_, t)) in params.iter().enumerate() {
                let at = caps.len() + i;
                if let Some(p) = self.p.sigs[sig].params.get(at) {
                    let old = p.ty.clone();
                    let j = self.p.shapes.join(&old, t);
                    if j != old {
                        self.p.sigs[sig].params[at].ty = j;
                        self.p.changed = true;
                    }
                }
            }
        }
        let saved_sig = self.sig.replace(sig);
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_pre = std::mem::take(&mut self.pre);
        let saved_notes = std::mem::take(&mut self.notes);
        let saved_hint = self.hint.take();
        let saved_engine = std::mem::take(&mut self.engine_arg);
        let saved_hoist = std::mem::take(&mut self.no_hoist);
        let saved_method = self.method;
        let saved_tail = self.tail_hint.take();
        self.method = this;
        self.push_scope();
        if this {
            self.bind("this", false);
        }
        let names: Vec<String> = self.p.sigs[sig]
            .params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut outs = vec![];
        for n in &names {
            outs.push(self.bind(n, false));
        }
        let (text, ret) = body(self);
        self.learn_ret(&ret);
        let notes = std::mem::take(&mut self.notes);
        self.pop_scope();
        let s = &self.p.sigs[sig];
        let out_name = s.out.clone();
        let params_text: Vec<String> = s
            .params
            .iter()
            .zip(&outs)
            .enumerate()
            .map(|(i, (p, o))| {
                let amp = if self.p.by_ref(s, i) { "&mut " } else { "" };
                format!("{o}: {amp}{}", self.p.ty_str(&p.ty, self.m))
            })
            .collect();
        let ret = self.p.norm(&s.ret);
        let ret_text = match ret {
            Ty::Void | Ty::Unknown | Ty::Nil => String::new(),
            t => format!(" -> {}", self.p.ty_str(&t, self.m)),
        };
        let mut lifted = std::mem::take(&mut self.lifted);
        if !self.learn && self.p.peeking == 0 {
            let mut f = notes.join("\n");
            if !f.is_empty() {
                f.push('\n');
            }
            let _ = write!(
                f,
                "fn {out_name}({}){ret_text} {text}",
                params_text.join(", ")
            );
            lifted.push((this, f));
        }
        self.sig = saved_sig;
        self.scopes = saved_scopes;
        self.pre = saved_pre;
        self.notes = saved_notes;
        self.hint = saved_hint;
        self.engine_arg = saved_engine;
        self.no_hoist = saved_hoist;
        self.method = saved_method;
        self.tail_hint = saved_tail;
        self.lifted.append(&mut lifted);
        let call = if this {
            format!("self.{out_name}")
        } else {
            out_name
        };
        (sig, call)
    }

    fn enclosing(&self) -> String {
        self.cur().map_or_else(
            || "top".to_string(),
            |s| {
                s.name
                    .trim_end_matches(|c: char| c == '_' || c.is_ascii_digit())
                    .to_string()
            },
        )
    }

    fn closure_params(&mut self, c: &ast::ExprClosure) -> Vec<String> {
        let mut out = vec![];
        if let ast::ExprClosureArgs::List { args, .. } = &c.args {
            for (i, (a, _)) in args.iter().enumerate() {
                match a {
                    ast::FnArg::Pat(ast::Pat::Path(p)) => {
                        out.push(self.text(p.span()).trim().to_string());
                    }
                    ast::FnArg::Pat(ast::Pat::Ignore(_)) => out.push(format!("_{i}")),
                    other => {
                        self.flag(
                            other.span(),
                            "closure-pattern",
                            "a closure parameter that is a pattern",
                        );
                        out.push(format!("_p{i}"));
                    }
                }
            }
        }
        out
    }

    pub(crate) fn closure(&mut self, c: &ast::ExprClosure) -> Ex {
        if c.async_token.is_some() {
            self.flag(
                c.span(),
                "async-closure",
                "an `async` closure: lift it to an `async fn` by hand",
            );
        }
        let params = self.closure_params(c);
        let caps = self.captures(&[&c.body], &params);
        let hint = self
            .hint
            .clone()
            .map_or_else(|| "fn".into(), |h| h.1.to_lowercase());
        let base = format!("{}_{}", self.enclosing(), hint);
        let ps: Vec<(String, Ty)> = params.iter().map(|p| (p.clone(), Ty::Unknown)).collect();
        let body: &ast::Expr = &c.body;
        let (sig, name) = self.lift(c.span().range().start, &base, &caps, &ps, &mut |cx| {
            cx.fn_body_expr(body)
        });
        let shown: Vec<String> = caps
            .iter()
            .filter(|c| *c != "this" || !self.method)
            .cloned()
            .collect();
        if !shown.is_empty() && shown != ["S"] {
            self.flag(
                c.span(),
                "capture",
                format!("the closure read {}: `{name}` takes them first, but a fn value cannot carry them", shown.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ")),
            );
        } else if caps.iter().any(|c| c == "this") {
            self.flag(c.span(), "capture", format!("the closure read `this`: `{name}` is a method, and a fn value names a plain function"));
        }
        Ex::atom(name, Ty::Fn(sig))
    }

    /// A widget spec's `on` closure as a handler the Editor's `handle`
    /// calls: `fn(S: &mut State, data: any, value: any)`, its captures read
    /// back out of `data` where the spec put them. The lifted name and the
    /// captures, for the spec to carry.
    pub(crate) fn handler(&mut self, c: &ast::ExprClosure) -> (String, Vec<String>) {
        let params = self.closure_params(c);
        let value = params.first().cloned().unwrap_or_else(|| "_value".into());
        let mut caps = self.captures(&[&c.body], &params);
        if caps.iter().any(|c| c == "this") {
            self.flag(
                c.span(),
                "capture",
                "the handler read `this`: the Editor's `handle` hands it only the state",
            );
        }
        caps.retain(|c| c != "this" && c != "S" && c != "data");
        let tys: Vec<Ty> = caps.iter().map(|c| self.local_ty(c)).collect();
        let base = format!("{}_on", self.enclosing());
        let ps = [("data".to_string(), Ty::Any), (value, Ty::Any)];
        let body: &ast::Expr = &c.body;
        let held = caps.clone();
        let (sig, name) = self.lift(
            c.span().range().start,
            &base,
            &["S".into()],
            &ps,
            &mut |cx| {
                let mut prologue = vec![];
                for (cap, t) in held.iter().zip(&tys) {
                    let out = cx.bind(cap, false);
                    if let Some(v) = cx.scopes.last_mut().and_then(|s| s.last_mut()) {
                        v.ty = Some(t.clone());
                    }
                    let ts = match cx.p.norm(t) {
                        Ty::Unknown | Ty::Nil | Ty::Void => "any".to_string(),
                        t => cx.ty_str(&t),
                    };
                    prologue.push(format!("let {out}: {ts} = data.{cap};"));
                }
                let (text, t) = cx.fn_body_expr(body);
                if prologue.is_empty() {
                    return (text, t);
                }
                let inner = text.strip_prefix('{').unwrap_or(&text);
                (format!("{{\n{}{inner}", indent(&prologue.join("\n"))), t)
            },
        );
        // What the Editor calls it with, whatever its body reads them as.
        for p in self.p.sigs[sig].params.iter_mut().skip(1) {
            p.ty = Ty::Any;
        }
        (name, caps)
    }

    /// A closure's body as a function body.
    pub(crate) fn fn_body_expr(&mut self, body: &ast::Expr) -> (String, Ty) {
        if let ast::Expr::Block(b) = body
            && b.async_token.is_none()
        {
            let (text, t) = self.block(&b.block, true);
            return (text, t);
        }
        let saved_pre = std::mem::take(&mut self.pre);
        let ex = self.expr(body);
        let pre = std::mem::replace(&mut self.pre, saved_pre);
        let notes = std::mem::take(&mut self.notes);
        let mut lines: Vec<String> = notes;
        lines.extend(pre);
        let value = !matches!(ex.t, Ty::Void) && !ex.s.starts_with("return");
        if value {
            lines.push(ex.s.clone());
        } else {
            lines.push(format!("{};", ex.s));
        }
        (
            format!("{{\n{}\n}}", indent(&lines.join("\n"))),
            if value { ex.t } else { Ty::Void },
        )
    }

    /// `xs.iter().filter(f).count()` and its kind, as a loop in a helper.
    pub(crate) fn iter_chain(&mut self, base: &ast::Expr, links: &[Link<'_>], at: usize) -> Ex {
        let span = links.last().map_or_else(|| base.span(), |l| l.span);
        // Links before `iter` run first: `x.keys().iter()..`.
        let mut recv = self.expr(base);
        for l in &links[..at] {
            let fake = Link {
                name: l.name.clone(),
                args: l.args.clone(),
                span: l.span,
            };
            recv = self.method_on(recv, &fake);
        }
        let stages = &links[at + 1..];
        let supported = stages.iter().enumerate().all(|(i, l)| {
            let last = i + 1 == stages.len();
            match l.name.as_str() {
                "filter" | "map" => {
                    !last && l.args.len() == 1 && matches!(l.args[0], ast::Expr::Closure(_))
                }
                "any" | "all" | "find" | "position" => {
                    last && l.args.len() == 1 && matches!(l.args[0], ast::Expr::Closure(_))
                }
                "count" | "collect" => last,
                _ => false,
            }
        });
        if !supported {
            let names: Vec<&str> = stages.iter().map(|l| l.name.as_str()).collect();
            self.flag(
                span,
                "iter",
                format!(
                    "an iterator chain (`{}`) the translator does not lower: write the loop",
                    names.join("().")
                ),
            );
            let mut cur = recv;
            for l in stages {
                cur = self.method_on(cur, l);
            }
            return cur;
        }
        let terminal = stages
            .last()
            .map_or("collect", |l| l.name.as_str())
            .to_string();
        let mut closures: Vec<&ast::ExprClosure> = vec![];
        for l in stages {
            if let Some(ast::Expr::Closure(c)) = l.args.first() {
                closures.push(c);
            }
        }
        let bodies: Vec<&ast::Expr> = closures.iter().map(|c| &*c.body).collect();
        let mut all_params = vec![];
        for c in &closures {
            all_params.extend(self.closure_params(c));
        }
        let caps = self.captures(&bodies, &all_params);
        let elem = match &recv.t {
            Ty::Array(t) => (**t).clone(),
            Ty::Any => Ty::Any,
            _ => Ty::Unknown,
        };
        let base_name = format!("{}_{terminal}", self.enclosing());
        let items_ty = Ty::Array(Box::new(elem.clone()));
        let stage_info: Vec<(String, Option<&ast::ExprClosure>)> = stages
            .iter()
            .map(|l| {
                (
                    l.name.clone(),
                    match l.args.first() {
                        Some(ast::Expr::Closure(c)) => Some(c),
                        _ => None,
                    },
                )
            })
            .collect();
        let (_, name) = self.lift(
            span.range().start,
            &base_name,
            &caps,
            &[("items".to_string(), items_ty)],
            &mut |cx| cx.chain_body(&stage_info, &elem, &terminal),
        );
        // Captures first, as every lifted function takes them, then the items.
        let mut args = vec![];
        for c in caps.iter().filter(|c| *c != "this" || !self.method) {
            let v = self.lookup(c).map_or_else(|| c.clone(), |v| v.out.clone());
            args.push(v);
        }
        args.push(recv.s.clone());
        let sig = self.p.lifted[&(self.m, span.range().start)];
        let ret = self.p.norm(&self.p.sigs[sig].ret);
        Ex::atom(format!("{name}({})", args.join(", ")), ret)
    }

    fn chain_body(
        &mut self,
        stages: &[(String, Option<&ast::ExprClosure>)],
        elem: &Ty,
        terminal: &str,
    ) -> (String, Ty) {
        let mut body: Vec<String> = vec![];
        let first = stages
            .iter()
            .find_map(|(_, c)| c.map(|c| self.closure_params(c)))
            .and_then(|p| p.into_iter().next())
            .unwrap_or_else(|| "item".into());
        let index = terminal == "position";
        let mut cur = self.bind(&first, false);
        let mut cur_ty = elem.clone();
        self.learn_local(&first, elem);
        for (name, closure) in stages {
            let Some(c) = closure else { continue };
            let params = self.closure_params(c);
            if let Some(p) = params.first()
                && (p != &first || cur != *p)
            {
                let out = self.bind(p, false);
                self.learn_local(p, &cur_ty);
                if out != cur {
                    body.push(format!("let {out} = {cur};"));
                }
                cur = out;
            }
            let (pre, value) = self.inline_body(&c.body);
            body.extend(pre);
            match name.as_str() {
                "filter" => body.push(format!(
                    "if !{} {{\n    continue;\n}}",
                    value.at(crate::cx::UNARY)
                )),
                "map" => {
                    let v = self.fresh("mapped");
                    body.push(format!("let {v} = {};", value.s));
                    self.bind(&v, false);
                    self.learn_local(&v, &value.t);
                    cur = v;
                    cur_ty = value.t.clone();
                }
                "any" => body.push(format!("if {} {{\n    return true;\n}}", value.s)),
                "all" => body.push(format!(
                    "if !{} {{\n    return false;\n}}",
                    value.at(crate::cx::UNARY)
                )),
                "find" => body.push(format!("if {} {{\n    return {cur};\n}}", value.s)),
                "position" => body.push(format!("if {} {{\n    return i;\n}}", value.s)),
                _ => {}
            }
        }
        let (head, tail, ret) = match terminal {
            "any" => (vec![], "false".to_string(), Ty::Bool),
            "all" => (vec![], "true".to_string(), Ty::Bool),
            "find" => (vec![], "nil".to_string(), Ty::opt(cur_ty.clone())),
            "position" => (vec![], "nil".to_string(), Ty::opt(Ty::Int)),
            "count" => {
                body.push("n += 1;".into());
                (vec!["let n = 0;".to_string()], "n".to_string(), Ty::Int)
            }
            _ => {
                body.push(format!("out.push({cur});"));
                let t = format!("[{}]", self.ty_str(&cur_ty));
                (
                    vec![format!("let out: {t} = [];")],
                    "out".to_string(),
                    Ty::Array(Box::new(cur_ty.clone())),
                )
            }
        };
        let loop_head = if index {
            format!("for i in 0..items.len() {{\n    let {first} = items[i];")
        } else {
            format!("for {first} in items {{")
        };
        let mut lines = head;
        lines.push(format!("{loop_head}\n{}\n}}", indent(&body.join("\n"))));
        lines.push(tail);
        let notes = std::mem::take(&mut self.notes);
        let mut all = notes;
        all.extend(lines);
        (format!("{{\n{}\n}}", indent(&all.join("\n"))), ret)
    }

    /// A closure body inside a loop: its statements, then its value.
    fn inline_body(&mut self, body: &ast::Expr) -> (Vec<String>, Ex) {
        if let ast::Expr::Block(b) = body {
            let mut lines = vec![];
            let n = b.block.statements.len();
            let mut value = Ex::atom("true", Ty::Bool);
            for (i, st) in b.block.statements.iter().enumerate() {
                let last = i + 1 == n;
                if let (true, ast::Stmt::Expr(e)) = (last, st) {
                    let saved = std::mem::take(&mut self.pre);
                    value = self.expr(e);
                    let pre = std::mem::replace(&mut self.pre, saved);
                    lines.extend(pre);
                } else {
                    lines.push(self.stmt(st, false).0);
                }
            }
            let mut n = Names::default();
            self.names_expr(body, &mut n);
            if self.text(body.span()).contains("return") {
                self.flag(
                    body.span(),
                    "closure-return",
                    "a `return` inside a closure body inlined into a loop",
                );
            }
            return (lines, value);
        }
        let saved = std::mem::take(&mut self.pre);
        let value = self.expr(body);
        let pre = std::mem::replace(&mut self.pre, saved);
        (pre, value)
    }

    /// One method link on a receiver already written.
    pub(crate) fn method_on(&mut self, recv: Ex, l: &Link<'_>) -> Ex {
        self.apply_method(None, recv, l)
    }
}

/// The identifiers in a piece of text, outside nothing: a rough read of a
/// macro's arguments.
fn idents(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut prev_dot = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            if !cur.is_empty() && !prev_dot && !cur.starts_with(|c: char| c.is_ascii_digit()) {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
            prev_dot = c == '.';
        }
    }
    if !cur.is_empty() && !prev_dot {
        out.push(cur);
    }
    out
}
