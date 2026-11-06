// A script built as units: which module functions get link slots, the stubs
// that call through them, and what a unit imports from the rest of its script.

use super::asyncs;
use super::rt::{G_LINK_BASE, GLOBALS};
use super::sections::PROF_GLOBALS;
use super::{Emitted, FuncBody, Gen, Kind, Linked, Sig, Signature};
use crate::ast::{Expr, ExprKind, FnDecl, Program, StmtKind};
use crate::{Options, Slot, words};
use std::collections::HashMap;
use wasm_encoder::{
    EntityType, GlobalType, ImportSection, Instruction as I, MemoryType, RefType, TableType,
    ValType,
};

/// A unit's module functions: each one its own functions name is a link,
/// each one they do not is left out.
#[derive(Default)]
pub(super) struct UnitPlan {
    pub(super) unit: bool,
    /// Each module function's file.
    pub(super) origins: std::collections::BTreeMap<String, String>,
    pub(super) called: std::collections::HashSet<String>,
    pub(super) methods: std::collections::HashSet<String>,
    pub(super) links: HashMap<String, Linked>,
    pub(super) slots: u32,
    pub(super) unused: std::collections::HashSet<String>,
}

impl UnitPlan {
    pub(super) fn of(prog: &Program, unit: bool) -> Self {
        if !unit {
            return UnitPlan::default();
        }
        let origins: std::collections::BTreeMap<String, String> =
            prog.origin.iter().cloned().collect();
        let (called, methods) = named(prog, &origins);
        UnitPlan {
            unit,
            origins,
            called,
            methods,
            ..UnitPlan::default()
        }
    }

    // How many function indices module function `f` takes: its stubs, or
    // none when nothing calls it; `None` for one of the file's own.
    pub(super) fn module_fn(&mut self, f: &FnDecl, key: &str) -> Option<u32> {
        let file = self.origins.get(key)?;
        let used = match &f.owner {
            Some(_) => self.methods.contains(&f.name),
            None => self.called.contains(key),
        };
        if !used {
            self.unused.insert(key.to_string());
            return Some(0);
        }
        let n = if f.is_async { 3 } else { 1 };
        self.links.insert(
            key.to_string(),
            Linked {
                slot: self.slots,
                file: file.clone(),
            },
        );
        self.slots += n;
        Some(n)
    }
}

// The functions a unit's own functions call or take as values, and the
// methods they call by name: the module functions it needs links for, or a
// few more.
pub(super) fn named(
    prog: &Program,
    origins: &std::collections::BTreeMap<String, String>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    use crate::ast::{Block, Part};
    type Names = std::collections::HashSet<String>;
    fn block(b: &Block, out: &mut (Names, Names)) {
        for s in &b.stmts {
            match &s.kind {
                StmtKind::Let { init, .. } => expr(init, out),
                StmtKind::Assign { target, value, .. } => {
                    expr(target, out);
                    expr(value, out);
                }
                StmtKind::Expr(e) | StmtKind::Return(Some(e)) => expr(e, out),
                StmtKind::While { cond, body } => {
                    expr(cond, out);
                    block(body, out);
                }
                StmtKind::Loop(body) => block(body, out),
                StmtKind::ForRange { from, to, body, .. } => {
                    expr(from, out);
                    expr(to, out);
                    block(body, out);
                }
                StmtKind::ForEach { items, body, .. } => {
                    expr(items, out);
                    block(body, out);
                }
                StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
            }
        }
        if let Some(t) = &b.tail {
            expr(t, out);
        }
    }
    fn expr(e: &Expr, out: &mut (Names, Names)) {
        match &e.kind {
            ExprKind::Call(name, args) => {
                out.0.insert(name.clone());
                for a in args {
                    expr(a, out);
                }
            }
            ExprKind::Var(v) => {
                out.0.insert(v.clone());
            }
            ExprKind::Method(base, name, args) => {
                out.1.insert(name.clone());
                expr(base, out);
                for a in args {
                    expr(a, out);
                }
            }
            ExprKind::Interp(parts) => {
                for p in parts {
                    if let Part::Value(v) = p {
                        expr(v, out);
                    }
                }
            }
            ExprKind::Bin(_, a, b) | ExprKind::Index(a, b) | ExprKind::ArrayRepeat(a, b) => {
                expr(a, out);
                expr(b, out);
            }
            ExprKind::Unary(_, a)
            | ExprKind::Field(a, _)
            | ExprKind::Await(a)
            | ExprKind::Cast(a, _) => expr(a, out),
            ExprKind::StructLit(_, fields) => fields.iter().for_each(|(_, f)| expr(f, out)),
            ExprKind::ArrayLit(items) => items.iter().for_each(|i| expr(i, out)),
            ExprKind::MapLit(entries) => entries.iter().for_each(|(key, value)| {
                expr(key, out);
                expr(value, out);
            }),
            ExprKind::If(c, then, els) => {
                expr(c, out);
                block(then, out);
                if let Some(b) = els {
                    block(b, out);
                }
            }
            ExprKind::Block(b) => block(b, out),
            ExprKind::Match(..) => unreachable!("`enums::lower` makes a `match` a block"),
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::Nil
            | ExprKind::SelfRef
            | ExprKind::Resumed(..) => {}
        }
    }
    let mut out = (Names::new(), Names::new());
    for f in prog.fns.iter().filter(|f| !origins.contains_key(&f.key())) {
        block(&f.body, &mut out);
    }
    out
}

/// Where the globals a build asks for sit, past what every module has: a
/// whole module's after its own, a unit's after its imports.
pub(super) struct AskedGlobals {
    pub(super) fuel: u32,
    /// The statement count, then the callees' share, then the table's address.
    pub(super) prof: Option<u32>,
    pub(super) dbg_unit: Option<u32>,
    pub(super) host_ctx: u32,
}

