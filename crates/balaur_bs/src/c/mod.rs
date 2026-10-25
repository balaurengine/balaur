// wasm to C, for a platform that runs no JIT: an iOS export compiles what this
// writes into the app, so its scripts run as native code there. It reads only
// what the compiler emits; anything else is refused by name.

mod func;

use std::fmt::Write as _;
use wasmparser::{ExternalKind, FuncType, Operator, Parser, Payload, TypeRef, ValType};

const PRELUDE: &str = include_str!("prelude.h");

/// Every module in `modules` as one C file: each behind a prefix of its own,
/// and `balaur_bs_modules` listing them by [`crate::module_hash`].
///
/// # Errors
/// A module that does not parse, or uses an instruction this does not write.
pub fn translate(modules: &[&[u8]]) -> Result<String, String> {
    let mut out = String::from(PRELUDE);
    let mut entries = vec![];
    for (i, wasm) in modules.iter().enumerate() {
        let prefix = format!("m{i}");
        let parsed = Module::parse(wasm)?;
        parsed.emit(&prefix, &mut out)?;
        entries.push((crate::module_hash(wasm), prefix));
    }
    out.push_str("\nstatic const bs_entry bs_entries[] = {\n");
    for (hash, prefix) in &entries {
        let _ = writeln!(out, "  {{\"{hash}\", &{prefix}_module}},");
    }
    if entries.is_empty() {
        out.push_str("  {0, 0},\n");
    }
    let _ = write!(
        out,
        "}};\n\nBS_EXPORT const bs_entry *balaur_bs_modules(uint32_t *count) {{\n  \
         *count = {};\n  return bs_entries;\n}}\n",
        entries.len()
    );
    Ok(out)
}

