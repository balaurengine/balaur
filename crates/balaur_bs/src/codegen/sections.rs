// The module's sections past its functions: exports, the `fn` table, the
// code, memory and globals.

use super::rt::{
    self, DATA_BASE, DEBUG_STACK, G_BUMP, G_DBG_BASE, G_DBG_BREAKS, G_DBG_FLAGS, G_DBG_SP, G_LIVE,
    G_TABLE_BASE, GLOBALS,
};
use super::{Emitted, FuncBody, Gen, show};
use crate::ast::Ty;
use crate::{Options, words};
use wasm_encoder::{
    CodeSection, ConstExpr, ElementSection, Elements, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, Instruction as I, MemorySection, MemoryType,
    RefType, TableSection, TableType, ValType,
};

// `shift` is how far a direct module moved every function.
pub(super) fn export_section(
    g: &Gen,
    out: &Emitted,
    dyns: &[(u32, Ty)],
    debug: bool,
    shift: u32,
) -> ExportSection {
    let mut ex = ExportSection::new();
    ex.export(words::MEMORY, ExportKind::Memory, 0);
    ex.export(words::ALLOC, ExportKind::Func, g.h.alloc + shift);
    ex.export(words::FREE, ExportKind::Func, g.h.free + shift);
    ex.export(words::LIVE, ExportKind::Global, G_LIVE);
    ex.export(words::DBG_RESET, ExportKind::Func, g.h.dbg_reset + shift);
    if g.meter {
        ex.export(words::FUEL, ExportKind::Global, rt::G_FUEL);
    }
    if let Some(base) = g.prof_globals {
        ex.export(words::STEPS, ExportKind::Global, base);
        ex.export(words::PROF, ExportKind::Global, base + 2);
    }
    if debug {
        for (name, at) in [
            (words::DBG_BASE, G_DBG_BASE),
            (words::DBG_SP, G_DBG_SP),
            (words::DBG_FLAGS, G_DBG_FLAGS),
            (words::DBG_BREAKS, G_DBG_BREAKS),
        ] {
            ex.export(name, ExportKind::Global, at);
        }
    }
    ex.export(
        &format!("{}str", words::DEC),
        ExportKind::Func,
        g.h.dec_str + shift,
    );
    for (i, ty) in dyns {
        ex.export(
            &format!("{}{}", words::DEC, show(ty)),
            ExportKind::Func,
            *i + shift,
        );
    }
    for (name, i) in &out.exports {
        ex.export(name, ExportKind::Func, *i + shift);
    }
    ex
}

// The table `fn` values index, when the module takes any: slot 0 empty, then
// each function a `fn` value names, moved as far as a direct module moved them.
pub(super) fn table_sections(
    g: &Gen,
    shift: u32,
) -> (Option<TableSection>, Option<ElementSection>) {
    let refs = g.fn_refs.borrow();
    // A unit fills the script's table from its own base.
    if g.unit {
        if refs.is_empty() {
            return (None, None);
        }
        let mut elements = ElementSection::new();
        elements.active(
            Some(0),
            &ConstExpr::global_get(G_TABLE_BASE),
            Elements::Functions(std::borrow::Cow::Owned(refs.clone())),
        );
        return (None, Some(elements));
    }
    if refs.is_empty() && !g.calls_indirect.get() {
        return (None, None);
    }
    let size = refs.len() as u64 + 1;
    let mut table = TableSection::new();
    table.table(TableType {
        element_type: RefType::FUNCREF,
        table64: false,
        minimum: size,
        maximum: Some(size),
        shared: false,
    });
    if refs.is_empty() {
        return (Some(table), None);
    }
    let funcs: Vec<u32> = refs.iter().map(|i| i + shift).collect();
    let mut elements = ElementSection::new();
    elements.active(
        Some(0),
        &ConstExpr::i32_const(1),
        Elements::Functions(std::borrow::Cow::Owned(funcs)),
    );
    (Some(table), Some(elements))
}

pub(super) fn code_sections(bodies: &[(u32, FuncBody)]) -> (FunctionSection, CodeSection) {
    let mut funcs = FunctionSection::new();
    let mut code = CodeSection::new();
    for (t, b) in bodies {
        funcs.function(*t);
        let mut f = Function::new(b.locals.iter().map(|v| (1u32, *v)));
        for i in &b.code {
            f.instruction(i);
        }
        f.instruction(&I::End);
        code.function(&f);
    }
    (funcs, code)
}

// Memory: the static strings, then in debug builds the shadow stack and the
// breakpoint table (a byte per statement), then the heap.
pub(super) fn memory_and_globals(
    data_len: u32,
    probes: u32,
    debug: bool,
    options: &Options,
) -> (MemorySection, GlobalSection) {
    let dbg_stack = (DATA_BASE + data_len).next_multiple_of(16);
    let breaks = dbg_stack + DEBUG_STACK;
    let heap = if debug {
        (breaks + probes).next_multiple_of(16)
    } else {
        dbg_stack
    };
    let minimum = u64::from(heap / 65536 + 1);
    let maximum = options
        .memory_max
        .map(|bytes| (bytes as u64).div_ceil(65536).max(minimum));
    let mut memory = MemorySection::new();
    memory.memory(MemoryType {
        minimum,
        maximum,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    let mut globals = GlobalSection::new();
    let mutable = GlobalType {
        val_type: ValType::I32,
        mutable: true,
        shared: false,
    };
    let mut init = [0u32; GLOBALS];
    for (at, v) in [
        (G_BUMP, heap),
        (G_LIVE, 0),
        (G_DBG_SP, dbg_stack),
        (G_DBG_BASE, dbg_stack),
        (G_DBG_FLAGS, 0),
        (G_DBG_BREAKS, breaks),
    ] {
        init[at as usize] = v;
    }
    for v in init {
        globals.global(mutable, &ConstExpr::i32_const(v as i32));
    }
    if options.meter {
        globals.global(mutable, &ConstExpr::i32_const(i32::MAX));
    }
    if options.profile {
        for _ in 0..PROF_GLOBALS {
            globals.global(mutable, &ConstExpr::i32_const(0));
        }
    }
    (memory, globals)
}

/// A profiled module's globals: the statement count, the callees' share of
/// the call running, and the table's address.
pub(super) const PROF_GLOBALS: u32 = 3;

// A direct module's instance id, after the fuel and the profile's globals.
pub(super) fn host_global(options: &Options) -> u32 {
    GLOBALS as u32 + u32::from(options.meter) + PROF_GLOBALS * u32::from(options.profile)
}
