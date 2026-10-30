// What a receiver's type is, and what may follow a `.` on it.

use super::{Completion, Kind};
use crate::ast::{Expr, ExprKind, FnDecl, Pos, Program, Ty};
use crate::codegen::show;
use crate::{HostApi, HostSignature};

pub(crate) const PRELUDE: [(&str, &[&str]); 3] = [
    ("Vec2", &["x", "y"]),
    ("Vec3", &["x", "y", "z"]),
    ("Color", &["r", "g", "b", "a"]),
];

pub(crate) const NATIVES: [(&str, &str); 11] = [
    ("sqrt", "fn sqrt(x: float) -> float"),
    ("abs", "fn abs(x: float) -> float"),
    ("floor", "fn floor(x: float) -> float"),
    ("ceil", "fn ceil(x: float) -> float"),
    ("min", "fn min(a: float, b: float) -> float"),
    ("max", "fn max(a: float, b: float) -> float"),
    ("vec2", "fn vec2(x: float, y: float) -> Vec2"),
    ("vec3", "fn vec3(x: float, y: float, z: float) -> Vec3"),
    (
        "color",
        "fn color(r: float, g: float, b: float, a: float) -> Color",
    ),
    ("nil", "fn nil() -> any"),
    ("map", "fn map() -> any"),
];

const ARRAY: [(&str, &str); 5] = [
    ("len", "fn len() -> int"),
    ("push", "fn push(value)"),
    ("pop", "fn pop() -> element"),
    ("insert", "fn insert(index: int, value)"),
    ("remove", "fn remove(index: int) -> element"),
];

/// What a `.` hangs off: a value, or a node's component.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Recv {
    Ty(Ty),
    Component(String),
}

/// The names in scope at the cursor, and what the file and engine declare.
pub(crate) struct Scope<'a> {
    pub prog: &'a Program,
    pub host: &'a HostApi,
    pub owner: Option<String>,
    pub vars: Vec<(String, Ty, Pos)>,
}

pub(crate) fn signature(name: &str, params: &[Ty], ret: &Ty) -> String {
    let p: Vec<String> = params.iter().map(show).collect();
    let ret = if *ret == Ty::Void {
        String::new()
    } else {
        format!(" -> {}", show(ret))
    };
    format!("fn {name}({}){ret}", p.join(", "))
}

pub(crate) fn host_signature(name: &str, f: &HostSignature, skip_node: bool) -> String {
    let params = if skip_node {
        &f.params[1..]
    } else {
        &f.params[..]
    };
    let sig = signature(name, params, &f.ret);
    if f.is_async {
        format!("async {sig}")
    } else {
        sig
    }
}

pub(crate) fn fn_signature(f: &FnDecl) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let t = show(&p.ty);
            if p.by_ref { format!("&mut {t}") } else { t }
        })
        .collect();
    let ret = if f.ret == Ty::Void {
        String::new()
    } else {
        format!(" -> {}", show(&f.ret))
    };
    let sig = format!("fn {}({}){ret}", f.name, params.join(", "));
    if f.is_async {
        format!("async {sig}")
    } else {
        sig
    }
}

