use super::Kind;
use super::func::{FnGen, R, err, show};
use super::rt::{load, mem, store};
use crate::Error;
use crate::ast::{Expr, ExprKind, Pos, Ty};
use wasm_encoder::{BlockType, Instruction as I, ValType};

impl FnGen<'_> {
    pub(crate) fn call(&mut self, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let native = match name {
            "sqrt" => Some((I::F64Sqrt, 1)),
            "abs" => Some((I::F64Abs, 1)),
            "floor" => Some((I::F64Floor, 1)),
            "ceil" => Some((I::F64Ceil, 1)),
            "min" => Some((I::F64Min, 2)),
            "max" => Some((I::F64Max, 2)),
            _ => self
                .g
                .imp
                .math
                .get(name)
                .map(|(idx, n)| (I::Call(*idx), *n)),
        };
        if let Some((i, n)) = native {
            self.args(&vec![Ty::Float; n], args, pos, name)?;
            self.e(i);
            return Ok(Ty::Float);
        }
        if name == "nil" {
            self.args(&[], args, pos, name)?;
            self.e(I::I32Const(0));
            return Ok(Ty::Any);
        }
        if let Some(fields) = match name {
            "vec2" => Some(2),
            "vec3" => Some(3),
            "color" => Some(4),
            _ => None,
        } {
            self.args(&vec![Ty::Float; fields], args, pos, name)?;
            return Ok(Ty::Struct(
                ["Vec2", "Vec3", "Color"][fields - 2].to_string(),
            ));
        }
        let sig = self.g.sigs.get(name).cloned().ok_or(Error {
            pos,
            msg: if name.contains("::") {
                format!("the engine has no `{name}`")
            } else {
                format!("unknown function `{name}`")
            },
        })?;
        match sig.kind {
            Kind::ExternAsync | Kind::HostAsync | Kind::Async => {
                return err(
                    pos,
                    format!("`{name}` is async: `await {name}(..)` inside an `async fn`"),
                );
            }
            Kind::Extern | Kind::Host => {
                let borrowed = self.host_args(&sig.params, args, pos, name)?;
                self.e(I::Call(sig.idx));
                for b in borrowed {
                    self.release(&b);
                }
            }
            Kind::Script if sig.owner.is_some() => {
                return err(
                    pos,
                    format!("`{name}` is a method: call it as `self.{name}(..)`"),
                );
            }
            Kind::Script => {
                self.args(&sig.params, args, pos, name)?;
                self.spill();
                self.e(I::Call(sig.idx));
            }
        }
        Ok(sig.ret)
    }

    pub(crate) fn method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        // `node.transform.translate(..)`: an operation a component drives is
        // called on that component, as Rune calls it.
        if let ExprKind::Field(node, comp) = &base.kind
            && let Some(sig) = self.g.sigs.get(&format!("node.{comp}.{name}")).cloned()
            && sig.kind == Kind::Host
        {
            if self.type_of(node)? != Ty::Node {
                return err(
                    pos,
                    format!("`{comp}.{name}` is called on a node's `{comp}`"),
                );
            }
            return self.node_call(node, &sig, args, pos, name);
        }
        let place = self.place(base)?;
        let bty = match &place {
            Some(lv) => lv.ty.clone(),
            None => self.type_of(base)?,
        };
        match &bty {
            Ty::Array(_) => self.array_method(base, name, args, pos),
            Ty::Str if name == "len" => {
                self.args(&[], args, pos, "len")?;
                self.expr(base)?;
                let t = self.temp(&Ty::Str);
                self.set_raw(&t);
                self.emit([
                    I::LocalGet(t.slots[0]),
                    I::I32Load(mem(4, 2)),
                    I::I64ExtendI32U,
                ]);
                self.release(&t);
                Ok(Ty::Int)
            }
            Ty::Str => self.str_method(base, name, args, pos),
            Ty::Any => self.any_method(base, name, args, pos),
            Ty::Node => self.node_method(base, name, args, pos),
            Ty::Struct(s) => {
                let key = format!("{s}.{name}");
                let sig = self.g.sigs.get(&key).cloned().ok_or(Error {
                    pos,
                    msg: format!("no method `{name}` on {s}"),
                })?;
                if sig.kind == Kind::Async {
                    return err(
                        pos,
                        format!("`{key}` is async: `await self.{name}(..)` inside an `async fn`"),
                    );
                }
                let lv = place.ok_or(Error {
                    pos,
                    msg: "a method changes its value: call it on a variable or a field".into(),
                })?;
                self.get_raw(&lv);
                self.args(&sig.params, args, pos, &key)?;
                self.spill();
                self.e(I::Call(sig.idx));
                let ret = self.alloc(&sig.ret);
                self.set_raw(&ret);
                self.set_raw(&lv);
                self.get_raw(&ret);
                Ok(sig.ret)
            }
            _ => err(pos, format!("{} has no methods", show(&bty))),
        }
    }

    // `node.f(a)` is the host's `f(node, a)`.
    fn node_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let takes_node = |s: &super::Sig| s.params.first() == Some(&Ty::Node);
        let sig = self
            .g
            .sigs
            .get(name)
            .cloned()
            .filter(|s| s.kind == Kind::Extern && takes_node(s));
        let sig = sig.or_else(|| {
            self.g
                .sigs
                .get(&format!("node.{name}"))
                .cloned()
                .filter(|s| s.kind == Kind::Host && takes_node(s))
        });
        let sig = sig.ok_or(Error {
            pos,
            msg: format!("no host function `{name}(node: Node, ..)`"),
        })?;
        self.node_call(base, &sig, args, pos, name)
    }

    fn node_call(
        &mut self,
        base: &Expr,
        sig: &super::Sig,
        args: &[Expr],
        pos: Pos,
        name: &str,
    ) -> R<Ty> {
        self.expr(base)?;
        let borrowed = self.host_args(&sig.params[1..], args, pos, name)?;
        self.e(I::Call(sig.idx));
        for b in borrowed {
            self.release(&b);
        }
        Ok(sig.ret.clone())
    }

    // `a.get("k")` is the host's `get(a, "k")`; `a["k"]` and `a[0]` are the
    // same calls.
    pub(crate) fn any_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let Some(m) = crate::anys::method(name) else {
            let names: Vec<&str> = crate::anys::METHODS.iter().map(|m| m.name).collect();
            return err(
                pos,
                format!("an `any` has `{}`, not `{name}`", names.join("`, `")),
            );
        };
        if args.len() != m.params.len() {
            return err(
                pos,
                format!(
                    "`{name}` takes {} argument(s), {} given",
                    m.params.len(),
                    args.len()
                ),
            );
        }
        let all: Vec<Expr> = std::iter::once(base.clone())
            .chain(args.iter().cloned())
            .collect();
        let borrowed = self.host_args(&crate::anys::params(m), &all, pos, name)?;
        self.e(I::Call(self.g.imp.anys[name]));
        for b in borrowed {
            self.release(&b);
        }
        Ok(crate::anys::ty(m.ret))
    }

    // `s.contains(t)` is the host's `contains(s, t)`.
    fn str_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let Some(m) = crate::strings::method(name) else {
            let names: Vec<&str> = crate::strings::METHODS.iter().map(|m| m.name).collect();
            return err(
                pos,
                format!("strings have `len`, `{}`, not `{name}`", names.join("`, `")),
            );
        };
        if args.len() != m.params.len() {
            return err(
                pos,
                format!(
                    "`{name}` takes {} argument(s), {} given",
                    m.params.len(),
                    args.len()
                ),
            );
        }
        let all: Vec<Expr> = std::iter::once(base.clone())
            .chain(args.iter().cloned())
            .collect();
        let borrowed = self.host_args(&crate::strings::params(m), &all, pos, name)?;
        self.e(I::Call(self.g.imp.strs[name]));
        for b in borrowed {
            self.release(&b);
        }
        Ok(crate::strings::ty(m.ret))
    }

    fn array_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let (a, et, release) = self.array_ref(base, pos)?;
        let esz = self.g.elem_size(&et) as i32;
        let out = match name {
            "len" => {
                self.args(&[], args, pos, "len")?;
                self.emit([I::LocalGet(a), I::I32Load(mem(4, 2)), I::I64ExtendI32U]);
                Ty::Int
            }
            "push" => {
                if args.len() != 1 {
                    return err(pos, "`push` takes one value");
                }
                self.expr_as(&args[0], &et)?;
                let v = self.alloc(&et);
                self.set_raw(&v);
                self.emit([
                    I::LocalGet(a),
                    I::I32Const(esz),
                    I::Call(self.g.h.arr_reserve),
                ]);
                let addr = self.end_addr(a, esz, 0);
                for (j, (vt, _)) in self.g.slots(&et).into_iter().enumerate() {
                    self.emit([
                        I::LocalGet(addr),
                        I::LocalGet(v.slots[j]),
                        store(vt, 8 * j as u64),
                    ]);
                }
                self.emit([
                    I::LocalGet(a),
                    I::LocalGet(a),
                    I::I32Load(mem(4, 2)),
                    I::I32Const(1),
                    I::I32Add,
                    I::I32Store(mem(4, 2)),
                ]);
                Ty::Void
            }
            "pop" => {
                self.args(&[], args, pos, "pop")?;
                self.emit([
                    I::LocalGet(a),
                    I::I32Load(mem(4, 2)),
                    I::I32Eqz,
                    I::If(BlockType::Empty),
                    I::Unreachable,
                    I::End,
                ]);
                self.emit([
                    I::LocalGet(a),
                    I::LocalGet(a),
                    I::I32Load(mem(4, 2)),
                    I::I32Const(1),
                    I::I32Sub,
                    I::I32Store(mem(4, 2)),
                ]);
                let addr = self.end_addr(a, esz, 0);
                for (j, (vt, _)) in self.g.slots(&et).into_iter().enumerate() {
                    self.emit([I::LocalGet(addr), load(vt, 8 * j as u64)]);
                }
                et.clone()
            }
            "insert" => self.array_insert(a, &et, esz, args, pos)?,
            "remove" => self.array_remove(a, &et, esz, args, pos)?,
            _ => {
                return err(
                    pos,
                    format!(
                        "arrays have `len`, `push`, `pop`, `insert` and `remove`, not `{name}`"
                    ),
                );
            }
        };
        if let Some(r) = release {
            self.release(&r);
        }
        Ok(out)
    }

    // The address just past the last element, plus `extra` elements.
    fn end_addr(&mut self, a: u32, esz: i32, extra: i32) -> u32 {
        let addr = self.local(ValType::I32);
        self.emit([
            I::LocalGet(a),
            I::I32Load(mem(12, 2)),
            I::LocalGet(a),
            I::I32Load(mem(4, 2)),
        ]);
        self.emit([
            I::I32Const(extra),
            I::I32Add,
            I::I32Const(esz),
            I::I32Mul,
            I::I32Add,
            I::LocalSet(addr),
        ]);
        addr
    }

    // Traps unless `i` is below the length, or at it when `room` allows the end.
    fn checked_index(&mut self, a: u32, idx: &Expr, room: bool) -> R<u32> {
        let i = self.index_local(idx)?;
        self.emit([
            I::LocalGet(i),
            I::LocalGet(a),
            I::I32Load(mem(4, 2)),
            I::I64ExtendI32U,
            if room { I::I64GtU } else { I::I64GeU },
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
        ]);
        Ok(i)
    }

    // The address of element `i`, kept in a local.
    fn element_at(&mut self, a: u32, i: u32, esz: i32) -> u32 {
        let addr = self.local(ValType::I32);
        self.emit([
            I::LocalGet(a),
            I::I32Load(mem(12, 2)),
            I::LocalGet(i),
            I::I32WrapI64,
            I::I32Const(esz),
            I::I32Mul,
            I::I32Add,
            I::LocalSet(addr),
        ]);
        addr
    }

    // `(len - i - skip) * esz` bytes: what lies after the gap being opened or closed.
    fn tail_bytes(&mut self, a: u32, i: u32, esz: i32, skip: i32) {
        self.emit([
            I::LocalGet(a),
            I::I32Load(mem(4, 2)),
            I::LocalGet(i),
            I::I32WrapI64,
            I::I32Sub,
            I::I32Const(skip),
            I::I32Sub,
            I::I32Const(esz),
            I::I32Mul,
        ]);
    }

    fn bump_len(&mut self, a: u32, by: i32) {
        self.emit([
            I::LocalGet(a),
            I::LocalGet(a),
            I::I32Load(mem(4, 2)),
            I::I32Const(by),
            I::I32Add,
            I::I32Store(mem(4, 2)),
        ]);
    }

    // `xs.insert(i, v)`: what was at `i` and after moves up one.
    fn array_insert(&mut self, a: u32, et: &Ty, esz: i32, args: &[Expr], pos: Pos) -> R<Ty> {
        if args.len() != 2 {
            return err(pos, "`insert` takes an index and a value");
        }
        let i = self.checked_index(a, &args[0], true)?;
        self.expr_as(&args[1], et)?;
        let v = self.alloc(et);
        self.set_raw(&v);
        self.emit([
            I::LocalGet(a),
            I::I32Const(esz),
            I::Call(self.g.h.arr_reserve),
        ]);
        let at = self.element_at(a, i, esz);
        self.emit([
            I::LocalGet(at),
            I::I32Const(esz),
            I::I32Add,
            I::LocalGet(at),
        ]);
        self.tail_bytes(a, i, esz, 0);
        self.e(I::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });
        for (j, (vt, _)) in self.g.slots(et).into_iter().enumerate() {
            self.emit([
                I::LocalGet(at),
                I::LocalGet(v.slots[j]),
                store(vt, 8 * j as u64),
            ]);
        }
        self.bump_len(a, 1);
        Ok(Ty::Void)
    }

    // `xs.remove(i)`: the element leaves the array to the caller, and what was
    // after it moves down one.
    fn array_remove(&mut self, a: u32, et: &Ty, esz: i32, args: &[Expr], pos: Pos) -> R<Ty> {
        if args.len() != 1 {
            return err(pos, "`remove` takes an index");
        }
        let i = self.checked_index(a, &args[0], false)?;
        let at = self.element_at(a, i, esz);
        for (j, (vt, _)) in self.g.slots(et).into_iter().enumerate() {
            self.emit([I::LocalGet(at), load(vt, 8 * j as u64)]);
        }
        let v = self.alloc(et);
        self.set_raw(&v);
        self.emit([
            I::LocalGet(at),
            I::LocalGet(at),
            I::I32Const(esz),
            I::I32Add,
        ]);
        self.tail_bytes(a, i, esz, 1);
        self.e(I::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });
        self.bump_len(a, -1);
        self.get_raw(&v);
        Ok(et.clone())
    }
}
