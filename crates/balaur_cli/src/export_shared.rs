//! What `export.*` is on any machine: the event a step reports.
//!
//! Which targets exist and what building one means are the halves that differ
//! between a desktop install and a browser tab, so they stay in
//! [`crate::export_api`] and `crate::web_export`. What a script sees of an
//! export in flight is [`crate::jobs`], which every verb of this shape shares;
//! what is here is the event itself.

use balaur_script::Value;
use serde::{Deserialize, Serialize};

/// One step of an export, crossing from wherever the work happens back to a
/// tick.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum ExportEvent {
    Started { target: String },
    Done { target: String, path: String },
    Failed { target: String, message: String },
}

impl ExportEvent {
    fn kind(&self) -> &'static str {
        match self {
            Self::Started { .. } => "started",
            Self::Done { .. } => "done",
            Self::Failed { .. } => "failed",
        }
    }

    fn target(&self) -> &str {
        match self {
            Self::Started { target } | Self::Done { target, .. } | Self::Failed { target, .. } => {
                target
            }
        }
    }
}

impl crate::jobs::Reported for ExportEvent {
    /// The table a handler is called with. One shape for every kind, so a
    /// script reads `kind` and then the field that kind carries.
    fn value(&self) -> Value {
        let mut pairs = vec![
            ("kind".into(), Value::Str(self.kind().into())),
            ("target".into(), Value::Str(self.target().into())),
        ];
        match self {
            Self::Started { .. } => {}
            Self::Done { path, .. } => pairs.push(("path".into(), Value::Str(path.clone()))),
            Self::Failed { message, .. } => {
                pairs.push(("message".into(), Value::Str(message.clone())));
            }
        }
        Value::Map(pairs)
    }
}

/// The project being edited, the channel exports report on, and who listens.
pub(crate) type ExportCore = crate::jobs::Reporting<ExportEvent>;

/// What `listen` is documented as, so both modules describe it the same way.
pub(crate) const LISTEN_DOC: &str = "Have the node's `on_export(event)`, or the `on_event` method the options name, called as each export starts, finishes or fails.";
