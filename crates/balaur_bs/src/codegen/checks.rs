// What a program must hold before any code is made: every type it names
// exists, no struct holds itself, and each field default is a literal.

use super::func::{R, err, show};
use crate::FieldDefaults;
use crate::Literal;
use crate::ast::{ExprKind, Program, Ty};

pub(crate) fn unknown_type(t: &Ty, names: &[&String]) -> Option<String> {
    match t {
        Ty::Struct(s) if !names.contains(&s) => Some(s.clone()),
        Ty::Array(e) | Ty::Optional(e) => unknown_type(e, names),
        Ty::Fn(params, ret, _) => params
            .iter()
            .chain(std::iter::once(&**ret))
            .find_map(|p| unknown_type(p, names)),
        _ => None,
    }
}

// Every type a signature names is a struct the program has; a `let`'s is
// checked where it is compiled.
pub(crate) fn check_types(prog: &Program) -> R<()> {
    let names: Vec<&String> = prog.structs.iter().map(|(n, _)| n).collect();
    let unknown = |t: &Ty, pos| match unknown_type(t, &names) {
        Some(u) => err(pos, format!("unknown type `{u}`")),
        None => Ok(()),
    };
    for x in &prog.externs {
        for p in &x.params {
            unknown(&p.ty, p.pos)?;
            if p.by_ref {
                return err(p.pos, "the engine takes no `&mut` parameter");
            }
        }
        unknown(&x.ret, x.pos)?;
    }
    for f in &prog.fns {
        for p in &f.params {
            unknown(&p.ty, p.pos)?;
            if p.by_ref && f.is_async {
                return err(
                    p.pos,
                    "an `async fn` takes no `&mut` parameter: its caller runs on while it waits",
                );
            }
        }
        unknown(&f.ret, f.pos)?;
    }
    Ok(())
}

// Whether a `t` holds a `target` inline: through value structs, and through
// `T?` too unless `optional_breaks`. A class field is a pointer, as is an array.
fn holds(
    t: &Ty,
    target: &str,
    prog: &Program,
    optional_breaks: bool,
    seen: &mut Vec<String>,
) -> bool {
    match t {
        Ty::Optional(inner) if !optional_breaks => {
            holds(inner, target, prog, optional_breaks, seen)
        }
        Ty::Struct(s) if s == target => true,
        Ty::Struct(s) if prog.classes.contains(s) || seen.contains(s) => false,
        Ty::Struct(s) => {
            seen.push(s.clone());
            prog.structs
                .iter()
                .find(|(n, _)| n == s)
                .is_some_and(|(_, fs)| {
                    fs.iter()
                        .any(|p| holds(&p.ty, target, prog, optional_breaks, seen))
                })
        }
        _ => false,
    }
}

pub(crate) fn check_structs(prog: &Program) -> R<()> {
    let names: Vec<&String> = prog.structs.iter().map(|(n, _)| n).collect();
    let at = crate::ast::Pos { line: 1, col: 1 };
    for (n, fields) in &prog.structs {
        let class = prog.classes.contains(n);
        for f in fields {
            if let Some(u) = unknown_type(&f.ty, &names) {
                return err(
                    at,
                    format!("struct {n}: unknown type `{u}` for field `{}`", f.name),
                );
            }
            if holds(&f.ty, n, prog, class, &mut vec![]) {
                let fix = if class {
                    format!("make it `{n}?`")
                } else {
                    "put it in an array, or make it a class".to_string()
                };
                return err(
                    at,
                    format!("struct {n} contains itself through `{}`; {fix}", f.name),
                );
            }
        }
    }
    Ok(())
}

// Field defaults are literals, checked against the field's type.
pub(crate) fn defaults(prog: &Program) -> R<Vec<(String, FieldDefaults)>> {
    let mut out = vec![];
    for (name, fields) in &prog.structs {
        let mut declared = vec![];
        for f in fields {
            let Some(e) = &f.default else { continue };
            let lit = match (&e.kind, &f.ty) {
                (ExprKind::Int(v), Ty::Int) => Literal::Int(*v),
                (ExprKind::Int(v), Ty::Float) => Literal::Float(*v as f64),
                (ExprKind::Float(v), Ty::Float) => Literal::Float(*v),
                (ExprKind::Bool(v), Ty::Bool) => Literal::Bool(*v),
                (ExprKind::Str(v), Ty::Str) => Literal::Str(v.clone()),
                (ExprKind::Unary(crate::ast::UnOp::Neg, inner), Ty::Int | Ty::Float) => {
                    match (&inner.kind, &f.ty) {
                        (ExprKind::Int(v), Ty::Int) => Literal::Int(-v),
                        (ExprKind::Int(v), Ty::Float) => Literal::Float(-(*v as f64)),
                        (ExprKind::Float(v), Ty::Float) => Literal::Float(-v),
                        _ => {
                            return err(
                                e.pos,
                                format!(
                                    "the default of `{}` must be a {} literal",
                                    f.name,
                                    show(&f.ty)
                                ),
                            );
                        }
                    }
                }
                _ => {
                    return err(
                        e.pos,
                        format!(
                            "the default of `{}` must be a {} literal",
                            f.name,
                            show(&f.ty)
                        ),
                    );
                }
            };
            declared.push((f.name.clone(), f.public, lit));
        }
        if !declared.is_empty() {
            out.push((name.clone(), declared));
        }
    }
    Ok(out)
}
