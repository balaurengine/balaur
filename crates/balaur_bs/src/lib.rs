//! Balaur Script (`.bs`): a small statically typed scripting language for
//! Balaur, compiled straight to WebAssembly so the same module runs on wasmtime, wasmi and a browser.
//!
//! Values: `int`, `float`, `bool`, `Node` and structs are plain values;
//! `str`, `[T]` and a `class`'s objects are reference counted. `docs/PLAN-balaur-script.md` is the
//! design and what is left.

// Wasm addresses, lengths and counts are i32 by definition; a module past 2 GiB fails to load first.
#![allow(clippy::cast_possible_wrap)]

pub mod analysis;
mod anys;
mod ast;
#[cfg(feature = "c")]
pub mod c;
mod codegen;
mod modules;
mod parse;
pub mod runtime;
mod strings;
pub mod words;

pub use ast::{Pos, Ty};
pub use modules::{Find, Modules};

/// What a module is known by outside the runtime: a compiled-module cache's
/// file name, and the native build of it an export made. Two FNV-1a passes.
#[must_use]
pub fn module_hash(wasm: &[u8]) -> String {
    format!(
        "{:016x}{:016x}",
        fnv(wasm, 0xcbf2_9ce4_8422_2325),
        fnv(wasm, 0x6c62_272e_07bb_0142)
    )
}

pub(crate) fn fnv(bytes: &[u8], seed: u64) -> u64 {
    bytes.iter().fold(seed, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}
pub(crate) use codegen::show;

/// A compile error, at the place in the source that caused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub pos: Pos,
    pub msg: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.pos.line, self.pos.col, self.msg)
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Instrument every statement for the debugger: slower, never shipped.
    pub debug: bool,
    /// What the engine offers beyond the script's own `extern fn`s.
    pub host: HostApi,
    /// Count loop passes and calls in the module itself, against the fuel a
    /// call is given: for an engine with no fuel of its own, a browser's.
    pub meter: bool,
    /// The most memory the module may grow to, declared in it so an engine
    /// with no limiter of its own (a browser's) keeps to it.
    pub memory_max: Option<usize>,
    /// Import a few generic entry points the engine exports instead of one
    /// function per import, so a browser wires the script straight to the
    /// engine's WebAssembly and a call never passes through JavaScript.
    pub direct_imports: bool,
    /// Where `use name;` finds `name`'s file, from the file that uses it.
    pub modules: Option<Modules>,
    /// This file's own key, which `modules` resolves a `use` from.
    pub path: String,
}

/// The engine's side, as the compiler types it: functions a script calls as
/// `module::name(..)` or `node.name(..)`, and components it reads and writes
/// as `node.component.property`.
#[derive(Clone, Debug, Default)]
pub struct HostApi {
    /// Keyed `module::name`, or `node.name` for an operation on a node, which
    /// takes the node first.
    pub functions: Vec<HostSignature>,
    pub components: Vec<(String, Vec<(String, Ty)>)>,
    /// Keyed `module::NAME`, written into the module where a script names it.
    pub constants: Vec<(String, Literal)>,
    /// Records the engine hands back, keyed `module::Name`: a script reads
    /// and builds them as it does its own structs.
    pub structs: Vec<(String, Vec<(String, Ty)>)>,
}

#[derive(Clone, Debug)]
pub struct HostSignature {
    pub key: String,
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// Awaited, as an `extern async fn` is: the call hands back a token, and
    /// the host resolves it with [`runtime::Runtime::wake`].
    pub is_async: bool,
}

/// A type as a script writes it: `[str]`, `ui::Rect?`.
impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&show(self))
    }
}

impl HostSignature {
    /// How many arguments a call must pass: trailing `T?` and `any`
    /// parameters may be left out, and are nil.
    pub fn least(&self) -> usize {
        least(&self.params)
    }

    /// `(str, int?) -> bool`, as a script reads it.
    pub fn text(&self) -> String {
        let params: Vec<String> = self.params.iter().map(show).collect();
        match &self.ret {
            Ty::Void => format!("({})", params.join(", ")),
            r => format!("({}) -> {}", params.join(", "), show(r)),
        }
    }
}

pub(crate) fn least(params: &[Ty]) -> usize {
    let optional = params
        .iter()
        .rev()
        .take_while(|t| matches!(t, Ty::Optional(_) | Ty::Any))
        .count();
    params.len() - optional
}

impl HostApi {
    pub fn function(&self, key: &str) -> Option<&HostSignature> {
        self.functions.iter().find(|f| f.key == key)
    }
    pub fn record(&self, key: &str) -> Option<&[(String, Ty)]> {
        self.structs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, f)| f.as_slice())
    }
    pub fn constant(&self, key: &str) -> Option<&Literal> {
        self.constants
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
    pub fn property(&self, component: &str, property: &str) -> Option<&Ty> {
        let (_, props) = self.components.iter().find(|(c, _)| c == component)?;
        props.iter().find(|(p, _)| p == property).map(|(_, t)| t)
    }
}

/// A wasm value type, for an engine linking the module's imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    I32,
    I64,
    F64,
}

/// One function the module imports, in the module's own order.
#[derive(Clone, Debug)]
pub struct Import {
    pub name: String,
    pub params: Vec<Slot>,
    pub results: Vec<Slot>,
}

