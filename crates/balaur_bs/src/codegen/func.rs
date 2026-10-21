// One function's code, type-checked as it is emitted. Every expression leaves an
// owned value, and a frame releases what it owns at every exit.

use super::rt::{
    DEBUG_STACK, FRAME_HEADER, G_DBG_BASE, G_DBG_BREAKS, G_DBG_FLAGS, G_DBG_SP, load, mem, store,
};
use super::{Gen, Site};
use crate::ast::{BinOp, Expr, ExprKind, Pos, Ty};
use crate::{Error, Probe, VarInfo};
use std::collections::HashMap;
use wasm_encoder::{BlockType, Instruction as I, ValType};

pub(crate) type R<T> = Result<T, Error>;

pub(crate) fn err<T>(pos: Pos, msg: impl Into<String>) -> R<T> {
    Err(Error {
        pos,
        msg: msg.into(),
    })
}

#[derive(Clone, Debug)]
pub(crate) struct LVal {
    pub ty: Ty,
    pub slots: Vec<u32>,
}

pub(crate) enum Mode {
    Normal { ret: Ty },
    Poll { ret: Ty, handle: u32 },
}

/// Where an element is: in a local, or a hoisted array's data pointer plus
/// the loop's scaled index, added where it is used rather than kept.
#[derive(Clone, Copy)]
pub(crate) enum Addr {
    Local(u32),
    Sum(u32, u32),
}

pub(crate) struct MemPlace {
    pub addr: Addr,
    pub ty: Ty,
    pub off: u64,
    pub release: Option<LVal>,
    /// In a class object, which never moves while held: a call may run
    /// between reading the place and writing it back. An element's array
    /// can grow under a call, so its place is not.
    pub stable: bool,
}

/// Where a `&mut` argument goes back to: a local, or a place in an object.
pub(crate) enum RefPlace {
    Local(LVal),
    Mem(MemPlace),
}

pub(crate) struct FnGen<'a> {
    pub g: &'a Gen,
    pub nparams: u32,
    pub locals: Vec<ValType>,
    pub scopes: Vec<Vec<(String, LVal)>>,
    pub code: Vec<I<'static>>,
    pub depth: u32,
    pub loops: Vec<(u32, u32)>,
    pub self_lv: Option<LVal>,
    /// The `&mut` parameters, handed back after `self` when the call ends.
    pub refs: Vec<LVal>,
    /// Each expression's type once worked out, by where it sits: a chain
    /// `a.f().g()` asks its receiver's type at every link. Cleared when a
    /// name is declared, which can change what a name means.
    pub typed: HashMap<(usize, u32, u32), Ty>,
    pub mode: Mode,
    pub owned: Vec<(u32, Ty)>,
    pub dbg: Option<(u32, u32)>,
    pub patches: Vec<(usize, String)>,
    pub sites: HashMap<u32, Site>,
    pub fast: Vec<super::hoist::Fast>,
}

impl<'a> FnGen<'a> {
    pub(crate) fn new(g: &'a Gen, params: usize, mode: Mode) -> Self {
        FnGen {
            g,
            nparams: params as u32,
            locals: vec![],
            scopes: vec![vec![]],
            code: vec![],
            depth: 0,
            loops: vec![],
            self_lv: None,
            refs: vec![],
            typed: HashMap::new(),
            mode,
            owned: vec![],
            dbg: None,
            patches: vec![],
            fast: vec![],
            sites: HashMap::new(),
        }
    }

