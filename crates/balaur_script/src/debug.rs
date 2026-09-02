//! What a debugger sees of a paused script, without naming a language.

use crate::value::{NodeId, Value};

/// How a paused script leaves its pause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepMode {
    /// Run until the next breakpoint.
    Continue,
    /// Run to the next line of the current function, skipping over calls.
    Over,
    /// Run to the next line, entering any call on the way.
    Into,
    /// Run until the current function returns to its caller.
    Out,
}

impl StepMode {
    /// The script-facing name, which is also what `parse` accepts.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Over => "over",
            Self::Into => "into",
            Self::Out => "out",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Continue, Self::Over, Self::Into, Self::Out]
            .into_iter()
            .find(|mode| mode.name() == name)
    }
}

/// What stopped the script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    Breakpoint,
    Step,
}

impl PauseReason {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Breakpoint => "breakpoint",
            Self::Step => "step",
        }
    }
}

/// One call frame of a paused script.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The function's name, or `?` when the VM has none for it.
    pub function: String,
    /// The script the frame is executing, as the host keys it.
    pub path: String,
    pub line: usize,
    /// Named locals as plain values; functions and foreign userdata are
    /// skipped, since neither can be shown.
    pub locals: Vec<(String, Value)>,
}

/// Where a script is stopped. `frames` is innermost first, so `frames[0]`
/// is the line the editor highlights.
#[derive(Clone, Debug, PartialEq)]
pub struct Pause {
    pub node: NodeId,
    pub path: String,
    pub line: usize,
    pub reason: PauseReason,
    pub frames: Vec<Frame>,
}

impl Pause {
    /// The pause as a script sees it: a map the editor can read without a
    /// language-specific type behind it.
    pub fn to_value(&self) -> Value {
        let frames = self
            .frames
            .iter()
            .map(|f| {
                Value::Map(vec![
                    ("function".into(), Value::Str(f.function.clone())),
                    ("path".into(), Value::Str(f.path.clone())),
                    (
                        "line".into(),
                        Value::Int(i64::try_from(f.line).unwrap_or(i64::MAX)),
                    ),
                    ("locals".into(), Value::Map(f.locals.clone())),
                ])
            })
            .collect();
        Value::Map(vec![
            ("node".into(), Value::Node(self.node.0)),
            ("path".into(), Value::Str(self.path.clone())),
            (
                "line".into(),
                Value::Int(i64::try_from(self.line).unwrap_or(i64::MAX)),
            ),
            ("reason".into(), Value::Str(self.reason.name().into())),
            ("frames".into(), Value::List(frames)),
        ])
    }
}
