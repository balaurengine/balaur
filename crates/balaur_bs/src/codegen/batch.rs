// A method's batch entry: the method over many instances in one call.

use super::rt::{load, store};
use super::{FuncBody, Gen, Sig, Structs, has_heap};
use crate::ast::FnDecl;
use std::collections::BTreeSet;
use wasm_encoder::{BlockType, Instruction as I, ValType};

// A batch entry runs the method once per instance with the same arguments,
// so it exists only where no argument or result needs releasing.
pub(super) fn batchable(f: &FnDecl, structs: &Structs, classes: &BTreeSet<String>) -> bool {
    f.public
        && f.owner.is_some()
        && !f.is_async
        && f.owner.as_ref().is_some_and(|o| !classes.contains(o))
        && f.params.iter().all(|p| !p.by_ref)
        && !has_heap(&f.ret, structs, classes)
        && f.params.iter().all(|p| !has_heap(&p.ty, structs, classes))
}

// `T.m$batch(base, count, args..)`: `m` over `count` instances stored at
// `base`, one 8-byte slot per field: one host call a frame, not one per instance.
pub(super) fn batch_fn(g: &Gen, sig: &Sig) -> (u32, FuncBody) {
    use ValType::I32;
    let selfv = g.self_vts(sig.owner.as_ref());
    let argv: Vec<ValType> = sig.params.iter().flat_map(|t| g.vts(t)).collect();
    let nret = g.vts(&sig.ret).len();
    let mut params = vec![I32, I32];
    params.extend(argv.clone());
    let np = params.len() as u32;
    let (k, addr) = (np, np + 1);
    let temps: Vec<u32> = (0..selfv.len() as u32).map(|i| np + 2 + i).collect();
    let mut locals = vec![I32, I32];
    locals.extend(selfv.iter().copied());
    let stride = 8 * selfv.len() as i32;
    let mut code = vec![
        I::Block(BlockType::Empty),
        I::Loop(BlockType::Empty),
        I::LocalGet(k),
        I::LocalGet(1),
        I::I32GeU,
        I::BrIf(1),
    ];
    code.extend([
        I::LocalGet(0),
        I::LocalGet(k),
        I::I32Const(stride),
        I::I32Mul,
        I::I32Add,
        I::LocalSet(addr),
    ]);
    for (j, vt) in selfv.iter().enumerate() {
        code.extend([I::LocalGet(addr), load(*vt, 8 * j as u64)]);
    }
    code.extend((0..argv.len() as u32).map(|a| I::LocalGet(2 + a)));
    code.push(I::Call(sig.idx));
    code.extend((0..nret).map(|_| I::Drop));
    code.extend(temps.iter().rev().map(|t| I::LocalSet(*t)));
    for (j, vt) in selfv.iter().enumerate() {
        code.extend([
            I::LocalGet(addr),
            I::LocalGet(temps[j]),
            store(*vt, 8 * j as u64),
        ]);
    }
    code.extend([
        I::LocalGet(k),
        I::I32Const(1),
        I::I32Add,
        I::LocalSet(k),
        I::Br(0),
        I::End,
        I::End,
    ]);
    (g.type_idx(params, vec![]), FuncBody { locals, code })
}
