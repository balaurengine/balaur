// `enum` and `match`, lowered before any code is made to the struct that
// holds an enum's values and the `if` that tests its tag: ARCHITECTURE.md,
// Scripting, Enums.

use crate::Error;
use crate::ast::{
    Arm, BinOp, Block, EnumDecl, Expr, ExprKind, Field, Part, Pattern, Pos, Program, Stmt,
    StmtKind, Ty, Variant,
};

const TAG: &str = "#tag";

fn slot(variant: &str, field: &str) -> String {
    format!("{variant}#{field}")
}

/// Whether a field of this type is stored as it is, already able to be nil.
fn holds_nil(t: &Ty) -> bool {
    matches!(t, Ty::Optional(_) | Ty::Any)
}

/// The fields of the struct that holds `decl`'s values.
pub(crate) fn holder(decl: &EnumDecl) -> Vec<Field> {
    let mut fields = vec![Field {
        name: TAG.into(),
        ty: Ty::Int,
        public: false,
        default: None,
        pos: decl.pos,
    }];
    for v in &decl.variants {
        for (f, t) in &v.fields {
            let ty = if holds_nil(t) {
                t.clone()
            } else {
                Ty::Optional(Box::new(t.clone()))
            };
            fields.push(Field {
                name: slot(&v.name, f),
                ty,
                public: false,
                default: None,
                pos: v.pos,
            });
        }
    }
    fields
}

fn fail<T>(pos: Pos, msg: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        pos,
        msg: msg.into(),
    })
}

fn at(kind: ExprKind, pos: Pos) -> Expr {
    Expr { kind, pos }
}

// Runs whether or not the program declares an enum: a `match` must never
// reach the code generator.
pub(crate) fn lower(prog: &mut Program) -> Result<(), Error> {
    let enums = std::mem::take(&mut prog.enums);
    for e in &enums {
        check(e)?;
    }
    let mut lowering = Lowering {
        enums: &enums,
        made: 0,
    };
    for (_, fields) in &mut prog.structs {
        for f in fields {
            if let Some(d) = &mut f.default {
                lowering.expr(d)?;
            }
        }
    }
    for (_, e) in &mut prog.consts {
        lowering.expr(e)?;
    }
    for f in &mut prog.fns {
        lowering.block(&mut f.body)?;
    }
    prog.enums = enums;
    Ok(())
}

fn check(e: &EnumDecl) -> Result<(), Error> {
    if e.variants.is_empty() {
        return fail(e.pos, format!("enum {} has no variant", e.name));
    }
    for (i, v) in e.variants.iter().enumerate() {
        if e.variants[..i].iter().any(|w| w.name == v.name) {
            return fail(
                v.pos,
                format!("enum {} has two variants named `{}`", e.name, v.name),
            );
        }
        for (j, (f, _)) in v.fields.iter().enumerate() {
            if v.fields[..j].iter().any(|(g, _)| g == f) {
                return fail(
                    v.pos,
                    format!("`{}::{}` has two fields named `{f}`", e.name, v.name),
                );
            }
        }
    }
    Ok(())
}

struct Lowering<'a> {
    enums: &'a [EnumDecl],
    /// How many `match` subjects have been bound, so each gets its own name.
    made: usize,
}

impl<'e> Lowering<'e> {
    /// The enum and the variant a path such as `Shape::Circle` names.
    fn variant(&self, path: &str) -> Option<(&'e EnumDecl, usize)> {
        let (head, name) = path.rsplit_once("::")?;
        let e = self.enums.iter().find(|e| e.name == head)?;
        let i = e.variants.iter().position(|v| v.name == name)?;
        Some((e, i))
    }

    fn block(&mut self, b: &mut Block) -> Result<(), Error> {
        let mut out = Vec::with_capacity(b.stmts.len());
        for mut s in std::mem::take(&mut b.stmts) {
            self.stmt(&mut s)?;
            // A `match` in a statement's place runs as statements, so the
            // async lowering sees the `if` it is and any `await` in an arm.
            match s.kind {
                StmtKind::Expr(Expr {
                    kind: ExprKind::Block(inner),
                    pos,
                }) => {
                    out.extend(inner.stmts);
                    if let Some(t) = inner.tail {
                        out.push(Stmt {
                            kind: StmtKind::Expr(*t),
                            pos,
                        });
                    }
                }
                _ => out.push(s),
            }
        }
        b.stmts = out;
        if let Some(t) = &mut b.tail {
            self.expr(t)?;
        }
        // A `match` that is the block's value moves into the block the same
        // way: its `let`, then the `if` as the block's value.
        if let Some(t) = b.tail.take() {
            match t.kind {
                ExprKind::Block(inner) => {
                    b.stmts.extend(inner.stmts);
                    b.tail = inner.tail;
                }
                kind => {
                    b.tail = Some(Box::new(Expr { kind, pos: t.pos }));
                }
            }
        }
        Ok(())
    }

