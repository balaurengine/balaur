//! What an editor asks of a `.bs` file: its error, what may be typed at the
//! cursor, where a name is declared, and what a name is.

mod edit;
mod scan;
mod scope;
mod types;

use crate::ast::{Pos, Program, Ty};
use crate::{Error, HostApi, Modules, Options};
pub use edit::Symbol;
use scan::Context;
use types::{NATIVES, Recv, fn_signature, host_signature};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Field,
    Method,
    Function,
    Variable,
    Struct,
    Module,
    Property,
    Keyword,
    Constant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Completion {
    pub label: String,
    pub kind: Kind,
    /// A signature or a type.
    pub detail: String,
}

const KEYWORDS: [&str; 23] = [
    "let", "if", "else", "while", "for", "in", "loop", "return", "break", "continue", "fn", "pub",
    "struct", "class", "impl", "async", "await", "true", "false", "self", "const", "extern", "as",
];

/// The value types every script holds, each with its fields: `Vec2` and the
/// rest, as the engine's vectors and colours arrive.
#[must_use]
pub fn value_types() -> &'static [(&'static str, &'static [&'static str])] {
    &types::PRELUDE
}

/// The functions every script calls without a module, with their signatures.
#[must_use]
pub fn builtins() -> &'static [(&'static str, &'static str)] {
    &NATIVES
}

/// One file, read against the engine's API and the modules it `use`s.
pub struct Analysis {
    src: String,
    host: HostApi,
    error: Option<Error>,
    modules: Option<Modules>,
    path: String,
}

pub fn analyze(src: &str, host: &HostApi) -> Analysis {
    analyze_in(src, host, None, "")
}

/// [`analyze`] for the file at `path`, whose `use`s `modules` finds.
pub fn analyze_in(src: &str, host: &HostApi, modules: Option<Modules>, path: &str) -> Analysis {
    let options = Options {
        debug: false,
        host: host.clone(),
        modules: modules.clone(),
        path: path.to_string(),
        ..Options::default()
    };
    Analysis {
        src: src.to_string(),
        host: host.clone(),
        error: crate::compile(src, &options).err(),
        modules,
        path: path.to_string(),
    }
}

