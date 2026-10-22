mod asyncs;
mod call;
mod direct;
mod expr;
mod func;
mod hoist;
mod host;
mod lower;
mod optional;
mod rt;
mod stmt;

use crate::ast::{Expr, ExprKind, FnDecl, Pos, Program, StmtKind, Ty};
use crate::{
    Compiled, Error, ExternInfo, FnInfo, HostApi, HostSignature, Import, Literal, Meta, Options,
    Probe, Slot, words,
};
mod checks;
pub(crate) use checks::unknown_type;
use checks::{check_structs, check_types, defaults};
pub(crate) use func::show;
use func::{FnGen, LVal, Mode, R, err};
use rt::{
    DATA_BASE, DEBUG_STACK, G_BUMP, G_DBG_BASE, G_DBG_BREAKS, G_DBG_FLAGS, G_DBG_SP, G_LIVE,
    GLOBALS, HELPER_COUNT, Helpers, load, mem, store,
};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, ElementSection, Elements, EntityType,
    ExportKind, ExportSection, Function, FunctionSection, GlobalSection, GlobalType, ImportSection,
    Instruction as I, MemorySection, MemoryType, Module, RefType, TableSection, TableType,
    TypeSection, ValType,
};

type Signature = (Vec<ValType>, Vec<ValType>);
type Structs = HashMap<String, Vec<(String, Ty)>>;

/// A class object: [count i32][pad], then one 8-byte slot per field slot.
pub(crate) const OBJ_HEADER: u64 = 8;

pub(crate) struct FuncBody {
    pub locals: Vec<ValType>,
    pub code: Vec<I<'static>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Extern,
    /// An engine function from the host API: called like an `extern fn`.
    Host,
    ExternAsync,
    /// An engine function awaited like an `extern async fn`.
    HostAsync,
    Script,
    Async,
}

#[derive(Clone)]
pub(crate) struct Sig {
    pub idx: u32,
    pub params: Vec<Ty>,
    /// Per parameter, whether it is `&mut`: passed in, and handed back after
    /// `self` with the result, for the caller to store where it came from.
    pub refs: Vec<bool>,
    pub ret: Ty,
    pub owner: Option<String>,
    pub kind: Kind,
}

impl Sig {
    /// The `&mut` parameters' types, in order.
    pub(crate) fn ref_tys(&self) -> impl Iterator<Item = &Ty> {
        self.params
            .iter()
            .zip(&self.refs)
            .filter(|(_, r)| **r)
            .map(|(t, _)| t)
    }
    pub(crate) fn has_refs(&self) -> bool {
        self.refs.contains(&true)
    }
}

#[derive(Clone)]
pub(crate) enum Site {
    Host,
    Script { child: u32 },
}

pub(crate) struct Builtins {
    pub fmt_f64: u32,
    pub await_i64: u32,
    pub await_f64: u32,
    pub await_i32: u32,
    pub await_heap: u32,
    pub math: HashMap<&'static str, (u32, usize)>,
    pub strs: HashMap<&'static str, u32>,
    pub anys: HashMap<&'static str, u32>,
    pub dbg_probe: u32,
    pub boxing: u32,
    pub unboxing: u32,
}

pub(crate) const MATH: [(&str, usize); 10] = [
    ("sin", 1),
    ("cos", 1),
    ("tan", 1),
    ("asin", 1),
    ("acos", 1),
    ("atan", 1),
    ("atan2", 2),
    ("pow", 2),
    ("exp", 1),
    ("ln", 1),
];

#[derive(Default)]
struct Dyn {
    base: u32,
    by_key: HashMap<String, u32>,
    pending: Vec<(u32, Ty)>,
    count: u32,
}

pub(crate) struct Gen {
    pub structs: Structs,
    pub classes: BTreeSet<String>,
    pub consts: HashMap<String, Expr>,
    pub sigs: HashMap<String, Sig>,
    pub h: Helpers,
    pub imp: Builtins,
    pub host: HostApi,
    pub host_calls: Vec<HostSignature>,
    pub meter: bool,
    types: RefCell<(Vec<Signature>, HashMap<Signature, u32>)>,
    data: RefCell<(Vec<u8>, HashMap<String, u32>)>,
    dyns: RefCell<Dyn>,
    probes: RefCell<Vec<Probe>>,
    /// The functions a `fn` value names, in table order from slot 1.
    fn_refs: RefCell<Vec<u32>>,
}

