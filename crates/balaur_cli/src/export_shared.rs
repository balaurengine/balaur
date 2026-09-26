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
pub(crate) const LISTEN_DOC: &str = "Have the node's `on_export_event(event)`, or the `on_event` method the options name, called as each export starts, finishes or fails.";

/// `export.preview`, the same on a desktop and in a tab.
pub(crate) const PREVIEW_DOC: &str = "What one file becomes in a target's pack, as the Import tab shows it: `{ source, before, after, width, height, drawn_width, drawn_height, gpu_bytes }`, where `source` is the file or the variant that ships and `drawn_*` is zero unless a smaller copy does. Nil when the file does not ship there. An empty target is this machine's own.";

/// One file's row for one target, or nil when it does not ship there.
pub(crate) fn preview(project: &std::path::Path, path: &str, target: &str) -> Value {
    let target = (!target.is_empty()).then_some(target);
    let shown = match balaur_export::preview::preview(project, path, target) {
        Ok(shown) => shown,
        Err(why) => {
            tracing::debug!("export.preview {path}: {why:#}");
            return Value::Nil;
        }
    };
    let count = |n: u64| Value::Int(i64::try_from(n).unwrap_or(i64::MAX));
    let (drawn_width, drawn_height) = shown.drawn.unwrap_or((0, 0));
    Value::Map(vec![
        ("source".into(), Value::Str(shown.source)),
        ("before".into(), count(shown.before as u64)),
        ("after".into(), count(shown.after as u64)),
        ("width".into(), count(u64::from(shown.width))),
        ("height".into(), count(u64::from(shown.height))),
        ("drawn_width".into(), count(u64::from(drawn_width))),
        ("drawn_height".into(), count(u64::from(drawn_height))),
        ("gpu_bytes".into(), count(shown.gpu_bytes)),
    ])
}
