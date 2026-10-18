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

pub(crate) struct MemPlace {
    pub addr: u32,
    pub ty: Ty,
    pub off: u64,
    pub release: Option<LVal>,
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
                Some(lv) if lv.ty == Ty::Node => None,
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
                self.emit([I::LocalGet(data), I::LocalGet(scaled), I::I32Add]);
                let addr = self.local(ValType::I32);
                self.e(I::LocalSet(addr));
                Ok(Some(MemPlace {
                    addr,
                    ty: *et,
                    off: 0,
                    release: None,
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
                    addr,
                    ty: et,
                    off: 0,
                    release,
                }))
            }
            ExprKind::Field(b, f)
                if self.place(b)?.is_none()
                    && matches!(b.kind, ExprKind::Index(..) | ExprKind::Field(..)) =>
            {
                let Some(mut mp) = self.mem_place(b)? else {
                    return Ok(None);
                };
                let (slot, ty) = self.field_slot(&mp.ty, f, e.pos)?;
                mp.off += 8 * slot as u64;
                mp.ty = ty;
                Ok(Some(mp))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn index_local(&mut self, idx: &Expr) -> R<u32> {
        match self.place(idx)? {
            Some(LVal { ty: Ty::Int, slots }) => Ok(slots[0]),
            Some(lv) => err(idx.pos, format!("an index is int, not {}", show(&lv.ty))),
            None => {
                let t = self.expr(idx)?;
                if t != Ty::Int {
                    return err(idx.pos, format!("an index is int, not {}", show(&t)));
                }
                let l = self.local(ValType::I64);
                self.e(I::LocalSet(l));
                Ok(l)
            }
        }
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
            self.emit([
                I::LocalGet(mp.addr),
                load(ValType::I32, mp.off),
                I::LocalSet(l),
            ]);
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

    pub(crate) fn mem_read(&mut self, mp: &MemPlace) {
        for (j, (vt, heap)) in self.g.slots(&mp.ty).into_iter().enumerate() {
            self.emit([I::LocalGet(mp.addr), load(vt, mp.off + 8 * j as u64)]);
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
                self.emit([
                    I::LocalGet(mp.addr),
                    load(vt, off),
                    I::Call(self.g.dec_fn(&h)),
                ]);
            }
            self.emit([
                I::LocalGet(mp.addr),
                I::LocalGet(tmp.slots[j]),
                store(vt, off),
            ]);
        }
    }

    pub(crate) fn coerce(&mut self, got: &Ty, want: &Ty, pos: Pos) -> R<()> {
        if got == want {
            Ok(())
        } else if *got == Ty::Int && *want == Ty::Float {
            self.e(I::F64ConvertI64S);
            Ok(())
        } else if *want == Ty::Any && self.box_any(got) {
            Ok(())
        } else {
            err(pos, format!("expected {}, got {}", show(want), show(got)))
        }
    }

    // The value on the stack, wrapped in a fresh `any` cell that takes it
    // over; false for a kind an `any` does not hold.
    pub(crate) fn box_any(&mut self, got: &Ty) -> bool {
        use crate::anys::{BOOL, CELL, FLOAT, INT, LIST, NODE, STR};
        let (tag, widen) = match got {
            Ty::Int => (INT, None),
            Ty::Node => (NODE, None),
            Ty::Float => (FLOAT, Some(I::I64ReinterpretF64)),
            Ty::Bool => (BOOL, Some(I::I64ExtendI32U)),
            Ty::Str => (STR, Some(I::I64ExtendI32U)),
            Ty::Array(et) if **et == Ty::Any => (LIST, Some(I::I64ExtendI32U)),
            _ => return false,
        };
        if let Some(w) = widen {
            self.e(w);
        }
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
        true
    }

    pub(crate) fn expr_as(&mut self, e: &Expr, want: &Ty) -> R<()> {
        if let (ExprKind::ArrayLit(items), Ty::Array(et)) = (&e.kind, want) {
            return self.array_lit(items, Some(et), e.pos).map(|_| ());
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

    // Debug builds: note the statement in the shadow frame, and only when a
    // breakpoint sits on it or the debugger is stepping, stop there. The host
    // may change the variables while stopped, so they are read back.
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
        self.emit([I::I32Const(id as i32), I::Call(self.g.imp.dbg_probe)]);
        for (s, vt, heap) in self.visible_slots() {
            if !heap {
                self.emit([I::LocalGet(frame), load(vt, slot_offset(s)), I::LocalSet(s)]);
            }
        }
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
