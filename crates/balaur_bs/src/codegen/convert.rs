//! A typed value into an `any` and back, as a function generated for each type
//! that crosses, the way a release is. Each is written as a script would write
//! it, so a field or an element that is itself a struct, a list or a `T?` goes
//! through its own: a struct is a map of its fields, a vector or a colour a list
//! of its numbers, a list a list of its elements, a `T?` its value or nil. Read
//! back, a field the map lacks is its
//! type's zero, as `as_int` and the rest give for nil.

use super::func::vector_fn;
use crate::ast::{Block, Expr, ExprKind, FnDecl, Param, Pos, Stmt, StmtKind, Ty};
use crate::vocabulary::words;

const ALL: &str = "all";
const BOXED: &str = "boxed";
const CELL: &str = "cell";
const INNER: &str = "inner";
const ITEM: &str = "item";
const KEY: &str = "key";
const KEYS: &str = "keys";
const MADE: &str = "made";
const OUT: &str = "out";
const VALUES: &str = "values";

/// `fn any::#box(value: T) -> any`, or with `into` false
/// `fn any::#unbox(cell: any) -> T`.
pub(crate) fn function(ty: &Ty, into: bool, fields: &[(String, Ty)]) -> FnDecl {
    let (param, ret) = if into {
        (words::convert::VALUE, Ty::Any)
    } else {
        (CELL, ty.clone())
    };
    let from = if into { ty.clone() } else { Ty::Any };
    let body = match (ty, into) {
        (Ty::Struct(class), _) if crate::maps::is_map(class) => map_body(class, into, fields),
        (Ty::Struct(name), true) if vector_fn(name).is_some() => vector_into(fields),
        (Ty::Struct(_), true) => struct_into(fields),
        (Ty::Struct(struct_name), false) => struct_from(struct_name, fields),
        (Ty::Array(_), true) => each_into(
            words::convert::VALUE,
            Ty::Array(Box::new(Ty::Any)),
            OUT,
            true,
        ),
        (Ty::Array(_), false) => each_into(CELL, ty.clone(), OUT, false),
        (Ty::Optional(_), true) => tail(expr(ExprKind::If(
            Box::new(nil_test(crate::ast::BinOp::Ne, var(words::convert::VALUE))),
            block(
                vec![bind(
                    INNER,
                    Ty::Any,
                    method(var(words::convert::VALUE), words::methods::UNWRAP, vec![]),
                )],
                var(INNER),
            ),
            Some(tail(expr(ExprKind::Nil))),
        ))),
        (Ty::Optional(inner), false) => tail(expr(ExprKind::If(
            Box::new(nil_test(crate::ast::BinOp::Eq, var(CELL))),
            tail(expr(ExprKind::Nil)),
            Some(block(
                vec![bind(INNER, (**inner).clone(), var(CELL))],
                var(INNER),
            )),
        ))),
        _ => unreachable!("only structs, lists and `T?` convert through a function"),
    };
    FnDecl {
        internal: false,
        name: format!("any::#{}", if into { "box" } else { "unbox" }),
        owner: None,
        public: false,
        is_async: false,
        params: vec![Param {
            name: param.into(),
            ty: from,
            by_ref: false,
            pos: Pos::default(),
        }],
        ret,
        body,
        pos: Pos::default(),
    }
}

// A struct read back from a map, a field the map lacks its type's zero. A
// vector or a colour may be a list of its numbers, as a literal may.
fn struct_from(struct_name: &str, fields: &[(String, Ty)]) -> Block {
    let by_key = expr(ExprKind::StructLit(
        struct_name.to_string(),
        fields
            .iter()
            .map(|(name, _)| (name.clone(), index(var(CELL), text(name))))
            .collect(),
    ));
    if !matches!(struct_name, "Vec2" | "Vec3" | "Color") {
        return tail(by_key);
    }
    let by_place = expr(ExprKind::StructLit(
        struct_name.to_string(),
        fields
            .iter()
            .zip(0i64..)
            .map(|((name, _), at)| (name.clone(), index(var(CELL), expr(ExprKind::Int(at)))))
            .collect(),
    ));
    let listed = expr(ExprKind::Bin(
        crate::ast::BinOp::Eq,
        Box::new(method(var(CELL), "kind", vec![])),
        Box::new(text("list")),
    ));
    tail(expr(ExprKind::If(
        Box::new(listed),
        tail(by_place),
        Some(tail(by_key)),
    )))
}

// A vector or a colour as a list of its numbers, which is what an `any` holds
// of one from anywhere else: TOML, a component, the engine.
fn vector_into(fields: &[(String, Ty)]) -> Block {
    tail(list(
        fields
            .iter()
            .map(|(name, _)| field(var(words::convert::VALUE), name))
            .collect(),
    ))
}

