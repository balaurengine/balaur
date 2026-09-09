//! What `export.*` is on any machine: the event a step reports, the state
//! every listener is kept in, and the pump that hands one to the other.
//!
//! Which targets exist and what building one means are the halves that differ
//! between a desktop install and a browser tab, so they stay in
//! [`crate::export_api`] and `crate::web_export`. Everything a script sees of
//! an export *in flight* is the same on both, and lives here once.

use std::path::PathBuf;

use anyhow::anyhow;
use balaur::Engine;
use balaur::replay::ExternalIo;
use balaur_core::handler::{Handler, handler_of};
use balaur_script::{Bindings, BindingsExt, Value};
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

    /// The table a handler is called with. One shape for every kind, so a
    /// script reads `kind` and then the field that kind carries.
    pub(crate) fn value(&self) -> Value {
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
/// Each plugin's own state holds one of these and adds what its machine needs.
pub(crate) struct ExportCore {
    pub(crate) io: ExternalIo<ExportEvent>,
    pub(crate) listeners: Vec<Handler>,
    pub(crate) project: PathBuf,
}

impl ExportCore {
    pub(crate) fn new(project: PathBuf) -> Self {
        Self {
            io: ExternalIo::default(),
            listeners: Vec::new(),
            project,
        }
    }
}

/// Deliver what the export reported, to whoever asked to hear it.
///
/// Generic over the plugin's own state so each registers it with its own:
/// `reg.add_system(Stage::First, pump::<ExportState>)`.
pub(crate) fn pump<S: AsMut<ExportCore> + 'static>(eng: &Engine, _: f32) {
    let mut dispatches = Vec::new();
    {
        let state = eng.resource::<S>();
        let mut state = state.borrow_mut();
        let core = state.as_mut();
        let events = core.io.drain();
        for event in events {
            let value = event.value();
            for handler in &core.listeners {
                dispatches.push((handler.clone(), value.clone()));
            }
        }
    }
    if let Some(host) = eng.script_host() {
        for (handler, value) in dispatches {
            host.call_on(handler.node, &handler.method, std::slice::from_ref(&value));
        }
    }
}

/// The `listen` verb, which is the same question on either machine: which
/// method on which node hears about an export.
pub(crate) fn install_listen<S: AsMut<ExportCore> + 'static>(m: &mut dyn Bindings<Engine>) {
    m.function(
        "listen",
        |eng: &Engine, (node, opts): (balaur_script::NodeId, Option<Value>)| {
            let handler = handler_of(&Value::Node(node.0), opts.as_ref(), "on_event", "on_export")?
                .ok_or_else(|| anyhow!("export.listen needs a node"))?;
            eng.resource::<S>()
                .borrow_mut()
                .as_mut()
                .listeners
                .push(handler);
            Ok(())
        },
    );
}

/// What `listen` is documented as, so both modules describe it the same way.
pub(crate) const LISTEN_DOC: &str = "Have the node's `on_export(event)`, or the `on_event` method the options name, called as each export starts, finishes or fails.";
