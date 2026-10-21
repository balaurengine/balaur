use super::Site;
use super::func::{FnGen, LVal, Mode, R, err, op_text, show};
use super::rt::{load, mem, store};
use crate::Error;
use crate::ast::{BinOp, Block, Expr, ExprKind, Part, Pos, StmtKind, Ty, UnOp};
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
            ExprKind::Nil => {
                return err(
                    e.pos,
                    "`nil` needs the type it stands for: `let x: T? = nil`",
                );
            }
            ExprKind::Interp(parts) => {
                self.interp(parts)?;
                Ty::Str
            }
            ExprKind::Var(n) if self.lookup(n).is_none() && self.g.consts.contains_key(n) => {
                let c = self.g.consts[n].clone();
                self.expr(&c)?
            }
            ExprKind::Var(n) if self.lookup(n).is_none() && self.g.sigs.contains_key(n) => {
                self.fn_value(n, e.pos)?
            }
            ExprKind::Index(arr, idx) if self.type_of(arr).ok() == Some(Ty::Any) => {
                let by = if self.type_of(idx)? == Ty::Str {
                    "get"
                } else {
                    "at"
                };
                self.any_method(arr, by, std::slice::from_ref(&**idx), e.pos)?
            }
            // `a.name` on an `any` is `a.get("name")`.
            ExprKind::Field(base, f) if self.type_of(base).ok() == Some(Ty::Any) => {
                let key = Expr {
                    kind: ExprKind::Str(f.clone()),
                    pos: e.pos,
                };
                self.any_method(base, "get", std::slice::from_ref(&key), e.pos)?
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
        if self.g.is_class(&bt).is_some() {
            let mp = self.object_place(tmp.slots[0], &bt, f, None, e.pos)?;
            self.mem_read(&mp);
            self.release(&tmp);
            return Ok(mp.ty);
        }
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
            (UnOp::Neg, Ty::Any) => {
                self.unbox_any(&Ty::Float);
                self.e(I::F64Neg);
                return Ok(Ty::Float);
            }
            (UnOp::Not, Ty::Any) => {
                self.unbox_any(&Ty::Bool);
                self.e(I::I32Eqz);
                return Ok(Ty::Bool);
            }
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
        let ty = Ty::Struct(s.to_string());
        // A class's fields go into a new object, counted once for its holder.
        let obj = self.g.is_class(&ty).is_some().then(|| {
            let size = super::OBJ_HEADER as i32 + 8 * self.g.object_slots(s).len() as i32;
            let obj = self.local(ValType::I32);
            self.emit([
                I::I32Const(size),
                I::Call(self.g.h.alloc),
                I::LocalTee(obj),
                I::I32Const(1),
                I::I32Store(mem(0, 2)),
            ]);
            obj
        });
        let mut off = super::OBJ_HEADER;
        for (f, t) in &decl {
            let (_, fe) = fields.iter().find(|(n, _)| n == f).ok_or(Error {
                pos,
                msg: format!("missing field `{f}` of {s}"),
            })?;
            self.expr_as(fe, t)?;
            if let Some(obj) = obj {
                let v = self.alloc(t);
                self.set_raw(&v);
                for (j, (vt, _)) in self.g.slots(t).into_iter().enumerate() {
                    self.emit([
                        I::LocalGet(obj),
                        I::LocalGet(v.slots[j]),
                        store(vt, off + 8 * j as u64),
                    ]);
                }
                off += 8 * v.slots.len() as u64;
            }
        }
        if let Some(obj) = obj {
            self.e(I::LocalGet(obj));
        }
        Ok(ty)
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
                    Ty::Str | Ty::Array(_) | Ty::Any => self.g.imp.await_heap,
                    Ty::Void => return Ok(Ty::Void),
                    Ty::Struct(_) | Ty::Optional(_) | Ty::Fn(..) => {
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
            (Some(first), None) => {
                let first = self.type_of(first)?;
                // Elements of more than one kind make a list of anys.
                let mut et = first;
                for item in &items[1..] {
                    let t = self.type_of(item)?;
                    if t != et && !matches!((&et, &t), (Ty::Float, Ty::Int)) {
                        et = if matches!((&et, &t), (Ty::Int, Ty::Float)) {
                            Ty::Float
                        } else {
                            Ty::Any
                        };
                    }
                }
                et
            }
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
        let at = (std::ptr::from_ref(e) as usize, e.pos.line, e.pos.col);
        if let Some(t) = self.typed.get(&at) {
            return Ok(t.clone());
        }
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
        if let Ok(t) = &t {
            self.typed.insert(at, t.clone());
        }
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

    pub(crate) fn stringify(&mut self, t: &Ty, pos: Pos) -> R<()> {
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
            // A list, struct or `T?`: what its `any` says of it.
            Ty::Array(_) | Ty::Struct(_) | Ty::Optional(_) => {
                self.box_any(t);
                self.stringify(&Ty::Any, pos)?;
            }
            // What `.text()` says of it.
            Ty::Any => {
                let v = self.temp(&Ty::Any);
                self.set_raw(&v);
                self.get_raw(&v);
                self.e(I::Call(self.g.imp.anys["text"]));
                self.release(&v);
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
        // `if c { x } else { nil }` or `{ [] }`: the other branch says what
        // `nil` or `[]` stands for.
        if let Some(e) = els
            && is_open(then) != is_open(e)
        {
            let (open, other) = if is_open(then) { (then, e) } else { (e, then) };
            let t = self.type_of(&Expr {
                kind: ExprKind::If(Box::new(c.clone()), other.clone(), Some(other.clone())),
                pos,
            })?;
            // Beside a branch that gives nothing, `nil` is nothing too.
            if t == Ty::Void && is_nil(open) {
                let empty = Block::default();
                let (a, b) = if is_open(then) {
                    (&empty, e)
                } else {
                    (then, &empty)
                };
                return self.if_expr(c, a, Some(b), pos);
            }
            let want = match t {
                Ty::Any | Ty::Optional(_) => t,
                Ty::Array(_) if !is_nil(open) => t,
                t => Ty::Optional(Box::new(t)),
            };
            self.if_as(c, then, els, &want, pos)?;
            return Ok(want);
        }
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
        let plain = |t: &Ty| matches!(t, Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Node);
        // A branch that leaves has whatever type the other gives: it never
        // hands a value to the `if`.
        let (tt, et) = match els {
            Some(e) if tt != et && et == Ty::Void && leaves(e) => {
                else_code.push(I::Unreachable);
                (tt.clone(), tt)
            }
            Some(_) if tt != et && tt == Ty::Void && leaves(then) => {
                then_code.push(I::Unreachable);
                (et.clone(), et)
            }
            _ => (tt, et),
        };
        let ty = match (&tt, &et) {
            (Ty::Int, Ty::Float) => {
                then_code.push(I::F64ConvertI64S);
                Ty::Float
            }
            // An `any` branch is read as the other branch's plain kind.
            (Ty::Any, t) if plain(t) => {
                let mark = self.code.len();
                self.unbox_any(t);
                then_code.extend(self.code.drain(mark..));
                t.clone()
            }
            (t, Ty::Any) if plain(t) => {
                let mark = self.code.len();
                self.unbox_any(t);
                else_code.extend(self.code.drain(mark..));
                t.clone()
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
        if matches!(op, BinOp::Eq | BinOp::Ne) {
            match (&a.kind, &b.kind) {
                (_, ExprKind::Nil) => return self.nil_compare(a, op, pos),
                (ExprKind::Nil, _) => return self.nil_compare(b, op, pos),
                _ => {}
            }
        }
        let mut ta = self.expr(a)?;
        let mark = self.code.len();
        let mut tb = self.expr(b)?;
        // An `any` meets a plain value as that value's kind.
        let plain = |t: &Ty| matches!(t, Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Node);
        if ta == Ty::Any && plain(&tb) {
            let held = self.temp(&tb);
            self.set_raw(&held);
            self.unbox_any(&tb);
            self.take_out(&held);
            ta = tb.clone();
        } else if tb == Ty::Any && plain(&ta) {
            self.unbox_any(&ta);
            tb = ta.clone();
        }
        if ta == Ty::Int && tb == Ty::Float {
            self.code.insert(mark, I::F64ConvertI64S);
            ta = Ty::Float;
        } else if ta == Ty::Float && tb == Ty::Int {
            self.e(I::F64ConvertI64S);
            tb = Ty::Float;
        }
        // Two `any`s: equal as values, else compared and counted as floats.
        if ta == Ty::Any && tb == Ty::Any {
            if matches!(op, BinOp::Eq | BinOp::Ne) {
                let b = self.temp(&Ty::Any);
                self.set_raw(&b);
                let a = self.temp(&Ty::Any);
                self.set_raw(&a);
                self.get_raw(&a);
                self.get_raw(&b);
                self.e(I::Call(self.g.imp.anys["equals"]));
                self.release(&a);
                self.release(&b);
                if op == BinOp::Ne {
                    self.e(I::I32Eqz);
                }
                return Ok(Ty::Bool);
            }
            let held = self.temp(&Ty::Any);
            self.set_raw(&held);
            self.unbox_any(&Ty::Float);
            self.take_out(&held);
            self.unbox_any(&Ty::Float);
            ta = Ty::Float;
            tb = Ty::Float;
        }
        // Lists or structs are equal as values: both boxed and compared.
        let boxed = |t: &Ty| matches!(t, Ty::Array(_) | Ty::Struct(_) | Ty::Optional(_));
        if matches!(op, BinOp::Eq | BinOp::Ne) && (boxed(&ta) || boxed(&tb)) {
            let b = self.temp(&tb);
            self.set_raw(&b);
            self.coerce(&ta, &Ty::Any, pos)?;
            self.take_out(&b);
            self.coerce(&tb, &Ty::Any, pos)?;
            let both = (self.temp(&Ty::Any), self.temp(&Ty::Any));
            self.set_raw(&both.1);
            self.set_raw(&both.0);
            self.get_raw(&both.0);
            self.get_raw(&both.1);
            self.e(I::Call(self.g.imp.anys["equals"]));
            self.release(&both.0);
            self.release(&both.1);
            if op == BinOp::Ne {
                self.e(I::I32Eqz);
            }
            return Ok(Ty::Bool);
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
    // Trailing `T?` and `any` parameters may be left out, and are nil.
    pub(crate) fn host_args(
        &mut self,
        want: &[Ty],
        args: &[Expr],
        pos: Pos,
        what: &str,
    ) -> R<Vec<LVal>> {
        let least = crate::least(want);
        if args.len() < least || args.len() > want.len() {
            let takes = if least == want.len() {
                want.len().to_string()
            } else {
                format!("{least} to {}", want.len())
            };
            return err(
                pos,
                format!("`{what}` takes {takes} argument(s), {} given", args.len()),
            );
        }
        let mut borrowed = vec![];
        for (w, a) in want.iter().zip(args) {
            self.expr_as(a, w)?;
            // A literal string lives in the data segment; boxed into an
            // `any`, it is a fresh cell like any other.
            let literal = matches!(a.kind, ExprKind::Str(_)) && *w == Ty::Str;
            if w.is_heap() && !literal {
                let t = self.temp(w);
                self.set_raw(&t);
                self.get_raw(&t);
                borrowed.push(t);
            }
        }
        for w in &want[args.len()..] {
            self.nil_as(w, pos)?;
        }
        Ok(borrowed)
    }
}

// A block that is only `nil`.
fn is_nil(b: &Block) -> bool {
    b.stmts.is_empty() && matches!(b.tail.as_deref().map(|e| &e.kind), Some(ExprKind::Nil))
}

/// A block that ends in `return`, `break` or `continue`.
fn leaves(b: &Block) -> bool {
    b.tail.is_none()
        && matches!(
            b.stmts.last().map(|s| &s.kind),
            Some(StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue)
        )
}

// A block that is only `nil` or `[]`: its type comes from where it goes.
fn is_open(b: &Block) -> bool {
    is_nil(b)
        || (b.stmts.is_empty()
            && matches!(b.tail.as_deref().map(|e| &e.kind), Some(ExprKind::ArrayLit(items)) if items.is_empty()))
}