// A struct as a map of its fields, each boxed; a `T?` field that is nil is
// left out, as a table that never set it.
fn struct_into(fields: &[(String, Ty)]) -> Block {
    if !fields.iter().any(|(_, ty)| matches!(ty, Ty::Optional(_))) {
        let names = fields.iter().map(|(name, _)| text(name)).collect();
        let values = fields
            .iter()
            .map(|(name, _)| field(var(words::convert::VALUE), name))
            .collect();
        return tail(call("__map", vec![list(names), list(values)]));
    }
    let mut stmts = vec![
        bind(
            KEYS,
            Ty::Array(Box::new(Ty::Str)),
            expr(ExprKind::ArrayLit(vec![])),
        ),
        bind(
            VALUES,
            Ty::Array(Box::new(Ty::Any)),
            expr(ExprKind::ArrayLit(vec![])),
        ),
    ];
    for (name, ty) in fields {
        let put = vec![
            stmt(StmtKind::Expr(method(
                var(KEYS),
                words::methods::PUSH,
                vec![text(name)],
            ))),
            stmt(StmtKind::Expr(method(
                var(VALUES),
                words::methods::PUSH,
                vec![field(var(words::convert::VALUE), name)],
            ))),
        ];
        if !matches!(ty, Ty::Optional(_)) {
            stmts.extend(put);
            continue;
        }
        let held = nil_test(
            crate::ast::BinOp::Ne,
            field(var(words::convert::VALUE), name),
        );
        stmts.push(stmt(StmtKind::Expr(expr(ExprKind::If(
            Box::new(held),
            Block {
                stmts: put,
                tail: None,
            },
            None,
        )))));
    }
    block(stmts, call("__map", vec![var(KEYS), var(VALUES)]))
}

// A `{ str: V }` as an `any` map, its values each boxed, or back: each value
// read as a `V`, the map made with the class's `of`.
fn map_body(class: &str, into: bool, fields: &[(String, Ty)]) -> Block {
    if into {
        let values = method(var(words::convert::VALUE), VALUES, vec![]);
        let boxed = each_into_items(values, Ty::Array(Box::new(Ty::Any)), BOXED);
        let keys = method(var(words::convert::VALUE), KEYS, vec![]);
        return block(boxed, call("__map", vec![keys, var(BOXED)]));
    }
    let values_ty = fields
        .iter()
        .find(|(name, _)| name == words::methods::VALUES_IN_ORDER)
        .map_or(Ty::Array(Box::new(Ty::Any)), |(_, ty)| ty.clone());
    let mut stmts = vec![
        bind(
            KEYS,
            Ty::Array(Box::new(Ty::Str)),
            method(var(CELL), KEYS, vec![]),
        ),
        bind(VALUES, values_ty, expr(ExprKind::ArrayLit(vec![]))),
        stmt(StmtKind::ForEach {
            var: KEY.into(),
            items: var(KEYS),
            body: Block {
                stmts: vec![stmt(StmtKind::Expr(method(
                    var(VALUES),
                    words::methods::PUSH,
                    vec![index(var(CELL), var(KEY))],
                )))],
                tail: None,
            },
        }),
    ];
    let made = call(
        &crate::maps::function(class, "of"),
        vec![var(KEYS), var(VALUES)],
    );
    stmts.push(bind(MADE, Ty::Struct(class.to_string()), made));
    block(stmts, var(MADE))
}

// `let out: [..] = []; for item in items { out.push(item); }` for `items` an
// expression, each element converted as it is pushed.
fn each_into_items(items: Expr, out_ty: Ty, out: &str) -> Vec<Stmt> {
    vec![
        bind(out, out_ty, expr(ExprKind::ArrayLit(vec![]))),
        stmt(StmtKind::ForEach {
            var: ITEM.into(),
            items,
            body: Block {
                stmts: vec![stmt(StmtKind::Expr(method(
                    var(out),
                    words::methods::PUSH,
                    vec![var(ITEM)],
                )))],
                tail: None,
            },
        }),
    ]
}

// `let out: [..] = []; for item in items { out.push(item); }`, then `out` as an
// `any` when boxing: each element converts as it is pushed.
fn each_into(items: &str, out_ty: Ty, out: &str, boxed: bool) -> Block {
    let mut stmts = each_into_items(var(items), out_ty, out);
    if !boxed {
        return block(stmts, var(out));
    }
    stmts.push(bind(ALL, Ty::Any, var(out)));
    block(stmts, var(ALL))
}

fn expr(kind: ExprKind) -> Expr {
    Expr {
        kind,
        pos: Pos::default(),
    }
}

fn stmt(kind: StmtKind) -> Stmt {
    Stmt {
        kind,
        pos: Pos::default(),
    }
}

fn var(name: &str) -> Expr {
    expr(ExprKind::Var(name.into()))
}

fn text(value: &str) -> Expr {
    expr(ExprKind::Str(value.into()))
}

fn field(base: Expr, name: &str) -> Expr {
    expr(ExprKind::Field(Box::new(base), name.into()))
}

fn nil_test(op: crate::ast::BinOp, value: Expr) -> Expr {
    expr(ExprKind::Bin(
        op,
        Box::new(value),
        Box::new(expr(ExprKind::Nil)),
    ))
}

fn index(base: Expr, key: Expr) -> Expr {
    expr(ExprKind::Index(Box::new(base), Box::new(key)))
}

fn method(base: Expr, name: &str, args: Vec<Expr>) -> Expr {
    expr(ExprKind::Method(Box::new(base), name.into(), args))
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    expr(ExprKind::Call(name.into(), args))
}

fn list(items: Vec<Expr>) -> Expr {
    expr(ExprKind::ArrayLit(items))
}

fn bind(name: &str, ty: Ty, init: Expr) -> Stmt {
    stmt(StmtKind::Let {
        name: name.into(),
        ty: Some(ty),
        init,
    })
}

fn tail(value: Expr) -> Block {
    block(vec![], value)
}

fn block(stmts: Vec<Stmt>, value: Expr) -> Block {
    Block {
        stmts,
        tail: Some(Box::new(value)),
    }
}