    pub(crate) fn e(&mut self, i: I<'static>) {
        self.code.push(i);
    }
    pub(crate) fn emit(&mut self, ins: impl IntoIterator<Item = I<'static>>) {
        self.code.extend(ins);
    }
    pub(crate) fn local(&mut self, vt: ValType) -> u32 {
        self.locals.push(vt);
        self.nparams + self.locals.len() as u32 - 1
    }
    pub(crate) fn alloc(&mut self, ty: &Ty) -> LVal {
        let slots = self
            .g
            .slots(ty)
            .into_iter()
            .map(|(vt, _)| self.local(vt))
            .collect();
        LVal {
            ty: ty.clone(),
            slots,
        }
    }
    pub(crate) fn own(&mut self, lv: &LVal) {
        for (s, (_, heap)) in lv.slots.iter().zip(self.g.slots(&lv.ty)) {
            if let Some(h) = heap {
                self.owned.push((*s, h));
            }
        }
    }
    pub(crate) fn temp(&mut self, ty: &Ty) -> LVal {
        let lv = self.alloc(ty);
        self.own(&lv);
        lv
    }
    pub(crate) fn declare(&mut self, name: &str, lv: LVal) {
        self.typed.clear();
        self.scopes
            .last_mut()
            .expect("a function always has its outer scope")
            .push((name.to_string(), lv));
    }
    pub(crate) fn lookup(&self, name: &str) -> Option<LVal> {
        self.scopes.iter().rev().find_map(|s| {
            s.iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, l)| l.clone())
        })
    }
    pub(crate) fn get_raw(&mut self, lv: &LVal) {
        for s in lv.slots.clone() {
            self.e(I::LocalGet(s));
        }
    }
    /// `lv`'s value moved onto the stack: its heap slots are zeroed, so the
    /// release a temp gets when the function ends finds nothing to free.
    pub(crate) fn take_out(&mut self, lv: &LVal) {
        for (s, (_, heap)) in lv.slots.clone().into_iter().zip(self.g.slots(&lv.ty)) {
            self.e(I::LocalGet(s));
            if heap.is_some() {
                self.emit([I::I32Const(0), I::LocalSet(s)]);
            }
        }
    }
    pub(crate) fn get_owned(&mut self, lv: &LVal) {
        for (s, (_, heap)) in lv.slots.clone().into_iter().zip(self.g.slots(&lv.ty)) {
            self.e(I::LocalGet(s));
            if heap.is_some() {
                self.e(I::Call(self.g.h.retain));
            }
        }
    }
    pub(crate) fn set_raw(&mut self, lv: &LVal) {
        for s in lv.slots.clone().into_iter().rev() {
            self.e(I::LocalSet(s));
        }
    }
    // The old value is released once the new one is in hand: `x = x` stays alive.
    pub(crate) fn set_owned(&mut self, lv: &LVal) {
        let slots = self.g.slots(&lv.ty);
        for (s, (_, heap)) in lv.slots.clone().into_iter().zip(slots).rev() {
            if let Some(h) = heap {
                self.e(I::LocalGet(s));
                self.e(I::Call(self.g.dec_fn(&h)));
            }
            self.e(I::LocalSet(s));
        }
    }
    pub(crate) fn release(&mut self, lv: &LVal) {
        for (s, (_, heap)) in lv.slots.clone().into_iter().zip(self.g.slots(&lv.ty)) {
            if let Some(h) = heap {
                self.emit([
                    I::LocalGet(s),
                    I::Call(self.g.dec_fn(&h)),
                    I::I32Const(0),
                    I::LocalSet(s),
                ]);
            }
        }
    }
    pub(crate) fn discard(&mut self, ty: &Ty) {
        for (_, heap) in self.g.slots(ty).into_iter().rev() {
            match heap {
                Some(h) => self.e(I::Call(self.g.dec_fn(&h))),
                None => self.e(I::Drop),
            }
        }
    }
    pub(crate) fn release_owned(&mut self) {
        for (s, h) in self.owned.clone() {
            self.emit([I::LocalGet(s), I::Call(self.g.dec_fn(&h))]);
        }
    }
    pub(crate) fn open(&mut self, i: I<'static>) -> u32 {
        self.e(i);
        self.depth += 1;
        self.depth
    }
    pub(crate) fn close(&mut self) {
        self.e(I::End);
        self.depth -= 1;
    }
    pub(crate) fn br(&mut self, level: u32) {
        self.e(I::Br(self.depth - level));
    }

    pub(crate) fn field_of(&self, lv: &LVal, field: &str, pos: Pos) -> R<LVal> {
        let (off, ty) = self.field_slot(&lv.ty, field, pos)?;
        let n = self.g.slots(&ty).len();
        Ok(LVal {
            ty,
            slots: lv.slots[off..off + n].to_vec(),
        })
    }
    fn field_slot(&self, ty: &Ty, field: &str, pos: Pos) -> R<(usize, Ty)> {
        let Ty::Struct(s) = ty else {
            return err(pos, format!("{} has no field `{field}`", show(ty)));
        };
        let mut off = 0;
        for (f, t) in &self.g.structs[s] {
            if f == field {
                return Ok((off, t.clone()));
            }
            off += self.g.slots(t).len();
        }
        err(pos, format!("struct {s} has no field `{field}`"))
    }

    pub(crate) fn place(&self, e: &Expr) -> R<Option<LVal>> {
        Ok(match &e.kind {
            ExprKind::Var(n) => match self.lookup(n) {
                Some(lv) => Some(lv),
                None if self.g.consts.contains_key(n) => None,
                None => return err(e.pos, format!("unknown name `{n}`")),
            },
            ExprKind::SelfRef => Some(self.self_lv.clone().ok_or(Error {
                pos: e.pos,
                msg: "`self` outside an impl".into(),
            })?),
            ExprKind::Field(b, f) => match self.place(b)? {
                // A node's component, or an `any`'s key, is read, not a place;
                // a class's field is in its object.
                Some(lv) if matches!(lv.ty, Ty::Node | Ty::Any) => None,
                Some(lv) if self.g.is_class(&lv.ty).is_some() => None,
                Some(lv) => Some(self.field_of(&lv, f, e.pos)?),
                None => None,
            },
            _ => None,
        })
    }

    // An array element, or a field of one, addressed in memory.
    pub(crate) fn mem_place(&mut self, e: &Expr) -> R<Option<MemPlace>> {
        match &e.kind {
            ExprKind::Index(arr, idx) if self.fast_element(arr, idx).is_some() => {
                let (data, scaled) = self.fast_element(arr, idx).unwrap_or_default();
                let Some(LVal {
                    ty: Ty::Array(et), ..
                }) = self.place(arr)?
                else {
                    return Ok(None);
                };
                Ok(Some(MemPlace {
                    addr: Addr::Sum(data, scaled),
                    ty: *et,
                    off: 0,
                    release: None,
                    stable: false,
                }))
            }
            ExprKind::Index(arr, idx) => {
                let (a, et, release) = self.array_ref(arr, e.pos)?;
                let i = self.index_local(idx)?;
                let esz = self.g.elem_size(&et) as i32;
                self.emit([
                    I::LocalGet(i),
                    I::LocalGet(a),
                    I::I32Load(mem(4, 2)),
                    I::I64ExtendI32U,
                    I::I64GeU,
                ]);
                self.emit([I::If(BlockType::Empty), I::Unreachable, I::End]);
                self.emit([
                    I::LocalGet(a),
                    I::I32Load(mem(12, 2)),
                    I::LocalGet(i),
                    I::I32WrapI64,
                    I::I32Const(esz),
                    I::I32Mul,
                    I::I32Add,
                ]);
                let addr = self.local(ValType::I32);
                self.e(I::LocalSet(addr));
                Ok(Some(MemPlace {
                    addr: Addr::Local(addr),
                    ty: et,
                    off: 0,
                    release,
                    stable: false,
                }))
            }
            // A field of a class variable: in the object the variable holds.
            ExprKind::Field(b, f)
                if self
                    .place(b)?
                    .is_some_and(|lv| self.g.is_class(&lv.ty).is_some()) =>
            {
                let Some(lv) = self.place(b)? else {
                    return Ok(None);
                };
                Ok(Some(self.object_place(
                    lv.slots[0],
                    &lv.ty,
                    f,
                    None,
                    e.pos,
                )?))
            }
            ExprKind::Field(b, f)
                if self.place(b)?.is_none()
                    && matches!(b.kind, ExprKind::Index(..) | ExprKind::Field(..)) =>
            {
                let Some(mut mp) = self.mem_place(b)? else {
                    return Ok(None);
                };
                // A class held in memory: its object, kept while it is used.
                if self.g.is_class(&mp.ty).is_some() {
                    let obj = self.temp(&mp.ty);
                    self.push_addr(mp.addr);
                    self.emit([
                        load(ValType::I32, mp.off),
                        I::Call(self.g.h.retain),
                        I::LocalSet(obj.slots[0]),
                    ]);
                    if let Some(r) = &mp.release {
                        self.release(r);
                    }
                    let ty = obj.ty.clone();
                    return Ok(Some(self.object_place(
                        obj.slots[0],
                        &ty,
                        f,
                        Some(obj),
                        e.pos,
                    )?));
                }
                let (slot, ty) = self.field_slot(&mp.ty, f, e.pos)?;
                mp.off += 8 * slot as u64;
                mp.ty = ty;
                Ok(Some(mp))
            }
            _ => Ok(None),
        }
    }

    /// Field `f` of the class object whose pointer is in local `obj`.
    pub(crate) fn object_place(
        &self,
        obj: u32,
        class: &Ty,
        f: &str,
        release: Option<LVal>,
        pos: Pos,
    ) -> R<MemPlace> {
        let Some(s) = self.g.is_class(class) else {
            return err(pos, format!("{} is no class", show(class)));
        };
        let (off, ty) = self.g.class_field(s, f).ok_or_else(|| Error {
            pos,
            msg: format!("class {s} has no field `{f}`"),
        })?;
        Ok(MemPlace {
            addr: Addr::Local(obj),
            ty,
            off,
            release,
            stable: true,
        })
    }

    /// A place's slots on the stack as they are: lent, not retained.
    pub(crate) fn mem_get_raw(&mut self, mp: &MemPlace) {
        for (j, (vt, _)) in self.g.slots(&mp.ty).into_iter().enumerate() {
            self.push_addr(mp.addr);
            self.e(load(vt, mp.off + 8 * j as u64));
        }
    }
    /// The slots on the stack stored as they are: what was lent comes back.
    pub(crate) fn mem_set_raw(&mut self, mp: &MemPlace) {
        let tmp = self.alloc(&mp.ty);
        self.set_raw(&tmp);
        for (j, (vt, _)) in self.g.slots(&mp.ty).into_iter().enumerate() {
            self.push_addr(mp.addr);
            self.emit([I::LocalGet(tmp.slots[j]), store(vt, mp.off + 8 * j as u64)]);
        }
    }

    // An index is an int; a float one counts toward zero, as `as int` does.
    pub(crate) fn index_local(&mut self, idx: &Expr) -> R<u32> {
        if let Some(LVal { ty: Ty::Int, slots }) = self.place(idx)? {
            return Ok(slots[0]);
        }
        match self.expr(idx)? {
            Ty::Int => {}
            Ty::Float => self.e(I::I64TruncSatF64S),
            Ty::Any => {
                self.unbox_any(&Ty::Int);
            }
            t => return err(idx.pos, format!("an index is int, not {}", show(&t))),
        }
        let l = self.local(ValType::I64);
        self.e(I::LocalSet(l));
        Ok(l)
    }

    // The array behind `arr`: a local read in place, or a temporary to release after use.
    pub(crate) fn array_ref(&mut self, arr: &Expr, pos: Pos) -> R<(u32, Ty, Option<LVal>)> {
        if let Some(lv) = self.place(arr)? {
            return match lv.ty {
                Ty::Array(et) => Ok((lv.slots[0], *et, None)),
                t => err(pos, format!("{} cannot be indexed", show(&t))),
            };
        }
        if let Some(mp) = self.mem_place(arr)? {
            let Ty::Array(et) = mp.ty.clone() else {
                return err(pos, format!("{} cannot be indexed", show(&mp.ty)));
            };
            let l = self.local(ValType::I32);
            self.push_addr(mp.addr);
            self.emit([load(ValType::I32, mp.off), I::LocalSet(l)]);
            return Ok((l, *et, mp.release));
        }
        let t = self.expr(arr)?;
        let Ty::Array(et) = t.clone() else {
            return err(pos, format!("{} cannot be indexed", show(&t)));
        };
        let tmp = self.temp(&t);
        self.set_raw(&tmp);
        Ok((tmp.slots[0], *et, Some(tmp)))
    }

    /// One unit of fuel, in a metered module: a loop pass or a call. The
    /// module traps when none is left, as a metering engine would.
    pub(crate) fn charge(&mut self) {
        if !self.g.meter {
            return;
        }
        let fuel = super::rt::G_FUEL;
        self.emit([
            I::GlobalGet(fuel),
            I::I32Const(1),
            I::I32Sub,
            I::GlobalSet(fuel),
            I::GlobalGet(fuel),
            I::I32Const(0),
            I::I32LtS,
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
        ]);
    }

    pub(crate) fn push_addr(&mut self, a: Addr) {
        match a {
            Addr::Local(l) => self.e(I::LocalGet(l)),
            Addr::Sum(base, off) => self.emit([I::LocalGet(base), I::LocalGet(off), I::I32Add]),
        }
    }

    /// An element of one plain slot (a number, a bool, a node): stored straight
    /// from what `value` leaves, with no temporary to hold it.
    pub(crate) fn plain_slot(&self, mp: &MemPlace) -> Option<ValType> {
        match self.g.slots(&mp.ty).as_slice() {
            [(vt, None)] => Some(*vt),
            _ => None,
        }
    }

    pub(crate) fn mem_read(&mut self, mp: &MemPlace) {
        for (j, (vt, heap)) in self.g.slots(&mp.ty).into_iter().enumerate() {
            self.push_addr(mp.addr);
            self.e(load(vt, mp.off + 8 * j as u64));
            if heap.is_some() {
                self.e(I::Call(self.g.h.retain));
            }
        }
    }
    // The new value is on the stack; the element it replaces is released.
    pub(crate) fn mem_write(&mut self, mp: &MemPlace) {
        let slots = self.g.slots(&mp.ty);
        let tmp = self.alloc(&mp.ty);
        self.set_raw(&tmp);
        for (j, (vt, heap)) in slots.into_iter().enumerate() {
            let off = mp.off + 8 * j as u64;
            if let Some(h) = heap {
                self.push_addr(mp.addr);
                self.emit([load(vt, off), I::Call(self.g.dec_fn(&h))]);
            }
            self.push_addr(mp.addr);
            self.emit([I::LocalGet(tmp.slots[j]), store(vt, off)]);
        }
    }

    pub(crate) fn coerce(&mut self, got: &Ty, want: &Ty, pos: Pos) -> R<()> {
        if got == want {
            Ok(())
        } else if *got == Ty::Int && *want == Ty::Float {
            self.e(I::F64ConvertI64S);
            Ok(())
        } else if (*want == Ty::Any && self.box_any(got))
            || (*got == Ty::Any && self.unbox_any(want))
        {
            Ok(())
        } else if let (Ty::Array(a), Ty::Array(b)) = (got, want)
            && (**a == Ty::Any || **b == Ty::Any || **b == Ty::Optional(a.clone()))
        {
            // A list of one kind where a list of another goes: a copy, each
            // element boxed or read back, through the host.
            self.box_by_host(got);
            self.unbox_any(want);
            Ok(())
        } else if let Ty::Optional(inner) = want
            && (**inner == *got || (*got == Ty::Int && **inner == Ty::Float))
        {
            // A value where a `T?` goes is there: its slots, then a set flag.
            self.coerce(got, inner, pos)?;
            self.e(I::I32Const(1));
            Ok(())
        } else {
            err(pos, format!("expected {}, got {}", show(want), show(got)))
        }
    }

    // The value on the stack, wrapped in a fresh `any` cell that takes it
    // over; false for a kind an `any` does not hold.
    pub(crate) fn box_any(&mut self, got: &Ty) -> bool {
        use crate::anys::{BOOL, FLOAT, INT, LIST, NODE, STR};
        let (tag, widen) = match got {
            Ty::Int => (INT, None),
            Ty::Node => (NODE, None),
            Ty::Float => (FLOAT, Some(I::I64ReinterpretF64)),
            Ty::Bool => (BOOL, Some(I::I64ExtendI32U)),
            Ty::Str => (STR, Some(I::I64ExtendI32U)),
            Ty::Array(et) if **et == Ty::Any => (LIST, Some(I::I64ExtendI32U)),
            Ty::Array(_) | Ty::Struct(_) | Ty::Optional(_) => {
                self.box_by_host(got);
                return true;
            }
            _ => return false,
        };
        if let Some(w) = widen {
            self.e(w);
        }
        self.cell(tag);
        true
    }

    /// The `any` on the stack as a `want`: a scalar read by its `as_` method,
    /// a struct, list or optional rebuilt by the host into scratch memory.
    /// The `any` is released after; false for a type it cannot become.
    pub(crate) fn unbox_any(&mut self, want: &Ty) -> bool {
        let read = match want {
            Ty::Int => Some("as_int"),
            Ty::Float => Some("as_float"),
            Ty::Bool => Some("as_bool"),
            Ty::Str => Some("as_str"),
            Ty::Node => Some("as_node"),
            Ty::Struct(_) | Ty::Array(_) | Ty::Optional(_) => None,
            _ => return false,
        };
        let v = self.temp(&Ty::Any);
        self.set_raw(&v);
        if let Some(m) = read {
            self.get_raw(&v);
            self.e(I::Call(self.g.imp.anys[m]));
        } else {
            let slots = self.g.slots(want);
            let addr = self.local(ValType::I32);
            self.emit([
                I::I32Const(8 * slots.len() as i32),
                I::Call(self.g.h.alloc),
                I::LocalSet(addr),
            ]);
            let text = self.g.string(&show(want));
            self.e(I::I32Const(text as i32));
            self.get_raw(&v);
            self.emit([I::LocalGet(addr), I::Call(self.g.imp.unboxing)]);
            for (j, (vt, _)) in slots.into_iter().enumerate() {
                self.emit([I::LocalGet(addr), super::rt::load(vt, 8 * j as u64)]);
            }
            self.emit([I::LocalGet(addr), I::Call(self.g.h.free)]);
        }
        self.release(&v);
        true
    }

    // A list, struct or optional in an `any`: its slots written to scratch
    // memory and read back by the host, which builds the cells. The value
    // itself is released after.
    fn box_by_host(&mut self, got: &Ty) {
        let v = self.temp(got);
        self.set_raw(&v);
        let slots = self.g.slots(got);
        let addr = self.local(ValType::I32);
        self.emit([
            I::I32Const(8 * slots.len() as i32),
            I::Call(self.g.h.alloc),
            I::LocalSet(addr),
        ]);
        for (j, (vt, _)) in slots.into_iter().enumerate() {
            self.emit([
                I::LocalGet(addr),
                I::LocalGet(v.slots[j]),
                super::rt::store(vt, 8 * j as u64),
            ]);
        }
        let text = self.g.string(&show(got));
        self.emit([
            I::I32Const(text as i32),
            I::LocalGet(addr),
            I::Call(self.g.imp.boxing),
            I::LocalGet(addr),
            I::Call(self.g.h.free),
        ]);
        self.release(&v);
    }

    /// A fresh `any` cell of `tag`, taking the i64 payload on the stack.
    pub(crate) fn cell(&mut self, tag: i32) {
        use crate::anys::CELL;
        let payload = self.local(ValType::I64);
        let cell = self.local(ValType::I32);
        self.emit([
            I::LocalSet(payload),
            I::I32Const(CELL),
            I::Call(self.g.h.alloc),
            I::LocalTee(cell),
            I::I32Const(1),
            I::I32Store(mem(0, 2)),
            I::LocalGet(cell),
            I::I32Const(tag),
            I::I32Store(mem(4, 2)),
            I::LocalGet(cell),
            I::LocalGet(payload),
            I::I64Store(mem(8, 3)),
            I::LocalGet(cell),
        ]);
    }

    pub(crate) fn expr_as(&mut self, e: &Expr, want: &Ty) -> R<()> {
        if let (ExprKind::ArrayLit(items), Ty::Array(et)) = (&e.kind, want) {
            return self.array_lit(items, Some(et), e.pos).map(|_| ());
        }
        // A list literal where a `[T]?` goes: the list, there.
        if let (ExprKind::ArrayLit(_), Ty::Optional(inner)) = (&e.kind, want)
            && matches!(**inner, Ty::Array(_))
        {
            self.expr_as(e, inner)?;
            self.e(I::I32Const(1));
            return Ok(());
        }
        // `[]` where an `any` goes: an empty list of anys.
        if let (ExprKind::ArrayLit(items), Ty::Any) = (&e.kind, want)
            && items.is_empty()
        {
            let list = Ty::Array(Box::new(Ty::Any));
            self.array_lit(items, Some(&Ty::Any), e.pos)?;
            return self.coerce(&list, want, e.pos);
        }
        if let ExprKind::Nil = e.kind {
            return self.nil_as(want, e.pos);
        }
        if let ExprKind::If(c, then, els) = &e.kind
            && matches!(want, Ty::Optional(_) | Ty::Any)
        {
            return self.if_as(c, then, els.as_ref(), want, e.pos);
        }
        let got = self.expr(e)?;
        self.coerce(&got, want, e.pos)
    }

    fn visible(&self) -> Vec<(String, LVal)> {
        let mut vars: Vec<(String, LVal)> = vec![];
        if let Some(s) = &self.self_lv {
            vars.push(("self".into(), s.clone()));
        }
        for (n, lv) in self.scopes.iter().flatten() {
            if !n.starts_with('$') {
                vars.retain(|(m, _)| m != n);
                vars.push((n.clone(), lv.clone()));
            }
        }
        vars
    }

    fn visible_slots(&self) -> Vec<(u32, ValType, bool)> {
        let mut slots = vec![];
        for (_, lv) in self.visible() {
            for (s, (vt, h)) in lv.slots.iter().zip(self.g.slots(&lv.ty)) {
                slots.push((*s, vt, h.is_some()));
            }
        }
        slots
    }

    /// Debug builds: the variables in scope go to the shadow frame, so a stop in
    /// a deeper call shows this frame as it was at the call.
    pub(crate) fn spill(&mut self) {
        let Some((frame, _)) = self.dbg else { return };
        for (s, vt, _) in self.visible_slots() {
            self.emit([
                I::LocalGet(frame),
                I::LocalGet(s),
                store(vt, slot_offset(s)),
            ]);
        }
    }

    // Debug builds: note the statement, spill and stop at a breakpoint or a
    // step (flag 1), spill alone under flag 2; a stop's edits are read back.
    pub(crate) fn probe(&mut self, pos: Pos) {
        let Some((frame, func)) = self.dbg else {
            return;
        };
        let vars = self
            .visible()
            .iter()
            .map(|(n, lv)| VarInfo {
                name: n.clone(),
                ty: lv.ty.clone(),
                offset: slot_offset(lv.slots.first().copied().unwrap_or(0)) as u32,
            })
            .collect();
        let id = self.g.probe(Probe {
            func,
            line: pos.line,
            col: pos.col,
            vars,
        });
        self.emit([
            I::LocalGet(frame),
            I::I32Const(id as i32),
            I::I32Store(mem(4, 2)),
        ]);
        self.emit([
            I::GlobalGet(G_DBG_BREAKS),
            I::I32Load8U(mem(u64::from(id), 0)),
            I::GlobalGet(G_DBG_FLAGS),
            I::I32Or,
        ]);
        self.e(I::If(BlockType::Empty));
        self.spill();
        self.emit([
            I::GlobalGet(G_DBG_BREAKS),
            I::I32Load8U(mem(u64::from(id), 0)),
            I::GlobalGet(G_DBG_FLAGS),
            I::I32Const(1),
            I::I32And,
            I::I32Or,
        ]);
        self.e(I::If(BlockType::Empty));
        self.emit([I::I32Const(id as i32), I::Call(self.g.imp.dbg_probe)]);
        for (s, vt, heap) in self.visible_slots() {
            if !heap {
                self.emit([I::LocalGet(frame), load(vt, slot_offset(s)), I::LocalSet(s)]);
            }
        }
        self.e(I::End);
        self.e(I::End);
    }

    /// A shadow frame: [function][statement][size][pad], then one slot per local.
    pub(crate) fn dbg_enter(&mut self, size: u32) -> Vec<I<'static>> {
        let Some((frame, func)) = self.dbg else {
            return vec![];
        };
        let size = FRAME_HEADER + size;
        let mut out = vec![
            I::GlobalGet(G_DBG_SP),
            I::LocalTee(frame),
            I::I32Const(size as i32),
            I::I32Add,
            I::GlobalSet(G_DBG_SP),
        ];
        out.extend([
            I::GlobalGet(G_DBG_SP),
            I::GlobalGet(G_DBG_BASE),
            I::I32Sub,
            I::I32Const(DEBUG_STACK as i32),
            I::I32GtU,
        ]);
        out.extend([I::If(BlockType::Empty), I::Unreachable, I::End]);
        out.extend([
            I::LocalGet(frame),
            I::I32Const(func as i32),
            I::I32Store(mem(0, 2)),
        ]);
        out.extend([I::LocalGet(frame), I::I32Const(-1), I::I32Store(mem(4, 2))]);
        out.extend([
            I::LocalGet(frame),
            I::I32Const(size as i32),
            I::I32Store(mem(8, 2)),
        ]);
        out
    }

    pub(crate) fn dbg_leave(&mut self) {
        if let Some((frame, _)) = self.dbg {
            self.emit([I::LocalGet(frame), I::GlobalSet(G_DBG_SP)]);
        }
    }

    /// Leaves the function with `v`: a method also hands `self` back; an async
    /// fn writes its result into its frame.
    pub(crate) fn ret(&mut self, v: Option<&Expr>, pos: Pos, explicit: bool) -> R<()> {
        let want = match &self.mode {
            Mode::Normal { ret } | Mode::Poll { ret, .. } => ret.clone(),
        };
        match v {
            Some(e) => self.expr_as(e, &want)?,
            None if want != Ty::Void => {
                return err(pos, format!("this function returns {}", show(&want)));
            }
            None => {}
        }
        self.leave(&want, explicit);
        Ok(())
    }

    pub(crate) fn leave(&mut self, ty: &Ty, explicit: bool) {
        let tmp = self.alloc(ty);
        self.set_raw(&tmp);
        self.release_owned();
        self.dbg_leave();
        if let Mode::Poll { .. } = self.mode {
            for (j, (vt, _)) in self.g.slots(ty).into_iter().enumerate() {
                self.emit([
                    I::LocalGet(0),
                    I::LocalGet(tmp.slots[j]),
                    store(vt, 8 + 8 * j as u64),
                ]);
            }
            self.e(I::I64Const(0));
            if let Some(s) = self.self_lv.clone() {
                self.get_raw(&s);
            }
        } else {
            if let Some(s) = self.self_lv.clone() {
                self.get_raw(&s);
            }
            for r in self.refs.clone() {
                self.get_raw(&r);
            }
            self.get_raw(&tmp);
        }
        if explicit {
            self.e(I::Return);
        }
    }
}

fn slot_offset(local: u32) -> u64 {
    u64::from(FRAME_HEADER) + 8 * u64::from(local)
}

pub(crate) fn show(t: &Ty) -> String {
    match t {
        Ty::Int => "int".into(),
        Ty::Float => "float".into(),
        Ty::Bool => "bool".into(),
        Ty::Str => "str".into(),
        Ty::Node => "Node".into(),
        Ty::Void => "nothing".into(),
        Ty::Array(e) => format!("[{}]", show(e)),
        Ty::Struct(s) => s.clone(),
        Ty::Any => "any".into(),
        Ty::Optional(t) => format!("{}?", show(t)),
        Ty::Fn(params, ret, refs) => {
            let params: Vec<String> = params
                .iter()
                .zip(refs)
                .map(|(p, r)| {
                    if *r {
                        format!("&mut {}", show(p))
                    } else {
                        show(p)
                    }
                })
                .collect();
            match &**ret {
                Ty::Void => format!("fn({})", params.join(", ")),
                r => format!("fn({}) -> {}", params.join(", "), show(r)),
            }
        }
    }
}

pub(crate) fn op_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}