impl Gen {
    pub(crate) fn slots(&self, ty: &Ty) -> Vec<(ValType, Option<Ty>)> {
        match ty {
            Ty::Int | Ty::Node => vec![(ValType::I64, None)],
            Ty::Float => vec![(ValType::F64, None)],
            Ty::Bool | Ty::Fn(..) => vec![(ValType::I32, None)],
            Ty::Str | Ty::Array(_) | Ty::Any => vec![(ValType::I32, Some(ty.clone()))],
            Ty::Void => vec![],
            // A class is its object's pointer, released like a string.
            Ty::Struct(s) if self.classes.contains(s) => vec![(ValType::I32, Some(ty.clone()))],
            Ty::Struct(s) => self.object_slots(s),
            Ty::Optional(t) => {
                let mut v = self.slots(t);
                v.push((ValType::I32, None));
                v
            }
        }
    }
    /// A struct's fields laid flat: a value's slots, or a class object's.
    pub(crate) fn object_slots(&self, s: &str) -> Vec<(ValType, Option<Ty>)> {
        self.structs[s]
            .iter()
            .flat_map(|(_, t)| self.slots(t))
            .collect()
    }
    pub(crate) fn is_class<'t>(&self, ty: &'t Ty) -> Option<&'t str> {
        match ty {
            Ty::Struct(s) if self.classes.contains(s) => Some(s),
            _ => None,
        }
    }
    /// Where field `field` of class `s` sits in its object, and its type.
    pub(crate) fn class_field(&self, s: &str, field: &str) -> Option<(u64, Ty)> {
        let mut off = OBJ_HEADER;
        for (f, t) in &self.structs[s] {
            if f == field {
                return Some((off, t.clone()));
            }
            off += 8 * self.slots(t).len() as u64;
        }
        None
    }
    /// Where function `idx` sits in the table a `fn` value indexes; slot 0
    /// is empty, so a `fn` never set traps when called.
    pub(crate) fn fn_ref(&self, idx: u32) -> u32 {
        let mut refs = self.fn_refs.borrow_mut();
        let at = refs.iter().position(|i| *i == idx).unwrap_or_else(|| {
            refs.push(idx);
            refs.len() - 1
        });
        at as u32 + 1
    }
    pub(crate) fn elem_size(&self, ty: &Ty) -> u32 {
        8 * self.slots(ty).len() as u32
    }
    fn vts(&self, ty: &Ty) -> Vec<ValType> {
        self.slots(ty).into_iter().map(|(v, _)| v).collect()
    }
    pub(crate) fn type_idx(&self, p: Vec<ValType>, r: Vec<ValType>) -> u32 {
        let mut t = self.types.borrow_mut();
        let key = (p, r);
        if let Some(i) = t.1.get(&key) {
            return *i;
        }
        let i = t.0.len() as u32;
        t.0.push(key.clone());
        t.1.insert(key, i);
        i
    }
    /// A static string object: a negative count, so retain and release skip it.
    pub(crate) fn string(&self, s: &str) -> u32 {
        let mut d = self.data.borrow_mut();
        if let Some(p) = d.1.get(s) {
            return *p;
        }
        while !d.0.len().is_multiple_of(8) {
            d.0.push(0);
        }
        let p = DATA_BASE + d.0.len() as u32;
        d.0.extend_from_slice(&(-1i32).to_le_bytes());
        d.0.extend_from_slice(&(s.len() as u32).to_le_bytes());
        d.0.extend_from_slice(s.as_bytes());
        d.1.insert(s.to_string(), p);
        p
    }
    pub(crate) fn dec_fn(&self, ty: &Ty) -> u32 {
        if *ty == Ty::Str {
            return self.h.dec_str;
        }
        let key = show(ty);
        let mut d = self.dyns.borrow_mut();
        if let Some(i) = d.by_key.get(&key) {
            return *i;
        }
        let i = d.base + d.count;
        d.count += 1;
        d.by_key.insert(key, i);
        d.pending.push((i, ty.clone()));
        i
    }
    pub(crate) fn probe(&self, p: Probe) -> u32 {
        let mut v = self.probes.borrow_mut();
        v.push(p);
        v.len() as u32 - 1
    }
    fn wasm_sig(&self, s: &Sig) -> Signature {
        let selfv = self.self_vts(s.owner.as_ref());
        let mut p = selfv.clone();
        for t in &s.params {
            p.extend(self.vts(t));
        }
        let mut r = selfv;
        for t in s.ref_tys() {
            r.extend(self.vts(t));
        }
        r.extend(self.vts(&s.ret));
        (p, r)
    }
    fn self_vts(&self, owner: Option<&String>) -> Vec<ValType> {
        owner
            .map(|o| self.vts(&Ty::Struct(o.clone())))
            .unwrap_or_default()
    }

    // dec for an array: release each element's heap fields, then its storage.
    fn dec_array(&self, et: &Ty) -> FuncBody {
        let esz = self.elem_size(et) as i32;
        let heap: Vec<(u64, u32)> = self
            .slots(et)
            .into_iter()
            .enumerate()
            .filter_map(|(j, (_, h))| h.map(|h| (8 * j as u64, self.dec_fn(&h))))
            .collect();
        let mut drop = vec![];
        if !heap.is_empty() {
            drop.extend([
                I::LocalGet(0),
                I::I32Load(mem(12, 2)),
                I::LocalSet(3),
                I::Block(BlockType::Empty),
                I::Loop(BlockType::Empty),
            ]);
            drop.extend([
                I::LocalGet(2),
                I::LocalGet(0),
                I::I32Load(mem(4, 2)),
                I::I32GeU,
                I::BrIf(1),
            ]);
            for (off, f) in heap {
                drop.extend([
                    I::LocalGet(3),
                    I::LocalGet(2),
                    I::I32Const(esz),
                    I::I32Mul,
                    I::I32Add,
                    I::I32Load(mem(off, 2)),
                    I::Call(f),
                ]);
            }
            drop.extend([
                I::LocalGet(2),
                I::I32Const(1),
                I::I32Add,
                I::LocalSet(2),
                I::Br(0),
                I::End,
                I::End,
            ]);
        }
        drop.extend([
            I::LocalGet(0),
            I::I32Load(mem(12, 2)),
            I::If(BlockType::Empty),
            I::LocalGet(0),
            I::I32Load(mem(12, 2)),
            I::Call(self.h.free),
            I::End,
        ]);
        FuncBody {
            locals: vec![ValType::I32; 3],
            code: rt::dec_body(self.h, &drop),
        }
    }

    // dec for a class object: release each heap field, then the object.
    fn dec_class(&self, s: &str) -> FuncBody {
        let mut drop = vec![];
        for (j, (_, heap)) in self.object_slots(s).into_iter().enumerate() {
            if let Some(h) = heap {
                drop.extend([
                    I::LocalGet(0),
                    I::I32Load(mem(OBJ_HEADER + 8 * j as u64, 2)),
                    I::Call(self.dec_fn(&h)),
                ]);
            }
        }
        FuncBody {
            locals: vec![ValType::I32],
            code: rt::dec_body(self.h, &drop),
        }
    }

    // dec for an `any`: release what its cell points at, by its tag.
    fn dec_any(&self) -> FuncBody {
        use crate::anys::{LIST, MAP, STR};
        let anys = self.dec_fn(&Ty::Array(Box::new(Ty::Any)));
        let strs = self.dec_fn(&Ty::Array(Box::new(Ty::Str)));
        let when = |tag: i32, then: &[I<'static>]| {
            let mut c = vec![
                I::LocalGet(2),
                I::I32Const(tag),
                I::I32Eq,
                I::If(BlockType::Empty),
            ];
            c.extend(then.iter().cloned());
            c.push(I::End);
            c
        };
        let at = |off: u64, f: u32| [I::LocalGet(0), I::I32Load(mem(off, 2)), I::Call(f)];
        let mut drop = vec![I::LocalGet(0), I::I32Load(mem(4, 2)), I::LocalSet(2)];
        drop.extend(when(STR, &at(8, self.h.dec_str)));
        drop.extend(when(LIST, &at(8, anys)));
        let mut map = at(8, strs).to_vec();
        map.extend(at(12, anys));
        drop.extend(when(MAP, &map));
        FuncBody {
            locals: vec![ValType::I32; 3],
            code: rt::dec_body(self.h, &drop),
        }
    }
}

