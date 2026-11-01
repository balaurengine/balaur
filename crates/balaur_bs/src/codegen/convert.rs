//! A typed value into an `any` and back, as a function generated for each type
//! that crosses, the way a release is. Each is written as a script would write
//! it, so a field or an element that is itself a struct, a list or a `T?` goes
//! through its own: a struct is a map of its fields, a list a list of its
//! elements, a `T?` its value or nil. Read back, a field the map lacks is its
//! type's zero, as `as_int` and the rest give for nil.

use crate::ast::{Block, Expr, ExprKind, FnDecl, Param, Pos, Stmt, StmtKind, Ty};

/// `fn any::#box(value: T) -> any`, or with `into` false
/// `fn any::#unbox(cell: any) -> T`.
pub(crate) fn function(ty: &Ty, into: bool, fields: &[(String, Ty)]) -> FnDecl {
    let (param, ret) = if into {
        ("value", Ty::Any)
    } else {
        ("cell", ty.clone())
    };
    let from = if into { ty.clone() } else { Ty::Any };
    let body = match (ty, into) {
        (Ty::Struct(class), _) if crate::maps::is_map(class) => map_body(class, into, fields),
        (Ty::Struct(_), true) => struct_into(fields),
        (Ty::Struct(struct_name), false) => struct_from(struct_name, fields),
        (Ty::Array(_), true) => each_into("value", Ty::Array(Box::new(Ty::Any)), "out", true),
        (Ty::Array(_), false) => each_into("cell", ty.clone(), "out", false),
        (Ty::Optional(_), true) => tail(expr(ExprKind::If(
            Box::new(method(var("value"), "is_some", vec![])),
            block(
                vec![bind(
                    "inner",
                    Ty::Any,
                    method(var("value"), "unwrap", vec![]),
                )],
                var("inner"),
            ),
            Some(tail(expr(ExprKind::Nil))),
        ))),
        (Ty::Optional(inner), false) => tail(expr(ExprKind::If(
            Box::new(method(var("cell"), "is_nil", vec![])),
            tail(expr(ExprKind::Nil)),
            Some(block(
                vec![bind("inner", (**inner).clone(), var("cell"))],
                var("inner"),
            )),
        ))),
        _ => unreachable!("only structs, lists and `T?` convert through a function"),
    };
    FnDecl {
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
            .map(|(name, _)| (name.clone(), index(var("cell"), text(name))))
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
            .map(|((name, _), at)| (name.clone(), index(var("cell"), expr(ExprKind::Int(at)))))
            .collect(),
    ));
    let listed = expr(ExprKind::Bin(
        crate::ast::BinOp::Eq,
        Box::new(method(var("cell"), "kind", vec![])),
        Box::new(text("list")),
    ));
    tail(expr(ExprKind::If(
        Box::new(listed),
        tail(by_place),
        Some(tail(by_key)),
    )))
}

// A struct as a map of its fields, each boxed; a `T?` field that is nil is
// left out, as a table that never set it.
fn struct_into(fields: &[(String, Ty)]) -> Block {
    if !fields.iter().any(|(_, ty)| matches!(ty, Ty::Optional(_))) {
        let names = fields.iter().map(|(name, _)| text(name)).collect();
        let values = fields
            .iter()
            .map(|(name, _)| field(var("value"), name))
            .collect();
        return tail(call("__map", vec![list(names), list(values)]));
    }
    let mut stmts = vec![
        bind(
            "keys",
            Ty::Array(Box::new(Ty::Str)),
            expr(ExprKind::ArrayLit(vec![])),
        ),
        bind(
            "values",
            Ty::Array(Box::new(Ty::Any)),
            expr(ExprKind::ArrayLit(vec![])),
        ),
    ];
    for (name, ty) in fields {
        let put = vec![
            stmt(StmtKind::Expr(method(
                var("keys"),
                "push",
                vec![text(name)],
            ))),
            stmt(StmtKind::Expr(method(
                var("values"),
                "push",
                vec![field(var("value"), name)],
            ))),
        ];
        if !matches!(ty, Ty::Optional(_)) {
            stmts.extend(put);
            continue;
        }
        let held = method(field(var("value"), name), "is_some", vec![]);
        stmts.push(stmt(StmtKind::Expr(expr(ExprKind::If(
            Box::new(held),
            Block {
                stmts: put,
                tail: None,
            },
            None,
        )))));
    }
    block(stmts, call("__map", vec![var("keys"), var("values")]))
}

// A `{ str: V }` as an `any` map, its values each boxed, or back: each value
// read as a `V`, the map made with the class's `of`.
fn map_body(class: &str, into: bool, fields: &[(String, Ty)]) -> Block {
    if into {
        let values = method(var("value"), "values", vec![]);
        let boxed = each_into_items(values, Ty::Array(Box::new(Ty::Any)), "boxed");
        let keys = method(var("value"), "keys", vec![]);
        return block(boxed, call("__map", vec![keys, var("boxed")]));
    }
    let values_ty = fields
        .iter()
        .find(|(name, _)| name == "values_in_order")
        .map_or(Ty::Array(Box::new(Ty::Any)), |(_, ty)| ty.clone());
    let mut stmts = vec![
        bind(
            "keys",
            Ty::Array(Box::new(Ty::Str)),
            method(var("cell"), "keys", vec![]),
        ),
        bind("values", values_ty, expr(ExprKind::ArrayLit(vec![]))),
        stmt(StmtKind::ForEach {
            var: "key".into(),
            items: var("keys"),
            body: Block {
                stmts: vec![stmt(StmtKind::Expr(method(
                    var("values"),
                    "push",
                    vec![index(var("cell"), var("key"))],
                )))],
                tail: None,
            },
        }),
    ];
    let made = call(
        &crate::maps::function(class, "of"),
        vec![var("keys"), var("values")],
    );
    stmts.push(bind("made", Ty::Struct(class.to_string()), made));
    block(stmts, var("made"))
}

// `let out: [..] = []; for item in items { out.push(item); }` for `items` an
// expression, each element converted as it is pushed.
fn each_into_items(items: Expr, out_ty: Ty, out: &str) -> Vec<Stmt> {
    vec![
        bind(out, out_ty, expr(ExprKind::ArrayLit(vec![]))),
        stmt(StmtKind::ForEach {
            var: "item".into(),
            items,
            body: Block {
                stmts: vec![stmt(StmtKind::Expr(method(
                    var(out),
                    "push",
                    vec![var("item")],
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
    stmts.push(bind("all", Ty::Any, var(out)));
    block(stmts, var("all"))
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