impl Scope<'_> {
    pub(crate) fn field(&self, s: &str, f: &str) -> Option<(Ty, Option<Pos>)> {
        if let Some((_, names)) = PRELUDE.iter().find(|(n, _)| *n == s) {
            return names.contains(&f).then_some((Ty::Float, None));
        }
        if let Some(record) = self.host.record(s) {
            let (_, t) = record.iter().find(|(n, _)| n == f)?;
            return Some((t.clone(), None));
        }
        let (_, fields) = self.prog.structs.iter().find(|(n, _)| n == s)?;
        let field = fields.iter().find(|x| x.name == f)?;
        Some((field.ty.clone(), Some(field.pos)))
    }

    pub(crate) fn method(&self, s: &str, m: &str) -> Option<&FnDecl> {
        self.prog
            .fns
            .iter()
            .find(|f| f.owner.as_deref() == Some(s) && f.name == m)
    }

    pub(crate) fn free_fn(&self, name: &str) -> Option<&FnDecl> {
        self.prog
            .fns
            .iter()
            .find(|f| f.owner.is_none() && f.name == name)
    }

    fn call_ret(&self, name: &str) -> Option<Ty> {
        if let Some(f) = self.free_fn(name) {
            return Some(f.ret.clone());
        }
        if let Some(x) = self.prog.externs.iter().find(|x| x.name == name) {
            return Some(x.ret.clone());
        }
        if let Some(f) = self.host.function(name) {
            return Some(f.ret.clone());
        }
        Some(match name {
            "vec2" => Ty::Struct("Vec2".into()),
            "vec3" => Ty::Struct("Vec3".into()),
            "color" => Ty::Struct("Color".into()),
            "nil" | "map" => Ty::Any,
            _ if NATIVES.iter().any(|(n, _)| *n == name) => Ty::Float,
            _ => return None,
        })
    }

    fn method_ret(&self, recv: &Recv, name: &str) -> Option<Ty> {
        Some(match recv {
            Recv::Ty(Ty::Struct(s)) => self.method(s, name)?.ret.clone(),
            Recv::Ty(Ty::Node) => self.host.function(&format!("node.{name}"))?.ret.clone(),
            Recv::Component(c) => self.host.function(&format!("node.{c}.{name}"))?.ret.clone(),
            Recv::Ty(Ty::Str) if name == "len" => Ty::Int,
            Recv::Ty(Ty::Str) => prelude_ret("str", name)?,
            Recv::Ty(Ty::Any) => prelude_ret("any", name)?,
            Recv::Ty(Ty::Array(t)) => match name {
                "len" => Ty::Int,
                "pop" | "remove" => (**t).clone(),
                _ => Ty::Void,
            },
            Recv::Ty(Ty::Optional(t)) => match name {
                "is_some" | "is_none" => Ty::Bool,
                "unwrap" | "or" => (**t).clone(),
                _ => return None,
            },
            Recv::Ty(_) => return None,
        })
    }

    /// The type of `e`, as far as the file and the engine say.
    pub(crate) fn resolve(&self, e: &Expr) -> Option<Recv> {
        let ty = |t: Ty| Some(Recv::Ty(t));
        match &e.kind {
            ExprKind::Var(n) => ty(self.vars.iter().rev().find(|(v, _, _)| v == n)?.1.clone()),
            ExprKind::SelfRef => ty(Ty::Struct(self.owner.clone()?)),
            ExprKind::Field(base, f) => match self.resolve(base)? {
                Recv::Ty(Ty::Struct(s)) => ty(self.field(&s, f)?.0),
                Recv::Ty(Ty::Node) if self.host.components.iter().any(|(c, _)| c == f) => {
                    Some(Recv::Component(f.clone()))
                }
                Recv::Component(c) => ty(self.host.property(&c, f)?.clone()),
                Recv::Ty(_) => None,
            },
            ExprKind::Index(base, _) => match self.resolve(base)? {
                Recv::Ty(Ty::Array(t)) => ty(*t),
                Recv::Ty(Ty::Any) => ty(Ty::Any),
                _ => None,
            },
            ExprKind::Method(base, name, _) => ty(self.method_ret(&self.resolve(base)?, name)?),
            ExprKind::Call(name, _) => ty(self.call_ret(name)?),
            ExprKind::Str(_) | ExprKind::Interp(_) => ty(Ty::Str),
            ExprKind::Int(_) => ty(Ty::Int),
            ExprKind::Float(_) => ty(Ty::Float),
            ExprKind::Bool(_) => ty(Ty::Bool),
            ExprKind::Cast(_, t) => ty(t.clone()),
            ExprKind::ArrayLit(items) => match items.first().map(|i| self.resolve(i)) {
                Some(Some(Recv::Ty(t))) => ty(Ty::Array(Box::new(t))),
                _ => None,
            },
            ExprKind::ArrayRepeat(v, _) => match self.resolve(v)? {
                Recv::Ty(t) => ty(Ty::Array(Box::new(t))),
                Recv::Component(_) => None,
            },
            ExprKind::StructLit(s, _) => ty(Ty::Struct(s.clone())),
            ExprKind::Bin(op, a, _) => {
                use crate::ast::BinOp::{And, Eq, Ge, Gt, Le, Lt, Ne, Or};
                if matches!(op, Eq | Ne | Lt | Le | Gt | Ge | And | Or) {
                    ty(Ty::Bool)
                } else {
                    self.resolve(a)
                }
            }
            ExprKind::Unary(_, a) | ExprKind::Await(a) => self.resolve(a),
            _ => None,
        }
    }

    /// What may follow a `.` on `recv`.
    pub(crate) fn members(&self, recv: &Recv) -> Vec<Completion> {
        let mut out = vec![];
        let mut add = |label: &str, kind: Kind, detail: String| {
            out.push(Completion {
                label: label.to_string(),
                kind,
                detail,
            });
        };
        match recv {
            Recv::Ty(Ty::Struct(s)) => {
                if let Some((_, names)) = PRELUDE.iter().find(|(n, _)| *n == s) {
                    for n in *names {
                        add(n, Kind::Field, "float".into());
                    }
                }
                if let Some((_, fields)) = self.prog.structs.iter().find(|(n, _)| n == s) {
                    for f in fields {
                        add(&f.name, Kind::Field, show(&f.ty));
                    }
                }
                for (name, t) in self.host.record(s).unwrap_or_default() {
                    add(name, Kind::Field, show(t));
                }
                for f in self
                    .prog
                    .fns
                    .iter()
                    .filter(|f| f.owner.as_deref() == Some(s))
                {
                    add(&f.name, Kind::Method, fn_signature(f));
                }
            }
            Recv::Ty(Ty::Node) => {
                for f in &self.host.functions {
                    if let Some(name) = f.key.strip_prefix("node.").filter(|n| !n.contains('.')) {
                        add(name, Kind::Method, host_signature(name, f, true));
                    }
                }
                for (c, _) in &self.host.components {
                    add(c, Kind::Property, format!("the node's `{c}`"));
                }
            }
            Recv::Component(c) => {
                if let Some((_, props)) = self.host.components.iter().find(|(n, _)| n == c) {
                    for (p, t) in props {
                        add(p, Kind::Property, show(t));
                    }
                }
                let prefix = format!("node.{c}.");
                for f in &self.host.functions {
                    if let Some(name) = f.key.strip_prefix(&prefix) {
                        add(name, Kind::Method, host_signature(name, f, true));
                    }
                }
            }
            Recv::Ty(Ty::Str) => {
                add("len", Kind::Method, "fn len() -> int".into());
                for (name, params, ret) in crate::prelude::methods("str") {
                    add(&name, Kind::Method, signature(&name, &params, &ret));
                }
            }
            Recv::Ty(Ty::Any) => {
                for (name, params, ret) in crate::prelude::methods("any") {
                    add(&name, Kind::Method, signature(&name, &params, &ret));
                }
            }
            Recv::Ty(Ty::Array(_)) => {
                for (n, d) in ARRAY {
                    add(n, Kind::Method, d.into());
                }
            }
            Recv::Ty(Ty::Optional(t)) => {
                let inner = show(t);
                add("is_some", Kind::Method, "fn is_some() -> bool".into());
                add("is_none", Kind::Method, "fn is_none() -> bool".into());
                add("unwrap", Kind::Method, format!("fn unwrap() -> {inner}"));
                add("or", Kind::Method, format!("fn or({inner}) -> {inner}"));
            }
            Recv::Ty(_) => {}
        }
        out
    }
}

// What the prelude's method `name` of a built-in type gives.
fn prelude_ret(ty: &str, name: &str) -> Option<Ty> {
    crate::prelude::methods(ty)
        .into_iter()
        .find(|method| method.0 == name)
        .map(|method| method.2)
}