fn has_heap(t: &Ty, structs: &Structs, classes: &BTreeSet<String>) -> bool {
    match t {
        Ty::Str | Ty::Array(_) | Ty::Any => true,
        Ty::Struct(s) if classes.contains(s) => true,
        Ty::Struct(s) => structs[s]
            .iter()
            .any(|(_, f)| has_heap(f, structs, classes)),
        Ty::Optional(t) => has_heap(t, structs, classes),
        _ => false,
    }
}

// A batch entry runs the method once per instance with the same arguments,
// so it exists only where no argument or result needs releasing.
fn batchable(f: &FnDecl, structs: &Structs, classes: &BTreeSet<String>) -> bool {
    f.public
        && f.owner.is_some()
        && !f.is_async
        && f.owner.as_ref().is_some_and(|o| !classes.contains(o))
        && f.params.iter().all(|p| !p.by_ref)
        && !has_heap(&f.ret, structs, classes)
        && f.params.iter().all(|p| !has_heap(&p.ty, structs, classes))
}

fn consts(prog: &Program, host: &HostApi) -> R<HashMap<String, Expr>> {
    let mut out = HashMap::new();
    for (n, v) in &host.constants {
        let kind = match v {
            Literal::Int(x) => ExprKind::Int(*x),
            Literal::Float(x) => ExprKind::Float(*x),
            Literal::Bool(b) => ExprKind::Bool(*b),
            Literal::Str(t) => ExprKind::Str(t.clone()),
        };
        let pos = Pos::default();
        out.insert(n.clone(), Expr { kind, pos });
    }
    for (n, e) in &prog.consts {
        let literal = matches!(
            e.kind,
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_)
        ) || matches!(&e.kind, ExprKind::Unary(_, inner) if matches!(inner.kind, ExprKind::Int(_) | ExprKind::Float(_)));
        if !literal {
            return err(e.pos, format!("const `{n}` must be a literal"));
        }
        if !n.contains("::") {
            out.insert(format!("{}::{n}", crate::modules::ROOT), e.clone());
        }
        out.insert(n.clone(), e.clone());
    }
    Ok(out)
}

