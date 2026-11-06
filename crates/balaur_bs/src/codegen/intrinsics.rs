//! The built-ins only the prelude may call (`crate::prelude`): a string's
//! bytes, a string made and filled byte by byte, and the parts of an `any`'s
//! cell. Each is a few loads and stores, with no call out. A string's index is
//! checked against its length and traps outside it; a cell's kind is the
//! caller's to check.

use super::func::{FnGen, R, err, show};
use super::rt::mem;
use crate::anys::{CELL, MAP};
use crate::ast::{Expr, ExprKind, Pos, Ty};
use wasm_encoder::{BlockType, Instruction as I, ValType};

/// What a map's key is folded by, byte by byte.
const MULTIPLIER: i64 = 1_099_511_628_211;
/// A one in every byte of a word, and each byte's top bit: what finds a byte
/// in eight at once.
const BYTES_LOW: i64 = 0x0101_0101_0101_0101;
const BYTES_HIGH: i64 = 0x8080_8080_8080_8080_u64.cast_signed();

/// The locals a key scan walks a map's keys with.
struct KeyScan {
    key: u32,
    keys: u32,
    data: u32,
    at: u32,
    end: u32,
    length: u32,
    first: u32,
    candidate: u32,
    found: u32,
}

fn element_array(name: &str) -> Ty {
    match name {
        "__keys" => Ty::Array(Box::new(Ty::Str)),
        _ => Ty::Array(Box::new(Ty::Any)),
    }
}

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
            "__key_index" => self.key_index(args, pos)?,
            "__val_at" => self.val_at(args, pos)?,
            "__same_at" => self.same_at(args, pos)?,
            "__find" => self.find(args, pos)?,
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
    // if it points at none, and shared with the caller.
    fn cell_array(&mut self, args: &[Expr], pos: Pos, name: &str, off: u64) -> R<Ty> {
        let held = self.borrowing(&[Ty::Any], args, pos, name)?;
        let array = self.cell_array_local(off);
        self.emit([I::LocalGet(array), I::Call(self.g.h.retain)]);
        self.release_all(held);
        Ok(element_array(name))
    }

    // The cell on the stack's array at `off`, in a local, not counted.
    fn cell_array_local(&mut self, off: u64) -> u32 {
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
        ]);
        array
    }

    /// `__items(c)`, `__keys(c)` or `__vals(c)` where only an element or the
    /// length is read: the array, lent by a cell the caller holds. What
    /// [`Self::array_ref`] answers.
    pub(crate) fn cell_array_in_hand(
        &mut self,
        arr: &Expr,
        pos: Pos,
    ) -> R<Option<(u32, Ty, Option<super::func::LVal>)>> {
        let ExprKind::Call(name, args) = &arr.kind else {
            return Ok(None);
        };
        let off = match name.as_str() {
            "__items" | "__keys" if self.prelude => 8,
            "__vals" if self.prelude => 12,
            _ => return Ok(None),
        };
        let mut held = self.borrowing(&[Ty::Any], args, pos, name)?;
        let array = self.cell_array_local(off);
        let Ty::Array(element) = element_array(name) else {
            unreachable!("a cell holds arrays");
        };
        Ok(Some((array, *element, held.pop())))
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

    /// `{ "k": v, .. }` where an `any` goes: an any map of those entries in
    /// that order, each value boxed. A key written twice is refused.
    pub(crate) fn any_map(&mut self, entries: &[(Expr, Expr)], pos: Pos) -> R<()> {
        let mut seen = std::collections::BTreeSet::new();
        for (key, _) in entries {
            if let crate::ast::ExprKind::Str(text) = &key.kind
                && !seen.insert(text.as_str())
            {
                return err(key.pos, format!("the key \"{text}\" is given twice"));
            }
        }
        let list = |items: Vec<Expr>| Expr {
            kind: crate::ast::ExprKind::ArrayLit(items),
            pos,
        };
        let (keys, values): (Vec<Expr>, Vec<Expr>) = entries.iter().cloned().unzip();
        self.map_of(&[list(keys), list(values)], pos)?;
        Ok(())
    }

    // `__hash(key)`: where a map's key starts its probe, never negative: each
    // byte folded in by a large odd multiplier, or an int's mixed as one, then
    // the high half into the low, which the table's size picks from.
    fn hash(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let [key] = args else {
            return err(pos, "`__hash` takes 1 argument(s)");
        };
        let mixed = self.local(ValType::I64);
        match self.type_of(key)? {
            Ty::Str => {
                let held = self.borrowing(&[Ty::Str], args, pos, "__hash")?;
                let (text, at, end) = (
                    self.local(ValType::I32),
                    self.local(ValType::I32),
                    self.local(ValType::I32),
                );
                self.emit([
                    I::LocalTee(text),
                    I::LocalTee(at),
                    I::I32Const(8),
                    I::I32Add,
                    I::LocalSet(at),
                    I::LocalGet(at),
                    I::LocalGet(text),
                    I::I32Load(mem(4, 2)),
                    I::I32Add,
                    I::LocalSet(end),
                    I::I64Const(0),
                    I::LocalSet(mixed),
                    I::Block(BlockType::Empty),
                    I::Loop(BlockType::Empty),
                ]);
                self.charge();
                self.emit([
                    I::LocalGet(at),
                    I::LocalGet(end),
                    I::I32GeU,
                    I::BrIf(1),
                    I::LocalGet(mixed),
                    I::LocalGet(at),
                    I::I64Load8U(mem(0, 0)),
                    I::I64Add,
                    I::I64Const(1),
                    I::I64Add,
                    I::I64Const(MULTIPLIER),
                    I::I64Mul,
                    I::LocalSet(mixed),
                    I::LocalGet(at),
                    I::I32Const(1),
                    I::I32Add,
                    I::LocalSet(at),
                    I::Br(0),
                    I::End,
                    I::End,
                ]);
                self.release_all(held);
            }
            Ty::Int | Ty::Node => {
                self.expr(key)?;
                self.emit([I::I64Const(MULTIPLIER), I::I64Mul, I::LocalSet(mixed)]);
            }
            other => {
                return err(
                    pos,
                    format!("`__hash` takes a str or an int, not {}", show(&other)),
                );
            }
        }
        self.emit([
            I::LocalGet(mixed),
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
        Ok(Ty::Int)
    }

    // `__key_index(map, key)`: where `key` sits among a map cell's keys, or -1:
    // a key of another length or first byte passed over, then the same
    // string, then the rest of its bytes.
    fn key_index(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let held = self.borrowing(&[Ty::Any, Ty::Str], args, pos, "__key_index")?;
        let scan = KeyScan {
            key: self.local(ValType::I32),
            keys: self.local(ValType::I32),
            data: self.local(ValType::I32),
            at: self.local(ValType::I32),
            end: self.local(ValType::I32),
            length: self.local(ValType::I32),
            first: self.local(ValType::I32),
            candidate: self.local(ValType::I32),
            found: self.local(ValType::I64),
        };
        self.emit([
            I::LocalSet(scan.key),
            I::I32Load(mem(8, 2)),
            I::LocalTee(scan.keys),
            I::I64Const(-1),
            I::LocalSet(scan.found),
            I::If(BlockType::Empty),
            I::LocalGet(scan.key),
            I::I32Load(mem(4, 2)),
            I::LocalSet(scan.length),
            I::LocalGet(scan.keys),
            I::I32Load(mem(12, 2)),
            I::LocalTee(scan.data),
            I::LocalTee(scan.at),
            I::LocalGet(scan.keys),
            I::I32Load(mem(4, 2)),
            I::I32Const(3),
            I::I32Shl,
            I::I32Add,
            I::LocalTee(scan.end),
            I::LocalGet(scan.at),
            I::I32GtU,
            I::If(BlockType::Empty),
            I::LocalGet(scan.length),
            I::I32Eqz,
            I::If(BlockType::Empty),
        ]);
        self.key_scan(&scan, true);
        self.e(I::Else);
        self.emit([
            I::LocalGet(scan.key),
            I::I32Load8U(mem(8, 0)),
            I::LocalSet(scan.first),
        ]);
        self.key_scan(&scan, false);
        self.emit([I::End, I::End, I::End]);
        self.release_all(held);
        self.e(I::LocalGet(scan.found));
        Ok(Ty::Int)
    }

    // The keys from `at` to `end`, one at least, for an empty key or another.
    // A near miss's work is in an `if`: wasmi charges a block's fuel on entry.
    fn key_scan(&mut self, scan: &KeyScan, empty: bool) {
        self.emit([I::Block(BlockType::Empty), I::Loop(BlockType::Empty)]);
        self.charge();
        self.emit([
            I::LocalGet(scan.at),
            I::I32Load(mem(0, 2)),
            I::LocalTee(scan.candidate),
            I::I32Load(mem(4, 2)),
            I::LocalGet(scan.length),
            I::I32Eq,
            I::If(BlockType::Empty),
        ]);
        let found = [
            I::LocalGet(scan.at),
            I::LocalGet(scan.data),
            I::I32Sub,
            I::I32Const(3),
            I::I32ShrU,
            I::I64ExtendI32U,
            I::LocalSet(scan.found),
        ];
        if empty {
            self.emit(found);
            self.e(I::Br(2));
        } else {
            self.emit([
                I::LocalGet(scan.candidate),
                I::I32Load8U(mem(8, 0)),
                I::LocalGet(scan.first),
                I::I32Eq,
                I::If(BlockType::Empty),
                I::LocalGet(scan.candidate),
                I::LocalGet(scan.key),
                I::I32Eq,
                I::If(BlockType::Result(ValType::I32)),
                I::I32Const(1),
                I::Else,
                I::LocalGet(scan.candidate),
                I::I32Const(9),
                I::I32Add,
                I::LocalGet(scan.key),
                I::I32Const(9),
                I::I32Add,
                I::LocalGet(scan.length),
                I::I32Const(1),
                I::I32Sub,
                I::Call(self.g.h.same_bytes),
                I::End,
                I::If(BlockType::Empty),
            ]);
            self.emit(found);
            self.emit([I::Br(4), I::End, I::End]);
        }
        self.emit([
            I::End,
            I::LocalGet(scan.at),
            I::I32Const(8),
            I::I32Add,
            I::LocalTee(scan.at),
            I::LocalGet(scan.end),
            I::I32LtU,
            I::BrIf(0),
            I::End,
            I::End,
        ]);
    }

    // `__val_at(map, index)`: a map cell's value at an index `__key_index`
    // found, counted once more for the caller.
    fn val_at(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let held = self.borrowing(&[Ty::Any, Ty::Int], args, pos, "__val_at")?;
        let index = self.local(ValType::I64);
        self.emit([
            I::LocalSet(index),
            I::I32Load(mem(12, 2)),
            I::I32Load(mem(12, 2)),
            I::LocalGet(index),
            I::I32WrapI64,
            I::I32Const(3),
            I::I32Shl,
            I::I32Add,
            I::I32Load(mem(0, 2)),
            I::Call(self.g.h.retain),
        ]);
        self.release_all(held);
        Ok(Ty::Any)
    }

    // `__same_at(text, at, needle)`: whether `needle` sits in `text` at `at`,
    // where the caller has seen it fits.
    fn same_at(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let held = self.borrowing(&[Ty::Str, Ty::Int, Ty::Str], args, pos, "__same_at")?;
        let needle = self.local(ValType::I32);
        self.emit([
            I::LocalSet(needle),
            I::I32WrapI64,
            I::I32Add,
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(needle),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(needle),
            I::I32Load(mem(4, 2)),
            I::Call(self.g.h.same_bytes),
        ]);
        self.release_all(held);
        Ok(Ty::Bool)
    }

    // `__find(text, needle, from)`: the first place at or after `from` that
    // `needle` sits in `text`, or -1. Its first byte is looked for a word at a
    // time, its bytes compared where one is found; an empty needle is at `from`.
    fn find(&mut self, args: &[Expr], pos: Pos) -> R<Ty> {
        let held = self.borrowing(&[Ty::Str, Ty::Str, Ty::Int], args, pos, "__find")?;
        let (text, needle, at, last, end) = (
            self.local(ValType::I32),
            self.local(ValType::I32),
            self.local(ValType::I32),
            self.local(ValType::I32),
            self.local(ValType::I32),
        );
        let (length, first, pattern, word, found) = (
            self.local(ValType::I32),
            self.local(ValType::I32),
            self.local(ValType::I64),
            self.local(ValType::I64),
            self.local(ValType::I64),
        );
        self.emit([
            I::I32WrapI64,
            I::LocalSet(at),
            I::LocalSet(needle),
            I::LocalTee(text),
            I::I32Load(mem(4, 2)),
            I::LocalTee(end),
            I::LocalGet(needle),
            I::I32Load(mem(4, 2)),
            I::LocalTee(length),
            I::I32Sub,
            I::LocalSet(last),
            I::LocalGet(text),
            I::I32Const(8),
            I::I32Add,
            I::LocalSet(text),
            I::I64Const(-1),
            I::LocalSet(found),
            I::Block(BlockType::Empty),
            I::LocalGet(at),
            I::LocalGet(last),
            I::I32GtS,
            I::BrIf(0),
            I::LocalGet(length),
            I::I32Eqz,
            I::If(BlockType::Empty),
            I::LocalGet(at),
            I::I64ExtendI32U,
            I::LocalSet(found),
            I::Br(1),
            I::End,
            I::LocalGet(needle),
            I::I32Load8U(mem(8, 0)),
            I::LocalTee(first),
            I::I64ExtendI32U,
            I::I64Const(BYTES_LOW),
            I::I64Mul,
            I::LocalSet(pattern),
            I::Loop(BlockType::Empty),
        ]);
        self.charge();
        self.first_byte_scan(text, at, end, pattern, word);
        self.emit([
            // A byte at a time from there.
            I::LocalGet(at),
            I::LocalGet(last),
            I::I32GtS,
            I::BrIf(1),
            I::LocalGet(text),
            I::LocalGet(at),
            I::I32Add,
            I::I32Load8U(mem(0, 0)),
            I::LocalGet(first),
            I::I32Eq,
            I::If(BlockType::Empty),
            I::LocalGet(text),
            I::LocalGet(at),
            I::I32Add,
            I::I32Const(1),
            I::I32Add,
            I::LocalGet(needle),
            I::I32Const(9),
            I::I32Add,
            I::LocalGet(length),
            I::I32Const(1),
            I::I32Sub,
            I::Call(self.g.h.same_bytes),
            I::If(BlockType::Empty),
            I::LocalGet(at),
            I::I64ExtendI32U,
            I::LocalSet(found),
            I::Br(3),
            I::End,
            I::End,
            I::LocalGet(at),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(at),
            I::Br(0),
            I::End,
            I::End,
        ]);
        self.release_all(held);
        self.e(I::LocalGet(found));
        Ok(Ty::Int)
    }

    // A word of the text at a time, while there are eight bytes from `at`: on
    // to the first that may be the needle's first byte, or to the tail.
    fn first_byte_scan(&mut self, text: u32, at: u32, end: u32, pattern: u32, word: u32) {
        self.emit([I::Block(BlockType::Empty), I::Loop(BlockType::Empty)]);
        self.charge();
        self.emit([
            I::LocalGet(at),
            I::I32Const(8),
            I::I32Add,
            I::LocalGet(end),
            I::I32GtU,
            I::BrIf(1),
            I::LocalGet(text),
            I::LocalGet(at),
            I::I32Add,
            I::I64Load(mem(0, 0)),
            I::LocalGet(pattern),
            I::I64Xor,
            I::LocalTee(word),
            I::I64Const(BYTES_LOW),
            I::I64Sub,
            I::LocalGet(word),
            I::I64Const(-1),
            I::I64Xor,
            I::I64And,
            I::I64Const(BYTES_HIGH),
            I::I64And,
            I::LocalTee(word),
            I::I64Eqz,
            I::If(BlockType::Empty),
            I::LocalGet(at),
            I::I32Const(8),
            I::I32Add,
            I::LocalSet(at),
            I::Br(1),
            I::End,
            I::LocalGet(at),
            I::LocalGet(word),
            I::I64Ctz,
            I::I32WrapI64,
            I::I32Const(3),
            I::I32ShrU,
            I::I32Add,
            I::LocalSet(at),
            I::End,
            I::End,
        ]);
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
