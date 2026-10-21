// `use util;`: another `.bs` file linked into this one when it compiles. Its
// items take the name `util::item`, and a private function a name no script
// can write, so only what it makes `pub` is reached from outside.

use crate::Error;
use crate::ast::{Block, Expr, ExprKind, Part, Pos, Program, StmtKind, Ty};
use crate::parse;
use std::collections::HashSet;
use std::rc::Rc;

/// What a `use` finds: `(from, name)` to the used file's key and source.
pub type Find = dyn Fn(&str, &str) -> Option<(String, String)>;

/// Finds a module for a file, or `None` when there is no such file.
#[derive(Clone)]
pub struct Modules(pub Rc<Find>);

impl std::fmt::Debug for Modules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Modules")
    }
}

/// Every module `root` uses, and each one's, merged into it; the keys of the
/// files that went in. Modules may use each other in a circle, and the root
/// itself: a module's `editor::boot` is then the root's `boot`.
pub(crate) fn link(
    root: &mut Program,
    from: &str,
    modules: Option<&Modules>,
) -> Result<Vec<String>, Error> {
    let mut linked = Linker {
        modules,
        root: from.to_string(),
        done: vec![],
        stack: vec![],
        aliases: vec![],
        out: Program::default(),
    };
    let uses = std::mem::take(&mut root.uses);
    linked.visit(from, &uses)?;
    let keys = linked.done.iter().map(|(k, _)| k.clone()).collect();
    let aliases = std::mem::take(&mut linked.aliases);
    let mut out = linked.out;
    root.linked.append(&mut out.linked);
    root.structs.append(&mut out.structs);
    root.classes.append(&mut out.classes);
    root.fns.append(&mut out.fns);
    root.consts.append(&mut out.consts);
    root.origin.append(&mut out.origin);
    for x in out.externs {
        if !root.externs.iter().any(|e| e.name == x.name) {
            root.externs.push(x);
        }
    }
    if !aliases.is_empty() {
        unalias(root, &aliases);
    }
    Ok(keys)
}

struct Linker<'a> {
    modules: Option<&'a Modules>,
    root: String,
    /// Each file linked so far, and the name it goes by.
    done: Vec<(String, String)>,
    /// The files being linked, outermost first, and their names.
    stack: Vec<(String, String)>,
    /// What modules call the root file.
    aliases: Vec<String>,
    out: Program,
}

fn in_file(e: &Error, key: &str) -> Error {
    Error {
        pos: e.pos,
        msg: format!("in {key}: {}", e.msg),
    }
}

impl Linker<'_> {
    fn visit(&mut self, from: &str, uses: &[(String, Pos)]) -> Result<(), Error> {
        for (path, pos) in uses {
            let name = path.rsplit("::").next().unwrap_or(path).to_string();
            let Some((key, source)) = self.modules.and_then(|m| (m.0)(from, path)) else {
                let file = path.replace("::", "/");
                return Err(Error {
                    pos: *pos,
                    msg: format!("no module `{path}`: no `{file}.bs` beside this file or above it"),
                });
            };
            if key == self.root {
                if !self.aliases.contains(&name) {
                    self.aliases.push(name);
                }
                continue;
            }
            let seen = self.stack.iter().chain(&self.done);
            if let Some((_, other)) = seen.clone().find(|(k, _)| *k == key) {
                if *other != name {
                    return Err(Error {
                        pos: *pos,
                        msg: format!("`{key}` is used as both `{other}` and `{name}`"),
                    });
                }
                // Being linked by a visit further out, or linked already.
                continue;
            }
            if let Some((other, _)) = seen.into_iter().find(|(_, n)| *n == name) {
                return Err(Error {
                    pos: *pos,
                    msg: format!("`{name}` names both `{other}` and `{key}`"),
                });
            }
            let mut m = parse::parse(&source).map_err(|e| in_file(&e, &key))?;
            self.stack.push((key.clone(), name.clone()));
            let inner = std::mem::take(&mut m.uses);
            self.visit(&key, &inner)?;
            self.stack.pop();
            prefix(&mut m, &name, &key);
            self.done.push((key, name.clone()));
            self.out.linked.push(name);
            self.out.structs.append(&mut m.structs);
            self.out.classes.append(&mut m.classes);
            self.out.fns.append(&mut m.fns);
            self.out.consts.append(&mut m.consts);
            self.out.externs.append(&mut m.externs);
            self.out.origin.append(&mut m.origin);
        }
        Ok(())
    }
}