// User externs, then the engine functions the script names: the first imports.
fn import_sigs(
    prog: &Program,
    options: &Options,
    sigs: &mut HashMap<String, Sig>,
) -> (u32, Vec<HostSignature>) {
    let mut idx = 0u32;
    for x in &prog.externs {
        let kind = if x.is_async {
            Kind::ExternAsync
        } else {
            Kind::Extern
        };
        sigs.insert(
            x.name.clone(),
            Sig {
                idx,
                params: x.params.iter().map(|p| p.ty.clone()).collect(),
                refs: vec![false; x.params.len()],
                ret: x.ret.clone(),
                owner: None,
                kind,
            },
        );
        idx += 1;
    }
    let host_calls = host::host_calls(prog, &options.host);
    for f in &host_calls {
        let sig = Sig {
            idx,
            params: f.params.clone(),
            refs: vec![false; f.params.len()],
            ret: f.ret.clone(),
            owner: None,
            kind: if f.is_async {
                Kind::HostAsync
            } else {
                Kind::Host
            },
        };
        sigs.insert(f.key.clone(), sig);
        idx += 1;
    }
    (idx, host_calls)
}

// Function indices: user imports, builtin imports, fixed helpers, script
// functions, then the release functions generated for each array type.
fn layout(prog: &Program, options: &Options) -> R<Gen> {
    let debug = options.debug;
    check_structs(prog)?;
    check_types(prog)?;
    let structs: Structs = prog
        .structs
        .iter()
        .map(|(n, fs)| {
            (
                n.clone(),
                fs.iter().map(|p| (p.name.clone(), p.ty.clone())).collect(),
            )
        })
        .collect();
    let classes: BTreeSet<String> = prog.classes.iter().cloned().collect();
    let mut sigs = HashMap::new();
    let (base, host_calls) = import_sigs(prog, options, &mut sigs);
    let math = MATH
        .iter()
        .enumerate()
        .map(|(k, (name, n))| (*name, (base + 5 + k as u32, *n)))
        .collect();
    let strs = crate::strings::METHODS
        .iter()
        .enumerate()
        .map(|(k, m)| (m.name, base + 5 + (MATH.len() + k) as u32))
        .collect();
    let after_strs = base + 5 + (MATH.len() + crate::strings::METHODS.len()) as u32;
    let anys = crate::anys::METHODS
        .iter()
        .enumerate()
        .map(|(k, m)| (m.name, after_strs + k as u32))
        .collect();
    let boxing = after_strs + crate::anys::METHODS.len() as u32;
    let dbg = boxing + 2;
    let pick = |k: u32| if debug { dbg + k } else { u32::MAX };
    let imp = Builtins {
        fmt_f64: base,
        await_i64: base + 1,
        await_f64: base + 2,
        await_i32: base + 3,
        await_heap: base + 4,
        math,
        strs,
        anys,
        dbg_probe: pick(0),
        boxing,
        unboxing: boxing + 1,
    };
    let mut idx = dbg + u32::from(debug);
    let h = Helpers::at(idx);
    idx += HELPER_COUNT;
    for f in &prog.fns {
        let key = f.key();
        let kind = if f.is_async {
            Kind::Async
        } else {
            Kind::Script
        };
        let sig = Sig {
            idx,
            params: f.params.iter().map(|p| p.ty.clone()).collect(),
            refs: f.params.iter().map(|p| p.by_ref).collect(),
            ret: f.ret.clone(),
            owner: f.owner.clone(),
            kind,
        };
        idx += if f.is_async || batchable(f, &structs, &classes) {
            2
        } else {
            1
        };
        // A root function is also what a module that uses the root calls.
        if f.owner.is_none() && !key.contains("::") {
            sigs.insert(format!("{}::{key}", crate::modules::ROOT), sig.clone());
        }
        if sigs.insert(key.clone(), sig).is_some() {
            return err(f.pos, format!("`{key}` is defined twice"));
        }
    }
    Ok(Gen {
        structs,
        classes,
        consts: consts(prog, &options.host)?,
        sigs,
        h,
        imp,
        host: options.host.clone(),
        host_calls,
        meter: options.meter,
        types: RefCell::new((vec![], HashMap::new())),
        data: RefCell::new((vec![], HashMap::new())),
        dyns: RefCell::new(Dyn {
            base: idx,
            ..Dyn::default()
        }),
        probes: RefCell::new(vec![]),
        fn_refs: RefCell::new(vec![]),
    })
}

