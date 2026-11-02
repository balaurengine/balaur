// A unit as C: one file of a script built as units, sharing the script's
// memory, globals and `fn` table, its static data and table slots placed
// from the bases the runtime gives it when it links the unit.

use super::{Module, c_type, each, err};
use crate::words;
use std::fmt::Write as _;
use wasmparser::{FuncType, Operator};

/// Where a global lives.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Place {
    /// One of the globals every unit of a script shares, by its place in
    /// [`words::UNIT_GLOBALS`]: `I->g[k]`.
    Shared(usize),
    /// The unit's memory, table or link base: `I->unit->base[k]`.
    Base(usize),
    /// A whole module's own, by its place among them: `I->g[k]`.
    Own(usize),
}

impl Place {
    /// An imported global, by the name a unit imports it under.
    pub(crate) fn imported(name: &str) -> Result<Self, String> {
        if let Some(k) = words::UNIT_GLOBALS.iter().position(|g| *g == name) {
            return Ok(Place::Shared(k));
        }
        [words::MEMORY_BASE, words::TABLE_BASE, words::LINK_BASE]
            .iter()
            .position(|g| *g == name)
            .map(Place::Base)
            .ok_or_else(|| format!("an import of the global `{name}`"))
    }

    /// The C lvalue that holds it.
    pub(crate) fn lvalue(self) -> String {
        match self {
            Place::Shared(k) | Place::Own(k) => format!("I->g[{k}]"),
            Place::Base(k) => format!("I->unit->base[{k}]"),
        }
    }
}

/// Where a data or element segment starts: a fixed place, or a unit's base
/// (its memory base for data, its table base for `fn` values).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Offset {
    At(u64),
    Base,
}

impl Offset {
    pub(crate) fn of(expr: &wasmparser::ConstExpr<'_>) -> Result<Self, String> {
        let mut ops = expr.get_operators_reader();
        match ops.read().map_err(err)? {
            Operator::GlobalGet { .. } => Ok(Offset::Base),
            _ => super::constant(expr).map(Offset::At),
        }
    }
}

/// A signature's tag: what a call through a `fn` table slot checks the
/// slot's function against, the same in every unit that writes it.
pub(crate) fn type_tag(ty: &FuncType) -> u64 {
    let codes: String = ty
        .params()
        .iter()
        .map(|t| super::code(*t))
        .chain(std::iter::once('>'))
        .chain(ty.results().iter().map(|t| super::code(*t)))
        .collect();
    // FNV-1a.
    codes.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A unit's `call_indirect` of type `t` is `{p}_ci{t}`: the slot's function,
/// when its signature is `t`'s, run as the unit the slot names.
pub(crate) fn indirect(m: &Module<'_>, p: &str, out: &mut String) {
    for (t, ty) in m.types.iter().enumerate() {
        let params = each(ty.params().iter().enumerate(), |s, (j, v)| {
            let _ = write!(s, ", {} l{j}", c_type(*v));
        });
        let types = each(ty.params().iter(), |s, v| {
            let _ = write!(s, ", {}", c_type(*v));
        });
        let args = each(0..ty.params().len(), |s, j| {
            let _ = write!(s, ", l{j}");
        });
        let ret = m.ret_type(p, t as u32);
        let tag = type_tag(ty);
        let call = format!("(({ret} (*)(bs_inst *{types}))s.fn)(I{args})");
        let (keep, give) = if ty.results().is_empty() {
            (format!("{call};"), String::new())
        } else {
            (format!("{ret} v = {call};"), "  return v;\n".to_string())
        };
        let _ = writeln!(
            out,
            "BS_FN {ret} {p}_ci{t}(bs_inst *I, uint32_t idx{params}) {{\n  \
             bs_slot s = bs_slot_at(I, idx, {tag}ull);\n  \
             bs_unit *was = I->unit;\n  I->unit = s.unit;\n  {keep}\n  I->unit = was;\n{give}}}"
        );
    }
}

/// A unit's exports as link slots take them, its stand-in for a whole
/// module's instance (a unit is never one), and `make`: its static data
/// copied to its memory base and its own `fn` values into the table from
/// its table base.
pub(crate) fn make(m: &Module<'_>, p: &str, out: &mut String) {
    let refs: Vec<String> = m
        .exported_funcs()
        .iter()
        .map(|(_, f)| format!("{{(bs_fn){p}_f{f}, {}ull}}", type_tag(m.func_type(*f))))
        .collect();
    let refs = if refs.is_empty() {
        "{0, 0}".to_string()
    } else {
        refs.join(", ")
    };
    let _ = writeln!(
        out,
        "static const bs_ref {p}_refs[] = {{{refs}}};\n\
         static bs_inst *{p}_new(const bs_host *host, void *ctx, uint64_t max_bytes) {{\n  \
         (void)host;\n  (void)ctx;\n  (void)max_bytes;\n  return 0;\n}}\n\
         static int32_t {p}_make(bs_inst *I, bs_unit *U) {{"
    );
    for (j, (at, bytes)) in m.data.iter().enumerate() {
        if bytes.is_empty() {
            continue;
        }
        let at = match at {
            Offset::At(at) => format!("{at}ull"),
            Offset::Base => "(uint64_t)U->base[0]".to_string(),
        };
        let _ = writeln!(
            out,
            "  if ({at} + {len}ull > I->mem_len) return BS_OUT_OF_BOUNDS;\n  \
             memcpy(I->mem + {at}, {p}_d{j}, {len});",
            len = bytes.len()
        );
    }
    if !m.unit_refs.is_empty() {
        let _ = writeln!(
            out,
            "  if ((uint64_t)U->base[1] + {}ull > I->table_len) return BS_INDIRECT;",
            m.unit_refs.len()
        );
    }
    for (k, f) in m.unit_refs.iter().enumerate() {
        let tag = type_tag(m.func_type(*f));
        let _ = writeln!(
            out,
            "  I->table[U->base[1] + {k}u] = (bs_slot){{(bs_fn){p}_f{f}, U, {tag}ull}};"
        );
    }
    out.push_str("  return 0;\n}\n");
}
