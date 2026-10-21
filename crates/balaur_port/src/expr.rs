// Expressions: everything but calls (call.rs), blocks and control flow
// (stmt.rs) and closures (closure.rs).

use crate::cx::{ATOM, Cx, Ex, Named, UNARY, UNWRAP};
use crate::source::{bs_string_body, singular};
use crate::ty::Ty;
use rune::ast;
use rune::ast::Spanned;

pub(crate) fn indent(s: &str) -> String {
    s.lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("    {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The helper a bit operator becomes, which the reader writes.
fn bit_helper(op: &ast::BinOp) -> Option<&'static str> {
    use ast::BinOp as B;
    match op {
        B::Shl(_) => Some("shl"),
        B::Shr(_) => Some("shr"),
        B::BitAnd(_) => Some("bit_and"),
        B::BitOr(_) => Some("bit_or"),
        B::BitXor(_) => Some("bit_xor"),
        _ => None,
    }
}

/// What a binary operator gives on these operands.
fn op_ty(op: &ast::BinOp, l: &Ty, r: &Ty) -> Ty {
    use ast::BinOp as B;
    if !matches!(
        op,
        B::Add(_) | B::Sub(_) | B::Mul(_) | B::Div(_) | B::Rem(_)
    ) {
        return Ty::Bool;
    }
    if matches!(op, B::Add(_)) && (*l == Ty::Str || *r == Ty::Str) {
        Ty::Str
    } else if l.is_num() && r.is_num() {
        if *l == Ty::Float || *r == Ty::Float {
            Ty::Float
        } else {
            Ty::Int
        }
    } else if l.is_num() && !r.known() {
        l.clone()
    } else if r.is_num() && !l.known() {
        r.clone()
    } else if *l == Ty::Any || *r == Ty::Any {
        Ty::Any
    } else if !l.known() && !r.known() {
        Ty::Unknown
    } else {
        l.clone()
    }
}

fn bin_prec(op: &ast::BinOp) -> u8 {
    use ast::BinOp as B;
    match op {
        B::Or(_) => 1,
        B::And(_) => 2,
        B::Eq(_)
        | B::Neq(_)
        | B::Lt(_)
        | B::Gt(_)
        | B::Lte(_)
        | B::Gte(_)
        | B::Is(_)
        | B::IsNot(_) => 3,
        B::Add(_) | B::Sub(_) | B::BitOr(_) | B::BitXor(_) => 4,
        B::Mul(_) | B::Div(_) | B::Rem(_) | B::BitAnd(_) | B::Shl(_) | B::Shr(_) => 5,
        _ => 0,
    }
}

impl Cx<'_> {
    pub(crate) fn rec(&self) -> bool {
        self.learn && self.p.peeking == 0
    }

    pub(crate) fn expr(&mut self, e: &ast::Expr) -> Ex {
        use ast::Expr as E;
        match e {
            E::Path(p) => self.path_expr(p),
            E::Lit(l) => self.lit(&l.lit),
            E::Group(g) => {
                let inner = self.expr(&g.expr);
                Ex::atom(format!("({})", inner.s), inner.t)
            }
            E::Empty(g) => self.expr(&g.expr),
            E::Binary(b) => self.binary(b),
            E::Unary(u) => self.unary(u),
            E::FieldAccess(f) => self.field(f),
            E::Index(ix) => self.index(ix),
            E::Call(c) => match &*c.expr {
                E::FieldAccess(_) => self.method_call(e),
                _ => self.call(c),
            },
            E::MacroCall(mc) => self.macro_call(mc),
            E::Object(o) => self.object(o),
            E::Vec(v) => self.vec(v),
            E::Tuple(t) => self.tuple(t),
            E::Closure(c) => self.closure(c),
            E::If(i) => self.if_expr(i),
            E::Match(m) => self.match_expr(m),
            E::Block(b) => self.block_expr(b),
            E::While(w) => self.while_expr(w),
            E::Loop(l) => self.loop_expr(l),
            E::For(f) => self.for_expr(f),
            E::Assign(a) => self.assign(a),
            E::Return(r) => self.return_expr(r),
            E::Break(b) => {
                if b.label.is_some() {
                    self.flag(b.span(), "label", "a labelled `break`: .bs has no labels");
                }
                if let Some(v) = &b.expr {
                    let v = self.expr(v);
                    self.flag(
                        b.span(),
                        "break-value",
                        format!("`break {}`: a .bs `loop` has no value", v.s),
                    );
                    return Ex::new("break", Ty::Void, 0);
                }
                Ex::new("break", Ty::Void, 0)
            }
            E::Continue(c) => {
                if c.label.is_some() {
                    self.flag(
                        c.span(),
                        "label",
                        "a labelled `continue`: .bs has no labels",
                    );
                }
                Ex::new("continue", Ty::Void, 0)
            }
            E::Await(a) => {
                let inner = self.expr(&a.expr);
                Ex::new(format!("await {}", inner.at(UNARY)), inner.t, UNARY)
            }
            E::Try(t) => {
                let inner = self.expr(&t.expr);
                self.flag(
                    t.span(),
                    "try",
                    "`?`: .bs has no error values; check the result by hand",
                );
                Ex::atom(inner.s, inner.t.inner())
            }
            E::Let(l) => {
                self.flag(l.span(), "let-expr", "a `let` used as a value");
                let v = self.expr(&l.expr);
                Ex::atom(v.s, Ty::Bool)
            }
            E::Range(r) => {
                self.flag(r.span(), "range", "a range outside a `for`: .bs has none");
                let from = r.start.as_ref().map(|s| self.expr(s).s).unwrap_or_default();
                let to = r.end.as_ref().map(|s| self.expr(s).s).unwrap_or_default();
                Ex::new(format!("{from}..{to}"), Ty::Any, 0)
            }
            E::Select(s) => {
                let first = self.text(s.span()).lines().next().unwrap_or("").to_string();
                self.flag(
                    s.span(),
                    "select",
                    format!("`{first}`: .bs has no select; await one task at a time"),
                );
                Ex::atom("nil", Ty::Any)
            }
            E::Yield(y) => {
                self.flag(y.span(), "yield", "`yield`: .bs has no generators");
                Ex::atom("nil", Ty::Any)
            }
            _ => {
                let first = self.text(e.span()).lines().next().unwrap_or("").to_string();
                self.flag(
                    e.span(),
                    "unknown",
                    format!("`{first}`: an expression the translator does not know"),
                );
                Ex::atom("nil", Ty::Any)
            }
        }
    }

    // Names.

    pub(crate) fn segments(&self, p: &ast::Path) -> Vec<String> {
        let mut out = vec![];
        let push = |seg: &ast::PathSegment, out: &mut Vec<String>| match seg {
            ast::PathSegment::Ident(i) => out.push(self.ident(i)),
            ast::PathSegment::Crate(_) => out.push("crate".into()),
            ast::PathSegment::SelfValue(_) => out.push("self".into()),
            ast::PathSegment::Super(_) => out.push("super".into()),
            ast::PathSegment::SelfType(_) => out.push("Self".into()),
            _ => {}
        };
        push(&p.first, &mut out);
        for (_, s) in &p.rest {
            push(s, &mut out);
        }
        out
    }

    pub(crate) fn resolve(&mut self, p: &ast::Path) -> Named {
        let segs = self.segments(p);
        if segs.len() == 1 {
            let name = &segs[0];
            if name == "None" {
                return Named::None;
            }
            if name == "this" && self.method {
                return Named::This;
            }
            if let Some(v) = self.lookup(name) {
                return Named::Local(v.clone());
            }
            if let Some(s) = self.p.modules[self.m].fns.get(name) {
                return Named::Fn(*s);
            }
            if self.p.modules[self.m].consts.contains_key(name) {
                return Named::Const {
                    module: self.m,
                    name: name.clone(),
                };
            }
            if let Some(full) = self.p.modules[self.m].uses.get(name).cloned() {
                // `use crate::a::f;` brought a function or const in.
                let segs: Vec<String> = full.split("::").map(str::to_string).collect();
                if let Some((_, s)) = self.p.resolve_fn(self.m, &segs) {
                    return Named::Fn(s);
                }
            }
            return Named::Unknown(name.clone());
        }
        if segs[0] == "super" {
            return Named::Unknown(segs.join("::"));
        }
        if let Some((_, s)) = self.p.resolve_fn(self.m, &segs) {
            return Named::Fn(s);
        }
        if let Some(m) = self.p.resolve_const(self.m, &segs) {
            return Named::Const {
                module: m,
                name: segs.last().cloned().unwrap_or_default(),
            };
        }
        let explicit = segs[0] == "crate"
            || segs[0] == "self"
            || self
                .p
                .resolve_module(self.m, &segs[..segs.len() - 1])
                .is_some();
        if explicit {
            // A file of the editor named, with nothing of that name in it.
            return Named::Unknown(segs.join("::"));
        }
        Named::Engine(segs.join("::"))
    }

    /// `module::name` for something module `target` declares.
    pub(crate) fn qualified(&mut self, target: usize, name: &str) -> String {
        if target == self.m {
            return name.to_string();
        }
        let key = self.p.modules[target].key.clone();
        let alias = self.p.modules[target].alias().to_string();
        self.p.modules[self.m].needs.insert(key);
        format!("{alias}::{name}")
    }

    fn path_expr(&mut self, p: &ast::Path) -> Ex {
        match self.resolve(p) {
            Named::Local(v) => {
                let t = self.local_ty(&v.name);
                Ex::atom(v.out, t)
            }
            Named::This => {
                let t = self.local_ty("this");
                Ex::atom("self", t)
            }
            Named::None => Ex::atom("nil", Ty::Nil),
            Named::Fn(sig) => {
                let s = &self.p.sigs[sig];
                let (module, out, method) = (s.module, s.out.clone(), s.method);
                if method {
                    self.flag(
                        p.span(),
                        "fn-ref",
                        format!("a reference to method `{out}`: a fn value names a plain function"),
                    );
                }
                let text = self.qualified(module, &out);
                Ex::atom(text, Ty::Fn(sig))
            }
            Named::Const { module, name } => {
                let c = &self.p.modules[module].consts[&name];
                let (t, literal) = (self.p.norm(&c.ty), c.literal);
                let mut text = self.qualified(module, &name);
                if !literal {
                    text.push_str("()");
                }
                Ex::atom(text, t)
            }
            Named::Engine(path) => {
                self.note(
                    p.span(),
                    "engine-const",
                    format!("`{path}` is the engine's"),
                );
                Ex::atom(path, Ty::Any)
            }
            Named::Unknown(name) => {
                self.flag(
                    p.span(),
                    "unknown-name",
                    format!(
                        "`{name}` is neither a local, a function of the editor nor the engine's"
                    ),
                );
                Ex::atom(name, Ty::Any)
            }
        }
    }

    // Literals.

    pub(crate) fn lit(&mut self, l: &ast::Lit) -> Ex {
        match l {
            ast::Lit::Bool(b) => Ex::atom(self.text(b.span).to_string(), Ty::Bool),
            ast::Lit::Number(n) => {
                let raw = self.text(n.span).replace('_', "");
                let lower = raw.to_ascii_lowercase();
                let (digits, float) = if let Some(hex) = lower.strip_prefix("0x") {
                    (
                        i64::from_str_radix(hex, 16).map_or(raw.clone(), |v| v.to_string()),
                        false,
                    )
                } else if let Some(bin) = lower.strip_prefix("0b") {
                    (
                        i64::from_str_radix(bin, 2).map_or(raw.clone(), |v| v.to_string()),
                        false,
                    )
                } else {
                    let trimmed = lower
                        .trim_end_matches("f64")
                        .trim_end_matches("i64")
                        .trim_end_matches("u8")
                        .to_string();
                    let float =
                        trimmed.contains('.') || trimmed.contains('e') || lower.ends_with("f64");
                    let mut t = trimmed;
                    if float && !t.contains('.') && !t.contains('e') {
                        t.push_str(".0");
                    }
                    (t, float)
                };
                Ex::atom(digits, if float { Ty::Float } else { Ty::Int })
            }
            ast::Lit::Str(s) => {
                let raw = self.text(s.span).to_string();
                let (body, wrapped) = match raw.strip_prefix('"').and_then(|r| r.strip_suffix('"'))
                {
                    Some(b) => (b.to_string(), true),
                    None => (raw.clone(), false),
                };
                let (out, problem) = bs_string_body(&body, !wrapped);
                if let Some(problem) = problem {
                    self.flag(s.span, "string", problem);
                }
                Ex::atom(format!("\"{out}\""), Ty::Str)
            }
            ast::Lit::Char(c) => {
                let raw = self.text(c.span).to_string();
                let body = raw.trim_start_matches('\'').trim_end_matches('\'');
                let body = if body == "\"" {
                    "\\\"".to_string()
                } else {
                    body.replace("\\'", "'")
                };
                let (out, problem) = bs_string_body(&body, false);
                if let Some(problem) = problem {
                    self.flag(c.span, "string", problem);
                }
                Ex::atom(format!("\"{out}\""), Ty::Str)
            }
            ast::Lit::Byte(b) => {
                self.flag(b.span, "byte", "a byte literal: .bs has no bytes");
                Ex::atom("0", Ty::Int)
            }
            ast::Lit::ByteStr(b) => {
                self.flag(b.span, "byte", "a byte string: .bs has no bytes");
                Ex::atom("\"\"", Ty::Str)
            }
            other => {
                self.flag(
                    other.span(),
                    "literal",
                    "a literal the translator does not know",
                );
                Ex::atom("nil", Ty::Any)
            }
        }
    }

    // Operators.

    fn binary(&mut self, b: &ast::ExprBinary) -> Ex {
        use ast::BinOp as B;
        match &b.op {
            B::As(_) => return self.cast(b),
            B::Is(_) | B::IsNot(_) => return self.is_test(b, matches!(b.op, B::IsNot(_))),
            B::AddAssign(_)
            | B::SubAssign(_)
            | B::MulAssign(_)
            | B::DivAssign(_)
            | B::RemAssign(_) => {
                return self.compound(b);
            }
            B::BitAndAssign(_)
            | B::BitXorAssign(_)
            | B::BitOrAssign(_)
            | B::ShlAssign(_)
            | B::ShrAssign(_) => {
                return self.bit_assign(b);
            }
            B::DotDot(_) | B::DotDotEq(_) => {
                self.flag(b.span(), "range", "a range outside a `for`");
            }
            _ => {}
        }
        let prec = bin_prec(&b.op);
        let mut l = self.expr(&b.lhs);
        let mut r = self.expr(&b.rhs);
        self.coerce_operands(&b.op, &mut l, &mut r);
        let op = self.text(b.op.span()).to_string();
        // An untyped name next to a typed operand takes its type.
        if self.rec() && prec >= 3 && !matches!(b.op, B::And(_) | B::Or(_)) {
            let (lt, rt) = (l.t.clone(), r.t.clone());
            if !lt.known() && matches!(rt, Ty::Int | Ty::Float | Ty::Str | Ty::Bool) {
                self.learn_place(&b.lhs, &rt);
            }
            if !rt.known() && matches!(lt, Ty::Int | Ty::Float | Ty::Str | Ty::Bool) {
                self.learn_place(&b.rhs, &lt);
            }
        }
        // `x == nil` on an optional reads better as the check.
        if matches!(b.op, B::Eq(_) | B::Neq(_)) && (r.s == "nil" || l.s == "nil") {
            let val = if r.s == "nil" { &l } else { &r };
            let neg = matches!(b.op, B::Neq(_));
            let s = Self::nil_check(val, neg);
            return Ex::new(s, Ty::Bool, if neg { UNARY } else { ATOM });
        }
        if let Some(helper) = bit_helper(&b.op) {
            // A call to a helper keeps the file parsing.
            self.flag(
                b.span(),
                "bits",
                format!("`{op}`: .bs has no bit operators; written as a call to a helper to write"),
            );
            return Ex::atom(format!("{helper}({}, {})", l.s, r.s), Ty::Int);
        }
        let t = op_ty(&b.op, &l.t, &r.t);
        let lhs = l.at(prec);
        let rhs = r.at(prec + 1);
        // Comparisons do not chain in .bs either; parenthesise one inside another.
        let lhs = if prec == 3 && l.prec == 3 {
            format!("({})", l.s)
        } else {
            lhs
        };
        Ex::new(format!("{lhs} {op} {rhs}"), t, prec)
    }

    /// `a &= b` and the rest, as a call to a helper the reader writes.
    fn bit_assign(&mut self, b: &ast::ExprBinary) -> Ex {
        self.flag(b.span(), "bits", "a bitwise assignment: .bs has no bit operators; written as a call to a helper to write");
        let l = self.expr(&b.lhs);
        let r = self.expr(&b.rhs);
        let helper = match self.text(b.op.span()) {
            "&=" => "bit_and",
            "|=" => "bit_or",
            "^=" => "bit_xor",
            "<<=" => "shl",
            _ => "shr",
        };
        Ex::new(format!("{} = {helper}({}, {})", l.s, l.s, r.s), Ty::Void, 0)
    }

    /// An `any` next to a typed operand is read out as that type.
    fn coerce_operands(&self, op: &ast::BinOp, l: &mut Ex, r: &mut Ex) {
        use ast::BinOp as B;
        if matches!(op, B::And(_) | B::Or(_)) || l.s == "nil" || r.s == "nil" {
            return;
        }
        let prim = |t: &Ty| matches!(t, Ty::Int | Ty::Float | Ty::Str | Ty::Bool);
        let arith = matches!(op, B::Div(_) | B::Mul(_) | B::Sub(_) | B::Add(_));
        // An int beside an `any` in arithmetic may be a float's partner.
        let want = |t: &Ty| {
            if arith && *t == Ty::Int {
                Ty::Float
            } else {
                t.clone()
            }
        };
        if l.t == Ty::Any && prim(&r.t) {
            let w = want(&r.t);
            *l = Ex::atom(self.coerce(l, &w), w);
        } else if r.t == Ty::Any && prim(&l.t) {
            let w = want(&l.t);
            *r = Ex::atom(self.coerce(r, &w), w);
        }
    }

    /// `x.is_none()` or its opposite, for whatever `x` is.
    pub(crate) fn nil_check(val: &Ex, negate: bool) -> String {
        let base = val.at(ATOM);
        if val.t == Ty::Any || !val.t.known() {
            if negate {
                format!("!{base}.is_nil()")
            } else {
                format!("{base}.is_nil()")
            }
        } else if negate {
            format!("{base}.is_some()")
        } else {
            format!("{base}.is_none()")
        }
    }

    fn cast(&mut self, b: &ast::ExprBinary) -> Ex {
        let l = self.expr(&b.lhs);
        let target = self.text(b.rhs.span()).trim().to_string();
        let (name, t) = match target.as_str() {
            "i64" | "i32" | "u64" | "u32" | "usize" | "u8" => ("int", Ty::Int),
            "f64" | "f32" => ("float", Ty::Float),
            "bool" => ("bool", Ty::Bool),
            _ => {
                self.flag(
                    b.span(),
                    "cast",
                    format!("`as {target}`: .bs casts only between numbers"),
                );
                ("any", Ty::Any)
            }
        };
        if l.t == Ty::Any {
            let m = match t {
                Ty::Int => "as_int",
                Ty::Float => "as_float",
                _ => "as_bool",
            };
            return Ex::atom(format!("{}.{m}()", l.at(ATOM)), t);
        }
        if l.t == t {
            return l;
        }
        Ex::new(format!("{} as {name}", l.at(ATOM)), t, UNARY)
    }

    /// `x is Tuple` and the rest: Rune's runtime type tests.
    fn is_test(&mut self, b: &ast::ExprBinary, negate: bool) -> Ex {
        let v = self.expr(&b.lhs);
        let target = self.text(b.rhs.span()).trim().to_string();
        if target == "Tuple" || target == "unit" {
            return Ex::new(Self::nil_check(&v, negate), Ty::Bool, UNARY);
        }
        let kind = match target.as_str() {
            "balaur::Node" | "Node" => "node",
            "Object" | "HashMap" => "map",
            "Vec" => "list",
            "String" => "str",
            "i64" => "int",
            "f64" => "float",
            "bool" => "bool",
            _ => "",
        };
        // An option of the very kind tested: the test is whether it holds one.
        if let Ty::Opt(inner) = &v.t {
            let same = matches!(
                (&**inner, kind),
                (Ty::Shape(_), "map")
                    | (Ty::Array(_), "list")
                    | (Ty::Str, "str")
                    | (Ty::Int, "int")
                    | (Ty::Float, "float")
                    | (Ty::Bool, "bool")
            );
            if same {
                return Ex::new(Self::nil_check(&v, !negate), Ty::Bool, UNARY);
            }
        }
        let static_ok = match (&v.t, kind) {
            (Ty::Int, "int")
            | (Ty::Float, "float")
            | (Ty::Str, "str")
            | (Ty::Bool, "bool")
            | (Ty::Array(_), "list")
            | (Ty::Node, "node")
            | (Ty::Shape(_), "map") => Some(true),
            (
                Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Array(_) | Ty::Shape(_) | Ty::Node,
                _,
            ) if !kind.is_empty() => Some(false),
            _ => None,
        };
        if let Some(ok) = static_ok {
            let res = ok != negate;
            let msg = format!(
                "`is {target}` on a value typed `{}`: always {res} here",
                self.ty_str(&v.t)
            );
            if ok {
                // The type says so; the test was Rune checking what .bs knows.
                self.note(b.span(), "type-test", msg);
            } else {
                // The type says never: Rune saw other kinds here, or the guess is off.
                self.flag(b.span(), "type-test", msg);
            }
            return Ex::atom(format!("{res}"), Ty::Bool);
        }
        if kind.is_empty() {
            self.flag(
                b.span(),
                "type-test",
                format!("`is {target}`: no .bs kind for it"),
            );
            return Ex::atom("false", Ty::Bool);
        }
        let op = if negate { "!=" } else { "==" };
        Ex::new(
            format!("{}.kind() {op} \"{kind}\"", v.at(ATOM)),
            Ty::Bool,
            3,
        )
    }

    fn compound(&mut self, b: &ast::ExprBinary) -> Ex {
        let r = self.expr(&b.rhs);
        let l = self.expr(&b.lhs);
        let op = self.text(b.op.span()).to_string();
        if self.rec() {
            let t = if op == "+=" && (r.t == Ty::Str || l.t == Ty::Str) {
                Ty::Str
            } else {
                r.t.clone()
            };
            self.learn_place(&b.lhs, &t);
            self.learn_write(&b.lhs);
        }
        self.check_write(&b.lhs);
        let rhs = self.coerce(&r, &l.t);
        Ex::new(format!("{} {op} {rhs}", l.s), Ty::Void, 0)
    }

    fn unary(&mut self, u: &ast::ExprUnary) -> Ex {
        let inner = self.expr(&u.expr);
        match &u.op {
            ast::UnOp::Not(_) => {
                let bare = inner
                    .s
                    .strip_prefix('(')
                    .and_then(|s| s.strip_suffix(')'))
                    .filter(|s| !s.contains('(') || s.ends_with("()"))
                    .unwrap_or(&inner.s);
                let simple = |s: &str| {
                    s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                };
                if let Some(x) = bare.strip_suffix(".is_some()").filter(|x| simple(x)) {
                    return Ex::atom(format!("{x}.is_none()"), Ty::Bool);
                }
                if let Some(x) = bare.strip_suffix(".is_none()").filter(|x| simple(x)) {
                    return Ex::atom(format!("{x}.is_some()"), Ty::Bool);
                }
                if inner.t == Ty::Any {
                    return Ex::new(format!("!{}.as_bool()", inner.at(ATOM)), Ty::Bool, UNARY);
                }
                Ex::new(
                    format!("!{}", inner.at(UNARY)),
                    if inner.t.known() { inner.t } else { Ty::Bool },
                    UNARY,
                )
            }
            ast::UnOp::Neg(_) => Ex::new(format!("-{}", inner.at(UNARY)), inner.t, UNARY),
            ast::UnOp::BorrowRef(_) | ast::UnOp::Deref(_) => {
                self.flag(u.span(), "ref", "`&`/`*`: .bs has no references");
                inner
            }
        }
    }

    // Places.

    fn field(&mut self, f: &ast::ExprFieldAccess) -> Ex {
        let Some(name) = self.field_name(&f.expr_field) else {
            self.flag(f.span(), "field", "a field the translator cannot name");
            let base = self.expr(&f.expr);
            return Ex::atom(
                format!("{}.{}", base.at(ATOM), self.text(f.expr_field.span())),
                Ty::Any,
            );
        };
        let base = self.expr(&f.expr);
        self.field_of(Some(&f.expr), f.span(), &base, &name)
    }

    pub(crate) fn field_of(
        &mut self,
        base_e: Option<&ast::Expr>,
        span: ast::Span,
        base: &Ex,
        name: &str,
    ) -> Ex {
        let b = base.at(ATOM);
        match &base.t {
            Ty::Shape(id) => {
                let id = *id;
                if self.rec() && self.p.shapes.field(id, name).is_none() {
                    self.p.shapes.set_field(id, name, Ty::Unknown);
                }
                if self.rec() && !self.p.shapes.get(id).accessed.contains(name) {
                    self.p.shapes.get_mut(id).accessed.insert(name.to_string());
                    self.p.shapes.changed = true;
                }
                let t = self.p.shapes.field(id, name).unwrap_or(Ty::Unknown);
                Ex::atom(format!("{b}.{name}"), self.p.norm(&t))
            }
            Ty::Unknown => {
                if let (true, Some(base_e)) = (self.rec(), base_e)
                    && let Some(id) = self.implicit_shape(base_e)
                {
                    self.p.shapes.set_field(id, name, Ty::Unknown);
                }
                Ex::atom(format!("{b}.{name}"), Ty::Unknown)
            }
            Ty::Named("Vec3" | "Vec2" | "Color") => Ex::atom(format!("{b}.{name}"), Ty::Float),
            Ty::Node => Ex::atom(format!("{b}.{name}"), Ty::Named("Component")),
            Ty::Named(_) => Ex::atom(format!("{b}.{name}"), Ty::Any),
            Ty::Opt(inner) if matches!(**inner, Ty::Shape(_)) => {
                let Ty::Shape(id) = **inner else {
                    unreachable!()
                };
                self.flag(
                    span,
                    "optional",
                    format!(
                        "`.{name}` on an optional `{}`: check it holds one first",
                        self.ty_str(inner)
                    ),
                );
                let t = self.p.shapes.field(id, name).unwrap_or(Ty::Unknown);
                Ex::atom(format!("{b}.{UNWRAP}().{name}"), self.p.norm(&t))
            }
            Ty::Any | Ty::Opt(_) | Ty::Nil => {
                if base.t != Ty::Any {
                    self.flag(
                        span,
                        "optional",
                        format!("`.{name}` on an optional: unwrap it first"),
                    );
                }
                if name.starts_with('_') && name[1..].chars().all(|c| c.is_ascii_digit()) {
                    return Ex::atom(format!("{b}.at({})", &name[1..]), Ty::Any);
                }
                Ex::atom(format!("{b}.get(\"{name}\")"), Ty::Any)
            }
            other => {
                self.flag(
                    span,
                    "field",
                    format!("`.{name}` on a `{}`", self.ty_str(other)),
                );
                Ex::atom(format!("{b}.{name}"), Ty::Any)
            }
        }
    }

    fn index(&mut self, ix: &ast::ExprIndex) -> Ex {
        let base = self.expr(&ix.target);
        // `s[a..b]`: a string's `slice`.
        if let ast::Expr::Range(r) = &*ix.index {
            let from = r.start.as_ref().map(|s| self.expr(s));
            let to = r.end.as_ref().map(|s| self.expr(s));
            let b = base.at(ATOM);
            let b = if base.t == Ty::Any {
                format!("{b}.as_str()")
            } else {
                b
            };
            let from = from.map_or_else(|| "0".to_string(), |f| self.coerce(&f, &Ty::Int));
            let to = match to {
                Some(t) => {
                    let s = self.coerce(&t, &Ty::Int);
                    if matches!(r.limits, ast::ExprRangeLimits::Closed(_)) {
                        format!("{s} + 1")
                    } else {
                        s
                    }
                }
                None => format!("{b}.len()"),
            };
            match &base.t {
                Ty::Array(_) => {
                    self.flag(
                        ix.span(),
                        "array-slice",
                        "an array slice: .bs arrays have none; copy the range in a loop",
                    );
                    return Ex::atom(format!("{b}.slice({from}, {to})"), base.t.clone());
                }
                Ty::Unknown if self.rec() => {
                    self.learn_place(&ix.target, &Ty::Str);
                }
                _ => {}
            }
            return Ex::atom(format!("{b}.slice({from}, {to})"), Ty::Str);
        }
        if let Ty::Shape(id) = base.t {
            if let Some(k) = self.lit_str(&ix.index) {
                return self.field_of(Some(&ix.target), ix.span(), &base, &k);
            }
            self.dynamic_use(id, ix.span(), "read under a computed key");
        }
        let i = self.expr(&ix.index);
        match &base.t {
            Ty::Array(t) => {
                let t = (**t).clone();
                if self.rec() && !i.t.known() {
                    self.learn_place(&ix.index, &Ty::Int);
                }
                Ex::atom(
                    format!("{}[{}]", base.at(ATOM), self.coerce(&i, &Ty::Int)),
                    t,
                )
            }
            Ty::Str => {
                self.flag(ix.span(), "string-index", "indexing a string: use `slice`");
                Ex::atom(
                    format!("{}.slice({}, {} + 1)", base.at(ATOM), i.s, i.at(5)),
                    Ty::Str,
                )
            }
            Ty::Unknown if i.t == Ty::Int => {
                if self.rec() {
                    self.learn_place(&ix.target, &Ty::Array(Box::new(Ty::Unknown)));
                }
                Ex::atom(format!("{}[{}]", base.at(ATOM), i.s), Ty::Unknown)
            }
            _ => {
                let rt = if base.t.known() { Ty::Any } else { Ty::Unknown };
                if i.t == Ty::Str {
                    Ex::atom(format!("{}.get({})", base.at(ATOM), i.s), rt)
                } else if i.t == Ty::Int {
                    Ex::atom(format!("{}.at({})", base.at(ATOM), i.s), rt)
                } else {
                    Ex::atom(format!("{}[{}]", base.at(ATOM), i.s), rt)
                }
            }
        }
    }

    fn assign(&mut self, a: &ast::ExprAssign) -> Ex {
        // The written place names the literal's struct: `S.history = #{..}`.
        let saved = self.hint.take();
        self.hint = self.place_hint(&a.lhs);
        let r = self.expr(&a.rhs);
        self.hint = saved;
        if self.rec() {
            self.learn_place(&a.lhs, &r.t);
            self.learn_write(&a.lhs);
        }
        self.check_write(&a.lhs);
        let l = self.expr(&a.lhs);
        // `S.sel = i`: an untyped name written to a typed place is of its type.
        if self.rec() && !r.t.known() && l.t.known() && l.t != Ty::Any {
            self.learn_place(&a.rhs, &l.t);
        }
        if let ast::Expr::Index(ix) = &*a.lhs
            && !matches!(self.peek_ty(&ix.target), Ty::Array(_) | Ty::Shape(_))
        {
            // An `any` map or list: written with `set`.
            let base = self.expr(&ix.target);
            let k = self.expr(&ix.index);
            if base.t == Ty::Any || !base.t.known() {
                let m = if k.t == Ty::Int { "set_at" } else { "set" };
                if m == "set_at" {
                    self.flag(
                        a.span(),
                        "any-write",
                        "writing an `any` list by index: `any` has no `set_at`",
                    );
                }
                return Ex::new(
                    format!("{}.{m}({}, {})", base.at(ATOM), k.s, r.s),
                    Ty::Void,
                    0,
                );
            }
        }
        if l.s.ends_with("\")") && l.s.contains(".get(\"") {
            // `x.f = v` on an `any`: `set`.
            let (recv, key) = l.s.rsplit_once(".get(").unwrap_or((&l.s, ""));
            return Ex::new(
                format!("{recv}.set({}, {})", key.trim_end_matches(')'), r.s),
                Ty::Void,
                0,
            );
        }
        let rhs = self.coerce_at(&r, &l.t, a.span());
        Ex::new(format!("{} = {rhs}", l.s), Ty::Void, 0)
    }

    /// A struct name from the place a literal is written to.
    pub(crate) fn place_hint(&self, e: &ast::Expr) -> Option<(u8, String)> {
        match e {
            ast::Expr::FieldAccess(f) => self.field_name(&f.expr_field).map(|n| {
                let h = crate::project::name_hint(&n);
                (h.0, h.1)
            }),
            ast::Expr::Path(p) if p.rest.is_empty() => {
                Some(crate::project::name_hint(self.text(p.span()).trim()))
            }
            ast::Expr::Index(ix) => self.place_hint(&ix.target).map(|(s, n)| (s, singular(&n))),
            _ => None,
        }
    }

    /// The base name a write goes through, and whether it is a parameter.
    fn write_root(&self, e: &ast::Expr) -> Option<String> {
        match e {
            ast::Expr::FieldAccess(f) => self.write_root(&f.expr).or(None),
            ast::Expr::Index(ix) => self.write_root(&ix.target),
            ast::Expr::Path(p) if p.rest.is_empty() => Some(self.text(p.span()).trim().to_string()),
            ast::Expr::Group(g) => self.write_root(&g.expr),
            _ => None,
        }
    }

    /// Notes a write through a parameter, for callers passing a struct.
    pub(crate) fn learn_write(&mut self, place: &ast::Expr) {
        if matches!(place, ast::Expr::Path(_)) {
            return;
        }
        self.learn_pass(place);
    }

    /// Notes a place handed to a parameter its callee writes: the parameter
    /// it starts from is written too, and so `&mut`.
    pub(crate) fn learn_pass(&mut self, place: &ast::Expr) {
        if !self.rec() {
            return;
        }
        let Some(root) = self.write_root(place) else {
            return;
        };
        let Some(sig) = self.sig else { return };
        if let Some(p) = self.p.sigs[sig].params.iter_mut().find(|p| p.name == root)
            && !p.mutated
        {
            p.mutated = true;
            self.p.changed = true;
        }
    }

    /// A write through a local that copied a struct out of a place.
    pub(crate) fn check_write(&mut self, place: &ast::Expr) {
        if self.learn || matches!(place, ast::Expr::Path(_)) {
            return;
        }
        let Some(root) = self.write_root(place) else {
            return;
        };
        let shared = |cx: &Self, v: &crate::cx::Var| {
            let ty = v.ty.clone().or_else(|| {
                let sig = cx.sig?;
                cx.p.sigs[sig].locals.get(&v.name).cloned()
            });
            ty.is_some_and(|t| cx.p.is_class(&t))
        };
        if let Some(v) = self.lookup(&root)
            && v.alias
            && !shared(self, v)
        {
            let out = v.out.clone();
            self.flag(
                    place.span(),
                    "copy-write",
                    format!("`{out}` is a copy in .bs (structs are values): this write does not reach the struct it was read from"),
                );
        }
    }
}
