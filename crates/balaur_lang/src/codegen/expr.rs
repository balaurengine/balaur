use super::Site;
use super::func::{FnGen, LVal, Mode, R, err, op_text, show};
use super::rt::{load, mem, store};
use crate::Error;
use crate::ast::{BinOp, Block, Expr, ExprKind, Part, Pos, Ty, UnOp};
use wasm_encoder::{BlockType, Instruction as I, ValType};

impl FnGen<'_> {
    pub(crate) fn expr(&mut self, e: &Expr) -> R<Ty> {
        Ok(match &e.kind {
            ExprKind::Int(v) => {
                self.e(I::I64Const(*v));
                Ty::Int
            }
            ExprKind::Float(v) => {
                self.e(I::F64Const((*v).into()));
                Ty::Float
            }
            ExprKind::Bool(b) => {
                self.e(I::I32Const(i32::from(*b)));
                Ty::Bool
            }
            ExprKind::Str(s) => {
                let p = self.g.string(s);
                self.e(I::I32Const(p as i32));
                Ty::Str
            }
            ExprKind::Interp(parts) => {
                self.interp(parts)?;
                Ty::Str
            }
            ExprKind::Var(n) if self.lookup(n).is_none() && self.g.consts.contains_key(n) => {
                let c = self.g.consts[n].clone();
                self.expr(&c)?
            }
            ExprKind::Index(arr, idx) if self.type_of(arr).ok() == Some(Ty::Any) => {
                let by = if self.type_of(idx)? == Ty::Str {
                    "get"
                } else {
                    "at"
                };
                self.any_method(arr, by, std::slice::from_ref(&**idx), e.pos)?
            }
            ExprKind::Var(_) | ExprKind::SelfRef | ExprKind::Field(..) | ExprKind::Index(..) => {
                self.read(e)?
            }
            ExprKind::Bin(op, a, b) => self.binary(*op, a, b, e.pos)?,
            ExprKind::Unary(op, a) => self.unary(*op, a, e.pos)?,
            ExprKind::Call(name, args) => self.call(name, args, e.pos)?,
            ExprKind::Method(base, name, args) => self.method(base, name, args, e.pos)?,
            ExprKind::StructLit(s, fields) => self.struct_lit(s, fields, e.pos)?,
            ExprKind::ArrayLit(items) => self.array_lit(items, None, e.pos)?,
            ExprKind::ArrayRepeat(v, n) => self.array_repeat(v, n)?,
            ExprKind::If(c, then, els) => self.if_expr(c, then, els.as_ref(), e.pos)?,
            ExprKind::Cast(a, to) => self.cast(a, to, e.pos)?,
            ExprKind::Await(_) => {
                return err(
                    e.pos,
                    "`await` goes in an `async fn`, as a statement, a `let` or an assignment",
                );
            }
            ExprKind::Resumed(ty, site) => self.resumed(ty, *site, e.pos)?,
        })
    }

    // A variable, a field or an element: a retained copy of what is there.
    fn read(&mut self, e: &Expr) -> R<Ty> {
        if let Some((node, get, _, ty)) = self.component_access(e)? {
            self.expr(&node)?;
            self.host_call(&get);
            return Ok(ty);
        }
        if let Some(lv) = self.place(e)? {
            self.get_owned(&lv);
            return Ok(lv.ty);
        }
        if let Some(mp) = self.mem_place(e)? {
            self.mem_read(&mp);
            if let Some(r) = &mp.release {
                self.release(r);
            }
            return Ok(mp.ty);
        }
        let ExprKind::Field(base, f) = &e.kind else {
            return err(e.pos, "this cannot be read");
        };
        let bt = self.expr(base)?;
        let tmp = self.temp(&bt);
        self.set_raw(&tmp);
        let lv = self.field_of(&tmp, f, e.pos)?;
        self.get_owned(&lv);
        self.release(&tmp);
        Ok(lv.ty)
    }

    fn unary(&mut self, op: UnOp, a: &Expr, pos: Pos) -> R<Ty> {
        let mark = self.code.len();
        let t = self.expr(a)?;
        match (op, &t) {
            (UnOp::Neg, Ty::Int) => {
                self.code.insert(mark, I::I64Const(0));
                self.e(I::I64Sub);
            }
            (UnOp::Neg, Ty::Float) => self.e(I::F64Neg),
            (UnOp::Not, Ty::Bool) => self.e(I::I32Eqz),
            (UnOp::Neg, _) => return err(pos, format!("cannot negate {}", show(&t))),
            (UnOp::Not, _) => return err(pos, format!("`!` needs a bool, got {}", show(&t))),
        }
        Ok(t)
    }

    fn struct_lit(&mut self, s: &str, fields: &[(String, Expr)], pos: Pos) -> R<Ty> {
        let decl = self.g.structs.get(s).cloned().ok_or(Error {
            pos,
            msg: format!("unknown struct `{s}`"),
        })?;
        if let Some((f, _)) = fields
            .iter()
            .find(|(f, _)| !decl.iter().any(|(n, _)| n == f))
        {
            return err(pos, format!("struct {s} has no field `{f}`"));
        }
        for (f, t) in &decl {
            let (_, fe) = fields.iter().find(|(n, _)| n == f).ok_or(Error {
                pos,
                msg: format!("missing field `{f}` of {s}"),
            })?;
            self.expr_as(fe, t)?;
        }
        Ok(Ty::Struct(s.to_string()))
    }

    fn cast(&mut self, a: &Expr, to: &Ty, pos: Pos) -> R<Ty> {
        let from = self.expr(a)?;
        match (&from, to) {
            (x, y) if x == y => {}
            (Ty::Int, Ty::Float) => self.e(I::F64ConvertI64S),
            (Ty::Float, Ty::Int) => self.e(I::I64TruncSatF64S),
            (Ty::Bool, Ty::Int) => self.e(I::I64ExtendI32U),
            _ => return err(pos, format!("cannot cast {} as {}", show(&from), show(to))),
        }
        Ok(to.clone())
    }

    // What a finished `await` gives back: the host's resolved value, or the result
    // the child fn left in its frame, which is freed once read.
    fn resumed(&mut self, ty: &Ty, site: u32, pos: Pos) -> R<Ty> {
        match self.sites.get(&site).cloned() {
            Some(Site::Host) => {
                let f = match ty {
                    Ty::Int | Ty::Node => self.g.imp.await_i64,
                    Ty::Float => self.g.imp.await_f64,
                    Ty::Bool => self.g.imp.await_i32,
                    Ty::Void => return Ok(Ty::Void),
                    _ => {
                        return err(
                            pos,
                            format!("a host `await` cannot return {} yet", show(ty)),
                        );
                    }
                };
                let Mode::Poll { handle, .. } = self.mode else {
                    return err(pos, "`await` outside an async fn");
                };
                self.emit([I::LocalGet(handle), I::Call(f)]);
            }
            Some(Site::Script { child }) => {
                for (j, (vt, _)) in self.g.slots(ty).into_iter().enumerate() {
                    self.emit([I::LocalGet(child), load(vt, 8 + 8 * j as u64)]);
                }
                self.emit([I::LocalGet(child), I::Call(self.g.h.free)]);
            }
            None => return err(pos, "internal: an await site was never started"),
        }
        Ok(ty.clone())
    }

    fn array_repeat(&mut self, v: &Expr, n: &Expr) -> R<Ty> {
        let nt = self.expr(n)?;
        if nt != Ty::Int {
            return err(n.pos, format!("an array length is int, not {}", show(&nt)));
        }
        self.e(I::I32WrapI64);
        let count = self.local(ValType::I32);
        self.e(I::LocalTee(count));
        let et = self.expr(v)?;
        let val = self.alloc(&et);
        self.set_raw(&val);
        let esz = self.g.elem_size(&et) as i32;
        self.emit([I::I32Const(esz), I::Call(self.g.h.arr_new)]);
        let arr = self.local(ValType::I32);
        self.e(I::LocalSet(arr));
        let i = self.local(ValType::I32);
        let brk = self.open(I::Block(BlockType::Empty));
        let top = self.open(I::Loop(BlockType::Empty));
        self.emit([
            I::LocalGet(i),
            I::LocalGet(count),
            I::I32GeU,
            I::BrIf(self.depth - brk),
        ]);
        for (j, (vt, heap)) in self.g.slots(&et).into_iter().enumerate() {
            self.emit([
                I::LocalGet(arr),
                I::I32Load(mem(12, 2)),
                I::LocalGet(i),
                I::I32Const(esz),
                I::I32Mul,
                I::I32Add,
                I::LocalGet(val.slots[j]),
            ]);
            if heap.is_some() {
                self.e(I::Call(self.g.h.retain));
            }
            self.e(store(vt, 8 * j as u64));
        }
        self.emit([I::LocalGet(i), I::I32Const(1), I::I32Add, I::LocalSet(i)]);
        self.br(top);
        self.close();
        self.close();
        // Each copy took a reference; the value gives its own back.
        for (s, (_, heap)) in val.slots.clone().into_iter().zip(self.g.slots(&et)) {
            if let Some(h) = heap {
                self.emit([I::LocalGet(s), I::Call(self.g.dec_fn(&h))]);
            }
        }
        self.e(I::LocalGet(arr));
        Ok(Ty::Array(Box::new(et)))
    }

    pub(crate) fn array_lit(&mut self, items: &[Expr], want: Option<&Ty>, pos: Pos) -> R<Ty> {
        let et = match (items.first(), want) {
            (_, Some(t)) => t.clone(),
            (Some(first), None) => self.type_of(first)?,
            (None, None) => return err(pos, "`[]` needs a type: `let a: [float] = [];`"),
        };
        let esz = self.g.elem_size(&et) as usize;
        self.emit([
            I::I32Const(items.len() as i32),
            I::I32Const(esz as i32),
            I::Call(self.g.h.arr_new),
        ]);
        let arr = self.local(ValType::I32);
        self.e(I::LocalSet(arr));
        for (k, item) in items.iter().enumerate() {
            self.expr_as(item, &et)?;
            let tmp = self.alloc(&et);
            self.set_raw(&tmp);
            for (j, (vt, _)) in self.g.slots(&et).into_iter().enumerate() {
                self.emit([
                    I::LocalGet(arr),
                    I::I32Load(mem(12, 2)),
                    I::LocalGet(tmp.slots[j]),
                    store(vt, (k * esz + 8 * j) as u64),
                ]);
            }
        }
        self.e(I::LocalGet(arr));
        Ok(Ty::Array(Box::new(et)))
    }

    // The type `e` would have, with nothing emitted.
    pub(crate) fn type_of(&mut self, e: &Expr) -> R<Ty> {
        let marks = (
            self.code.len(),
            self.locals.len(),
            self.owned.len(),
            self.patches.len(),
        );
        let t = self.expr(e);
        self.code.truncate(marks.0);
        self.locals.truncate(marks.1);
        self.owned.truncate(marks.2);
        self.patches.truncate(marks.3);
        t
    }

    fn interp(&mut self, parts: &[Part]) -> R<()> {
        if parts.is_empty() {
            let p = self.g.string("");
            self.e(I::I32Const(p as i32));
        }
        for (k, part) in parts.iter().enumerate() {
            match part {
                Part::Text(s) => {
                    let p = self.g.string(s);
                    self.e(I::I32Const(p as i32));
                }
                Part::Value(v) => {
                    let t = self.expr(v)?;
                    self.stringify(&t, v.pos)?;
                }
            }
            if k > 0 {
                self.e(I::Call(self.g.h.concat));
            }
        }
        Ok(())
    }

    fn stringify(&mut self, t: &Ty, pos: Pos) -> R<()> {
        match t {
            Ty::Str => {}
            Ty::Int | Ty::Node => self.e(I::Call(self.g.h.str_of_i64)),
            Ty::Float => self.e(I::Call(self.g.h.str_of_f64)),
            Ty::Bool => {
                let (yes, no) = (self.g.string("true"), self.g.string("false"));
                self.emit([
                    I::If(BlockType::Result(ValType::I32)),
                    I::I32Const(yes as i32),
                    I::Else,
                    I::I32Const(no as i32),
                    I::End,
                ]);
            }
            _ => {
                return err(
                    pos,
                    format!("{} has no text form; print its fields", show(t)),
                );
            }
        }
        Ok(())
    }

    fn branch(&mut self, b: &Block) -> R<(Ty, Vec<I<'static>>)> {
        let mark = self.code.len();
        self.scopes.push(vec![]);
        let t = self.block_body(b)?;
        self.scopes.pop();
        Ok((t, self.code.drain(mark..).collect()))
    }

    fn if_expr(&mut self, c: &Expr, then: &Block, els: Option<&Block>, pos: Pos) -> R<Ty> {
        self.expr_as(c, &Ty::Bool)?;
        self.depth += 1;
        let (tt, mut then_code) = self.branch(then)?;
        let (et, mut else_code) = match els {
            Some(b) => self.branch(b)?,
            None => (Ty::Void, vec![]),
        };
        self.depth -= 1;
        if els.is_none() && tt != Ty::Void {
            return err(
                pos,
                "an `if` without `else` has no value; end the block with `;` or add `else`",
            );
        }
        let ty = match (&tt, &et) {
            (Ty::Int, Ty::Float) => {
                then_code.push(I::F64ConvertI64S);
                Ty::Float
            }
            (Ty::Float, Ty::Int) => {
                else_code.push(I::F64ConvertI64S);
                Ty::Float
            }
            _ if els.is_some() && tt != et => {
                return err(
                    pos,
                    format!(
                        "the branches of this `if` differ: {} and {}",
                        show(&tt),
                        show(&et)
                    ),
                );
            }
            _ => tt,
        };
        let flat: Vec<ValType> = self.g.slots(&ty).into_iter().map(|(v, _)| v).collect();
        let bt = match flat.len() {
            0 => BlockType::Empty,
            1 => BlockType::Result(flat[0]),
            _ => BlockType::FunctionType(self.g.type_idx(vec![], flat)),
        };
        self.e(I::If(bt));
        self.code.extend(then_code);
        if els.is_some() {
            self.e(I::Else);
            self.code.extend(else_code);
        }
        self.e(I::End);
        Ok(ty)
    }

    fn logic(&mut self, op: BinOp, a: &Expr, b: &Expr) -> R<Ty> {
        self.expr_as(a, &Ty::Bool).map_err(|e| Error {
            pos: e.pos,
            msg: format!("`{}` needs bools: {}", op_text(op), e.msg),
        })?;
        self.open(I::If(BlockType::Result(ValType::I32)));
        if op == BinOp::Or {
            self.emit([I::I32Const(1), I::Else]);
        }
        self.expr_as(b, &Ty::Bool).map_err(|e| Error {
            pos: e.pos,
            msg: format!("`{}` needs bools: {}", op_text(op), e.msg),
        })?;
        if op == BinOp::And {
            self.emit([I::Else, I::I32Const(0)]);
        }
        self.close();
        Ok(Ty::Bool)
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr, pos: Pos) -> R<Ty> {
        if matches!(op, BinOp::And | BinOp::Or) {
            return self.logic(op, a, b);
        }
        let mut ta = self.expr(a)?;
        let mark = self.code.len();
        let mut tb = self.expr(b)?;
        if ta == Ty::Int && tb == Ty::Float {
            self.code.insert(mark, I::F64ConvertI64S);
            ta = Ty::Float;
        } else if ta == Ty::Float && tb == Ty::Int {
            self.e(I::F64ConvertI64S);
            tb = Ty::Float;
        }
        if ta != tb {
            return err(
                pos,
                format!("cannot combine {} and {}", show(&ta), show(&tb)),
            );
        }
        if ta == Ty::Str && op == BinOp::Ne {
            self.emit([I::Call(self.g.h.str_eq), I::I32Eqz]);
            return Ok(Ty::Bool);
        }
        let (i, out) = self.operator(op, &ta).ok_or(Error {
            pos,
            msg: format!("`{}` is not defined on {}", op_text(op), show(&ta)),
        })?;
        self.e(i);
        Ok(out)
    }

    pub(crate) fn operator(&self, op: BinOp, t: &Ty) -> Option<(I<'static>, Ty)> {
        use BinOp::{Add, Div, Eq, Ge, Gt, Le, Lt, Mul, Ne, Rem, Sub};
        Some(match (t, op) {
            (Ty::Str, Add) => (I::Call(self.g.h.concat), Ty::Str),
            (Ty::Str, Eq) => (I::Call(self.g.h.str_eq), Ty::Bool),
            (Ty::Int, Add) => (I::I64Add, Ty::Int),
            (Ty::Int, Sub) => (I::I64Sub, Ty::Int),
            (Ty::Int, Mul) => (I::I64Mul, Ty::Int),
            (Ty::Int, Div) => (I::I64DivS, Ty::Int),
            (Ty::Int, Rem) => (I::I64RemS, Ty::Int),
            (Ty::Int | Ty::Node, Eq) => (I::I64Eq, Ty::Bool),
            (Ty::Int | Ty::Node, Ne) => (I::I64Ne, Ty::Bool),
            (Ty::Int, Lt) => (I::I64LtS, Ty::Bool),
            (Ty::Int, Le) => (I::I64LeS, Ty::Bool),
            (Ty::Int, Gt) => (I::I64GtS, Ty::Bool),
            (Ty::Int, Ge) => (I::I64GeS, Ty::Bool),
            (Ty::Float, Add) => (I::F64Add, Ty::Float),
            (Ty::Float, Sub) => (I::F64Sub, Ty::Float),
            (Ty::Float, Mul) => (I::F64Mul, Ty::Float),
            (Ty::Float, Div) => (I::F64Div, Ty::Float),
            (Ty::Float, Eq) => (I::F64Eq, Ty::Bool),
            (Ty::Float, Ne) => (I::F64Ne, Ty::Bool),
            (Ty::Float, Lt) => (I::F64Lt, Ty::Bool),
            (Ty::Float, Le) => (I::F64Le, Ty::Bool),
            (Ty::Float, Gt) => (I::F64Gt, Ty::Bool),
            (Ty::Float, Ge) => (I::F64Ge, Ty::Bool),
            (Ty::Bool, Eq) => (I::I32Eq, Ty::Bool),
            (Ty::Bool, Ne) => (I::I32Ne, Ty::Bool),
            _ => return None,
        })
    }

    pub(crate) fn args(&mut self, want: &[Ty], args: &[Expr], pos: Pos, what: &str) -> R<()> {
        if want.len() != args.len() {
            return err(
                pos,
                format!(
                    "`{what}` takes {} argument(s), {} given",
                    want.len(),
                    args.len()
                ),
            );
        }
        for (w, a) in want.iter().zip(args) {
            self.expr_as(a, w)?;
        }
        Ok(())
    }

    // A host call borrows its strings: they are released once it returns.
    pub(crate) fn host_args(
        &mut self,
        want: &[Ty],
        args: &[Expr],
        pos: Pos,
        what: &str,
    ) -> R<Vec<LVal>> {
        if want.len() != args.len() {
            return err(
                pos,
                format!(
                    "`{what}` takes {} argument(s), {} given",
                    want.len(),
                    args.len()
                ),
            );
        }
        let mut borrowed = vec![];
        for (w, a) in want.iter().zip(args) {
            self.expr_as(a, w)?;
            if w.is_heap() && !matches!(a.kind, ExprKind::Str(_)) {
                let t = self.temp(w);
                self.set_raw(&t);
                self.get_raw(&t);
                borrowed.push(t);
            }
        }
        Ok(borrowed)
    }
}
