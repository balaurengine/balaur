// The runtime every module carries, as raw wasm. Objects start with an i32
// reference count; a negative count marks a static object the counts never touch.

use super::FuncBody;
use ValType::{F64, I32, I64};
use wasm_encoder::{BlockType, Instruction as I, MemArg, ValType};

pub(crate) const FREE_LISTS: u64 = 64;
pub(crate) const SCRATCH: i32 = 256;
pub(crate) const DATA_BASE: u32 = 1024;
pub(crate) const DEBUG_STACK: u32 = 64 << 10;

pub(crate) const G_BUMP: u32 = 0;
pub(crate) const G_LIVE: u32 = 1;
pub(crate) const G_DBG_SP: u32 = 2;
pub(crate) const G_DBG_BASE: u32 = 3;
pub(crate) const G_DBG_FLAGS: u32 = 4;
pub(crate) const G_DBG_BREAKS: u32 = 5;
pub(crate) const GLOBALS: usize = 6;
/// Only in a metered module, after the rest.
pub(crate) const G_FUEL: u32 = 6;
pub(crate) const FRAME_HEADER: u32 = 16;

pub(crate) fn mem(offset: u64, align: u32) -> MemArg {
    MemArg {
        offset,
        align,
        memory_index: 0,
    }
}
pub(crate) fn load(vt: ValType, off: u64) -> I<'static> {
    match vt {
        I32 => I::I32Load(mem(off, 2)),
        I64 => I::I64Load(mem(off, 3)),
        _ => I::F64Load(mem(off, 3)),
    }
}
pub(crate) fn store(vt: ValType, off: u64) -> I<'static> {
    match vt {
        I32 => I::I32Store(mem(off, 2)),
        I64 => I::I64Store(mem(off, 3)),
        _ => I::F64Store(mem(off, 3)),
    }
}

/// Indices of the fixed helpers, which follow the imports.
#[derive(Clone, Copy)]
pub(crate) struct Helpers {
    pub alloc: u32,
    pub free: u32,
    pub retain: u32,
    pub dec_str: u32,
    pub concat: u32,
    pub str_eq: u32,
    pub str_of_i64: u32,
    pub str_of_f64: u32,
    pub arr_new: u32,
    pub arr_reserve: u32,
    pub dbg_reset: u32,
}

pub(crate) const HELPER_COUNT: u32 = 11;

impl Helpers {
    pub(crate) fn at(base: u32) -> Self {
        Helpers {
            alloc: base,
            free: base + 1,
            retain: base + 2,
            dec_str: base + 3,
            concat: base + 4,
            str_eq: base + 5,
            str_of_i64: base + 6,
            str_of_f64: base + 7,
            arr_new: base + 8,
            arr_reserve: base + 9,
            dbg_reset: base + 10,
        }
    }
}

pub(crate) struct Imports {
    pub fmt_f64: u32,
}

type Helper = (Vec<ValType>, Vec<ValType>, FuncBody);

fn f(params: &[ValType], results: &[ValType], locals: &[ValType], code: Vec<I<'static>>) -> Helper {
    (
        params.to_vec(),
        results.to_vec(),
        FuncBody {
            locals: locals.to_vec(),
            code,
        },
    )
}

/// In index order: alloc, free, retain, dec_str, concat, str_eq, str_of_i64,
/// str_of_f64, arr_new, arr_reserve, dbg_reset.
pub(crate) fn helpers(h: Helpers, imp: &Imports) -> Vec<Helper> {
    let mut out = vec![
        alloc(),
        free(),
        retain(),
        f(&[I32], &[], &[I32], dec_body(h, &[])),
    ];
    out.extend([
        concat(h),
        str_eq(h),
        str_of_i64(h),
        str_of(h, F64, imp.fmt_f64),
        arr_new(h),
        arr_reserve(h),
    ]);
    // After a trap the shadow stack is wherever the trap left it.
    out.push(f(
        &[],
        &[],
        &[],
        vec![I::GlobalGet(G_DBG_BASE), I::GlobalSet(G_DBG_SP)],
    ));
    out
}

