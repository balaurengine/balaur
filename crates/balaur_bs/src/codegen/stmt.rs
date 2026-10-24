use super::func::{FnGen, LVal, MemPlace, R, err, op_text, show};
use super::rt::{load, mem};
use crate::Error;
use crate::ast::{BinOp, Block, Expr, ExprKind, Pos, Stmt, StmtKind, Ty};
use wasm_encoder::{BlockType, Instruction as I, ValType};

impl FnGen<'_> {
    pub(crate) fn block_body(&mut self, b: &Block) -> R<Ty> {
        for s in &b.stmts {
            self.stmt(s)?;
        }
        match &b.tail {
            Some(t) => {
                self.probe(t.pos);
                self.expr(t)
            }
            None => Ok(Ty::Void),
        }
    }

    fn void_block(&mut self, b: &Block) -> R<()> {
        self.scopes.push(vec![]);
        let t = self.block_body(b)?;
        self.discard(&t);
        self.scopes.pop();
        Ok(())
    }

    pub(crate) fn stmt(&mut self, s: &Stmt) -> R<()> {
        self.probe(s.pos);
        let pos = s.pos;
        match &s.kind {
            StmtKind::Let { name, ty, init } => self.let_stmt(name, ty.as_ref(), init, pos)?,
            StmtKind::Assign { target, op, value } => self.assign(target, *op, value, pos)?,
            StmtKind::Expr(e) => {
                let t = self.expr(e)?;
                self.discard(&t);
            }
            StmtKind::While { cond, body } => self.looped(Some(cond), body)?,
            StmtKind::Loop(body) => self.looped(None, body)?,
            StmtKind::ForRange {
                var,
                from,
                to,
                body,
            } => self.for_range(var, from, to, body, pos)?,
            StmtKind::ForEach { var, items, body } => self.for_each(var, items, body)?,
            StmtKind::Return(v) => self.ret(v.as_ref(), pos, true)?,
            StmtKind::Break | StmtKind::Continue => {
                let &(brk, cont) = self.loops.last().ok_or(Error {
                    pos,
                    msg: "`break` and `continue` belong in a loop".into(),
                })?;
                self.br(if matches!(s.kind, StmtKind::Break) {
                    brk
                } else {
                    cont
                });
            }
        }
        Ok(())
    }

    fn let_stmt(&mut self, name: &str, ty: Option<&Ty>, init: &Expr, pos: Pos) -> R<()> {
        let t = if let Some(want) = ty {
            let names: Vec<&String> = self.g.structs.keys().collect();
            if let Some(u) = super::unknown_type(want, &names) {
                return err(pos, format!("unknown type `{u}`"));
            }
            self.expr_as(init, want)?;
            want.clone()
        } else {
            self.expr(init)?
        };
        if t == Ty::Void {
            return err(
                pos,
                format!("`{name}` would hold nothing: the right side returns no value"),
            );
        }
        let lv = self.alloc(&t);
        self.own(&lv);
        self.set_owned(&lv);
        self.declare(name, lv);
        Ok(())
    }

    // The `any` a target sets a key or an index of, the key, and the method
    // that sets it.
    fn any_slot(&mut self, target: &Expr) -> R<Option<(Expr, Expr, &'static str)>> {
        let (base, key) = match &target.kind {
            ExprKind::Field(b, f) => (
                b,
                Expr {
                    kind: ExprKind::Str(f.clone()),
                    pos: target.pos,
                },
            ),
            ExprKind::Index(b, k) => (b, (**k).clone()),
            ExprKind::Method(b, m, args) if m == "get" && args.len() == 1 => (b, args[0].clone()),
            _ => return Ok(None),
        };
        if self.type_of(base)? != Ty::Any {
            return Ok(None);
        }
        // A whole number is a list's index; anything else is a map's key.
        let method = if self.type_of(&key)? == Ty::Int {
            "put"
        } else {
            "set"
        };
        Ok(Some(((**base).clone(), key, method)))
    }

    fn assign(&mut self, target: &Expr, op: Option<BinOp>, value: &Expr, pos: Pos) -> R<()> {
        if let Some((node, get, set, ty)) = self.component_access(target)? {
            self.expr(&node)?;
            if let Some(op) = op {
                self.expr(&node)?;
                self.host_call(&get);
                let vt = self.expr(value)?;
                self.coerce(&vt, &ty, pos)?;
                let (i, _) = self.operator(op, &ty).ok_or(Error {
                    pos,
                    msg: format!("cannot apply `{}=` to {}", op_text(op), show(&ty)),
                })?;
                self.e(i);
            } else {
                self.expr_as(value, &ty)?;
            }
            self.host_call(&set);
            return Ok(());
        }
        if let Some(lv) = self.place(target)? {
            if let Some(op) = op {
                let b = Expr {
                    kind: ExprKind::Bin(op, Box::new(target.clone()), Box::new(value.clone())),
                    pos,
                };
                let t = self.expr(&b)?;
                self.coerce(&t, &lv.ty, pos)?;
            } else {
                self.expr_as(value, &lv.ty)?;
            }
            self.set_owned(&lv);
            return Ok(());
        }
        // `a.key = v`, `a[k] = v` or `a.get(k) = v` on an `any`: set in place.
        if let Some((base, key, method)) = self.any_slot(target)? {
            let v = match op {
                Some(op) => Expr {
                    kind: ExprKind::Bin(op, Box::new(target.clone()), Box::new(value.clone())),
                    pos,
                },
                None => value.clone(),
            };
            self.any_method(&base, method, &[key, v], pos)?;
            self.e(I::Drop);
            return Ok(());
        }
        let mp = self.mem_place(target)?.ok_or(Error {
            pos,
            msg: "this cannot be assigned to".into(),
        })?;
        if let Some(vt) = self.plain_slot(&mp) {
            self.push_addr(mp.addr);
            self.element_value(&mp, op, value, pos)?;
            self.e(super::rt::store(vt, mp.off));
            if let Some(r) = &mp.release {
                self.release(r);
            }
            return Ok(());
        }
        self.element_value(&mp, op, value, pos)?;
        self.mem_write(&mp);
        if let Some(r) = &mp.release {
            self.release(r);
        }
        Ok(())
    }

    // What an element assignment stores: `value`, or the element `op` it.
    fn element_value(&mut self, mp: &MemPlace, op: Option<BinOp>, value: &Expr, pos: Pos) -> R<()> {
        if let Some(op) = op {
            self.mem_read(mp);
            let vt = self.expr(value)?;
            let vt = if mp.ty == Ty::Float && vt == Ty::Int {
                self.e(I::F64ConvertI64S);
                Ty::Float
            } else {
                vt
            };
            let apply = self
                .operator(op, &mp.ty)
                .filter(|(_, out)| *out == mp.ty && vt == mp.ty && op != BinOp::Eq);
            let (i, _) = apply.ok_or(Error {
                pos,
                msg: format!(
                    "cannot apply `{}=` to {} with {}",
                    op_text(op),
                    show(&mp.ty),
                    show(&vt)
                ),
            })?;
            self.e(i);
        } else {
            self.expr_as(value, &mp.ty)?;
        }
        Ok(())
    }

    fn looped(&mut self, cond: Option<&Expr>, body: &Block) -> R<()> {
        let brk = self.open(I::Block(BlockType::Empty));
        let top = self.open(I::Loop(BlockType::Empty));
        self.charge();
        if let Some(c) = cond {
            self.expr_as(c, &Ty::Bool)?;
            self.emit([I::I32Eqz, I::BrIf(self.depth - brk)]);
        }
        self.loops.push((brk, top));
        self.void_block(body)?;
        self.loops.pop();
        self.br(top);
        self.close();
        self.close();
        Ok(())
    }

    fn for_range(&mut self, var: &str, from: &Expr, to: &Expr, body: &Block, pos: Pos) -> R<()> {
        let as_range = |e: Error| Error {
            pos,
            msg: format!("a range counts in int: {}", e.msg),
        };
        self.scopes.push(vec![]);
        self.expr_as(from, &Ty::Int).map_err(as_range)?;
        let i = self.alloc(&Ty::Int);
        self.set_raw(&i);
        self.expr_as(to, &Ty::Int).map_err(as_range)?;
        let end = self.local(ValType::I64);
        self.e(I::LocalSet(end));
        self.declare(var, i.clone());
        self.counted_loop(var, &i, end, body)?;
        self.scopes.pop();
        Ok(())
    }

    /// The loop itself: `i` from where it is up to `end`.
    pub(crate) fn count(&mut self, i: &LVal, end: u32, body: &Block) -> R<()> {
        let brk = self.open(I::Block(BlockType::Empty));
        let top = self.open(I::Loop(BlockType::Empty));
        self.emit([
            I::LocalGet(i.slots[0]),
            I::LocalGet(end),
            I::I64GeS,
            I::BrIf(self.depth - brk),
        ]);
        self.charge();
        self.scale_index();
        let cont = self.open(I::Block(BlockType::Empty));
        self.loops.push((brk, cont));
        self.void_block(body)?;
        self.loops.pop();
        self.close();
        self.emit([
            I::LocalGet(i.slots[0]),
            I::I64Const(1),
            I::I64Add,
            I::LocalSet(i.slots[0]),
        ]);
        self.br(top);
        self.close();
        self.close();
        Ok(())
    }

    fn for_each(&mut self, var: &str, items: &Expr, body: &Block) -> R<()> {
        if self.type_of(items)? == Ty::Any {
            return self.for_each_any(var, items, body);
        }
        self.scopes.push(vec![]);
        let at = self.expr(items)?;
        let Ty::Array(et) = at.clone() else {
            return err(
                items.pos,
                format!("`for .. in` walks an array, not {}", show(&at)),
            );
        };
        let arr = self.temp(&at);
        self.set_raw(&arr);
        let i = self.local(ValType::I32);
        self.emit([I::I32Const(0), I::LocalSet(i)]);
        let x = self.alloc(&et);
        self.own(&x);
        self.declare(var, x.clone());
        let esz = self.g.elem_size(&et) as i32;
        let brk = self.open(I::Block(BlockType::Empty));
        let top = self.open(I::Loop(BlockType::Empty));
        self.emit([
            I::LocalGet(i),
            I::LocalGet(arr.slots[0]),
            I::I32Load(mem(4, 2)),
            I::I32GeU,
            I::BrIf(self.depth - brk),
        ]);
        self.charge();
        for (j, (vt, heap)) in self.g.slots(&et).into_iter().enumerate() {
            self.emit([
                I::LocalGet(arr.slots[0]),
                I::I32Load(mem(12, 2)),
                I::LocalGet(i),
                I::I32Const(esz),
                I::I32Mul,
                I::I32Add,
                load(vt, 8 * j as u64),
            ]);
            if heap.is_some() {
                self.e(I::Call(self.g.h.retain));
            }
        }
        self.set_owned(&x);
        let cont = self.open(I::Block(BlockType::Empty));
        self.loops.push((brk, cont));
        self.void_block(body)?;
        self.loops.pop();
        self.close();
        self.emit([I::LocalGet(i), I::I32Const(1), I::I32Add, I::LocalSet(i)]);
        self.br(top);
        self.close();
        self.close();
        self.release(&arr);
        self.scopes.pop();
        Ok(())
    }

    // An `any` list walked as `xs.at(i)` for each `i`: the elements the list
    // holds, where reading it out as a `[any]` copied each.
    fn for_each_any(&mut self, var: &str, items: &Expr, body: &Block) -> R<()> {
        let pos = items.pos;
        let k = self.locals.len();
        let (list, at) = (format!("#each{k}"), format!("#at{k}"));
        let named = |n: &str| Expr {
            kind: ExprKind::Var(n.to_string()),
            pos,
        };
        let method = |name: &str, args: Vec<Expr>| Expr {
            kind: ExprKind::Method(Box::new(named(&list)), name.to_string(), args),
            pos,
        };
        let mut stmts = vec![Stmt {
            kind: StmtKind::Let {
                name: var.to_string(),
                ty: Some(Ty::Any),
                init: method("at", vec![named(&at)]),
            },
            pos,
        }];
        stmts.extend(body.stmts.iter().cloned());
        let walk = Block {
            stmts: vec![
                Stmt {
                    kind: StmtKind::Let {
                        name: list.clone(),
                        ty: Some(Ty::Any),
                        init: items.clone(),
                    },
                    pos,
                },
                Stmt {
                    kind: StmtKind::ForRange {
                        var: at.clone(),
                        from: Expr {
                            kind: ExprKind::Int(0),
                            pos,
                        },
                        to: method("len", vec![]),
                        body: Block {
                            stmts,
                            tail: body.tail.clone(),
                        },
                    },
                    pos,
                },
            ],
            tail: None,
        };
        self.void_block(&walk)
    }
}