impl Analysis {
    /// What the compiler stopped at, if it did.
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }

    // The file as it parses with what is being typed at `at` cut out: the
    // half-written member goes, and the line is ended, until one parses.
    fn program(&self, at: usize, cut: usize) -> Option<Program> {
        self.patched(at, cut, &[";", "", "0;"])
            .map(|p| self.linked(p))
    }

    // The file with what its `use`s bring in, as far as they can be found.
    fn linked(&self, mut prog: Program) -> Program {
        let _ = crate::modules::link(&mut prog, &self.path, self.modules.as_ref());
        prog
    }

    /// The file as it is, or with the `cut` bytes before `at` replaced by
    /// the first of `patches` that makes it parse.
    fn patched(&self, at: usize, cut: usize, patches: &[&str]) -> Option<Program> {
        let (head, tail) = (&self.src[..at - cut], &self.src[at..]);
        crate::parse::parse(&self.src).ok().or_else(|| {
            patches
                .iter()
                .find_map(|p| crate::parse::parse(&format!("{head}{p}{tail}")).ok())
        })
    }

    /// What may be typed at 1-based `line`, `col`, filtered by what is there.
    pub fn complete(&self, line: u32, col: u32) -> Vec<Completion> {
        let at = scan::offset(&self.src, line, col);
        let ctx = scan::context(&self.src, at);
        let (cut, partial) = match &ctx {
            Context::Member(_, p) | Context::Path(_, p) => (
                p.len() + 1 + usize::from(matches!(ctx, Context::Path(..))),
                p.clone(),
            ),
            Context::Name(p) => (p.len(), p.clone()),
        };
        let prog = self.program(at, cut).unwrap_or_default();
        let scope = scope::at(&prog, &self.host, Pos { line, col });
        let mut out = match &ctx {
            Context::Member(recv, _) => crate::parse::parse_expr(recv)
                .ok()
                .and_then(|e| scope.resolve(&e))
                .map(|r| scope.members(&r))
                .unwrap_or_default(),
            Context::Path(module, _) => self.path(&prog, module),
            Context::Name(_) => names(&scope, &self.host),
        };
        out.retain(|c| c.label.starts_with(partial.as_str()));
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out.dedup_by(|a, b| a.label == b.label);
        out
    }

    fn path(&self, prog: &Program, module: &str) -> Vec<Completion> {
        let prefix = format!("{module}::");
        let mut out: Vec<Completion> = self
            .host
            .functions
            .iter()
            .filter_map(|f| {
                let name = f.key.strip_prefix(&prefix)?;
                Some(Completion {
                    label: name.to_string(),
                    kind: Kind::Function,
                    detail: host_signature(name, f, false),
                })
            })
            .collect();
        for (key, fields) in &self.host.structs {
            if let Some(name) = key.strip_prefix(&prefix) {
                out.push(Completion {
                    label: name.to_string(),
                    kind: Kind::Struct,
                    detail: record_detail(key, fields),
                });
            }
        }
        for (key, v) in &self.host.constants {
            if let Some(name) = key.strip_prefix(&prefix) {
                out.push(Completion {
                    label: name.to_string(),
                    kind: Kind::Constant,
                    detail: constant_detail(key, v),
                });
            }
        }
        // A used module's `pub` functions and its structs.
        for f in prog.fns.iter().filter(|f| f.owner.is_none()) {
            if let Some(name) = f.name.strip_prefix(&prefix).filter(|n| !n.starts_with('#')) {
                out.push(Completion {
                    label: name.to_string(),
                    kind: Kind::Function,
                    detail: fn_signature(f),
                });
            }
        }
        for (s, _) in &prog.structs {
            if let Some(name) = s.strip_prefix(&prefix) {
                out.push(Completion {
                    label: name.to_string(),
                    kind: Kind::Struct,
                    detail: format!("struct {s}"),
                });
            }
        }
        out
    }

    // The receiver and name of the word at `at`, with the scope there.
    fn word(&self, line: u32, col: u32) -> Option<(Program, String, usize)> {
        let at = scan::offset(&self.src, line, col);
        let (word, start) = scan::word_at(&self.src, at)?;
        let prog = self.linked(crate::parse::parse(&self.src).ok()?);
        Some((prog, word, start))
    }

    /// Where the name at `line`, `col` is declared in this file.
    pub fn definition(&self, line: u32, col: u32) -> Option<Pos> {
        let (prog, word, start) = self.word(line, col)?;
        let scope = scope::at(&prog, &self.host, Pos { line, col });
        let head = &self.src[..start];
        if let Some(recv) = head.strip_suffix('.') {
            let r = scope.resolve(&crate::parse::parse_expr(scan::receiver(recv)).ok()?)?;
            let Recv::Ty(crate::Ty::Struct(s)) = r else {
                return None;
            };
            return scope
                .field(&s, &word)
                .and_then(|(_, p)| p)
                .or_else(|| scope.method(&s, &word).map(|f| f.pos));
        }
        if head.ends_with("::") {
            return None;
        }
        if let Some((_, _, p)) = scope.vars.iter().rev().find(|(n, _, _)| *n == word) {
            return Some(*p);
        }
        prog.fns
            .iter()
            .find(|f| f.owner.is_none() && f.name == word)
            .map(|f| f.pos)
            .or_else(|| prog.externs.iter().find(|x| x.name == word).map(|x| x.pos))
            .or_else(|| prog.decls.iter().find(|(n, _)| *n == word).map(|(_, p)| *p))
    }

    /// What the name at `line`, `col` is: a type or a signature.
    pub fn hover(&self, line: u32, col: u32) -> Option<String> {
        let (prog, word, start) = self.word(line, col)?;
        let scope = scope::at(&prog, &self.host, Pos { line, col });
        let head = &self.src[..start];
        if let Some(recv) = head.strip_suffix('.') {
            let r = scope.resolve(&crate::parse::parse_expr(scan::receiver(recv)).ok()?)?;
            return scope
                .members(&r)
                .into_iter()
                .find(|c| c.label == word)
                .map(|c| format!("{word}: {}", c.detail));
        }
        if let Some(module) = head.strip_suffix("::") {
            let module = scan::receiver(module);
            let key = format!("{module}::{word}");
            if let Some(f) = prog.fns.iter().find(|f| f.owner.is_none() && f.name == key) {
                return Some(fn_signature(f));
            }
            if let Some(v) = self.host.constant(&key) {
                return Some(constant_detail(&key, v));
            }
            if let Some(fields) = self.host.record(&key) {
                return Some(record_detail(&key, fields));
            }
            let f = self.host.function(&key)?;
            return Some(host_signature(&key, f, false));
        }
        if let Some((n, t, _)) = scope.vars.iter().rev().find(|(n, _, _)| *n == word) {
            return Some(format!("{n}: {}", crate::codegen::show(t)));
        }
        names(&scope, &self.host)
            .into_iter()
            .find(|c| c.label == word && !c.detail.is_empty())
            .map(|c| c.detail)
    }
}

