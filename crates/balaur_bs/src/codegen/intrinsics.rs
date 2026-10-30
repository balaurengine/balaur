// The built-ins only the prelude may call (`crate::prelude`): a string's
// bytes, a string made and filled byte by byte, and the parts of an `any`'s
// cell. Each is a few loads and stores, with no call out. A string's index is
// checked against its length and traps outside it; a cell's kind is the
// caller's to check.

use super::func::{FnGen, R, err, show};
use super::rt::mem;
use crate::anys::{CELL, MAP};
use crate::ast::{Expr, Pos, Ty};
use wasm_encoder::{BlockType, Instruction as I, ValType};

impl FnGen<'_> {
    /// `name(args)` when `name` is a built-in, and what it gives.
    pub(crate) fn intrinsic(&mut self, name: &str, args: &[Expr], pos: Pos) -> R<Option<Ty>> {
        let ty = match name {
            "__byte" => self.byte(args, pos)?,
            "__hash" => self.hash(args, pos)?,
            "__str_new" => self.str_new(args, pos)?,
            "__str_copy" => self.str_copy(args, pos)?,
            "__str_put" => self.str_put(args, pos)?,
            "__parse_float" => {
                let held = self.borrowing(&[Ty::Str], args, pos, name)?;
                self.e(I::Call(self.g.imp.parse_f64));
                self.release_all(held);
                Ty::Float
            }
            "__tag" => {
                let held = self.borrowing(&[Ty::Any], args, pos, name)?;
                let cell = self.local(ValType::I32);
                self.emit([
                    I::LocalTee(cell),
                    I::If(BlockType::Result(ValType::I64)),
                    I::LocalGet(cell),
                    I::I32Load(mem(4, 2)),
                    I::I64ExtendI32U,
                    I::Else,
                    I::I64Const(0),
                    I::End,
                ]);
                self.release_all(held);
                Ty::Int
            }
            "__int" | "__node" => self.payload(args, pos, name, I::I64Load(mem(8, 3)))?,
            "__float" => self.payload(args, pos, name, I::F64Load(mem(8, 3)))?,
            "__bool" => {
                let ty = self.payload(args, pos, name, I::I32Load(mem(8, 2)))?;
                self.emit([I::I32Const(0), I::I32Ne]);
                ty
            }
            // Retained before the cell may go, with the temp holding it.
            "__str" => {
                let held = self.borrowing(&[Ty::Any], args, pos, name)?;
                self.emit([I::I32Load(mem(8, 2)), I::Call(self.g.h.retain)]);
                self.release_all(held);
                Ty::Str
            }
            "__items" | "__keys" => self.cell_array(args, pos, name, 8)?,
            "__vals" => self.cell_array(args, pos, name, 12)?,
            "__map" => self.map_of(args, pos)?,
            _ => return Ok(None),
        };
        Ok(Some(ty))
    }

    // The arguments on the stack, a heap one borrowed: the temps that hold
    // those, to release once the built-in has used them.
    fn borrowing(
        &mut self,
        want: &[Ty],
        args: &[Expr],
        pos: Pos,
        name: &str,
    ) -> R<Vec<super::func::LVal>> {
        if args.len() != want.len() {
            return err(pos, format!("`{name}` takes {} argument(s)", want.len()));
        }
        let all: Vec<&Expr> = args.iter().collect();
        self.host_args(want, &all, pos, name)
    }

    fn release_all(&mut self, held: Vec<super::func::LVal>) {
        for temp in held {
            self.release(&temp);
        }
    }

    // A value the cell holds where its kind says it is.
    fn payload(&mut self, args: &[Expr], pos: Pos, name: &str, load: I<'static>) -> R<Ty> {
        let held = self.borrowing(&[Ty::Any], args, pos, name)?;
        self.e(load);
        self.release_all(held);
        Ok(match name {
            "__int" => Ty::Int,
            "__node" => Ty::Node,
            "__float" => Ty::Float,
            _ => Ty::Bool,
        })
    }

    // The list, keys or values array a cell points at `off`, made empty first
    // if it points at none (a `map()` has neither), and shared with the caller.
    fn cell_array(&mut self, args: &[Expr], pos: Pos, name: &str, off: u64) -> R<Ty> {
        let held = self.borrowing(&[Ty::Any], args, pos, name)?;
        let (cell, array) = (self.local(ValType::I32), self.local(ValType::I32));
        self.emit([
            I::LocalTee(cell),
            I::I32Load(mem(off, 2)),
            I::LocalTee(array),
            I::I32Eqz,
            I::If(BlockType::Empty),
            I::I32Const(0),
            I::I32Const(8),
            I::Call(self.g.h.arr_new),
            I::LocalSet(array),
            I::LocalGet(cell),
            I::LocalGet(array),
            I::I32Store(mem(off, 2)),
            I::End,
            I::LocalGet(array),
            I::Call(self.g.h.retain),
        ]);
        self.release_all(held);
        Ok(match name {
            "__keys" => Ty::Array(Box::new(Ty::Str)),
            _ => Ty::Array(Box::new(Ty::Any)),
        })
    }

    // A map cell owning the two arrays it is handed.
    fn map_of(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let want = [Ty::Array(Box::new(Ty::Str)), Ty::Array(Box::new(Ty::Any))];
        self.args(&want, args, pos, "__map")?;
        let (keys, vals, cell) = (
            self.local(ValType::I32),
            self.local(ValType::I32),
            self.local(ValType::I32),
        );
        self.emit([
            I::LocalSet(vals),
            I::LocalSet(keys),
            I::I32Const(CELL),
            I::Call(self.g.h.alloc),
            I::LocalTee(cell),
            I::I32Const(1),
            I::I32Store(mem(0, 2)),
            I::LocalGet(cell),
            I::I32Const(MAP),
            I::I32Store(mem(4, 2)),
            I::LocalGet(cell),
            I::LocalGet(keys),
            I::I32Store(mem(8, 2)),
            I::LocalGet(cell),
            I::LocalGet(vals),
            I::I32Store(mem(12, 2)),
            I::LocalGet(cell),
        ]);
        Ok(Ty::Any)
    }

    // `__hash(key)`: where a map's key starts its probe, never negative. A
    // string's is the prelude's; an int's is mixed here as a string's is.
    fn hash(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let [key] = args else {
            return err(pos, "`__hash` takes 1 argument(s)");
        };
        match self.expr(key)? {
            Ty::Str => {
                self.call_prelude("str::#hash");
            }
            Ty::Int | Ty::Node => {
                let mixed = self.local(ValType::I64);
                self.emit([
                    I::I64Const(1_099_511_628_211),
                    I::I64Mul,
                    I::LocalTee(mixed),
                    I::LocalGet(mixed),
                    I::I64Const(4_294_967_296),
                    I::I64DivS,
                    I::I64Add,
                    I::LocalTee(mixed),
                    I::I64Const(0),
                    I::I64LtS,
                    I::If(BlockType::Result(ValType::I64)),
                    I::I64Const(-1),
                    I::LocalGet(mixed),
                    I::I64Sub,
                    I::Else,
                    I::LocalGet(mixed),
                    I::End,
                ]);
            }
            other => {
                return err(
                    pos,
                    format!("`__hash` takes a str or an int, not {}", show(&other)),
                );
            }
        }
        Ok(Ty::Int)
    }

    // `__byte(text, at)`: the byte at `at`, which must be inside the string.
    fn byte(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let held = self.borrowing(&[Ty::Str, Ty::Int], args, pos, "__byte")?;
        let (text, at) = (self.local(ValType::I32), self.local(ValType::I64));
        self.emit([I::LocalSet(at), I::LocalSet(text)]);
        self.inside(text, at, None);
        self.emit([
            I::LocalGet(text),
            I::LocalGet(at),
            I::I32WrapI64,
            I::I32Add,
            I::I32Load8U(mem(8, 0)),
            I::I64ExtendI32U,
        ]);
        self.release_all(held);
        Ok(Ty::Int)
    }

    // `__str_put(text, at, byte)`: byte `at` of a string the prelude has just made.
    fn str_put(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let held = self.borrowing(&[Ty::Str, Ty::Int, Ty::Int], args, pos, "__str_put")?;
        let (text, at, byte) = (
            self.local(ValType::I32),
            self.local(ValType::I64),
            self.local(ValType::I64),
        );
        self.emit([I::LocalSet(byte), I::LocalSet(at), I::LocalSet(text)]);
        self.inside(text, at, None);
        self.emit([
            I::LocalGet(text),
            I::LocalGet(at),
            I::I32WrapI64,
            I::I32Add,
            I::LocalGet(byte),
            I::I32WrapI64,
            I::I32Store8(mem(8, 0)),
        ]);
        self.release_all(held);
        Ok(Ty::Void)
    }

    // `__str_new(length)`: a string of `length` zero bytes, for the prelude to fill.
    fn str_new(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        self.args(&[Ty::Int], args, pos, "__str_new")?;
        let (length, text) = (self.local(ValType::I64), self.local(ValType::I32));
        self.emit([
            I::LocalTee(length),
            I::I64Const(1 << 30),
            I::I64GtU,
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
            I::LocalGet(length),
            I::I32WrapI64,
            I::I32Const(8),
            I::I32Add,
            I::Call(self.g.h.alloc),
            I::LocalTee(text),
            I::I32Const(1),
            I::I32Store(mem(0, 2)),
            I::LocalGet(text),
            I::LocalGet(length),
            I::I32WrapI64,
            I::I32Store(mem(4, 2)),
            I::LocalGet(text),
            I::I32Const(8),
            I::I32Add,
            I::I32Const(0),
            I::LocalGet(length),
            I::I32WrapI64,
            I::MemoryFill(0),
            I::LocalGet(text),
        ]);
        Ok(Ty::Str)
    }

    // `__str_copy(to, at, from, start, count)`: `count` bytes of `from` from `start`
    // into `to` at `at`, every one inside both.
    fn str_copy(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let want = [Ty::Str, Ty::Int, Ty::Str, Ty::Int, Ty::Int];
        let held = self.borrowing(&want, args, pos, "__str_copy")?;
        let (to, at, from, start, count) = (
            self.local(ValType::I32),
            self.local(ValType::I64),
            self.local(ValType::I32),
            self.local(ValType::I64),
            self.local(ValType::I64),
        );
        self.emit([
            I::LocalSet(count),
            I::LocalSet(start),
            I::LocalSet(from),
            I::LocalSet(at),
            I::LocalSet(to),
        ]);
        self.inside(to, at, Some(count));
        self.inside(from, start, Some(count));
        self.emit([
            I::LocalGet(to),
            I::LocalGet(at),
            I::I32WrapI64,
            I::I32Add,
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(from),
            I::LocalGet(start),
            I::I32WrapI64,
            I::I32Add,
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(count),
            I::I32WrapI64,
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
        ]);
        self.release_all(held);
        Ok(Ty::Void)
    }

    // Traps unless byte `at` of the string in `text` exists, or with `count`
    // the `count` bytes from `at` do. The compares are unsigned, so a negative
    // traps.
    fn inside(&mut self, text: u32, at: u32, count: Option<u32>) {
        self.e(I::LocalGet(at));
        if let Some(count) = count {
            // Each part on its own, so a negative count cannot cancel a
            // start past the end.
            self.emit([
                I::LocalGet(count),
                I::I64Or,
                I::I64Const(0),
                I::I64LtS,
                I::If(BlockType::Empty),
                I::Unreachable,
                I::End,
                I::LocalGet(at),
                I::LocalGet(count),
                I::I64Add,
            ]);
        }
        self.emit([
            I::LocalGet(text),
            I::I32Load(mem(4, 2)),
            I::I64ExtendI32U,
            if count.is_some() {
                I::I64GtU
            } else {
                I::I64GeU
            },
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
        ]);
    }
}
