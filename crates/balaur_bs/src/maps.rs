// `{ str: V }` and `{ int: V }`: built in, typed, a hash table in the module.
// Each pair a program names is the prelude's `map.bs` made for it: a class
// named `{ K: V }::Table` with its methods, and `{ K: V }::of`, compiled into the
// program as its own. A map type is made its class before code is made, so
// codegen, the runtime and the host see a class; `map[key]` is its `get`,
// `map[key] = value` its `set`, and a literal its `of`.

use crate::Error;
use crate::ast::{Block, Expr, ExprKind, Part, Program, StmtKind, Ty};
use std::collections::BTreeMap;

const TEMPLATE: &str = include_str!("prelude/map.bs");
const CLASS: &str = "Table";
/// What the template's key and value types are called in it.
const KEY: &str = "K";
const VALUE: &str = "V";

thread_local! {
    static PARSED: std::cell::OnceCell<Program> = const { std::cell::OnceCell::new() };
}

/// Whether `name` is a map's class.
pub(crate) fn is_map(name: &str) -> bool {
    name.starts_with('{') && name.ends_with(&format!("}}::{CLASS}"))
}

/// What an error shows for type `name`: a map's class as the map it is.
pub(crate) fn shown(name: &str) -> &str {
    name.strip_suffix(CLASS)
        .and_then(|rest| rest.strip_suffix("::"))
        .filter(|map| map.starts_with('{'))
        .unwrap_or(name)
}

/// Whether `key` is a function a map's class was made with, which may call
/// the built-ins only the prelude may.
pub(crate) fn owns(key: &str) -> bool {
    key.starts_with('{')
}

/// The free function `name` of the map whose class is `class`:
/// `{ str: int }::of`.
pub(crate) fn function(class: &str, name: &str) -> String {
    format!("{}::{name}", shown(class))
}

/// Every map type in `prog` made its class, and each class made from the
/// template once.
pub(crate) fn lower(prog: &mut Program) -> Result<(), Error> {
    let mut made: BTreeMap<String, (Ty, Ty)> = BTreeMap::new();
    each_ty(prog, &mut |ty| lower_ty(ty, &mut made));
    for (name, (key, value)) in made {
        if prog.classes.contains(&format!("{name}::{CLASS}")) {
            continue;
        }
        let mut map = template()?;
        each_ty(&mut map, &mut |ty| substitute(ty, &key, &value));
        // Prefixed as a module is, but compiled into the program as its own.
        crate::modules::prefix(&mut map, &name, &name);
        prog.structs.append(&mut map.structs);
        prog.classes.append(&mut map.classes);
        prog.fns.append(&mut map.fns);
    }
    Ok(())
}

fn template() -> Result<Program, Error> {
    PARSED.with(|parsed| {
        if let Some(map) = parsed.get() {
            return Ok(map.clone());
        }
        let map = crate::parse::parse(TEMPLATE).map_err(|e| Error {
            pos: e.pos,
            msg: format!("internal: the prelude's map: {}", e.msg),
        })?;
        Ok(parsed.get_or_init(|| map).clone())
    })
}

// `ty` with each map in it made its class, inner ones first, and each class
// it names noted with its key and value.
fn lower_ty(ty: &mut Ty, made: &mut BTreeMap<String, (Ty, Ty)>) {
    match ty {
        Ty::Map(key, value) => {
            lower_ty(value, made);
            let name = format!("{{ {}: {} }}", crate::show(key), crate::show(value));
            made.insert(name.clone(), ((**key).clone(), (**value).clone()));
            *ty = Ty::Struct(format!("{name}::{CLASS}"));
        }
        Ty::Array(inner) | Ty::Optional(inner) => lower_ty(inner, made),
        Ty::Fn(params, ret, _) => {
            for param in params {
                lower_ty(param, made);
            }
            lower_ty(ret, made);
        }
        _ => {}
    }
}