// A scalar a built-in import passes: a heap value goes as its pointer.
fn wasm_of(t: &Ty) -> ValType {
    match t {
        Ty::Int | Ty::Node => ValType::I64,
        Ty::Float => ValType::F64,
        _ => ValType::I32,
    }
}

fn imports(g: &Gen, prog: &Program, debug: bool) -> (ImportSection, Vec<ExternInfo>, Vec<Import>) {
    use ValType::{F64, I32, I64};
    let mut sec = ImportSection::new();
    let mut externs = vec![];
    let mut list = vec![];
    let slot = |v: &ValType| match v {
        I64 => Slot::I64,
        F64 => Slot::F64,
        _ => Slot::I32,
    };
    let mut add = |sec: &mut ImportSection, name: &str, p: Vec<ValType>, r: Vec<ValType>| {
        list.push(Import {
            name: name.to_string(),
            params: p.iter().map(slot).collect(),
            results: r.iter().map(slot).collect(),
        });
        sec.import(
            words::HOST_MODULE,
            name,
            EntityType::Function(g.type_idx(p, r)),
        );
    };
    for x in &prog.externs {
        let s = &g.sigs[&x.name];
        let (p, mut r) = g.wasm_sig(s);
        if s.kind == Kind::ExternAsync {
            r = vec![I64];
        }
        add(&mut sec, &x.name, p, r);
        externs.push(ExternInfo {
            name: x.name.clone(),
            params: s.params.clone(),
            ret: s.ret.clone(),
            is_async: x.is_async,
        });
    }
    for f in &g.host_calls {
        let s = &g.sigs[&f.key];
        let (p, mut r) = g.wasm_sig(s);
        if s.kind == Kind::HostAsync {
            r = vec![I64];
        }
        add(&mut sec, &f.key, p, r);
    }
    let mut builtins: Vec<(String, Vec<ValType>, Vec<ValType>)> = vec![
        (words::FMT_F64.into(), vec![F64, I32], vec![I32]),
        (words::AWAIT_I64.into(), vec![I64], vec![I64]),
        (words::AWAIT_F64.into(), vec![I64], vec![F64]),
        (words::AWAIT_I32.into(), vec![I64], vec![I32]),
        (words::AWAIT_HEAP.into(), vec![I64], vec![I32]),
    ];
    builtins.extend(MATH.iter().map(|(name, n)| {
        (
            format!("{}{name}", words::MATH_PREFIX),
            vec![F64; *n],
            vec![F64],
        )
    }));
    builtins.extend(crate::strings::METHODS.iter().map(|m| {
        let p = crate::strings::params(m);
        let r = crate::strings::ty(m.ret);
        (
            format!("{}{}", words::STR_PREFIX, m.name),
            p.iter().map(wasm_of).collect(),
            vec![wasm_of(&r)],
        )
    }));
    builtins.extend(crate::anys::METHODS.iter().map(|m| {
        let p = crate::anys::params(m);
        let r = crate::anys::ty(m.ret);
        (
            format!("{}{}", words::ANY_PREFIX, m.name),
            p.iter().map(wasm_of).collect(),
            vec![wasm_of(&r)],
        )
    }));
    builtins.push((words::BOX.into(), vec![I32, I32], vec![I32]));
    builtins.push((words::UNBOX.into(), vec![I32, I32, I32], vec![]));
    if debug {
        builtins.push((words::DBG_PROBE.into(), vec![I32], vec![]));
    }
    for (name, p, r) in builtins {
        add(&mut sec, &name, p, r);
    }
    (sec, externs, list)
}

