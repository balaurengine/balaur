// A counted loop that indexes arrays by its own variable runs a copy without
// bounds checks once one test at the top puts every index in range: on an
// interpreter the check and the header loads are most of an access.

use super::func::{FnGen, LVal, R};
use crate::ast::{Block, Expr, ExprKind, Part, Stmt, StmtKind, Ty};
use wasm_encoder::{BlockType, Instruction as I, ValType};

/// Inside an unchecked copy: the loop variable, each array's data pointer,
/// and the variable times each element size, worked out once an iteration.
pub(crate) struct Fast {
    pub var: u32,
    pub arrays: Vec<(u32, u32)>,
    pub scaled: Vec<(i32, u32)>,
}

const PURE: [&str; 11] = [
    "sqrt", "abs", "floor", "ceil", "min", "max", "vec2", "vec3", "color", "nil", "map",
];

/// What a body does that decides whether its arrays can be hoisted.
struct Scan<'f, 'g> {
    fg: &'f FnGen<'g>,
    var: String,
    var_slot: u32,
    arrays: Vec<(u32, Ty)>,
    roots: Vec<String>,
    bound: Vec<String>,
    ok: bool,
}

impl Scan<'_, '_> {
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
            StmtKind::Let { name, init, .. } => {
                self.bound.push(name.clone());
                self.expr(init);
            }
            StmtKind::Assign { target, value, .. } => {
                self.assign(target);
                self.expr(value);
            }
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) => self.expr(e),
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::Loop(body) => self.block(body),
            StmtKind::ForRange {
                var,
                from,
                to,
                body,
            } => {
                self.bound.push(var.clone());
                self.expr(from);
                self.expr(to);
                self.block(body);
            }
            StmtKind::ForEach { var, items, body } => {
                self.bound.push(var.clone());
                self.expr(items);
                self.block(body);
            }
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }

    // Writing the loop variable or an array itself (not one of its elements)
    // could move what the check vouched for.
    fn assign(&mut self, target: &Expr) {
        match self.fg.place(target) {
            Ok(Some(lv)) => {
                let hits = |s: &u32| *s == self.var_slot || self.arrays.iter().any(|(a, _)| a == s);
                if lv.slots.iter().any(hits) || matches!(lv.ty, Ty::Array(_) | Ty::Struct(_)) {
                    self.ok = false;
                }
            }
            Ok(None) => self.expr(target),
            Err(_) => self.ok = false,
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            // A call could resize an array through another reference to it.
            ExprKind::Call(name, args) => {
                if !PURE.contains(&name.as_str()) && !self.fg.g.imp.math.contains_key(name.as_str())
                {
                    self.ok = false;
                }
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Method(base, name, args) => {
                if name != "len" || !args.is_empty() {
                    self.ok = false;
                }
                self.expr(base);
            }
            ExprKind::Await(_) | ExprKind::Resumed(..) => self.ok = false,
            ExprKind::Index(arr, idx) => {
                if matches!(&idx.kind, ExprKind::Var(v) if *v == self.var)
                    && let Ok(Some(LVal {
                        ty: Ty::Array(et),
                        slots,
                    })) = self.fg.place(arr)
                {
                    if !self.arrays.iter().any(|(a, _)| *a == slots[0]) {
                        self.arrays.push((slots[0], *et));
                    }
                    if let Some(root) = root(arr) {
                        self.roots.push(root);
                    }
                }
                self.expr(arr);
                self.expr(idx);
            }
            ExprKind::Bin(_, a, b) | ExprKind::ArrayRepeat(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::Unary(_, a) | ExprKind::Cast(a, _) | ExprKind::Field(a, _) => self.expr(a),
            ExprKind::Interp(parts) => {
                for p in parts {
                    if let Part::Value(v) = p {
                        self.expr(v);
                    }
                }
            }
            ExprKind::StructLit(_, fields) => fields.iter().for_each(|(_, v)| self.expr(v)),
            ExprKind::ArrayLit(items) => items.iter().for_each(|v| self.expr(v)),
            ExprKind::MapLit(entries) => entries.iter().for_each(|(key, value)| {
                self.expr(key);
                self.expr(value);
            }),
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
            | ExprKind::Nil
            | ExprKind::Str(_)
            | ExprKind::Var(_)
            | ExprKind::SelfRef => {}
        }
    }
}

/// The variable an array expression starts from: `xs` in `xs`, `self` in `self.xs`.
fn root(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Var(n) => Some(n.clone()),
        ExprKind::SelfRef => Some("self".into()),
        ExprKind::Field(b, _) => root(b),
        _ => None,
    }
}