// `struct ui::Rect { x: float, y: float }`.
fn record_detail(key: &str, fields: &[(String, Ty)]) -> String {
    let fields: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("{n}: {}", crate::codegen::show(t)))
        .collect();
    format!("struct {key} {{ {} }}", fields.join(", "))
}

// `input::KEY_SPACE: str = "space"`.
fn constant_detail(key: &str, v: &crate::Literal) -> String {
    use crate::Literal as L;
    match v {
        L::Int(x) => format!("{key}: int = {x}"),
        L::Float(x) => format!("{key}: float = {x:?}"),
        L::Bool(b) => format!("{key}: bool = {b}"),
        L::Str(t) => format!("{key}: str = {t:?}"),
    }
}

// Everything a bare name may be: locals, the file's functions and types,
// the built-ins, the engine's modules and the keywords.
fn names(scope: &types::Scope<'_>, host: &HostApi) -> Vec<Completion> {
    let mut out = vec![];
    let mut add = |label: &str, kind: Kind, detail: String| {
        out.push(Completion {
            label: label.to_string(),
            kind,
            detail,
        });
    };
    for (n, t, _) in scope.vars.iter().rev() {
        add(n, Kind::Variable, crate::codegen::show(t));
    }
    // A module's own functions are reached through its name.
    for f in scope
        .prog
        .fns
        .iter()
        .filter(|f| f.owner.is_none() && !f.name.contains("::"))
    {
        add(&f.name, Kind::Function, fn_signature(f));
    }
    for m in &scope.prog.linked {
        add(m, Kind::Module, format!("module {m}"));
    }
    for x in &scope.prog.externs {
        let params: Vec<crate::Ty> = x.params.iter().map(|p| p.ty.clone()).collect();
        add(
            &x.name,
            Kind::Function,
            types::signature(&x.name, &params, &x.ret),
        );
    }
    for (s, _) in scope.prog.structs.iter().filter(|(s, _)| !s.contains("::")) {
        add(s, Kind::Struct, format!("struct {s}"));
    }
    for s in ["Vec2", "Vec3", "Color"] {
        add(s, Kind::Struct, format!("struct {s}"));
    }
    for (n, d) in NATIVES {
        add(n, Kind::Function, d.into());
    }
    let mut modules: Vec<&str> = host
        .functions
        .iter()
        .filter_map(|f| f.key.split_once("::").map(|(m, _)| m))
        .collect();
    modules.sort_unstable();
    modules.dedup();
    for m in modules {
        add(m, Kind::Module, format!("module {m}"));
    }
    for k in KEYWORDS {
        add(k, Kind::Keyword, String::new());
    }
    out
}