/// How a module's names change as it is linked.
trait Rename {
    /// A function called by this name.
    fn func(&self, n: &str) -> Option<String>;
    /// A name read as a value: a constant, or a function.
    fn value(&self, n: &str) -> Option<String>;
    fn strukt(&self, n: &str) -> Option<String>;

    fn ty(&self, t: &mut Ty) {
        match t {
            Ty::Struct(s) => {
                if let Some(full) = self.strukt(s) {
                    *s = full;
                }
            }
            Ty::Array(e) | Ty::Optional(e) => self.ty(e),
            Ty::Fn(params, ret, _) => {
                for p in params {
                    self.ty(p);
                }
                self.ty(ret);
            }
            _ => {}
        }
    }
}

/// What a module's items are called from outside it.
struct Names {
    prefix: String,
    public_fns: HashSet<String>,
    private_fns: HashSet<String>,
    structs: HashSet<String>,
    consts: HashSet<String>,
}

impl Rename for Names {
    fn func(&self, n: &str) -> Option<String> {
        if self.public_fns.contains(n) {
            Some(format!("{}::{n}", self.prefix))
        } else if self.private_fns.contains(n) {
            // `#` is no character a script can write.
            Some(format!("{}::#{n}", self.prefix))
        } else {
            None
        }
    }
    fn value(&self, n: &str) -> Option<String> {
        if self.consts.contains(n) {
            Some(format!("{}::{n}", self.prefix))
        } else {
            self.func(n)
        }
    }
    fn strukt(&self, n: &str) -> Option<String> {
        self.structs
            .contains(n)
            .then(|| format!("{}::{n}", self.prefix))
    }
}

/// The names modules call the root file by: `editor::boot` is `boot`.
struct Unalias<'a>(&'a [String]);

impl Unalias<'_> {
    fn bare(&self, n: &str) -> Option<String> {
        self.0
            .iter()
            .find_map(|a| n.strip_prefix(a.as_str())?.strip_prefix("::"))
            .map(str::to_string)
    }
}

impl Rename for Unalias<'_> {
    fn func(&self, n: &str) -> Option<String> {
        self.bare(n)
    }
    fn value(&self, n: &str) -> Option<String> {
        self.bare(n)
    }
    fn strukt(&self, n: &str) -> Option<String> {
        self.bare(n)
    }
}

fn unalias(prog: &mut Program, aliases: &[String]) {
    let names = Unalias(aliases);
    for (_, fields) in &mut prog.structs {
        for f in fields {
            names.ty(&mut f.ty);
            if let Some(d) = &mut f.default {
                expr(&names, &mut vec![], d);
            }
        }
    }
    for (_, e) in &mut prog.consts {
        expr(&names, &mut vec![], e);
    }
    for f in &mut prog.fns {
        let mut scope: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        for p in &mut f.params {
            names.ty(&mut p.ty);
        }
        names.ty(&mut f.ret);
        block(&names, &mut scope, &mut f.body);
    }
}

