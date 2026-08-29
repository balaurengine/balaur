//! The scripting seam: traits only, no backend.
//!
//! Subsystems declare their bindings against [`Bindings`] and never name a
//! language. Backends (`balaur_script_luau`, `balaur_script_rune`) implement
//! [`ScriptLanguage`] and consume those declarations, so a second language
//! costs one new crate rather than an edit to every subsystem.

mod bindings;
mod language;
mod value;

pub use bindings::{Bindings, BindingsExt, BoundFn};
pub use language::{InstanceId, ReloadReport, ScriptId, ScriptLanguage};
pub use value::{expect_arity, FromArg, FromArgs, IntoValue, NodeId, Value};