    fn stmt(&mut self, s: &mut Stmt) -> Result<(), Error> {
        match &mut s.kind {
            StmtKind::Let { init, .. } => self.expr(init),
            StmtKind::Assign { target, value, .. } => {
                self.expr(target)?;
                self.expr(value)
            }
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) => self.expr(e),
            StmtKind::While { cond, body } => {
                self.expr(cond)?;
                self.block(body)
            }
            StmtKind::Loop(body) => self.block(body),
            StmtKind::ForRange { from, to, body, .. } => {
                self.expr(from)?;
                self.expr(to)?;
                self.block(body)
            }
            StmtKind::ForEach { items, body, .. } => {
                self.expr(items)?;
                self.block(body)
            }
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => Ok(()),
        }
    }

    fn expr(&mut self, e: &mut Expr) -> Result<(), Error> {
        let pos = e.pos;
        match &mut e.kind {
            ExprKind::Call(name, args) => {
                for a in args.iter_mut() {
                    self.expr(a)?;
                }
                if let Some(made) = self.called(name, args, pos)? {
                    e.kind = made;
                }
            }
            ExprKind::Var(name) => {
                if let Some((decl, i)) = self.variant(name) {
                    let v = &decl.variants[i];
                    if !v.fields.is_empty() {
                        let shape = if v.tuple { "(..)" } else { " { .. }" };
                        return fail(pos, format!("`{name}` carries values: `{name}{shape}`"));
                    }
                    e.kind = make(decl, i, vec![], pos);
                }
            }
            ExprKind::StructLit(name, fields) => {
                for (_, f) in fields.iter_mut() {
                    self.expr(f)?;
                }
                if let Some(made) = self.literal(name, fields, pos)? {
                    e.kind = made;
                }
            }
            ExprKind::Match(subject, arms) => {
                self.expr(subject)?;
                for arm in arms.iter_mut() {
                    self.expr(&mut arm.body)?;
                }
                let lowered = self.lower_match(subject, std::mem::take(arms), pos)?;
                e.kind = lowered;
            }
            ExprKind::Block(b) => self.block(b)?,
            ExprKind::If(c, then, els) => {
                self.expr(c)?;
                self.block(then)?;
                if let Some(b) = els {
                    self.block(b)?;
                }
            }
            ExprKind::Interp(parts) => {
                for p in parts {
                    if let Part::Value(v) = p {
                        self.expr(v)?;
                    }
                }
            }
            ExprKind::Bin(_, a, b) | ExprKind::Index(a, b) | ExprKind::ArrayRepeat(a, b) => {
                self.expr(a)?;
                self.expr(b)?;
            }
            ExprKind::Unary(_, a)
            | ExprKind::Field(a, _)
            | ExprKind::Await(a)
            | ExprKind::Cast(a, _) => self.expr(a)?,
            ExprKind::Method(base, _, args) => {
                self.expr(base)?;
                for a in args {
                    self.expr(a)?;
                }
            }
            ExprKind::ArrayLit(items) => {
                for i in items {
                    self.expr(i)?;
                }
            }
            ExprKind::MapLit(entries) => {
                for (k, v) in entries {
                    self.expr(k)?;
                    self.expr(v)?;
                }
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::Nil
            | ExprKind::SelfRef
            | ExprKind::Resumed(..) => {}
        }
        Ok(())
    }

    // `Shape::Circle(2.0)`: the variant made from its values by position.
    fn called(
        &self,
        name: &str,
        args: &mut Vec<Expr>,
        pos: Pos,
    ) -> Result<Option<ExprKind>, Error> {
        let Some((decl, i)) = self.variant(name) else {
            return Ok(None);
        };
        let v = &decl.variants[i];
        if v.fields.is_empty() {
            return fail(pos, format!("`{name}` carries nothing: write `{name}`"));
        }
        if !v.tuple {
            return fail(
                pos,
                format!("`{name}` has named fields: write `{name} {{ .. }}`"),
            );
        }
        if args.len() != v.fields.len() {
            return fail(
                pos,
                format!(
                    "`{name}` takes {} values, given {}",
                    v.fields.len(),
                    args.len()
                ),
            );
        }
        let given = v
            .fields
            .iter()
            .map(|(f, _)| f.clone())
            .zip(std::mem::take(args));
        Ok(Some(make(decl, i, given.collect(), pos)))
    }

    // `Shape::Rect { w: 1.0, h: 2.0 }`: the variant made from named values.
    fn literal(
        &self,
        name: &str,
        fields: &mut Vec<(String, Expr)>,
        pos: Pos,
    ) -> Result<Option<ExprKind>, Error> {
        if self.enums.iter().any(|d| d.name == name) {
            return fail(
                pos,
                format!("`{name}` is an enum: make one with `{name}::Variant`"),
            );
        }
        let Some((decl, i)) = self.variant(name) else {
            return Ok(None);
        };
        let v = &decl.variants[i];
        if v.fields.is_empty() {
            return fail(pos, format!("`{name}` carries nothing: write `{name}`"));
        }
        if v.tuple {
            return fail(
                pos,
                format!("`{name}` holds values by position: write `{name}(..)`"),
            );
        }
        if let Some((f, _)) = fields
            .iter()
            .find(|(f, _)| !v.fields.iter().any(|(g, _)| g == f))
        {
            return fail(pos, format!("`{name}` has no field `{f}`"));
        }
        if let Some((missing, _)) = v
            .fields
            .iter()
            .find(|(f, _)| !fields.iter().any(|(g, _)| g == f))
        {
            return fail(pos, format!("missing field `{missing}` of `{name}`"));
        }
        Ok(Some(make(decl, i, std::mem::take(fields), pos)))
    }

    // `{ let match#n: E = subject; if match#n.#tag == 0 { .. } else .. }`,
    // the last arm the `else` once every variant is named.
    fn lower_match(&mut self, subject: &Expr, arms: Vec<Arm>, pos: Pos) -> Result<ExprKind, Error> {
        if arms.is_empty() {
            return fail(pos, "a `match` needs an arm");
        }
        let mut decl: Option<&EnumDecl> = None;
        let mut covered: Vec<usize> = Vec::new();
        let mut cases: Vec<(Option<usize>, Arm)> = Vec::new();
        let count = arms.len();
        for (n, arm) in arms.into_iter().enumerate() {
            let tag = match &arm.pattern {
                Pattern::Any | Pattern::Bind(_) => {
                    if n + 1 != count {
                        return fail(
                            arm.pos,
                            "an arm after one that takes any value is never reached",
                        );
                    }
                    None
                }
                Pattern::Variant { path, binds } => {
                    let Some((d, i)) = self.variant(path) else {
                        return fail(arm.pos, format!("`{path}` is no enum variant"));
                    };
                    if decl.is_some_and(|known| known.name != d.name) {
                        return fail(
                            arm.pos,
                            format!(
                                "`{path}` is not a `{}`",
                                decl.map_or("", |k| k.name.as_str())
                            ),
                        );
                    }
                    decl = Some(d);
                    if covered.contains(&i) {
                        return fail(arm.pos, format!("`{path}` is matched twice"));
                    }
                    covered.push(i);
                    check_binds(&d.variants[i], path, binds, arm.pos)?;
                    Some(i)
                }
            };
            cases.push((tag, arm));
        }
        let wildcard = cases.last().is_some_and(|(tag, _)| tag.is_none());
        let Some(decl) = decl else {
            // One arm that takes anything: the subject is still worked out
            // once, for what it does, and bound when the arm names it.
            let (_, arm) = cases.pop().expect("a match has an arm");
            let name = match &arm.pattern {
                Pattern::Bind(name) => name.clone(),
                _ => "_".into(),
            };
            return Ok(ExprKind::Block(Block {
                stmts: vec![bind_stmt(&name, None, subject.clone())],
                tail: Some(Box::new(arm.body)),
            }));
        };
        if !wildcard && covered.len() < decl.variants.len() {
            let missing: Vec<String> = decl
                .variants
                .iter()
                .enumerate()
                .filter(|(i, _)| !covered.contains(i))
                .map(|(_, v)| format!("`{}::{}`", decl.name, v.name))
                .collect();
            return fail(pos, format!("this `match` misses {}", missing.join(", ")));
        }
        let held = format!("match#{}", self.made);
        self.made += 1;
        // Built from the last arm up, as the parser builds `else if`: the
        // last arm is the final `else` block, each earlier one an `if` in
        // the `else` of the one before.
        let mut otherwise: Option<Block> = None;
        let mut top: Option<Expr> = None;
        for (tag, arm) in cases.into_iter().rev() {
            let body = arm_block(decl, tag, &held, arm.body, &arm.pattern);
            let (Some(rest), Some(i)) = (otherwise.take(), tag) else {
                top = Some(at(ExprKind::Block(body.clone()), arm.pos));
                otherwise = Some(body);
                continue;
            };
            let read = at(ExprKind::Var(held.clone()), arm.pos);
            let test = at(
                ExprKind::Bin(
                    BinOp::Eq,
                    Box::new(at(ExprKind::Field(Box::new(read), TAG.into()), arm.pos)),
                    Box::new(at(ExprKind::Int(i as i64), arm.pos)),
                ),
                arm.pos,
            );
            let branch = at(ExprKind::If(Box::new(test), body, Some(rest)), arm.pos);
            otherwise = Some(Block {
                stmts: vec![],
                tail: Some(Box::new(branch.clone())),
            });
            top = Some(branch);
        }
        Ok(ExprKind::Block(Block {
            stmts: vec![bind_stmt(
                &held,
                Some(Ty::Struct(decl.name.clone())),
                subject.clone(),
            )],
            tail: top.map(Box::new),
        }))
    }
}