// The template's `K` and `V` as the pair's types. A missing key's `V?` is
// plain `any` for an `any` value, which is nil already.
fn substitute(ty: &mut Ty, key: &Ty, value: &Ty) {
    if *value == Ty::Any
        && let Ty::Optional(inner) = ty
        && matches!(&**inner, Ty::Struct(name) if name == VALUE)
    {
        *ty = Ty::Any;
        return;
    }
    match ty {
        Ty::Struct(name) if name == KEY => *ty = key.clone(),
        Ty::Struct(name) if name == VALUE => *ty = value.clone(),
        Ty::Array(inner) | Ty::Optional(inner) => substitute(inner, key, value),
        Ty::Map(inner_key, inner_value) => {
            substitute(inner_key, key, value);
            substitute(inner_value, key, value);
        }
        Ty::Fn(params, ret, _) => {
            for param in params {
                substitute(param, key, value);
            }
            substitute(ret, key, value);
        }
        _ => {}
    }
}

// `f` on every type `prog` spells: fields, parameters, results, `let`s and
// casts. The prelude's own functions spell no map.
fn each_ty(prog: &mut Program, f: &mut dyn FnMut(&mut Ty)) {
    for (_, fields) in &mut prog.structs {
        for field in fields {
            f(&mut field.ty);
            if let Some(default) = &mut field.default {
                expr(default, f);
            }
        }
    }
    for func in &mut prog.fns {
        for param in &mut func.params {
            f(&mut param.ty);
        }
        f(&mut func.ret);
        block(&mut func.body, f);
    }
    for external in &mut prog.externs {
        for param in &mut external.params {
            f(&mut param.ty);
        }
        f(&mut external.ret);
    }
    for (_, value) in &mut prog.consts {
        expr(value, f);
    }
}

fn block(b: &mut Block, f: &mut dyn FnMut(&mut Ty)) {
    for stmt in &mut b.stmts {
        match &mut stmt.kind {
            StmtKind::Let { ty, init, .. } => {
                if let Some(ty) = ty {
                    f(ty);
                }
                expr(init, f);
            }
            StmtKind::Assign { target, value, .. } => {
                expr(target, f);
                expr(value, f);
            }
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) => expr(e, f),
            StmtKind::While { cond, body } => {
                expr(cond, f);
                block(body, f);
            }
            StmtKind::Loop(body) => block(body, f),
            StmtKind::ForRange { from, to, body, .. } => {
                expr(from, f);
                expr(to, f);
                block(body, f);
            }
            StmtKind::ForEach { items, body, .. } => {
                expr(items, f);
                block(body, f);
            }
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }
    if let Some(tail) = &mut b.tail {
        expr(tail, f);
    }
}

fn expr(e: &mut Expr, f: &mut dyn FnMut(&mut Ty)) {
    match &mut e.kind {
        ExprKind::Cast(inner, ty) => {
            expr(inner, f);
            f(ty);
        }
        ExprKind::Resumed(ty, _) => f(ty),
        ExprKind::Interp(parts) => {
            for part in parts {
                if let Part::Value(value) = part {
                    expr(value, f);
                }
            }
        }
        ExprKind::Bin(_, a, b) | ExprKind::Index(a, b) | ExprKind::ArrayRepeat(a, b) => {
            expr(a, f);
            expr(b, f);
        }
        ExprKind::Unary(_, inner) | ExprKind::Field(inner, _) | ExprKind::Await(inner) => {
            expr(inner, f);
        }
        ExprKind::Call(_, args) | ExprKind::ArrayLit(args) => {
            for arg in args {
                expr(arg, f);
            }
        }
        ExprKind::Method(base, _, args) => {
            expr(base, f);
            for arg in args {
                expr(arg, f);
            }
        }
        ExprKind::StructLit(_, fields) => {
            for (_, value) in fields {
                expr(value, f);
            }
        }
        ExprKind::MapLit(entries) => {
            for (key, value) in entries {
                expr(key, f);
                expr(value, f);
            }
        }
        ExprKind::If(cond, then, otherwise) => {
            expr(cond, f);
            block(then, f);
            if let Some(otherwise) = otherwise {
                block(otherwise, f);
            }
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_)
        | ExprKind::SelfRef => {}
    }
}
