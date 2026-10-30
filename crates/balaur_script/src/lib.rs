//! The scripting seam: traits only, no backend.
//!
//! Subsystems declare their bindings against [`Bindings`] and never name a
//! language. A backend (`balaur_script_bs`) implements
//! [`ScriptHost`] and consume those declarations, so a second language
//! costs one new crate rather than an edit to every subsystem.

mod bindings;
mod debug;
mod language;
mod tools;
mod value;

pub use bindings::CallbackHost;
pub use bindings::{Bindings, BindingsExt, BoundFn, FnDoc, NoBindings, RecordDoc, TypedFn};
pub use debug::{Frame, Pause, PauseReason, StepMode};
pub use language::{ScriptCompiler, ScriptHost};
pub use tools::{Diagnostic, Item, LanguageTools, Location, Symbol, tool_answer};
pub use value::{
    Args, CallbackId, FromArg, FromArgs, IntoValue, NodeId, Value, Values, expect_arity,
};
