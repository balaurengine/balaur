// `name$start(frame, args..)` writes the arguments into a host-allocated frame;
// `name$poll(frame, self..)` runs to the next `await` and returns its wait handle, or 0.

use super::func::{FnGen, Mode, R};
use super::lower::{Cfg, Term};
use super::rt::{load, mem, store};
use super::{Emitted, FuncBody, Gen, Sig, Site};
use crate::ast::{Expr, ExprKind, FnDecl, Stmt, StmtKind, Ty};
use crate::words;
use wasm_encoder::{BlockType, Instruction as I, ValType};

struct Labels {
    state: u32,
    handle: u32,
    suspend: u32,
    dispatch: u32,
}

pub(super) fn emit(
    g: &Gen,
    sig: &Sig,
    f: &FnDecl,
    fid: Option<u32>,
    out: &mut Emitted,
) -> R<(u32, Vec<(usize, String)>)> {
    let selfv = g.self_vts(sig.owner.as_ref());
    let mut fg = FnGen::new(g, 1 + selfv.len(), Mode::Normal { ret: f.ret.clone() });
    if let Some(o) = &sig.owner {
        fg.self_lv = Some(super::LVal {
            ty: Ty::Struct(o.clone()),
            slots: (1..=selfv.len() as u32).collect(),
        });
    }
    // The fn's own parameters are its first locals, so `$start` knows their frame slots.
    let mut param_locals = vec![];
    for prm in &f.params {
        let lv = fg.alloc(&prm.ty);
        param_locals.extend(lv.slots.iter().copied());
        fg.own(&lv);
        fg.declare(&prm.name, lv);
    }
    if let Some(id) = fid {
        let frame = fg.local(ValType::I32);
        fg.dbg = Some((frame, id));
    }
    let (state, handle) = (fg.local(ValType::I32), fg.local(ValType::I64));
    let mut body = f.body.clone();
    if let Some(t) = body.tail.take() {
        body.stmts.push(Stmt {
            pos: t.pos,
            kind: StmtKind::Return(Some(*t)),
        });
    }
    let mut cfg = Cfg::new(g, sig.owner.clone());
    let entry = cfg.new_block();
    let end = cfg.lower(&body.stmts, entry, false)?;
    cfg.seal(end, Term::Done);

    let suspend = fg.open(I::Block(BlockType::Empty));
    let dispatch = fg.open(I::Loop(BlockType::Empty));
    fg.mode = Mode::Poll {
        ret: f.ret.clone(),
        handle,
    };
    let labels = Labels {
        state,
        handle,
        suspend,
        dispatch,
    };
    run_blocks(&mut fg, &mut cfg, sig, f, &labels)?;
    fg.close();
    fg.e(I::Unreachable);
    fg.close();

    let nret = g.slots(&f.ret).len() as u64;
    let base = 8 + 8 * nret;
    let (frame_size, patches) = save_and_restore(&mut fg, base, state, handle);
    let poll = FuncBody {
        locals: fg.locals.clone(),
        code: std::mem::take(&mut fg.code),
    };
    let mut start = vec![I::LocalGet(0), I::I32Const(0), I::I32Store(mem(0, 2))];
    for (k, l) in param_locals.iter().enumerate() {
        let at = l - fg.nparams;
        start.extend([
            I::LocalGet(0),
            I::LocalGet(1 + k as u32),
            store(fg.locals[at as usize], base + 8 * u64::from(at)),
        ]);
    }
    let ((sp, sr), (pp, pr)) = signatures(g, sig);
    out.bodies.push((
        g.type_idx(sp, sr),
        FuncBody {
            locals: vec![],
            code: start,
        },
    ));
    out.bodies.push((g.type_idx(pp, pr), poll));
    if f.public || f.owner.is_some() {
        // A unit's again for the units that call it.
        let prefixes: &[&str] = if g.unit { &["", words::LINK] } else { &[""] };
        for pre in prefixes {
            out.exports
                .push((format!("{pre}{}{}", f.key(), words::START), sig.idx));
            out.exports
                .push((format!("{pre}{}{}", f.key(), words::POLL), sig.idx + 1));
        }
    }
    Ok((frame_size, patches))
}

/// `$start`'s parameters and results, then `$poll`'s.
pub(super) fn signatures(g: &Gen, sig: &Sig) -> (super::Signature, super::Signature) {
    let selfv = g.self_vts(sig.owner.as_ref());
    let mut sp = vec![ValType::I32];
    sp.extend(sig.params.iter().flat_map(|t| g.vts(t)));
    let mut pp = vec![ValType::I32];
    pp.extend(selfv.clone());
    let mut pr = vec![ValType::I64];
    pr.extend(selfv);
    ((sp, vec![]), (pp, pr))
}

// Suspending writes the state and every local to the frame; resuming reads them back.
fn save_and_restore(
    fg: &mut FnGen<'_>,
    base: u64,
    state: u32,
    handle: u32,
) -> (u32, Vec<(usize, String)>) {
    let saved: Vec<(u32, ValType)> = fg
        .locals
        .iter()
        .enumerate()
        .map(|(i, vt)| (fg.nparams + i as u32, *vt))
        .collect();
    fg.emit([I::LocalGet(0), I::LocalGet(state), I::I32Store(mem(0, 2))]);
    for (k, (slot, vt)) in saved.iter().enumerate() {
        fg.emit([
            I::LocalGet(0),
            I::LocalGet(*slot),
            store(*vt, base + 8 * k as u64),
        ]);
    }
    fg.dbg_leave();
    fg.e(I::LocalGet(handle));
    if let Some(s) = fg.self_lv.clone() {
        fg.get_raw(&s);
    }
    let mut prologue = vec![I::LocalGet(0), I::I32Load(mem(0, 2)), I::LocalSet(state)];
    for (k, (slot, vt)) in saved.iter().enumerate() {
        prologue.extend([
            I::LocalGet(0),
            load(*vt, base + 8 * k as u64),
            I::LocalSet(*slot),
        ]);
    }
    let size = 8 * (fg.nparams + fg.locals.len() as u32);
    prologue.extend(fg.dbg_enter(size));
    let shift = prologue.len();
    prologue.append(&mut fg.code);
    fg.code = prologue;
    let patches = fg
        .patches
        .iter()
        .map(|(p, k)| (p + shift, k.clone()))
        .collect();
    ((base + 8 * saved.len() as u64) as u32, patches)
}