impl AskedGlobals {
    pub(super) fn of(options: &Options) -> Self {
        let mut next = if options.unit {
            G_LINK_BASE + 1
        } else {
            GLOBALS as u32
        };
        let fuel = next;
        next += u32::from(options.meter);
        let prof = options.profile.then_some(next);
        next += PROF_GLOBALS * u32::from(options.profile);
        let dbg_unit = (options.unit && options.debug).then_some(next);
        next += u32::from(dbg_unit.is_some());
        AskedGlobals {
            fuel,
            prof,
            dbg_unit,
            host_ctx: next,
        }
    }
}

// What a unit shares with the rest of its script: the memory, the `fn`
// table, the allocator's globals, where its own parts start, and what its
// build counts with. A direct unit's id comes last, once it is known to be one.
pub(super) fn unit_imports(sec: &mut ImportSection, options: &Options) {
    let host = words::HOST_MODULE;
    let memory = MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    };
    sec.import(host, words::MEMORY, EntityType::Memory(memory));
    let table = TableType {
        element_type: RefType::FUNCREF,
        table64: false,
        minimum: 1,
        maximum: None,
        shared: false,
    };
    sec.import(host, words::TABLE, EntityType::Table(table));
    let global = |mutable| {
        EntityType::Global(GlobalType {
            val_type: ValType::I32,
            mutable,
            shared: false,
        })
    };
    for name in words::UNIT_GLOBALS {
        sec.import(host, name, global(true));
    }
    for name in [words::MEMORY_BASE, words::TABLE_BASE, words::LINK_BASE] {
        sec.import(host, name, global(false));
    }
    if options.meter {
        sec.import(host, words::FUEL, global(true));
    }
    if options.profile {
        sec.import(host, words::STEPS, global(true));
        sec.import(host, words::PROF_CALLEES, global(true));
        sec.import(host, words::PROF, global(false));
    }
    if options.debug {
        sec.import(host, words::DBG_UNIT, global(false));
    }
}

// A direct unit's id, past everything [`unit_imports`] asked for.
pub(super) fn host_ctx_import(sec: &mut ImportSection) {
    let id = GlobalType {
        val_type: ValType::I32,
        mutable: false,
        shared: false,
    };
    sec.import(words::HOST_MODULE, words::HOST_CTX, EntityType::Global(id));
}

// A debug build's breakpoint table and a profiled build's rows, zeros past a
// unit's static strings: `data`, and where each starts.
pub(super) fn unit_tables(data: &mut Vec<u8>, probes: usize, rows: usize) -> (u32, u32) {
    let breaks_at = data.len();
    data.resize(breaks_at + probes, 0);
    let rows_at = if rows == 0 {
        data.len()
    } else {
        data.len().next_multiple_of(16)
    };
    data.resize(rows_at + 16 * rows, 0);
    (breaks_at as u32, rows_at as u32)
}

// A function of another unit: a stub that calls through its link slot, or
// three for an async one. A call and a `fn` value of it go to the stub.
pub(super) fn link_stubs(g: &Gen, sig: &Sig, slot: u32, out: &mut Emitted) {
    let stub = |p: Vec<ValType>, r: Vec<ValType>, at: u32| {
        let ty = g.type_idx(p.clone(), r);
        let mut code: Vec<I<'static>> = (0..p.len() as u32).map(I::LocalGet).collect();
        code.extend([
            I::I32Const(at as i32),
            I::GlobalGet(G_LINK_BASE),
            I::I32Add,
            I::CallIndirect {
                type_index: ty,
                table_index: 0,
            },
        ]);
        (
            ty,
            FuncBody {
                locals: vec![],
                code,
            },
        )
    };
    g.calls_indirect.set(true);
    if sig.kind == Kind::Async {
        let ((sp, sr), (pp, pr)) = asyncs::signatures(g, sig);
        out.bodies.push(stub(sp, sr, slot));
        out.bodies.push(stub(pp, pr, slot + 1));
        out.bodies.push(stub(vec![], vec![ValType::I32], slot + 2));
    } else {
        let (p, r) = g.wasm_sig(sig);
        out.bodies.push(stub(p, r, slot));
    }
}

// Each link slot's target, in slot order: the file, its export, its types.
pub(super) fn link_list(g: &Gen, prog: &Program) -> Vec<crate::Link> {
    let slots = |v: &[ValType]| -> Vec<Slot> {
        v.iter()
            .map(|t| match t {
                ValType::I64 => Slot::I64,
                ValType::F64 => Slot::F64,
                _ => Slot::I32,
            })
            .collect()
    };
    let mut out = vec![];
    for f in &prog.fns {
        let key = f.key();
        let Some(link) = g.links.get(&key) else {
            continue;
        };
        let sig = &g.sigs[&key];
        // Its name in its own unit: `util::Thing.m` is `Thing.m` there.
        let own = key.split_once("::").map_or(key.as_str(), |(_, k)| k);
        let mut add = |suffix: &str, (p, r): Signature| {
            out.push(crate::Link {
                file: link.file.clone(),
                export: format!("{}{own}{suffix}", words::LINK),
                params: slots(&p),
                results: slots(&r),
            });
        };
        if sig.kind == Kind::Async {
            let (start, poll) = asyncs::signatures(g, sig);
            add(words::START, start);
            add(words::POLL, poll);
            add(words::FRAME, (vec![], vec![ValType::I32]));
        } else {
            add("", g.wasm_sig(sig));
        }
    }
    out
}