// A block is [class i32][pad][payload]; a free block keeps the next free one in
// its class word. A request it cannot meet traps with the bump pointer unmoved.
#[rustfmt::skip]
fn alloc() -> Helper {
    f(&[I32], &[I32], &[I32, I32, I32], vec![
        I::LocalGet(0), I::I32Const(1 << 30), I::I32GtU, I::If(BlockType::Empty), I::Unreachable, I::End,
        I::LocalGet(0), I::I32Const(8), I::I32Add, I::LocalSet(0),
        I::I32Const(16), I::LocalSet(2),
        I::Block(BlockType::Empty), I::Loop(BlockType::Empty),
        I::LocalGet(2), I::LocalGet(0), I::I32GeU, I::BrIf(1),
        I::LocalGet(2), I::I32Const(1), I::I32Shl, I::LocalSet(2),
        I::LocalGet(1), I::I32Const(1), I::I32Add, I::LocalSet(1),
        I::Br(0), I::End, I::End,
        I::LocalGet(1), I::I32Const(2), I::I32Shl, I::I32Load(mem(FREE_LISTS, 2)), I::LocalTee(3),
        I::If(BlockType::Empty),
        I::LocalGet(1), I::I32Const(2), I::I32Shl, I::LocalGet(3), I::I32Load(mem(0, 2)), I::I32Store(mem(FREE_LISTS, 2)),
        I::Else,
        I::GlobalGet(G_BUMP), I::LocalSet(3),
        I::GlobalGet(G_BUMP), I::LocalGet(2), I::I32Add, I::GlobalSet(G_BUMP),
        I::GlobalGet(G_BUMP), I::MemorySize(0), I::I32Const(16), I::I32Shl, I::I32GtU,
        I::If(BlockType::Empty),
        I::GlobalGet(G_BUMP), I::MemorySize(0), I::I32Const(16), I::I32Shl, I::I32Sub, I::I32Const(65535), I::I32Add,
        I::I32Const(16), I::I32ShrU, I::MemoryGrow(0), I::I32Const(-1), I::I32Eq,
        I::If(BlockType::Empty), I::LocalGet(3), I::GlobalSet(G_BUMP), I::Unreachable, I::End,
        I::End,
        I::End,
        I::LocalGet(3), I::LocalGet(1), I::I32Store(mem(0, 2)),
        I::GlobalGet(G_LIVE), I::I32Const(1), I::I32Add, I::GlobalSet(G_LIVE),
        I::LocalGet(3), I::I32Const(8), I::I32Add,
    ])
}

#[rustfmt::skip]
fn free() -> Helper {
    f(&[I32], &[], &[I32, I32], vec![
        I::LocalGet(0), I::I32Const(8), I::I32Sub, I::LocalTee(1), I::I32Load(mem(0, 2)), I::I32Const(2), I::I32Shl, I::LocalSet(2),
        I::LocalGet(1), I::LocalGet(2), I::I32Load(mem(FREE_LISTS, 2)), I::I32Store(mem(0, 2)),
        I::LocalGet(2), I::LocalGet(1), I::I32Store(mem(FREE_LISTS, 2)),
        I::GlobalGet(G_LIVE), I::I32Const(1), I::I32Sub, I::GlobalSet(G_LIVE),
    ])
}

#[rustfmt::skip]
fn retain() -> Helper {
    f(&[I32], &[I32], &[I32], vec![
        I::LocalGet(0),
        I::If(BlockType::Empty),
        I::LocalGet(0), I::I32Load(mem(0, 2)), I::LocalTee(1), I::I32Const(0), I::I32GtS,
        I::If(BlockType::Empty), I::LocalGet(0), I::LocalGet(1), I::I32Const(1), I::I32Add, I::I32Store(mem(0, 2)), I::End,
        I::End,
        I::LocalGet(0),
    ])
}