pub(crate) struct Module<'a> {
    pub types: Vec<FuncType>,
    /// The type of each imported function, then of each defined one.
    pub funcs: Vec<u32>,
    pub imports: usize,
    pub memory: (u64, Option<u64>),
    pub globals: Vec<(ValType, u64)>,
    pub exports: Vec<(String, ExternalKind, u32)>,
    pub data: Vec<(u32, &'a [u8])>,
    pub bodies: Vec<wasmparser::FunctionBody<'a>>,
    /// The table `fn` values index: the function in each slot, if any.
    pub table: Vec<Option<u32>>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl<'a> Module<'a> {
    fn parse(wasm: &'a [u8]) -> Result<Self, String> {
        let mut m = Module {
            types: vec![],
            funcs: vec![],
            imports: 0,
            memory: (0, None),
            globals: vec![],
            exports: vec![],
            data: vec![],
            bodies: vec![],
            table: vec![],
        };
        for payload in Parser::new(0).parse_all(wasm) {
            match payload.map_err(err)? {
                Payload::TypeSection(r) => {
                    for group in r {
                        for ty in group.map_err(err)?.into_types() {
                            m.types.push(ty.unwrap_func().clone());
                        }
                    }
                }
                Payload::ImportSection(r) => {
                    for import in r.into_imports() {
                        match import.map_err(err)?.ty {
                            TypeRef::Func(t) => m.funcs.push(t),
                            other => return Err(format!("an import of {other:?}")),
                        }
                    }
                    m.imports = m.funcs.len();
                }
                Payload::FunctionSection(r) => {
                    for t in r {
                        m.funcs.push(t.map_err(err)?);
                    }
                }
                Payload::MemorySection(r) => {
                    for mem in r {
                        let mem = mem.map_err(err)?;
                        m.memory = (mem.initial, mem.maximum);
                    }
                }
                Payload::GlobalSection(r) => {
                    for g in r {
                        let g = g.map_err(err)?;
                        let init = constant(&g.init_expr)?;
                        m.globals.push((g.ty.content_type, init));
                    }
                }
                Payload::ExportSection(r) => {
                    for e in r {
                        let e = e.map_err(err)?;
                        m.exports.push((e.name.to_string(), e.kind, e.index));
                    }
                }
                Payload::DataSection(r) => {
                    for d in r {
                        let d = d.map_err(err)?;
                        let wasmparser::DataKind::Active { offset_expr, .. } = d.kind else {
                            return Err("a passive data segment".into());
                        };
                        let at = u32::try_from(constant(&offset_expr)?).map_err(err)?;
                        m.data.push((at, d.data));
                    }
                }
                Payload::CodeSectionEntry(body) => m.bodies.push(body),
                Payload::TableSection(r) => {
                    for t in r {
                        let size = t.map_err(err)?.ty.initial;
                        m.table = vec![None; usize::try_from(size).map_err(err)?];
                    }
                }
                Payload::ElementSection(r) => {
                    for e in r {
                        m.element(e.map_err(err)?)?;
                    }
                }
                Payload::StartSection { .. } | Payload::TagSection(_) => {
                    return Err("a start function and tags are not written".into());
                }
                _ => {}
            }
        }
        Ok(m)
    }

    fn element(&mut self, e: wasmparser::Element<'a>) -> Result<(), String> {
        let wasmparser::ElementKind::Active { offset_expr, .. } = e.kind else {
            return Err("a passive element segment".into());
        };
        let wasmparser::ElementItems::Functions(funcs) = e.items else {
            return Err("an element segment of expressions".into());
        };
        let start = usize::try_from(constant(&offset_expr)?).map_err(err)?;
        for (at, f) in (start..).zip(funcs) {
            let slot = self
                .table
                .get_mut(at)
                .ok_or("an element past the table's end")?;
            *slot = Some(f.map_err(err)?);
        }
        Ok(())
    }

    pub(crate) fn func_type(&self, index: u32) -> &FuncType {
        &self.types[self.funcs[index as usize] as usize]
    }

    fn emit(&self, p: &str, out: &mut String) -> Result<(), String> {
        let _ = writeln!(out, "\n/* ---- module {p} ---- */");
        for (i, ty) in self.types.iter().enumerate() {
            if ty.results().len() > 1 {
                let fields = each(ty.results().iter().enumerate(), |s, (j, t)| {
                    let _ = write!(s, " {} r{j};", c_type(*t));
                });
                let _ = writeln!(out, "typedef struct {{{fields} }} {p}_t{i};");
            }
        }
        for f in 0..self.funcs.len() {
            let _ = writeln!(out, "BS_FN {};", self.signature(p, f as u32));
        }
        for f in 0..self.imports {
            self.import_thunk(p, f as u32, out);
        }
        if !self.table.is_empty() {
            self.indirect(p, out);
        }
        for (i, body) in self.bodies.iter().enumerate() {
            let index = (self.imports + i) as u32;
            func::emit(self, p, index, body, out).map_err(|e| format!("function {index}: {e}"))?;
        }
        self.dispatch(p, out);
        self.descriptor(p, out);
        Ok(())
    }

    // A `call_indirect` of type `t` is `{p}_ci{t}`: a switch over the slots
    // whose function has that type, and a trap for any other slot.
    fn indirect(&self, p: &str, out: &mut String) {
        for (t, ty) in self.types.iter().enumerate() {
            let params = each(ty.params().iter().enumerate(), |s, (j, v)| {
                let _ = write!(s, ", {} l{j}", c_type(*v));
            });
            let args = each(0..ty.params().len(), |s, j| {
                let _ = write!(s, ", l{j}");
            });
            let ret = self.ret_type(p, t as u32);
            let _ = writeln!(
                out,
                "BS_FN {ret} {p}_ci{t}(bs_inst *I, uint32_t idx{params}) {{\n  switch (idx) {{"
            );
            for (slot, f) in self.table.iter().enumerate() {
                if let Some(f) = f
                    && self.funcs[*f as usize] as usize == t
                {
                    let _ = writeln!(out, "  case {slot}: return {p}_f{f}(I{args});");
                }
            }
            let _ = writeln!(out, "  default: bs_trap(I, BS_INDIRECT);\n  }}\n}}");
        }
    }

    /// `ret name(bs_inst *I, params)`.
    pub(crate) fn signature(&self, p: &str, f: u32) -> String {
        let ty = self.func_type(f);
        let params = each(ty.params().iter().enumerate(), |s, (j, t)| {
            let _ = write!(s, ", {} l{j}", c_type(*t));
        });
        format!(
            "{} {p}_f{f}(bs_inst *I{params})",
            self.ret_type(p, self.funcs[f as usize])
        )
    }

    pub(crate) fn ret_type(&self, p: &str, ty: u32) -> String {
        let results = self.types[ty as usize].results();
        match results.len() {
            0 => "void".into(),
            1 => c_type(results[0]).into(),
            _ => format!("{p}_t{ty}"),
        }
    }

    // An import runs on the host: its arguments go over as raw bits.
    fn import_thunk(&self, p: &str, f: u32, out: &mut String) {
        let ty = self.func_type(f);
        let (np, nr) = (ty.params().len(), ty.results().len());
        let _ = writeln!(out, "BS_FN {} {{", self.signature(p, f));
        let _ = writeln!(out, "  uint64_t a[{}], r[{}];", np.max(1), nr.max(1));
        for (j, t) in ty.params().iter().enumerate() {
            let _ = writeln!(out, "  a[{j}] = {};", to_bits(*t, &format!("l{j}")));
        }
        let _ = writeln!(
            out,
            "  int32_t c = I->host->import(I->host_ctx, {f}u, a, r);\n  if (c) bs_trap(I, c);"
        );
        match nr {
            0 => {}
            1 => {
                let _ = writeln!(out, "  return {};", from_bits(ty.results()[0], "r[0]"));
            }
            _ => {
                let _ = writeln!(out, "  {} v;", self.ret_type(p, self.funcs[f as usize]));
                for (j, t) in ty.results().iter().enumerate() {
                    let _ = writeln!(out, "  v.r{j} = {};", from_bits(*t, &format!("r[{j}]")));
                }
                out.push_str("  return v;\n");
            }
        }
        out.push_str("}\n");
    }

    fn exported_funcs(&self) -> Vec<(&str, u32)> {
        self.exports
            .iter()
            .filter(|(_, k, _)| *k == ExternalKind::Func)
            .map(|(n, _, i)| (n.as_str(), *i))
            .collect()
    }

    // The host calls an export by its place in the export list.
    fn dispatch(&self, p: &str, out: &mut String) {
        let _ = writeln!(
            out,
            "static void {p}_dispatch(bs_inst *I, uint32_t f, const uint64_t *a, uint64_t *r) {{\n  \
             (void)a;\n  (void)r;\n  switch (f) {{"
        );
        for (e, (_, f)) in self.exported_funcs().iter().enumerate() {
            let ty = self.func_type(*f);
            let args = each(ty.params().iter().enumerate(), |s, (j, t)| {
                let _ = write!(s, ", {}", from_bits(*t, &format!("a[{j}]")));
            });
            let call = format!("{p}_f{f}(I{args})");
            let _ = write!(out, "  case {e}: {{ ");
            match ty.results().len() {
                0 => {
                    let _ = write!(out, "{call};");
                }
                1 => {
                    let _ = write!(out, "r[0] = {};", to_bits(ty.results()[0], &call));
                }
                _ => {
                    let _ = write!(
                        out,
                        "{} v = {call};",
                        self.ret_type(p, self.funcs[*f as usize])
                    );
                    for (j, t) in ty.results().iter().enumerate() {
                        let _ = write!(out, " r[{j}] = {};", to_bits(*t, &format!("v.r{j}")));
                    }
                }
            }
            out.push_str(" break; }\n");
        }
        out.push_str("  }\n}\n");
        let _ = writeln!(
            out,
            "static int32_t {p}_call(bs_inst *I, uint32_t f, const uint64_t *a, uint64_t *r) {{\n  \
             return bs_run(I, {p}_dispatch, f, a, r);\n}}"
        );
    }

    fn descriptor(&self, p: &str, out: &mut String) {
        let funcs = self.exported_funcs();
        let globals: Vec<(&str, u32)> = self
            .exports
            .iter()
            .filter(|(_, k, _)| *k == ExternalKind::Global)
            .map(|(n, _, i)| (n.as_str(), *i))
            .collect();
        let names = |list: &mut dyn Iterator<Item = String>| {
            let items: Vec<String> = list.map(|s| format!("\"{s}\"")).collect();
            if items.is_empty() {
                "0".into()
            } else {
                items.join(", ")
            }
        };
        let codes = |ts: &[ValType]| ts.iter().map(|t| code(*t)).collect::<String>();
        let fnames = names(&mut funcs.iter().map(|(n, _)| escape(n)));
        let fres = names(
            &mut funcs
                .iter()
                .map(|(_, f)| codes(self.func_type(*f).results())),
        );
        let gnames = names(&mut globals.iter().map(|(n, _)| escape(n)));
        let gidx: Vec<String> = globals.iter().map(|(_, i)| format!("{i}u")).collect();
        let gidx = if gidx.is_empty() {
            "0".into()
        } else {
            gidx.join(", ")
        };
        let imports = 0..self.imports as u32;
        let ipar = names(&mut imports.clone().map(|f| codes(self.func_type(f).params())));
        let ires = names(&mut imports.map(|f| codes(self.func_type(f).results())));
        let _ = writeln!(
            out,
            "static const char *const {p}_fnames[] = {{{fnames}}};\n\
             static const char *const {p}_fres[] = {{{fres}}};\n\
             static const char *const {p}_gnames[] = {{{gnames}}};\n\
             static const uint32_t {p}_gidx[] = {{{gidx}}};\n\
             static const char *const {p}_ipar[] = {{{ipar}}};\n\
             static const char *const {p}_ires[] = {{{ires}}};"
        );
        for (j, (_, bytes)) in self.data.iter().enumerate() {
            let list: Vec<String> = bytes.iter().map(u8::to_string).collect();
            let _ = writeln!(
                out,
                "static const uint8_t {p}_d{j}[] = {{{}}};",
                list.join(",")
            );
        }
        let (initial, max) = self.memory;
        let max_bytes = max.map_or(1u64 << 32, |m| m << 16);
        let _ = writeln!(
            out,
            "static bs_inst *{p}_new(const bs_host *host, void *ctx, uint64_t max_bytes) {{\n  \
             uint64_t max = {max_bytes}ull < max_bytes ? {max_bytes}ull : max_bytes;\n  \
             bs_inst *I = bs_new(host, ctx, {}u, {}ull, max);\n  if (!I) return 0;",
            self.globals.len().max(1),
            initial << 16
        );
        for (j, (at, bytes)) in self.data.iter().enumerate() {
            if !bytes.is_empty() {
                let _ = writeln!(
                    out,
                    "  if ({at}ull + {len}ull > I->mem_len) {{ bs_destroy(I); return 0; }}\n  \
                     memcpy(I->mem + {at}, {p}_d{j}, {len});",
                    len = bytes.len()
                );
            }
        }
        for (j, (_, init)) in self.globals.iter().enumerate() {
            let _ = writeln!(out, "  I->g[{j}] = {init}ull;");
        }
        out.push_str("  return I;\n}\n");
        let _ = writeln!(
            out,
            "static const bs_module {p}_module = {{BS_ABI, {}u, {p}_fnames, {p}_fres, {}u, {p}_gnames, {p}_gidx, \
             {}u, {p}_ipar, {p}_ires, {p}_new, {p}_call, bs_destroy, bs_memory, bs_globals}};",
            funcs.len(),
            globals.len(),
            self.imports
        );
    }
}

/// A constant expression's value, as the raw bits a global holds.
fn constant(expr: &wasmparser::ConstExpr<'_>) -> Result<u64, String> {
    let mut ops = expr.get_operators_reader();
    let value = match ops.read().map_err(err)? {
        Operator::I32Const { value } => u64::from(value.cast_unsigned()),
        Operator::I64Const { value } => value.cast_unsigned(),
        Operator::F64Const { value } => value.bits(),
        other => return Err(format!("a constant {other:?}")),
    };
    Ok(value)
}

pub(crate) fn c_type(t: ValType) -> &'static str {
    match t {
        ValType::I32 => "uint32_t",
        ValType::I64 => "uint64_t",
        ValType::F64 => "double",
        _ => "float",
    }
}

/// The letter the host reads a value's kind from.
pub(crate) fn code(t: ValType) -> char {
    match t {
        ValType::I32 => 'i',
        ValType::I64 => 'l',
        ValType::F64 => 'd',
        _ => 'f',
    }
}

pub(crate) fn to_bits(t: ValType, v: &str) -> String {
    match t {
        ValType::F64 => format!("bs_bits({v})"),
        _ => format!("(uint64_t)({v})"),
    }
}

pub(crate) fn from_bits(t: ValType, v: &str) -> String {
    match t {
        ValType::I32 => format!("(uint32_t)({v})"),
        ValType::F64 => format!("bs_f64({v})"),
        _ => v.to_string(),
    }
}

/// `f` run over each item into one string.
pub(crate) fn each<T>(
    items: impl IntoIterator<Item = T>,
    mut f: impl FnMut(&mut String, T),
) -> String {
    let mut s = String::new();
    for item in items {
        f(&mut s, item);
    }
    s
}

fn escape(name: &str) -> String {
    name.replace('\\', "\\\\").replace('"', "\\\"")
}