#[derive(Default)]
struct Emitted {
    bodies: Vec<(u32, FuncBody)>,
    exports: Vec<(String, u32)>,
    infos: Vec<FnInfo>,
    func_names: Vec<String>,
}

/// Every function's error, the module's own first: what `balaur check` lists.
pub(crate) fn errors(prog: &Program, options: &Options) -> Vec<Error> {
    let g = match layout(prog, options) {
        Ok(g) => g,
        Err(e) => return vec![e],
    };
    if let Err(e) = defaults(prog) {
        return vec![e];
    }
    let mut found = vec![];
    if let Err(e) = emit_fns(&g, prog, options.debug, Some(&mut found)) {
        found.push(e);
    }
    found
}

// `sink`: collect each function's error and go on, rather than stop.
fn emit_fns(g: &Gen, prog: &Program, debug: bool, mut sink: Option<&mut Vec<Error>>) -> R<Emitted> {
    let mut out = Emitted::default();
    for (p, r, body) in rt::helpers(
        g.h,
        &rt::Imports {
            fmt_f64: g.imp.fmt_f64,
        },
    ) {
        out.bodies.push((g.type_idx(p, r), body));
    }
    let mut frame_sizes: HashMap<String, u32> = HashMap::new();
    let mut patches: Vec<(usize, usize, String)> = vec![];
    for f in &prog.fns {
        let key = f.key();
        let sig = g.sigs[&key].clone();
        let fid = debug.then_some(out.func_names.len() as u32);
        out.func_names.push(key.clone());
        let at = out.bodies.len() + usize::from(f.is_async);
        let made = if f.is_async {
            asyncs::emit(g, &sig, f, fid, &mut out)
        } else {
            emit_sync(g, &sig, f, fid, &mut out)
        };
        // A linked function's error names the file it is in.
        let made = made.map_err(|e| match prog.origin.iter().find(|(k, _)| *k == key) {
            Some((_, file)) => Error {
                pos: e.pos,
                msg: format!("in {file}: {}", e.msg),
            },
            None => e,
        });
        let (frame, pts) = match (made, sink.as_deref_mut()) {
            (Ok(v), _) => v,
            (Err(e), Some(found)) => {
                found.push(e);
                continue;
            }
            (Err(e), None) => return Err(e),
        };
        patches.extend(pts.into_iter().map(|(pos, k)| (at, pos, k)));
        if f.is_async {
            frame_sizes.insert(key.clone(), frame);
        }
        let info = FnInfo {
            key,
            owner: sig.owner,
            params: sig.params,
            ret: sig.ret,
            is_async: f.is_async,
            frame_size: frame,
            public: f.public,
        };
        out.infos.push(info);
    }
    if sink.is_some_and(|found| !found.is_empty()) {
        return Ok(out);
    }
    for (at, pos, key) in patches {
        let size = *frame_sizes.get(&key).ok_or(Error {
            pos: crate::ast::Pos::default(),
            msg: format!("internal: no frame size for `{key}`"),
        })?;
        out.bodies[at].1.code[pos] = I::I32Const(size as i32);
    }
    Ok(out)
}