fn check_binds(v: &Variant, path: &str, binds: &[(String, String)], pos: Pos) -> Result<(), Error> {
    if v.tuple {
        if binds.len() != v.fields.len() {
            return fail(
                pos,
                format!(
                    "`{path}` holds {} values, bound {}",
                    v.fields.len(),
                    binds.len()
                ),
            );
        }
        return Ok(());
    }
    for (f, _) in binds {
        if !v.fields.iter().any(|(g, _)| g == f) {
            return fail(pos, format!("`{path}` has no field `{f}`"));
        }
    }
    Ok(())
}

fn bind_stmt(name: &str, ty: Option<Ty>, init: Expr) -> Stmt {
    let pos = init.pos;
    Stmt {
        kind: StmtKind::Let {
            name: name.into(),
            ty,
            init,
        },
        pos,
    }
}

// An arm's body, after a `let` for each field it binds.
fn arm_block(
    decl: &EnumDecl,
    tag: Option<usize>,
    held: &str,
    body: Expr,
    pattern: &Pattern,
) -> Block {
    let mut stmts = Vec::new();
    if let Pattern::Bind(name) = pattern {
        let whole = at(ExprKind::Var(held.into()), body.pos);
        stmts.push(bind_stmt(name, Some(Ty::Struct(decl.name.clone())), whole));
    }
    if let (Some(i), Pattern::Variant { binds, .. }) = (tag, pattern) {
        let v = &decl.variants[i];
        for (field, name) in binds {
            if name == "_" {
                continue;
            }
            let ty = v
                .fields
                .iter()
                .find(|(f, _)| f == field)
                .map(|(_, t)| t.clone());
            let read = at(
                ExprKind::Field(
                    Box::new(at(ExprKind::Var(held.into()), body.pos)),
                    slot(&v.name, field),
                ),
                body.pos,
            );
            let value = match &ty {
                Some(t) if holds_nil(t) => read,
                _ => at(
                    ExprKind::Method(Box::new(read), "unwrap".into(), vec![]),
                    body.pos,
                ),
            };
            stmts.push(bind_stmt(name, ty, value));
        }
    }
    match body.kind {
        ExprKind::Block(b) if stmts.is_empty() => b,
        ExprKind::Block(b) => {
            stmts.extend(b.stmts);
            Block {
                stmts,
                tail: b.tail,
            }
        }
        _ => Block {
            stmts,
            tail: Some(Box::new(body)),
        },
    }
}

// `decl`'s struct with variant `i` and `given`, its fields by name.
// The other variants' optional fields are left out, as nil; an `any` has no
// optional, so it is named, as nil.
fn make(decl: &EnumDecl, i: usize, given: Vec<(String, Expr)>, pos: Pos) -> ExprKind {
    let v = &decl.variants[i];
    let mut fields = vec![(TAG.to_string(), at(ExprKind::Int(i as i64), pos))];
    fields.extend(given.into_iter().map(|(f, e)| (slot(&v.name, &f), e)));
    for (j, other) in decl.variants.iter().enumerate() {
        for (f, t) in &other.fields {
            if j != i && *t == Ty::Any {
                fields.push((slot(&other.name, f), at(ExprKind::Nil, pos)));
            }
        }
    }
    ExprKind::StructLit(decl.name.clone(), fields)
}
