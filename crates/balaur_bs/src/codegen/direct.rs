// A module for a browser: its imports become a few generic entry points the
// engine exports, and each import a small function that passes its index, so
// a call from the script to the engine never leaves WebAssembly.

use super::{FuncBody, Gen};
use crate::{Import, Slot, words};
use wasm_encoder::{EntityType, ImportSection, Instruction as I, ValType};

/// The most slots one import may take and still go direct.
pub(crate) const MAX_ARGS: usize = 16;

pub(crate) fn applies(imports: &[Import]) -> bool {
    imports.iter().all(|i| i.params.len() <= MAX_ARGS)
}

fn vt(s: Slot) -> ValType {
    match s {
        Slot::I32 => ValType::I32,
        Slot::I64 => ValType::I64,
        Slot::F64 => ValType::F64,
    }
}

// Every slot crosses as an i64: an i32 zero-extended, an f64 by its bits.
fn widen(s: Slot) -> Option<I<'static>> {
    match s {
        Slot::I32 => Some(I::I64ExtendI32U),
        Slot::I64 => None,
        Slot::F64 => Some(I::I64ReinterpretF64),
    }
}

fn narrow(s: Slot) -> Option<I<'static>> {
    match s {
        Slot::I32 => Some(I::I32WrapI64),
        Slot::I64 => None,
        Slot::F64 => Some(I::F64ReinterpretI64),
    }
}

/// The module's new imports, with each old import made a function that calls
/// one of them; every function index moves up by how many there are, which
/// is returned. `host` is the global the engine sets to this instance's id.
pub(crate) fn rewrite(
    g: &Gen,
    imports: &[Import],
    bodies: &mut Vec<(u32, FuncBody)>,
    host: u32,
) -> (ImportSection, u32) {
    let mut arities: Vec<usize> = imports.iter().map(|i| i.params.len()).collect();
    arities.sort_unstable();
    arities.dedup();
    let multi = imports.iter().any(|i| i.results.len() > 1);
    let shift = arities.len() as u32 + u32::from(multi);
    let mut sec = ImportSection::new();
    for n in &arities {
        let mut p = vec![ValType::I32, ValType::I32];
        p.extend(vec![ValType::I64; *n]);
        let ty = g.type_idx(p, vec![ValType::I64]);
        sec.import(
            words::HOST_MODULE,
            &format!("{}{n}", words::DIRECT_CALL),
            EntityType::Function(ty),
        );
    }
    let result = arities.len() as u32;
    if multi {
        let ty = g.type_idx(vec![ValType::I32, ValType::I32], vec![ValType::I64]);
        sec.import(
            words::HOST_MODULE,
            words::DIRECT_RESULT,
            EntityType::Function(ty),
        );
    }
    for (_, body) in bodies.iter_mut() {
        for ins in &mut body.code {
            if let I::Call(i) = ins {
                *i += shift;
            }
        }
    }
    let wrappers = imports.iter().enumerate().map(|(k, imp)| {
        let mut code = vec![I::GlobalGet(host), I::I32Const(k as i32)];
        for (j, s) in imp.params.iter().enumerate() {
            code.push(I::LocalGet(j as u32));
            code.extend(widen(*s));
        }
        let entry = arities.binary_search(&imp.params.len()).unwrap_or(0) as u32;
        code.push(I::Call(entry));
        match imp.results.split_first() {
            None => code.push(I::Drop),
            Some((first, rest)) => {
                code.extend(narrow(*first));
                for (j, s) in rest.iter().enumerate() {
                    code.extend([
                        I::GlobalGet(host),
                        I::I32Const(j as i32 + 1),
                        I::Call(result),
                    ]);
                    code.extend(narrow(*s));
                }
            }
        }
        let ty = g.type_idx(
            imp.params.iter().map(|s| vt(*s)).collect(),
            imp.results.iter().map(|s| vt(*s)).collect(),
        );
        (
            ty,
            FuncBody {
                locals: vec![],
                code,
            },
        )
    });
    let wrappers: Vec<(u32, FuncBody)> = wrappers.collect();
    bodies.splice(0..0, wrappers);
    (sec, shift)
}
