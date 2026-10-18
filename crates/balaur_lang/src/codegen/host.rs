// The engine functions a script reaches, found before layout so their imports
// come first: `module::name(..)`, `node.op(..)`, `node.component.property`.

use super::func::{FnGen, R};
use crate::ast::{Block, Expr, ExprKind, Part, Program, Stmt, StmtKind, Ty};
use crate::{HostApi, HostSignature};

/// A component property's getter and setter import keys.
pub(crate) fn property_keys(component: &str, property: &str) -> (String, String) {
    (
        format!("{component}.{property}.get"),
        format!("{component}.{property}.set"),
    )
}

struct Scan<'a> {
    api: &'a HostApi,
    found: Vec<HostSignature>,
}

impl Scan<'_> {
    fn add(&mut self, f: HostSignature) {
        if !self.found.iter().any(|g| g.key == f.key) {
            self.found.push(f);
        }
    }
    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
    }
    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let { init: e, .. } | StmtKind::Expr(e) | StmtKind::Return(Some(e)) => {
                self.expr(e);
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::Loop(body) => self.block(body),
            StmtKind::ForRange { from, to, body, .. } => {
                self.expr(from);
                self.expr(to);
                self.block(body);
            }
            StmtKind::ForEach { items, body, .. } => {
                self.expr(items);
                self.block(body);
            }
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }
    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call(name, args) => {
                if let Some(f) = self.api.function(name) {
                    self.add(f.clone());
                }
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Method(base, name, args) => {
                let on_component = match &base.kind {
                    ExprKind::Field(_, comp) => self.api.function(&format!("node.{comp}.{name}")),
                    _ => None,
                };
                if let Some(f) = on_component.or_else(|| self.api.function(&format!("node.{name}")))
                {
                    self.add(f.clone());
                }
                self.expr(base);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Field(base, prop) => {
                if let ExprKind::Field(_, comp) = &base.kind
                    && let Some(ty) = self.api.property(comp, prop)
                {
                    let (get, set) = property_keys(comp, prop);
                    self.add(HostSignature {
                        key: get,
                        params: vec![Ty::Node],
                        ret: ty.clone(),
                        is_async: false,
                    });
                    self.add(HostSignature {
                        key: set,
                        params: vec![Ty::Node, ty.clone()],
                        ret: Ty::Void,
                        is_async: false,
                    });
                }
                self.expr(base);
            }
            ExprKind::Interp(parts) => {
                for p in parts {
                    if let Part::Value(v) = p {
                        self.expr(v);
                    }
                }
            }
            ExprKind::Bin(_, a, b) | ExprKind::Index(a, b) | ExprKind::ArrayRepeat(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::Unary(_, a) | ExprKind::Cast(a, _) | ExprKind::Await(a) => self.expr(a),
            ExprKind::StructLit(_, fields) => fields.iter().for_each(|(_, v)| self.expr(v)),
            ExprKind::ArrayLit(items) => items.iter().for_each(|v| self.expr(v)),
            ExprKind::If(c, t, el) => {
                self.expr(c);
                self.block(t);
                if let Some(b) = el {
                    self.block(b);
                }
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::Var(_)
            | ExprKind::SelfRef
            | ExprKind::Resumed(..) => {}
        }
    }
}

/// Every engine function `prog` names, in the order it first names them.
pub(crate) fn host_calls(prog: &Program, api: &HostApi) -> Vec<HostSignature> {
    let mut scan = Scan { api, found: vec![] };
    for f in &prog.fns {
        scan.block(&f.body);
    }
    scan.found
}

impl FnGen<'_> {
    /// `node.component.property`: the node expression, the getter and setter
    /// keys, and the property's type, when `e` is one.
    pub(crate) fn component_access(&mut self, e: &Expr) -> R<Option<(Expr, String, String, Ty)>> {
        let ExprKind::Field(base, prop) = &e.kind else {
            return Ok(None);
        };
        let ExprKind::Field(node, comp) = &base.kind else {
            return Ok(None);
        };
        let Some(ty) = self.g.host.property(comp, prop).cloned() else {
            return Ok(None);
        };
        if self.type_of(node)? != Ty::Node {
            return Ok(None);
        }
        let (get, set) = property_keys(comp, prop);
        Ok(Some(((**node).clone(), get, set, ty)))
    }

    pub(crate) fn host_call(&mut self, key: &str) {
        let idx = self.g.sigs[key].idx;
        self.e(wasm_encoder::Instruction::Call(idx));
    }
}