fn run_blocks(fg: &mut FnGen<'_>, cfg: &mut Cfg<'_>, sig: &Sig, f: &FnDecl, l: &Labels) -> R<()> {
    let k = cfg.blocks.len() as u32;
    for _ in 0..k {
        fg.open(I::Block(BlockType::Empty));
    }
    fg.emit([
        I::LocalGet(l.state),
        I::BrTable((0..k).collect::<Vec<_>>().into(), 0),
    ]);
    for b in 0..k as usize {
        fg.close();
        for s in std::mem::take(&mut cfg.blocks[b].stmts) {
            fg.stmt(&s)?;
        }
        let term = cfg.blocks[b].term.clone();
        terminate(fg, term, b, sig, f, l)?;
    }
    Ok(())
}

fn goto(fg: &mut FnGen<'_>, l: &Labels, block: usize) {
    fg.emit([I::I32Const(block as i32), I::LocalSet(l.state)]);
}

fn terminate(fg: &mut FnGen<'_>, term: Term, b: usize, sig: &Sig, f: &FnDecl, l: &Labels) -> R<()> {
    match term {
        Term::Goto(t) => {
            goto(fg, l, t);
            fg.br(l.dispatch);
        }
        Term::Branch(c, t, e) => {
            fg.expr_as(&c, &Ty::Bool)?;
            fg.open(I::If(BlockType::Empty));
            goto(fg, l, t);
            fg.e(I::Else);
            goto(fg, l, e);
            fg.close();
            fg.br(l.dispatch);
        }
        Term::AwaitHost { call, site, next } => {
            let ExprKind::Call(name, args) = &call.kind else {
                unreachable!("the lowering only makes host awaits of calls")
            };
            let s = fg.g.sigs[name].clone();
            fg.sites.insert(site, Site::Host);
            let borrowed =
                fg.host_args(&s.params, &args.iter().collect::<Vec<_>>(), call.pos, name)?;
            fg.emit([I::Call(s.idx), I::LocalSet(l.handle)]);
            for v in borrowed {
                fg.release(&v);
            }
            goto(fg, l, next);
            fg.br(l.suspend);
        }
        Term::AwaitStart { call, site, poll } => start_child(fg, &call, site, poll, sig, l)?,
        Term::AwaitPoll {
            key,
            with_self,
            site,
            next,
        } => poll_child(fg, &key, with_self, site, b, next, l),
        Term::Done | Term::Open if f.ret == Ty::Void => fg.leave(&Ty::Void, true),
        Term::Done | Term::Open => fg.e(I::Unreachable),
    }
    Ok(())
}

// The callee's frame comes from the allocator, zeroed; its size is patched in
// once every async fn has been laid out.
fn start_child(
    fg: &mut FnGen<'_>,
    call: &Expr,
    site: u32,
    poll: usize,
    sig: &Sig,
    l: &Labels,
) -> R<()> {
    let (key, args) = match &call.kind {
        ExprKind::Call(n, a) => (n.clone(), a),
        ExprKind::Method(_, n, a) => (format!("{}.{n}", sig.owner.clone().unwrap_or_default()), a),
        _ => unreachable!("the lowering only awaits calls and `self` methods"),
    };
    let callee = fg.g.sigs[&key].clone();
    let child = fg.local(ValType::I32);
    fg.sites.insert(site, Site::Script { child });
    fg.patches.push((fg.code.len(), key.clone()));
    fg.emit([
        I::I32Const(0),
        I::Call(fg.g.h.alloc),
        I::LocalTee(child),
        I::I32Const(0),
    ]);
    fg.patches.push((fg.code.len(), key.clone()));
    fg.emit([I::I32Const(0), I::MemoryFill(0), I::LocalGet(child)]);
    fg.args(&callee.params, args, call.pos, &key)?;
    fg.e(I::Call(callee.idx));
    goto(fg, l, poll);
    fg.br(l.dispatch);
    Ok(())
}

fn poll_child(
    fg: &mut FnGen<'_>,
    key: &str,
    with_self: bool,
    site: u32,
    here: usize,
    next: usize,
    l: &Labels,
) {
    let Some(Site::Script { child }) = fg.sites.get(&site).cloned() else {
        unreachable!("a poll block follows its start block")
    };
    let callee = fg.g.sigs[key].clone();
    let own_self = fg.self_lv.clone().filter(|_| with_self);
    fg.e(I::LocalGet(child));
    if let Some(s) = &own_self {
        fg.get_raw(s);
    }
    fg.e(I::Call(callee.idx + 1));
    if let Some(s) = &own_self {
        fg.set_raw(s);
    }
    fg.emit([I::LocalTee(l.handle), I::I64Const(0), I::I64Ne]);
    fg.open(I::If(BlockType::Empty));
    goto(fg, l, here);
    fg.br(l.suspend);
    fg.e(I::Else);
    goto(fg, l, next);
    fg.close();
    fg.br(l.dispatch);
}