fn emit_sync(
    g: &Gen,
    sig: &Sig,
    f: &FnDecl,
    fid: Option<u32>,
    out: &mut Emitted,
) -> R<(u32, Vec<(usize, String)>)> {
    let key = f.key();
    let (body, pts) = sync_fn(g, sig, f, fid)?;
    let (p, r) = g.wasm_sig(sig);
    out.bodies.push((g.type_idx(p, r), body));
    // Every method is exported: the host calls `init` and `update` by name.
    // One with a `&mut` parameter has no caller outside the module to hand
    // its value back to.
    if (f.public || f.owner.is_some()) && !sig.has_refs() {
        out.exports.push((key.clone(), sig.idx));
        if batchable(f, &g.structs, &g.classes) {
            let (bt, bf) = batch_fn(g, sig);
            out.bodies.push((bt, bf));
            out.exports
                .push((format!("{key}{}", words::BATCH), sig.idx + 1));
        }
    }
    Ok((0, pts))
}

// The release function for each array type the module touched, in index order.
fn emit_dyns(g: &Gen, out: &mut Emitted) -> Vec<(u32, Ty)> {
    let mut dyns: Vec<(u32, Ty, FuncBody)> = vec![];
    loop {
        let next = g.dyns.borrow_mut().pending.pop();
        let Some((i, ty)) = next else { break };
        let body = match &ty {
            Ty::Array(et) => g.dec_array(et),
            Ty::Any => g.dec_any(),
            Ty::Struct(s) => g.dec_class(s),
            _ => unreachable!("only arrays, classes and `any` get a generated release function"),
        };
        dyns.push((i, ty.clone(), body));
    }
    dyns.sort_by_key(|d| d.0);
    let list = dyns.iter().map(|d| (d.0, d.1.clone())).collect();
    for (_, _, body) in dyns {
        out.bodies
            .push((g.type_idx(vec![ValType::I32], vec![]), body));
    }
    list
}

