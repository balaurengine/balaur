// `T?`: a `T`'s slots, then an i32 that says whether it is there. A nil one
// has zero slots, so releasing it releases nothing.

use super::func::{FnGen, LVal, R, err, show};
use crate::ast::{BinOp, Block, Expr, Pos, Ty};
use wasm_encoder::{BlockType, Instruction as I, ValType};

pub(crate) const METHODS: [&str; 4] = ["is_some", "is_none", "unwrap", "or"];

fn zero(vt: ValType) -> I<'static> {
    match vt {
        ValType::I64 => I::I64Const(0),
        ValType::F64 => I::F64Const(0.0.into()),
        _ => I::I32Const(0),
    }
}

impl FnGen<'_> {
    /// `nil` where `want` is wanted: a `T?` with nothing in it, or an empty `any`.
    pub(crate) fn nil_as(&mut self, want: &Ty, pos: Pos) -> R<()> {
        match want {
            Ty::Optional(t) => {
                for (vt, _) in self.g.slots(t) {
                    self.e(zero(vt));
                }
                self.e(I::I32Const(0));
                Ok(())
            }
            Ty::Any => {
                self.e(I::I32Const(0));
                Ok(())
            }
            // No node: the engine's own, which `is_valid` says false of.
            Ty::Node => {
                self.e(I::I64Const(0));
                Ok(())
            }
            _ => err(
                pos,
                format!(
                    "`nil` goes where a `T?`, an `any` or a `Node` is wanted, not a {}",
                    show(want)
                ),
            ),
        }
    }

    /// A block whose value goes where `want` is wanted, so `nil` and plain
    /// values become the `T?` it asks for.
    pub(crate) fn block_as(&mut self, b: &Block, want: &Ty) -> R<Ty> {
        for s in &b.stmts {
            self.stmt(s)?;
        }
        match &b.tail {
            Some(t) => {
                self.probe(t.pos);
                self.expr_as(t, want)?;
                Ok(want.clone())
            }
            None => Ok(Ty::Void),
        }
    }

    /// `if c { a } else { b }` where a `T?` is wanted: each branch is one.
    pub(crate) fn if_as(
        &mut self,
        c: &Expr,
        then: &Block,
        els: Option<&Block>,
        want: &Ty,
        pos: Pos,
    ) -> R<()> {
        let Some(els) = els else {
            return err(pos, "an `if` without `else` has no value");
        };
        self.expr_as(c, &Ty::Bool)?;
        self.depth += 1;
        let mut arms = vec![];
        for b in [then, els] {
            let mark = self.code.len();
            self.scopes.push(vec![]);
            let t = self.block_as(b, want)?;
            self.scopes.pop();
            if t == Ty::Void {
                return err(
                    pos,
                    format!("each branch of this `if` needs a {}", show(want)),
                );
            }
            arms.push(self.code.drain(mark..).collect::<Vec<_>>());
        }
        self.depth -= 1;
        let flat: Vec<ValType> = self.g.slots(want).into_iter().map(|(v, _)| v).collect();
        let bt = BlockType::FunctionType(self.g.type_idx(vec![], flat));
        self.e(I::If(bt));
        self.code.extend(arms.remove(0));
        self.e(I::Else);
        self.code.extend(arms.remove(0));
        self.e(I::End);
        Ok(())
    }

    // The optional on the stack, held in a temporary while it is looked at.
    fn hold(&mut self, ty: &Ty) -> (LVal, u32) {
        let t = self.temp(ty);
        self.set_raw(&t);
        let flag = *t.slots.last().expect("an optional has its flag");
        (t, flag)
    }

    /// `x == nil` and `x != nil`: whether it is empty, or there.
    pub(crate) fn nil_compare(&mut self, e: &Expr, op: BinOp, pos: Pos) -> R<Ty> {
        let ty = self.expr(e)?;
        match &ty {
            Ty::Optional(_) => {
                let (t, flag) = self.hold(&ty);
                self.e(I::LocalGet(flag));
                if op == BinOp::Eq {
                    self.e(I::I32Eqz);
                }
                self.release(&t);
            }
            Ty::Any => {
                self.e(I::I32Eqz);
                if op == BinOp::Ne {
                    self.e(I::I32Eqz);
                }
            }
            _ => {
                return err(
                    pos,
                    format!("a {} is never nil; only a `T?` or an `any` is", show(&ty)),
                );
            }
        }
        Ok(Ty::Bool)
    }

    pub(crate) fn optional_method(
        &mut self,
        base: &Expr,
        inner: &Ty,
        name: &str,
        args: &[Expr],
        pos: Pos,
    ) -> R<Ty> {
        let ty = Ty::Optional(Box::new(inner.clone()));
        // Rust's name for `or`.
        let name = if name == "unwrap_or" { "or" } else { name };
        match name {
            "is_some" | "is_none" => {
                self.args(&[], args, pos, name)?;
                self.expr(base)?;
                let (t, flag) = self.hold(&ty);
                self.e(I::LocalGet(flag));
                if name == "is_none" {
                    self.e(I::I32Eqz);
                }
                self.release(&t);
                Ok(Ty::Bool)
            }
            // A nil one traps, at its line in a debug build.
            "unwrap" => {
                self.args(&[], args, pos, name)?;
                self.expr(base)?;
                let (t, flag) = self.hold(&ty);
                self.emit([
                    I::LocalGet(flag),
                    I::I32Eqz,
                    I::If(BlockType::Empty),
                    I::Unreachable,
                    I::End,
                ]);
                self.take(&t, inner);
                Ok(inner.clone())
            }
            "or" => {
                let [default] = args else {
                    return err(pos, "`or` takes the value to use when it is nil");
                };
                self.expr(base)?;
                let (t, flag) = self.hold(&ty);
                self.expr_as(default, inner)?;
                let d = self.temp(inner);
                self.set_raw(&d);
                // There: the default is not needed. Nil: its slots are zero.
                self.e(I::LocalGet(flag));
                self.e(I::If(BlockType::Empty));
                self.release(&d);
                self.e(I::End);
                let n = d.slots.len();
                for j in 0..n {
                    self.emit([
                        I::LocalGet(t.slots[j]),
                        I::LocalGet(d.slots[j]),
                        I::LocalGet(flag),
                        I::Select,
                    ]);
                }
                self.forget(&t, inner);
                self.forget(&d, inner);
                Ok(inner.clone())
            }
            _ => err(
                pos,
                format!(
                    "this may be nil: check `{name}` first; a {} has `{}`",
                    show(&ty),
                    METHODS.join("`, `")
                ),
            ),
        }
    }

    // The value inside `t`, handed on: its slots pushed, then cleared so the
    // frame does not release what now belongs to the stack.
    fn take(&mut self, t: &LVal, inner: &Ty) {
        let n = self.g.slots(inner).len();
        for &s in &t.slots[..n] {
            self.e(I::LocalGet(s));
        }
        self.forget(t, inner);
    }

    fn forget(&mut self, t: &LVal, inner: &Ty) {
        for (&s, (vt, heap)) in t.slots.iter().zip(self.g.slots(inner)) {
            if heap.is_some() {
                self.emit([zero(vt), I::LocalSet(s)]);
            }
        }
    }
}
