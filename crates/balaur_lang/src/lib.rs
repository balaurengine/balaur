//! A small statically typed scripting language for Balaur, compiled straight
//! to WebAssembly so the same module runs on wasmtime, wasmi and a browser.
//!
//! Values: `int`, `float`, `bool`, `Node` and structs are plain values;
//! `str` and `[T]` are reference counted. `docs/PLAN-balaur-lang.md` is the
//! design and what is left.

// Wasm addresses, lengths and counts are i32 by definition; a module past 2 GiB fails to load first.
#![allow(clippy::cast_possible_wrap)]

mod anys;
mod ast;
mod codegen;
mod parse;
pub mod runtime;
mod strings;
pub mod words;

pub use ast::{Pos, Ty};
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

impl HostApi {
    pub fn function(&self, key: &str) -> Option<&HostSignature> {
        self.functions.iter().find(|f| f.key == key)
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
    pub fns: Vec<FnInfo>,
    pub externs: Vec<ExternInfo>,
    pub probes: Vec<Probe>,
    pub func_names: Vec<String>,
    pub debug: bool,
    /// Every import in the module's order, so any engine can link them.
    pub imports: Vec<Import>,
    /// The engine functions the module calls: key, parameters, result.
    pub host_calls: Vec<HostSignature>,
    /// Per struct, each field that has a default: name, whether it is `pub`, value.
    pub defaults: Vec<(String, FieldDefaults)>,
}

impl Meta {
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

pub fn compile(src: &str, options: &Options) -> Result<Compiled, Error> {
    let mut prog = parse::parse(src)?;
    let prelude = parse::parse(PRELUDE).map_err(|e| Error {
        pos: e.pos,
        msg: format!("prelude: {}", e.msg),
    })?;
    let mut structs = prelude.structs;
    structs.append(&mut prog.structs);
    prog.structs = structs;
    codegen::generate(&prog, options)
}