// concat(a, b) -> a new string; both arguments are consumed.
#[rustfmt::skip]
fn concat(h: Helpers) -> Helper {
    f(&[I32, I32], &[I32], &[I32, I32, I32], vec![
        I::LocalGet(0), I::I32Load(mem(4, 2)), I::LocalSet(2),
        I::LocalGet(1), I::I32Load(mem(4, 2)), I::LocalSet(3),
        I::LocalGet(2), I::LocalGet(3), I::I32Add, I::I32Const(8), I::I32Add, I::Call(h.alloc), I::LocalTee(4),
        I::I32Const(1), I::I32Store(mem(0, 2)),
        I::LocalGet(4), I::LocalGet(2), I::LocalGet(3), I::I32Add, I::I32Store(mem(4, 2)),
        I::LocalGet(4), I::I32Const(8), I::I32Add, I::LocalGet(0), I::I32Const(8), I::I32Add, I::LocalGet(2),
        I::MemoryCopy { src_mem: 0, dst_mem: 0 },
        I::LocalGet(4), I::I32Const(8), I::I32Add, I::LocalGet(2), I::I32Add, I::LocalGet(1), I::I32Const(8), I::I32Add,
        I::LocalGet(3), I::MemoryCopy { src_mem: 0, dst_mem: 0 },
        I::LocalGet(0), I::Call(h.dec_str), I::LocalGet(1), I::Call(h.dec_str),
        I::LocalGet(4),
    ])
}

// str_eq(a, b) -> bool; both arguments are consumed.
#[rustfmt::skip]
fn str_eq(h: Helpers) -> Helper {
    f(&[I32, I32], &[I32], &[I32, I32, I32], vec![
        I::I32Const(1), I::LocalSet(4),
        I::LocalGet(0), I::I32Load(mem(4, 2)), I::LocalTee(2), I::LocalGet(1), I::I32Load(mem(4, 2)), I::I32Ne,
        I::If(BlockType::Empty),
        I::I32Const(0), I::LocalSet(4),
        I::Else,
        I::Block(BlockType::Empty), I::Loop(BlockType::Empty),
        I::LocalGet(3), I::LocalGet(2), I::I32GeU, I::BrIf(1),
        I::LocalGet(0), I::LocalGet(3), I::I32Add, I::I32Load8U(mem(8, 0)),
        I::LocalGet(1), I::LocalGet(3), I::I32Add, I::I32Load8U(mem(8, 0)), I::I32Ne,
        I::If(BlockType::Empty), I::I32Const(0), I::LocalSet(4), I::Br(2), I::End,
        I::LocalGet(3), I::I32Const(1), I::I32Add, I::LocalSet(3), I::Br(0),
        I::End, I::End,
        I::End,
        I::LocalGet(0), I::Call(h.dec_str), I::LocalGet(1), I::Call(h.dec_str),
        I::LocalGet(4),
    ])
}

// The host formats the number into the scratch area; the text is copied out.
#[rustfmt::skip]
fn str_of(h: Helpers, vt: ValType, fmt: u32) -> Helper {
    f(&[vt], &[I32], &[I32, I32], vec![
        I::LocalGet(0), I::I32Const(SCRATCH), I::Call(fmt), I::LocalSet(1),
        I::LocalGet(1), I::I32Const(8), I::I32Add, I::Call(h.alloc), I::LocalTee(2),
        I::I32Const(1), I::I32Store(mem(0, 2)),
        I::LocalGet(2), I::LocalGet(1), I::I32Store(mem(4, 2)),
        I::LocalGet(2), I::I32Const(8), I::I32Add, I::I32Const(SCRATCH), I::LocalGet(1),
        I::MemoryCopy { src_mem: 0, dst_mem: 0 },
        I::LocalGet(2),
    ])
}

// str_of_i64(x): its decimal digits, written backwards from the end of the
// scratch area; the magnitude is taken unsigned, so i64::MIN prints whole.
#[rustfmt::skip]
fn str_of_i64(h: Helpers) -> Helper {
    let end = SCRATCH + 24;
    // Locals after x: neg, magnitude, cursor, length, string.
    f(&[I64], &[I32], &[I32, I64, I32, I32, I32], vec![
        I::LocalGet(0), I::I64Const(0), I::I64LtS, I::LocalSet(1),
        I::I64Const(0), I::LocalGet(0), I::I64Sub, I::LocalGet(0), I::LocalGet(1), I::Select, I::LocalSet(2),
        I::I32Const(end), I::LocalSet(3),
        I::Loop(BlockType::Empty),
        I::LocalGet(3), I::I32Const(1), I::I32Sub, I::LocalTee(3),
        I::LocalGet(2), I::I64Const(10), I::I64RemU, I::I32WrapI64, I::I32Const(48), I::I32Add,
        I::I32Store8(mem(0, 0)),
        I::LocalGet(2), I::I64Const(10), I::I64DivU, I::LocalTee(2), I::I64Const(0), I::I64Ne,
        I::BrIf(0),
        I::End,
        I::LocalGet(1),
        I::If(BlockType::Empty),
        I::LocalGet(3), I::I32Const(1), I::I32Sub, I::LocalTee(3), I::I32Const(45), I::I32Store8(mem(0, 0)),
        I::End,
        I::I32Const(end), I::LocalGet(3), I::I32Sub, I::LocalSet(4),
        I::LocalGet(4), I::I32Const(8), I::I32Add, I::Call(h.alloc), I::LocalTee(5),
        I::I32Const(1), I::I32Store(mem(0, 2)),
        I::LocalGet(5), I::LocalGet(4), I::I32Store(mem(4, 2)),
        I::LocalGet(5), I::I32Const(8), I::I32Add, I::LocalGet(3), I::LocalGet(4),
        I::MemoryCopy { src_mem: 0, dst_mem: 0 },
        I::LocalGet(5),
    ])
}