/// A field's default, as written.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

#[derive(Clone, Debug)]
pub struct Compiled {
    pub wasm: Vec<u8>,
    pub meta: Meta,
}

/// A struct's declared defaults: field, whether it is `pub`, the literal.
pub type FieldDefaults = Vec<(String, bool, Literal)>;

#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub structs: Vec<(String, Vec<(String, Ty)>)>,
    /// The structs declared `class`, each held as a pointer to its object.
    pub classes: Vec<String>,
    pub fns: Vec<FnInfo>,
    pub externs: Vec<ExternInfo>,
    pub probes: Vec<Probe>,
    pub func_names: Vec<String>,
    pub debug: bool,
    /// The module counts its own fuel in [`words::FUEL`].
    pub metered: bool,
    /// Its imports are [`words::DIRECT_CALL`] entry points; `imports` still
    /// lists what each import index means.
    pub direct: bool,
    /// The files `use` linked in, by key: a change to one rebuilds this.
    pub uses: Vec<String>,
    /// Every import in the module's order, so any engine can link them.
    pub imports: Vec<Import>,
    /// The engine functions the module calls: key, parameters, result.
    pub host_calls: Vec<HostSignature>,
    /// Per struct, each field that has a default: name, whether it is `pub`, value.
    pub defaults: Vec<(String, FieldDefaults)>,
}

impl Meta {
    pub fn is_class(&self, ty: &str) -> bool {
        self.classes.iter().any(|c| c == ty)
    }
    pub fn fields(&self, ty: &str) -> Option<&[(String, Ty)]> {
        self.structs
            .iter()
            .find(|(n, _)| n == ty)
            .map(|(_, f)| f.as_slice())
    }
    pub fn function(&self, key: &str) -> Option<&FnInfo> {
        self.fns.iter().find(|f| f.key == key)
    }
    pub fn defaults(&self, ty: &str) -> &[(String, bool, Literal)] {
        self.defaults
            .iter()
            .find(|(n, _)| n == ty)
            .map_or(&[], |(_, d)| d.as_slice())
    }
}

#[derive(Clone, Debug)]
pub struct FnInfo {
    /// `name`, or `Struct.name` for a method.
    pub key: String,
    pub owner: Option<String>,
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub is_async: bool,
    pub public: bool,
    /// Bytes the host allocates for one run of an async fn.
    pub frame_size: u32,
}

#[derive(Clone, Debug)]
pub struct ExternInfo {
    pub name: String,
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub is_async: bool,
}

/// A place the debugger can stop: a statement, and the variables in scope there.
#[derive(Clone, Debug)]
pub struct Probe {
    pub func: u32,
    pub line: u32,
    pub col: u32,
    pub vars: Vec<VarInfo>,
}

#[derive(Clone, Debug)]
pub struct VarInfo {
    pub name: String,
    pub ty: Ty,
    /// Where the variable's first slot sits in its function's debug frame.
    pub offset: u32,
}

/// Built-in value types the engine's vectors and colours arrive as.
const PRELUDE: &str = "pub struct Vec2 { x: float, y: float }
pub struct Vec3 { x: float, y: float, z: float }
pub struct Color { r: float, g: float, b: float, a: float }
";

// The engine's records as structs, but for one the file declares itself.
fn host_structs(host: &HostApi, prog: &ast::Program) -> Vec<(String, Vec<ast::Field>)> {
    host.structs
        .iter()
        .filter(|(n, _)| !prog.structs.iter().any(|(m, _)| m == n))
        .map(|(n, fields)| {
            let fields = fields
                .iter()
                .map(|(name, ty)| ast::Field {
                    name: name.clone(),
                    ty: ty.clone(),
                    public: true,
                    default: None,
                    pos: Pos::default(),
                })
                .collect();
            (n.clone(), fields)
        })
        .collect()
}

pub fn compile(src: &str, options: &Options) -> Result<Compiled, Error> {
    let (prog, uses) = front(src, options)?;
    let mut compiled = codegen::generate(&prog, options)?;
    compiled.meta.uses = uses;
    Ok(compiled)
}

/// Every error in `src`: the first that stops it being read at all, else one
/// for each function that does not compile. Empty when it compiles.
pub fn errors(src: &str, options: &Options) -> Vec<Error> {
    match front(src, options) {
        Ok((prog, _)) => codegen::errors(&prog, options),
        Err(e) => vec![e],
    }
}

// The file parsed, its modules linked in, the prelude and the engine's
// records beside its own structs; the files linked.
fn front(src: &str, options: &Options) -> Result<(ast::Program, Vec<String>), Error> {
    let mut prog = parse::parse(src)?;
    let uses = if prog.uses.is_empty() {
        vec![]
    } else {
        modules::link(&mut prog, &options.path, options.modules.as_ref())?
    };
    let prelude = parse::parse(PRELUDE).map_err(|e| Error {
        pos: e.pos,
        msg: format!("prelude: {}", e.msg),
    })?;
    let mut structs = prelude.structs;
    structs.extend(host_structs(&options.host, &prog));
    structs.append(&mut prog.structs);
    prog.structs = structs;
    Ok((prog, uses))
}