// `shift` is how far a direct module moved every function.
fn export_section(
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
fn table_sections(g: &Gen, shift: u32) -> (Option<TableSection>, Option<ElementSection>) {
    let refs = g.fn_refs.borrow();
    if refs.is_empty() {
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
    let funcs: Vec<u32> = refs.iter().map(|i| i + shift).collect();
    let mut elements = ElementSection::new();
    elements.active(
        Some(0),
        &ConstExpr::i32_const(1),
        Elements::Functions(std::borrow::Cow::Owned(funcs)),
    );
    (Some(table), Some(elements))
}

fn code_sections(bodies: &[(u32, FuncBody)]) -> (FunctionSection, CodeSection) {
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
fn memory_and_globals(
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
    (memory, globals)
}

pub(crate) fn generate(prog: &Program, options: &Options) -> Result<Compiled, Error> {
    let debug = options.debug;
    let g = layout(prog, options)?;
    let defaults = defaults(prog)?;
    let (imports, externs, import_list) = imports(&g, prog, debug);
    let mut out = emit_fns(&g, prog, debug, None)?;
    let dyns = emit_dyns(&g, &mut out);
    // A browser build calls the engine through its exports, never JavaScript.
    let direct = options.direct_imports && direct::applies(&import_list);
    let (imports, shift) = if direct {
        let host = GLOBALS as u32 + u32::from(options.meter);
        direct::rewrite(&g, &import_list, &mut out.bodies, host)
    } else {
        (imports, 0)
    };
    let (funcs, code) = code_sections(&out.bodies);
    let (table, elements) = table_sections(&g, shift);
    let mut exports = export_section(&g, &out, &dyns, debug, shift);
    if direct {
        let host = GLOBALS as u32 + u32::from(options.meter);
        exports.export(words::HOST_CTX, ExportKind::Global, host);
    }
    let data = g.data.borrow().0.clone();
    let mut types = TypeSection::new();
    for (p, r) in &g.types.borrow().0 {
        types.ty().function(p.clone(), r.clone());
    }
    let (memory, mut globals) = memory_and_globals(
        data.len() as u32,
        g.probes.borrow().len() as u32,
        debug,
        options,
    );
    if direct {
        let mutable = GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        };
        globals.global(mutable, &ConstExpr::i32_const(0));
    }
    let mut ds = DataSection::new();
    ds.active(0, &ConstExpr::i32_const(DATA_BASE as i32), data);
    let mut m = Module::new();
    m.section(&types).section(&imports).section(&funcs);
    if let Some(t) = &table {
        m.section(t);
    }
    m.section(&memory).section(&globals).section(&exports);
    if let Some(e) = &elements {
        m.section(e);
    }
    m.section(&code).section(&ds);
    let structs = prog
        .structs
        .iter()
        .map(|(n, fs)| {
            (
                n.clone(),
                fs.iter().map(|p| (p.name.clone(), p.ty.clone())).collect(),
            )
        })
        .collect();
    let meta = Meta {
        structs,
        classes: prog.classes.clone(),
        fns: out.infos,
        externs,
        probes: g.probes.borrow().clone(),
        func_names: out.func_names,
        debug,
        metered: options.meter,
        direct,
        uses: vec![],
        imports: import_list,
        host_calls: g.host_calls.clone(),
        defaults,
    };
    Ok(Compiled {
        wasm: m.finish(),
        meta,
    })
}

fn sync_fn(
    g: &Gen,
    sig: &Sig,
    f: &FnDecl,
    fid: Option<u32>,
) -> R<(FuncBody, Vec<(usize, String)>)> {
    let (p, _) = g.wasm_sig(sig);
    let mut fg = FnGen::new(g, p.len(), Mode::Normal { ret: f.ret.clone() });
    let mut slot = 0u32;
    if let Some(o) = &sig.owner {
        let n = g.slots(&Ty::Struct(o.clone())).len() as u32;
        fg.self_lv = Some(LVal {
            ty: Ty::Struct(o.clone()),
            slots: (0..n).collect(),
        });
        slot = n;
    }
    for prm in &f.params {
        let n = g.slots(&prm.ty).len() as u32;
        let lv = LVal {
            ty: prm.ty.clone(),
            slots: (slot..slot + n).collect(),
        };
        // A `&mut` value goes back to its caller, so this call never frees it.
        if prm.by_ref {
            fg.refs.push(lv.clone());
        } else {
            fg.own(&lv);
        }
        fg.declare(&prm.name, lv);
        slot += n;
    }
    if let Some(id) = fid {
        let frame = fg.local(ValType::I32);
        fg.dbg = Some((frame, id));
    }
    fg.charge();
    let t = if matches!(f.ret, Ty::Optional(_) | Ty::Any | Ty::Node) {
        fg.block_as(&f.body, &f.ret)?
    } else {
        fg.block_body(&f.body)?
    };
    let returns_last = matches!(
        f.body.stmts.last().map(|s| &s.kind),
        Some(StmtKind::Return(_))
    ) && f.body.tail.is_none();
    if f.ret == Ty::Void {
        fg.discard(&t);
        fg.leave(&Ty::Void, false);
    } else if t == Ty::Void && returns_last {
        fg.e(I::Unreachable);
    } else {
        let at = f.body.tail.as_ref().map_or(f.pos, |e| e.pos);
        if t == Ty::Void {
            return err(
                at,
                format!(
                    "`{}` must return {}; end it with a value or `return`",
                    f.key(),
                    show(&f.ret)
                ),
            );
        }
        fg.coerce(&t, &f.ret, at)?;
        fg.leave(&f.ret, false);
    }
    let size = 8 * (fg.nparams + fg.locals.len() as u32);
    let mut code = fg.dbg_enter(size);
    let shift = code.len();
    code.append(&mut fg.code);
    let patches = fg
        .patches
        .into_iter()
        .map(|(p, k)| (p + shift, k))
        .collect();
    Ok((
        FuncBody {
            locals: fg.locals,
            code,
        },
        patches,
    ))
}

// `T.m$batch(base, count, args..)`: `m` over `count` instances stored at
// `base`, one 8-byte slot per field: one host call a frame, not one per instance.
fn batch_fn(g: &Gen, sig: &Sig) -> (u32, FuncBody) {
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