impl FnGen<'_> {
    /// The arrays `body` indexes by the loop variable `i`, when nothing in it
    /// can resize one. A debug build keeps one copy of each statement.
    fn hoistable(&self, var: &str, i: &LVal, body: &Block) -> Vec<(u32, i32)> {
        if self.g.imp.dbg_probe != u32::MAX {
            return vec![];
        }
        let mut scan = Scan {
            fg: self,
            var: var.to_string(),
            var_slot: i.slots[0],
            arrays: vec![],
            roots: vec![],
            bound: vec![],
            ok: true,
        };
        scan.block(body);
        // A name bound inside the body could shadow the loop variable or an
        // array between the scan and the access.
        let shadowed = scan
            .bound
            .iter()
            .any(|n| *n == scan.var || scan.roots.contains(n));
        if !scan.ok || shadowed {
            return vec![];
        }
        scan.arrays
            .into_iter()
            .map(|(slot, et)| (slot, self.g.elem_size(&et) as i32))
            .collect()
    }

    /// `for i in from..to { body }` with `i` and `end` set: the unchecked copy
    /// behind a test that every hoisted array is at least `end` long.
    pub(crate) fn counted_loop(&mut self, var: &str, i: &LVal, end: u32, body: &Block) -> R<()> {
        let arrays = self.hoistable(var, i, body);
        if arrays.is_empty() {
            return self.count(i, end, body);
        }
        self.emit([I::LocalGet(i.slots[0]), I::I64Const(0), I::I64GeS]);
        for (a, _) in &arrays {
            self.emit([
                I::LocalGet(end),
                I::LocalGet(*a),
                I::I32Load(super::rt::mem(4, 2)),
                I::I64ExtendI32U,
                I::I64LeS,
                I::I32And,
            ]);
        }
        self.open(I::If(BlockType::Empty));
        let mut fast = Fast {
            var: i.slots[0],
            arrays: vec![],
            scaled: vec![],
        };
        for (a, esz) in arrays {
            let data = self.local(ValType::I32);
            self.emit([
                I::LocalGet(a),
                I::I32Load(super::rt::mem(12, 2)),
                I::LocalSet(data),
            ]);
            fast.arrays.push((a, data));
            if !fast.scaled.iter().any(|(e, _)| *e == esz) {
                let l = self.local(ValType::I32);
                fast.scaled.push((esz, l));
            }
        }
        self.fast.push(fast);
        let unchecked = self.count(i, end, body);
        self.fast.pop();
        unchecked?;
        self.e(I::Else);
        self.count(i, end, body)?;
        self.close();
        Ok(())
    }

    /// At the top of an unchecked iteration: the variable times each size.
    pub(crate) fn scale_index(&mut self) {
        let Some(fast) = self.fast.last() else {
            return;
        };
        let (var, scaled) = (fast.var, fast.scaled.clone());
        for (esz, l) in scaled {
            self.emit([
                I::LocalGet(var),
                I::I32WrapI64,
                I::I32Const(esz),
                I::I32Mul,
                I::LocalSet(l),
            ]);
        }
    }

    /// `arr[idx]`'s data pointer and scaled index, inside an unchecked copy.
    pub(crate) fn fast_element(&self, arr: &Expr, idx: &Expr) -> Option<(u32, u32)> {
        let fast = self.fast.last()?;
        let Ok(Some(i)) = self.place(idx) else {
            return None;
        };
        let Ok(Some(a)) = self.place(arr) else {
            return None;
        };
        if i.slots[0] != fast.var {
            return None;
        }
        let Ty::Array(et) = &a.ty else {
            return None;
        };
        let esz = self.g.elem_size(et) as i32;
        let data = fast.arrays.iter().find(|(slot, _)| *slot == a.slots[0])?.1;
        let scaled = fast.scaled.iter().find(|(e, _)| *e == esz)?.1;
        Some((data, scaled))
    }
}