// arr_new(len, elem_size) -> header [count][len][cap][data]; the caller fills it.
#[rustfmt::skip]
fn arr_new(h: Helpers) -> Helper {
    f(&[I32, I32], &[I32], &[I32], vec![
        I::I32Const(16), I::Call(h.alloc), I::LocalTee(2),
        I::I32Const(1), I::I32Store(mem(0, 2)),
        I::LocalGet(2), I::LocalGet(0), I::I32Store(mem(4, 2)),
        I::LocalGet(2), I::LocalGet(0), I::I32Store(mem(8, 2)),
        I::LocalGet(2),
        I::LocalGet(0),
        I::If(BlockType::Result(I32)),
        I::LocalGet(0), I::LocalGet(1), I::I32Mul, I::Call(h.alloc),
        I::Else, I::I32Const(0), I::End,
        I::I32Store(mem(12, 2)),
        I::LocalGet(2),
    ])
}

// arr_reserve(a, elem_size): room for one more, doubling the capacity.
#[rustfmt::skip]
fn arr_reserve(h: Helpers) -> Helper {
    f(&[I32, I32], &[], &[I32, I32], vec![
        I::LocalGet(0), I::I32Load(mem(4, 2)), I::LocalGet(0), I::I32Load(mem(8, 2)), I::I32GeU,
        I::If(BlockType::Empty),
        I::LocalGet(0), I::I32Load(mem(8, 2)), I::LocalTee(2), I::I32Eqz,
        I::If(BlockType::Result(I32)), I::I32Const(4), I::Else, I::LocalGet(2), I::I32Const(1), I::I32Shl, I::End,
        I::LocalSet(2),
        I::LocalGet(2), I::LocalGet(1), I::I32Mul, I::Call(h.alloc), I::LocalTee(3),
        I::LocalGet(0), I::I32Load(mem(12, 2)),
        I::LocalGet(0), I::I32Load(mem(4, 2)), I::LocalGet(1), I::I32Mul,
        I::MemoryCopy { src_mem: 0, dst_mem: 0 },
        I::LocalGet(0), I::I32Load(mem(12, 2)),
        I::If(BlockType::Empty), I::LocalGet(0), I::I32Load(mem(12, 2)), I::Call(h.free), I::End,
        I::LocalGet(0), I::LocalGet(3), I::I32Store(mem(12, 2)),
        I::LocalGet(0), I::LocalGet(2), I::I32Store(mem(8, 2)),
        I::End,
    ])
}

/// `dec(ptr)` for an object whose last reference runs `drop` before the
/// header is freed; `drop` sees the object in local 0.
#[rustfmt::skip]
pub(crate) fn dec_body(h: Helpers, drop: &[I<'static>]) -> Vec<I<'static>> {
    let mut code = vec![
        I::LocalGet(0),
        I::If(BlockType::Empty),
        I::LocalGet(0), I::I32Load(mem(0, 2)), I::LocalTee(1), I::I32Const(1), I::I32Eq,
        I::If(BlockType::Empty),
    ];
    code.extend(drop.iter().cloned());
    code.extend([
        I::LocalGet(0), I::Call(h.free),
        I::Else,
        I::LocalGet(1), I::I32Const(1), I::I32GtS,
        I::If(BlockType::Empty), I::LocalGet(0), I::LocalGet(1), I::I32Const(1), I::I32Sub, I::I32Store(mem(0, 2)), I::End,
        I::End,
        I::End,
    ]);
    code
}
