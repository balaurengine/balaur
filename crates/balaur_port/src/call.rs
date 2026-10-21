// Calls: to the editor's own functions (typed from what the inference
// learned), to the engine, and Rune's methods mapped to `.bs` ones.

use crate::api::Api;
use crate::cx::{ATOM, Cx, Ex, Named, UNARY, UNWRAP};
use crate::ty::Ty;
use rune::ast;
use rune::ast::Spanned;

/// A method chain flattened: `a.iter().filter(f).count()` is `a` and three links.
pub(crate) struct Link<'e> {
    pub name: String,
    pub args: Vec<&'e ast::Expr>,
    pub span: ast::Span,
}

impl Cx<'_> {
    pub(crate) fn call(&mut self, c: &ast::ExprCall) -> Ex {
        match &*c.expr {
            ast::Expr::Path(p) => self.call_path(p, c),
            other => {
                let f = self.expr(other);
                let mut args: Vec<String> = c.args.iter().map(|(a, _)| self.expr(a).s).collect();
                self.pass_state(&f.t, &mut args, c.span());
                // `(row.run)()`: the function a field holds, called by name.
                if self.no_hoist == 0 {
                    let ret = match &f.t {
                        Ty::Fn(sig) => self.p.norm(&self.p.sigs[*sig].ret),
                        other => {
                            let t = self.ty_str(other);
                            self.flag(
                                c.span(),
                                "call",
                                format!("calling a value typed `{t}`, not a function"),
                            );
                            Ty::Any
                        }
                    };
                    let base: String =
                        f.s.rsplit(['.', '('])
                            .next()
                            .unwrap_or("f")
                            .chars()
                            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                            .collect();
                    let base = if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit())
                    {
                        "f".to_string()
                    } else {
                        base
                    };
                    let name = self.fresh(&base);
                    let callee = if f.s.starts_with('(') && f.s.ends_with(')') {
                        f.s[1..f.s.len() - 1].to_string()
                    } else {
                        f.s.clone()
                    };
                    self.pre.push(format!("let {name} = {callee};"));
                    return Ex::atom(format!("{name}({})", args.join(", ")), ret);
                }
                self.flag(c.span(), "call", "a call of a computed value");
                Ex::atom(format!("{}({})", f.at(ATOM), args.join(", ")), Ty::Any)
            }
        }
    }

    /// A call through a callback given the editor's state: the caller's `S`
    /// goes first, as the Rune closure read it.
    fn pass_state(&mut self, f: &Ty, args: &mut Vec<String>, span: ast::Span) {
        let Ty::Fn(sig) = f else { return };
        let s = &self.p.sigs[*sig];
        if !s.state || args.len() + 1 != s.params.len() {
            return;
        }
        let here = if let Some(v) = self.lookup("S") {
            Some(v.out.clone())
        } else if self.method {
            Some("self.S".to_string())
        } else {
            None
        };
        match here {
            Some(state) => args.insert(0, state),
            None => self.flag(
                span,
                "state",
                "a callback that wants `S`, called where there is none",
            ),
        }
    }

    fn args(&mut self, c: &ast::ExprCall) -> Vec<Ex> {
        c.args.iter().map(|(a, _)| self.expr(a)).collect()
    }

    fn call_path(&mut self, p: &ast::Path, c: &ast::ExprCall) -> Ex {
        let segs = self.segments(p);
        if segs.len() == 1 {
            match segs[0].as_str() {
                "Some" => {
                    let a = self.args(c);
                    let inner = a.into_iter().next().unwrap_or(Ex::atom("nil", Ty::Nil));
                    return Ex::new(inner.s, Ty::opt(inner.t), inner.prec);
                }
                "Ok" | "Err" => {
                    self.flag(
                        c.span(),
                        "result",
                        format!("`{}(..)`: .bs has no Result", segs[0]),
                    );
                    let a = self.args(c);
                    let inner = a.into_iter().next().unwrap_or(Ex::atom("nil", Ty::Nil));
                    return inner;
                }
                _ => {}
            }
        }
        match self.resolve(p) {
            Named::Fn(sig) => self.call_fn(sig, c),
            Named::Local(v) => {
                // A function value: Rune called a closure held in a local.
                let t = self.local_ty(&v.name);
                let args = self.args(c);
                let ret = match &t {
                    Ty::Fn(s) => self.p.norm(&self.p.sigs[*s].ret),
                    _ => Ty::Any,
                };
                let mut a: Vec<String> = args.into_iter().map(|a| a.s).collect();
                self.pass_state(&t, &mut a, c.span());
                Ex::atom(format!("{}({})", v.out, a.join(", ")), ret)
            }
            Named::Engine(path) => self.call_engine(&path, c),
            Named::This | Named::None | Named::Const { .. } => {
                self.flag(
                    c.span(),
                    "call",
                    "a call of something that is not a function",
                );
                let args: Vec<String> = self.args(c).into_iter().map(|a| a.s).collect();
                Ex::atom(
                    format!("{}({})", self.text(p.span()), args.join(", ")),
                    Ty::Any,
                )
            }
            Named::Unknown(name) => {
                let args: Vec<String> = self.args(c).into_iter().map(|a| a.s).collect();
                if name.contains("::") {
                    self.flag(
                        c.span(),
                        "unknown-fn",
                        format!("`{name}`: that file of the editor has no such function"),
                    );
                } else {
                    self.flag(
                        c.span(),
                        "unknown-fn",
                        format!("`{name}` is not a function the translator knows"),
                    );
                }
                Ex::atom(format!("{name}({})", args.join(", ")), Ty::Any)
            }
        }
    }

    /// A call to one of the ported files' functions.
    pub(crate) fn call_fn(&mut self, sig: usize, c: &ast::ExprCall) -> Ex {
        let (module, out, method, is_async) = {
            let s = &self.p.sigs[sig];
            (s.module, s.out.clone(), s.method, s.is_async)
        };
        let mut args: Vec<&ast::Expr> = c.args.iter().map(|(a, _)| a).collect();
        let mut recv = None;
        if method {
            let first = args.first().map(|a| self.text(a.span()).trim().to_string());
            if module == self.m && self.method && first.as_deref() == Some("this") {
                args.remove(0);
                recv = Some("self".to_string());
            } else if !args.is_empty() {
                let r = self.expr(args.remove(0));
                self.flag(
                    c.span(),
                    "method",
                    format!(
                        "`{out}` is a method of {}'s struct, called on another instance",
                        self.p.modules[module].key
                    ),
                );
                recv = Some(r.at(ATOM));
            }
        }
        let n = self.p.sigs[sig].params.len();
        if args.len() != n {
            self.flag(
                c.span(),
                "arity",
                format!("`{out}` takes {n} arguments, called with {}", args.len()),
            );
        }
        let mut texts = vec![];
        let mut arg_tys = vec![];
        for (i, a) in args.iter().enumerate() {
            let (pname, pty, mutated) = match self.p.sigs[sig].params.get(i) {
                Some(p) => (p.name.clone(), p.ty.clone(), p.mutated),
                None => (String::new(), Ty::Unknown, false),
            };
            let saved = self.hint.take();
            self.hint = Some(crate::project::name_hint(&pname));
            let ex = self.expr(a);
            self.hint = saved;
            arg_tys.push(ex.t.clone());
            if self.rec() && i < n {
                // An `any` argument says nothing of what the parameter is:
                // the call site reads it out as whatever the others pass.
                if ex.t == Ty::Any {
                    if !self.p.sigs[sig].params[i].saw_any {
                        self.p.sigs[sig].params[i].saw_any = true;
                        self.p.changed = true;
                    }
                } else {
                    let at = match &ex.t {
                        Ty::Opt(inner) => (**inner).clone(),
                        t => t.clone(),
                    };
                    let old = self.p.sigs[sig].params[i].ty.clone();
                    let j = self.p.shapes.join(&old, &at);
                    if j != old {
                        self.p.sigs[sig].params[i].ty = j;
                        self.p.changed = true;
                    }
                }
                // And back: a name passed where a struct is wanted is one.
                let want = self.p.norm(&self.p.sigs[sig].params[i].ty);
                // Not numbers: a helper taking floats says nothing of an int.
                if !ex.t.known() && want.known() && !matches!(want, Ty::Any | Ty::Int | Ty::Float) {
                    self.learn_place(a, &want);
                }
                if !ex.t.known()
                    && (want == Ty::Any || self.p.sigs[sig].params[i].saw_any && !want.known())
                {
                    self.flows_into_any(a);
                }
            }
            if mutated {
                self.learn_pass(a);
            }
            let want = self.p.norm(&pty);
            texts.push(self.coerce_at(&ex, &want, a.span()));
        }
        let mut ret = self.p.norm(&self.p.sigs[sig].ret);
        if let Some(leaves) = self.p.sigs[sig].poly.clone() {
            // `util::max(a, b)` on ints is an int, whatever else it is called on.
            let mut t = Ty::Unknown;
            for i in leaves {
                let at = arg_tys.get(i).cloned().unwrap_or(Ty::Unknown);
                let at = match at {
                    Ty::Opt(inner) => *inner,
                    t => t,
                };
                t = self.p.shapes.join(&t, &at);
            }
            if t.known() && t != Ty::Any {
                ret = t;
            }
        }
        let name = match &recv {
            Some(r) => format!("{r}.{out}"),
            None => self.qualified(module, &out),
        };
        let _ = is_async;
        Ex::atom(format!("{name}({})", texts.join(", ")), ret)
    }

    fn call_engine(&mut self, path: &str, c: &ast::ExprCall) -> Ex {
        if !self.p.api.knows(path) {
            self.note(
                c.span(),
                "engine-api",
                format!(
                    "`{path}` is not in docs/generated/api.json; check the engine gives .bs one"
                ),
            );
        }
        if path == "script::attempt" {
            self.flag(
                c.span(),
                "attempt",
                "`script::attempt` takes a closure and catches what it throws: .bs has neither",
            );
        }
        let mut texts = vec![];
        for (i, (a, _)) in c.args.iter().enumerate() {
            self.engine_arg = true;
            let ex = self.expr(a);
            self.engine_arg = false;
            let want = self.p.api.param(path, i);
            if self.rec() && !ex.t.known() {
                if want == Ty::Any {
                    self.flows_into_any(a);
                } else {
                    // The docs type the argument: so is the name passed.
                    self.learn_place(a, &want);
                }
            }
            texts.push(self.coerce(&ex, &want));
        }
        let ret = self.p.api.result(path);
        Ex::atom(format!("{path}({})", texts.join(", ")), ret)
    }

    // Methods.

    /// `e` as a base and the method links on it, innermost first.
    pub(crate) fn chain<'e>(&self, e: &'e ast::Expr) -> (&'e ast::Expr, Vec<Link<'e>>) {
        let mut links = vec![];
        let mut cur = e;
        while let ast::Expr::Call(c) = cur {
            let ast::Expr::FieldAccess(f) = &*c.expr else {
                break;
            };
            let ast::ExprField::Path(p) = &f.expr_field else {
                break;
            };
            let ast::PathSegment::Ident(i) = &p.first else {
                break;
            };
            links.push(Link {
                name: self.ident(i),
                args: c.args.iter().map(|(a, _)| a).collect(),
                span: c.span(),
            });
            cur = &f.expr;
        }
        links.reverse();
        (cur, links)
    }

    pub(crate) fn method_call(&mut self, e: &ast::Expr) -> Ex {
        let (base, links) = self.chain(e);
        if let Some(i) = links
            .iter()
            .position(|l| l.name == "iter" || l.name == "into_iter")
            && links[i + 1..].iter().any(|l| {
                matches!(
                    l.name.as_str(),
                    "any" | "all" | "find" | "filter" | "map" | "count" | "position" | "collect"
                )
            })
        {
            return self.iter_chain(base, &links, i);
        }
        let mut cur = self.expr(base);
        let mut base_e = Some(base);
        for l in &links {
            cur = self.apply_method(base_e, cur, l);
            base_e = None;
        }
        cur
    }

    /// One method on a receiver already written.
    pub(crate) fn apply_method(
        &mut self,
        base_e: Option<&ast::Expr>,
        recv: Ex,
        l: &Link<'_>,
    ) -> Ex {
        let name = l.name.as_str();
        let r = recv.at(ATOM);
        let t = recv.t.clone();
        let mut args: Vec<Ex> = vec![];
        let engine_recv = matches!(t, Ty::Node | Ty::Named(_) | Ty::Any);
        // `S.insert("key", v)` on a struct is `S.key = v`.
        if let (Ty::Shape(id), "insert", [k, v]) = (&t, name, l.args.as_slice())
            && let Some(key) = self.lit_str(k)
        {
            let id = *id;
            let saved = self.hint.replace(crate::project::name_hint(&key));
            let ex = self.expr(v);
            self.hint = saved;
            if self.rec() {
                let vt = match &ex.t {
                    Ty::Opt(inner) => (**inner).clone(),
                    t => t.clone(),
                };
                self.p.shapes.set_field(id, &key, vt);
                self.p.shapes.get_mut(id).accessed.insert(key.clone());
                if let Some(b) = base_e {
                    self.learn_write(b);
                }
            }
            if let (false, Some(b)) = (self.learn, base_e) {
                self.check_write(b);
            }
            let want = self.p.shapes.field(id, &key).unwrap_or(Ty::Unknown);
            let v = self.coerce_at(&ex, &want, l.span);
            return Ex::new(format!("{r}.{key} = {v}"), Ty::Void, 0);
        }
        for a in &l.args {
            if name == "push" || (name == "insert" && matches!(t, Ty::Array(_))) {
                // `rows.push(#{..})` names the literal's struct `Row`.
                let saved = self.hint.take();
                self.hint = base_e.and_then(|b| self.place_hint(b)).map(|(s, n)| {
                    (
                        s,
                        crate::source::pascal(&crate::source::singular(&n.to_lowercase())),
                    )
                });
                self.engine_arg = engine_recv;
                args.push(self.expr(a));
                self.engine_arg = false;
                self.hint = saved;
            } else {
                self.engine_arg = engine_recv;
                let ex = self.expr(a);
                self.engine_arg = false;
                if engine_recv && self.rec() && !ex.t.known() {
                    self.flows_into_any(a);
                }
                args.push(ex);
            }
        }
        let a: Vec<String> = args.iter().map(|a| a.s.clone()).collect();
        let joined = a.join(", ");
        let span = l.span;
        let same = |s: String, ty: Ty| Ex::atom(s, ty);
        // Learn from what is done with an untyped receiver.
        if let (true, false, Some(base_e)) = (self.rec(), t.known(), base_e) {
            match name {
                "push" => {
                    let et = args.first().map_or(Ty::Unknown, |a| a.t.clone());
                    self.learn_place(base_e, &Ty::Array(Box::new(et)));
                }
                "starts_with" | "ends_with" | "trim" | "to_lowercase" | "to_uppercase"
                | "split" | "chars" | "replace" => {
                    self.learn_place(base_e, &Ty::Str);
                }
                _ => {}
            }
        }
        if let (true, Some(base_e)) = (self.rec(), base_e) {
            if let (Ty::Array(_), "push" | "insert") = (&t, name) {
                let et = args.last().map_or(Ty::Unknown, |a| a.t.clone());
                self.learn_place(base_e, &Ty::Array(Box::new(et)));
            }
            if matches!(
                name,
                "push" | "insert" | "remove" | "pop" | "clear" | "sort" | "extend" | "truncate"
            ) {
                self.learn_write(base_e);
            }
        }
        if let (false, Some(base_e)) = (self.learn, base_e)
            && matches!(
                name,
                "push" | "insert" | "remove" | "pop" | "clear" | "sort" | "extend" | "truncate"
            )
        {
            self.check_write(base_e);
        }
        match (name, &t) {
            ("len", _) => same(format!("{r}.len()"), Ty::Int),
            ("is_empty", _) => Ex::new(format!("{r}.len() == 0"), Ty::Bool, 3),
            (
                "iter" | "into_iter" | "clone" | "as_str" | "to_owned" | "copied" | "cloned" | "ok",
                _,
            )
            | ("to_string", Ty::Str)
            | ("collect", Ty::Array(_)) => recv,
            ("to_string", _) => same(format!("\"{{{}}}\"", recv.s), Ty::Str),
            ("push", _) => {
                let want = t.elem();
                let v = args
                    .first()
                    .map(|x| self.coerce(x, &want))
                    .unwrap_or_default();
                if t == Ty::Any {
                    same(format!("{r}.push({v})"), Ty::Bool)
                } else {
                    same(format!("{r}.push({v})"), Ty::Void)
                }
            }
            ("pop", Ty::Array(e)) => same(format!("{r}.pop()"), Ty::opt((**e).clone())),
            ("insert", Ty::Array(_)) => same(format!("{r}.insert({joined})"), Ty::Void),
            ("insert", Ty::Any) => same(format!("{r}.set({joined})"), Ty::Bool),
            ("insert", Ty::Shape(id)) => {
                let id = *id;
                self.dynamic_use(id, span, "keys are inserted");
                same(format!("{r}.set({joined})"), Ty::Void)
            }
            ("remove", Ty::Array(e)) => same(format!("{r}.remove({joined})"), (**e).clone()),
            ("remove", Ty::Shape(id)) => {
                let id = *id;
                self.dynamic_use(id, span, "keys are removed");
                same(format!("{r}.remove({joined})"), Ty::Any)
            }
            ("remove", Ty::Any) => same(format!("{r}.remove({joined})"), Ty::Any),
            ("sort", Ty::Array(_)) => same(format!("{r}.sort()"), Ty::Void),
            ("split_once", Ty::Str) => same(
                format!("{r}.split_once({joined})"),
                Ty::Array(Box::new(Ty::Str)),
            ),
            ("chars", Ty::Str) => same(format!("{r}.chars()"), Ty::Array(Box::new(Ty::Str))),
            ("contains", Ty::Str) => same(format!("{r}.contains({joined})"), Ty::Bool),
            ("contains", _) => {
                self.flag(
                    span,
                    "contains",
                    "`contains` on an array: .bs arrays have none; loop",
                );
                same(format!("{r}.contains({joined})"), Ty::Bool)
            }
            ("contains_key", Ty::Shape(_)) => {
                let key = l.args.first().and_then(|a| self.lit_str(a));
                if let Some(k) = key {
                    self.note(
                        span,
                        "has-field",
                        format!("`contains_key(\"{k}\")` is `true`: a struct has every field"),
                    );
                    same("true".to_string(), Ty::Bool)
                } else {
                    if let Ty::Shape(id) = t {
                        self.dynamic_use(id, span, "keys are tested by value");
                    }
                    same(format!("{r}.has({joined})"), Ty::Bool)
                }
            }
            ("contains_key", _) => same(format!("{r}.has({joined})"), Ty::Bool),
            ("get", Ty::Shape(id)) => {
                let id = *id;
                if let Some(k) = l.args.first().and_then(|a| self.lit_str(a)) {
                    let f = self.field_of(base_e, span, &recv, &k);
                    Ex::new(f.s, Ty::opt(f.t), ATOM)
                } else {
                    self.dynamic_use(id, span, "read under a computed key");
                    same(format!("{r}.get({joined})"), Ty::Any)
                }
            }
            ("get", Ty::Array(e)) => {
                let i = args.first().map(|x| x.s.clone()).unwrap_or_default();
                if self.no_hoist == 0 && is_simple(&r) && is_simple(&i) {
                    return Ex::atom(
                        format!("if {i} >= 0 && {i} < {r}.len() {{ {r}[{i}] }} else {{ nil }}"),
                        Ty::opt((**e).clone()),
                    );
                }
                self.flag(span, "get", "`Vec::get`: index after checking `len`");
                same(format!("{r}[{i}]"), Ty::opt((**e).clone()))
            }
            ("get", _) => {
                let rt = if t.known() { Ty::Any } else { Ty::Unknown };
                if args.first().is_some_and(|a| a.t == Ty::Int) {
                    same(format!("{r}.at({joined})"), rt)
                } else {
                    same(format!("{r}.get({joined})"), rt)
                }
            }
            ("keys", Ty::Shape(id)) => {
                let id = *id;
                self.dynamic_use(id, span, "its keys are listed");
                same(format!("{r}.keys()"), Ty::Array(Box::new(Ty::Str)))
            }
            ("keys", _) => same(format!("{r}.keys()"), Ty::Array(Box::new(Ty::Str))),
            ("values", _) => {
                self.flag(
                    span,
                    "map",
                    "`values()`: `any` has `keys()`; read each with `get`",
                );
                same(format!("{r}.values()"), Ty::Array(Box::new(Ty::Any)))
            }
            ("is_some", _) => Ex::new(Self::nil_check(&recv, true), Ty::Bool, UNARY),
            ("is_none", _) => Ex::new(Self::nil_check(&recv, false), Ty::Bool, ATOM),
            ("unwrap" | "expect", _) => {
                if t == Ty::Any || !t.known() {
                    recv
                } else {
                    same(format!("{r}.{UNWRAP}()"), t.inner())
                }
            }
            ("unwrap_or", _) => {
                let d = args.into_iter().next().unwrap_or(Ex::atom("nil", Ty::Nil));
                if t == Ty::Any || !t.known() {
                    let want = d.t.clone();
                    let v = self.coerce(&Ex::atom(r.clone(), Ty::Any), &want);
                    let rt = if want.known() && want != Ty::Nil {
                        want.clone()
                    } else {
                        Ty::Any
                    };
                    if is_simple(&r) {
                        return Ex::atom(
                            format!("if {r}.is_nil() {{ {} }} else {{ {v} }}", d.s),
                            rt,
                        );
                    }
                    if self.no_hoist == 0 {
                        let tmp = self.fresh("got");
                        self.pre.push(format!("let {tmp} = {};", recv.s));
                        let v = self.coerce(&Ex::atom(tmp.clone(), Ty::Any), &want);
                        return Ex::atom(
                            format!("if {tmp}.is_nil() {{ {} }} else {{ {v} }}", d.s),
                            rt,
                        );
                    }
                    self.flag(
                        span,
                        "unwrap_or",
                        "`unwrap_or` on an `any` in a loop condition",
                    );
                    return same(format!("{r}.unwrap_or({})", d.s), Ty::Any);
                }
                let inner = t.inner();
                let rt = self.p.shapes.join(&inner, &d.t);
                same(format!("{r}.unwrap_or({})", self.coerce(&d, &inner)), rt)
            }
            ("unwrap_or_default", _) => {
                let inner = t.inner();
                same(
                    format!("{r}.unwrap_or({})", self.default_value(&inner, 0)),
                    inner,
                )
            }
            ("to_lowercase", _) => same(format!("{r}.to_lower()"), Ty::Str),
            ("to_uppercase", _) => same(format!("{r}.to_upper()"), Ty::Str),
            ("trim" | "trim_start" | "trim_end", _) => {
                if name != "trim" {
                    self.flag(span, "string", format!("`{name}`: .bs has only `trim`"));
                }
                same(format!("{r}.trim()"), Ty::Str)
            }
            ("starts_with" | "ends_with", _) => same(format!("{r}.{name}({joined})"), Ty::Bool),
            ("replace", _) => same(format!("{r}.replace({joined})"), Ty::Str),
            ("split", _) => {
                // `split(..).collect::<Vec>()` is one `split`.
                same(format!("{r}.split({joined})"), Ty::Array(Box::new(Ty::Str)))
            }
            ("find", Ty::Str) => {
                self.flag(
                    span,
                    "find",
                    "`find` on a string: Rune's is an Option, check what .bs returns for none",
                );
                same(format!("{r}.find({joined})"), Ty::opt(Ty::Int))
            }
            ("parse", _) => {
                let target = self.text(span).to_string();
                let m = if target.contains("f64") {
                    "parse_float"
                } else {
                    "parse_int"
                };
                self.flag(
                    span,
                    "parse",
                    format!("`parse` is a Result in Rune; `{m}` gives 0 for bad text"),
                );
                same(
                    format!("{r}.{m}()"),
                    if m == "parse_float" {
                        Ty::Float
                    } else {
                        Ty::Int
                    },
                )
            }
            ("chars", _) => {
                self.flag(
                    span,
                    "chars",
                    "`chars()`: .bs strings have no char iterator; use `slice`",
                );
                same(format!("{r}.chars()"), Ty::Array(Box::new(Ty::Str)))
            }
            (
                "abs" | "sqrt" | "floor" | "ceil" | "sin" | "cos" | "tan" | "ln" | "exp" | "asin"
                | "acos" | "atan",
                _,
            ) if t.is_num() || !t.known() => same(
                format!("{name}({})", recv.s),
                if name == "abs" && t == Ty::Int {
                    Ty::Int
                } else {
                    Ty::Float
                },
            ),
            ("round" | "trunc" | "fract" | "signum", _) if t.is_num() || !t.known() => {
                self.flag(
                    span,
                    "math",
                    format!("`{name}`: .bs has `floor`/`ceil`, not `{name}`"),
                );
                same(format!("{name}({})", recv.s), Ty::Float)
            }
            ("min" | "max", _) if t.is_num() || !t.known() => {
                let rt = args.first().map_or(t.clone(), |a| {
                    if a.t == Ty::Float {
                        Ty::Float
                    } else {
                        t.clone()
                    }
                });
                same(format!("{name}({}, {joined})", recv.s), rt)
            }
            ("powf" | "powi" | "pow", _) => same(format!("pow({}, {joined})", recv.s), Ty::Float),
            ("atan2", _) => same(format!("atan2({}, {joined})", recv.s), Ty::Float),
            ("clamp", _) if t.is_num() || !t.known() => same(
                format!(
                    "min(max({}, {}), {})",
                    recv.s,
                    a.first().cloned().unwrap_or_default(),
                    a.get(1).cloned().unwrap_or_default()
                ),
                t,
            ),
            (
                "sort" | "sort_by" | "sort_by_key" | "dedup" | "reverse" | "extend" | "truncate"
                | "clear" | "join" | "retain",
                _,
            ) => {
                self.flag(
                    span,
                    "array",
                    format!("`{name}`: .bs arrays have push, pop, insert, remove and len"),
                );
                same(
                    format!("{r}.{name}({joined})"),
                    if name == "join" { Ty::Str } else { Ty::Void },
                )
            }
            (
                "enumerate" | "rev" | "skip" | "take" | "zip" | "sum" | "last" | "first" | "next",
                _,
            ) => {
                self.flag(
                    span,
                    "iter",
                    format!("`{name}()`: .bs has no iterators; write the loop"),
                );
                same(format!("{r}.{name}({joined})"), Ty::Any)
            }
            ("await", _) => Ex::new(format!("await {}", recv.at(UNARY)), t, UNARY),
            (_, Ty::Node) => {
                let rt = Api::node_op(name).unwrap_or(Ty::Any);
                same(format!("{r}.{name}({joined})"), rt)
            }
            (_, Ty::Named(_) | Ty::Any) => same(format!("{r}.{name}({joined})"), Ty::Any),
            _ => {
                if !t.known() {
                    // Nothing known of the receiver yet, or ever: say so once written.
                    if !self.learn {
                        self.flag(
                            span,
                            "method",
                            format!("`.{name}()` on a value of no inferred type"),
                        );
                    }
                    return same(format!("{r}.{name}({joined})"), Ty::Unknown);
                }
                if !self.learn {
                    let tn = self.ty_str(&t);
                    self.flag(
                        span,
                        "method",
                        format!("`.{name}()` on a `{tn}`: no .bs method of that name is known"),
                    );
                }
                same(format!("{r}.{name}({joined})"), Ty::Any)
            }
        }
    }
}

fn is_simple(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}