fn prefix(m: &mut Program, name: &str, key: &str) {
    let free = m.fns.iter().filter(|f| f.owner.is_none());
    let (public, private): (Vec<_>, Vec<_>) = free.partition(|f| f.public);
    let names = Names {
        prefix: name.to_string(),
        public_fns: public.iter().map(|f| f.name.clone()).collect(),
        private_fns: private.iter().map(|f| f.name.clone()).collect(),
        structs: m.structs.iter().map(|(n, _)| n.clone()).collect(),
        consts: m.consts.iter().map(|(n, _)| n.clone()).collect(),
    };
    for (n, fields) in &mut m.structs {
        for f in fields {
            names.ty(&mut f.ty);
            if let Some(d) = &mut f.default {
                expr(&names, &mut vec![], d);
            }
        }
        *n = format!("{name}::{n}");
    }
    for n in &mut m.classes {
        *n = format!("{name}::{n}");
    }
    for (n, e) in &mut m.consts {
        expr(&names, &mut vec![], e);
        *n = format!("{name}::{n}");
    }
    for f in &mut m.fns {
        match &mut f.owner {
            Some(o) => *o = format!("{name}::{o}"),
            None => f.name = names.func(&f.name).unwrap_or_else(|| f.name.clone()),
        }
        let mut scope: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        for p in &mut f.params {
            names.ty(&mut p.ty);
        }
        names.ty(&mut f.ret);
        block(&names, &mut scope, &mut f.body);
        m.origin.push((f.key(), key.to_string()));
    }
    for x in &mut m.externs {
        for p in &mut x.params {
            names.ty(&mut p.ty);
        }
        names.ty(&mut x.ret);
    }
}

fn block(n: &dyn Rename, scope: &mut Vec<String>, b: &mut Block) {
    let mark = scope.len();
    for s in &mut b.stmts {
        match &mut s.kind {
            StmtKind::Let { name, ty, init } => {
                expr(n, scope, init);
                if let Some(t) = ty {
                    n.ty(t);
                }
                scope.push(name.clone());
            }
            StmtKind::Assign { target, value, .. } => {
                expr(n, scope, target);
                expr(n, scope, value);
            }
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) => expr(n, scope, e),
            StmtKind::While { cond, body } => {
                expr(n, scope, cond);
                block(n, scope, body);
            }
            StmtKind::Loop(body) => block(n, scope, body),
            StmtKind::ForRange {
                var,
                from,
                to,
                body,
            } => {
                expr(n, scope, from);
                expr(n, scope, to);
                scope.push(var.clone());
                block(n, scope, body);
                scope.pop();
            }
            StmtKind::ForEach { var, items, body } => {
                expr(n, scope, items);
                scope.push(var.clone());
                block(n, scope, body);
                scope.pop();
            }
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }
    if let Some(t) = &mut b.tail {
        expr(n, scope, t);
    }
    scope.truncate(mark);
}

fn expr(n: &dyn Rename, scope: &mut Vec<String>, e: &mut Expr) {
    let local = |scope: &Vec<String>, v: &str| scope.iter().any(|s| s == v);
    match &mut e.kind {
        ExprKind::Call(name, args) => {
            if !local(scope, name)
                && let Some(full) = n.func(name)
            {
                *name = full;
            }
            for a in args {
                expr(n, scope, a);
            }
        }
        ExprKind::Var(v) if !local(scope, v) => {
            if let Some(full) = n.value(v) {
                *v = full;
            }
        }
        ExprKind::StructLit(s, fields) => {
            if let Some(full) = n.strukt(s) {
                *s = full;
            }
            for (_, f) in fields {
                expr(n, scope, f);
            }
        }
        ExprKind::Cast(a, t) => {
            expr(n, scope, a);
            n.ty(t);
        }
        ExprKind::Interp(parts) => {
            for p in parts {
                if let Part::Value(v) = p {
                    expr(n, scope, v);
                }
            }
        }
        ExprKind::Bin(_, a, b) | ExprKind::Index(a, b) | ExprKind::ArrayRepeat(a, b) => {
            expr(n, scope, a);
            expr(n, scope, b);
        }
        ExprKind::Unary(_, a) | ExprKind::Field(a, _) | ExprKind::Await(a) => expr(n, scope, a),
        ExprKind::Method(base, _, args) => {
            expr(n, scope, base);
            for a in args {
                expr(n, scope, a);
            }
        }
        ExprKind::ArrayLit(items) => {
            for i in items {
                expr(n, scope, i);
            }
        }
        ExprKind::If(c, then, els) => {
            expr(n, scope, c);
            block(n, scope, then);
            if let Some(b) = els {
                block(n, scope, b);
            }
        }
        ExprKind::Resumed(t, _) => n.ty(t),
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_)
        | ExprKind::SelfRef => {}
    }
}
