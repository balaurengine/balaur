// The names in scope at the cursor, read off the file's syntax alone: a
// half-typed line still parses once the part being typed is cut out.

use super::types::{Recv, Scope};
use crate::HostApi;
use crate::ast::{Block, FnDecl, Pos, Program, StmtKind, Ty};

fn before(a: Pos, b: Pos) -> bool {
    (a.line, a.col) < (b.line, b.col)
}

/// The function the cursor is in: the last one starting before it.
pub(crate) fn enclosing(prog: &Program, at: Pos) -> Option<&FnDecl> {
    prog.fns
        .iter()
        .filter(|f| before(f.pos, at))
        .max_by_key(|f| (f.pos.line, f.pos.col))
}

pub(crate) fn at<'a>(prog: &'a Program, host: &'a HostApi, at: Pos) -> Scope<'a> {
    let mut scope = Scope {
        prog,
        host,
        owner: None,
        vars: vec![],
    };
    if let Some(f) = enclosing(prog, at) {
        scope.owner.clone_from(&f.owner);
        for p in &f.params {
            scope.vars.push((p.name.clone(), p.ty.clone(), p.pos));
        }
        walk(&mut scope, &f.body, at);
    }
    scope
}

fn element(scope: &Scope<'_>, items: &crate::ast::Expr) -> Ty {
    match scope.resolve(items) {
        Some(Recv::Ty(Ty::Array(t))) => *t,
        _ => Ty::Any,
    }
}

// A statement that starts before the cursor binds its names; a block whose
// next statement starts after the cursor may hold it, so its names count too.
fn walk(scope: &mut Scope<'_>, block: &Block, at: Pos) {
    let stmts = &block.stmts;
    for (k, s) in stmts.iter().enumerate() {
        if !before(s.pos, at) {
            break;
        }
        let inside = stmts.get(k + 1).is_none_or(|next| !before(next.pos, at));
        match &s.kind {
            StmtKind::Let { name, ty, init } => {
                let t = ty.clone().or_else(|| match scope.resolve(init) {
                    Some(Recv::Ty(t)) => Some(t),
                    _ => None,
                });
                scope.vars.push((name.clone(), t.unwrap_or(Ty::Any), s.pos));
            }
            StmtKind::ForRange { var, body, .. } if inside => {
                scope.vars.push((var.clone(), Ty::Int, s.pos));
                walk(scope, body, at);
            }
            StmtKind::ForEach { var, items, body } if inside => {
                let t = element(scope, items);
                scope.vars.push((var.clone(), t, s.pos));
                walk(scope, body, at);
            }
            StmtKind::While { body, .. } | StmtKind::Loop(body) if inside => walk(scope, body, at),
            _ => {}
        }
    }
}
