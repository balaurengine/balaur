//! What a verb that takes many frames looks like to a script: the event it
//! reports, who is listening, and the pump that hands one to the other.
//!
//! The work itself is elsewhere and differs per machine. What a script sees of
//! it *in flight* is the same everywhere and lives here once.
//!
//! [`crate::export_shared`] is this shape written out for export alone, from
//! before there was a second verb with it. It should move onto this, and is
//! left where it is until an export can be driven end to end in a test:
//! nothing but the editor exercises its events today.

use std::path::PathBuf;

use anyhow::anyhow;
use balaur::Engine;
use balaur::replay::ExternalIo;
use balaur_core::handler::{Handler, handler_of};
use balaur_script::{Bindings, BindingsExt, Value};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// One step of a job, as the table a script's handler is called with.
///
/// `Serialize` and `DeserializeOwned` are [`ExternalIo`]'s: what crossed into
/// a tick rides in the recording, so a replay hands a script the same steps.
pub(crate) trait Reported: Clone + Serialize + DeserializeOwned + 'static {
    /// The table a handler is called with. One shape per verb, so a script
    /// reads `kind` and then the field that kind carries.
    fn value(&self) -> Value;
}

/// The project a job works on, the channel it reports on, and who listens.
///
/// Each plugin's own state holds one of these and adds what its machine needs.
pub(crate) struct Reporting<E> {
    pub(crate) io: ExternalIo<E>,
    pub(crate) listeners: Vec<Handler>,
    pub(crate) project: PathBuf,
}

impl<E> Reporting<E> {
    pub(crate) fn new(project: PathBuf) -> Self {
        Self {
            io: ExternalIo::default(),
            listeners: Vec::new(),
            project,
        }
    }
}

/// Deliver what a job reported, to whoever asked to hear it.
///
/// Generic over the plugin's own state so each registers it with its own:
/// `reg.add_system(Stage::First, pump::<ImportState, ImportEvent>)`.
pub(crate) fn pump<S, E>(eng: &Engine, _: f32)
where
    S: AsMut<Reporting<E>> + 'static,
    E: Reported,
{
    let mut dispatches = Vec::new();
    {
        let state = eng.resource::<S>();
        let mut state = state.borrow_mut();
        let core = state.as_mut();
        for event in core.io.drain() {
            let value = event.value();
            for handler in &core.listeners {
                dispatches.push((handler.clone(), value.clone()));
            }
        }
    }
    // Outside the borrow: a handler may call the verb again.
    if let Some(host) = eng.script_host() {
        for (handler, value) in dispatches {
            host.call_on(handler.node, &handler.method, std::slice::from_ref(&value));
        }
    }
}

/// The `listen` verb: which method on which node hears about the work.
///
/// `named` is the method a node gets by default, and `on_event` in the options
/// names another.
pub(crate) fn install_listen<S, E>(m: &mut dyn Bindings<Engine>, verb: &'static str)
where
    S: AsMut<Reporting<E>> + 'static,
    E: Reported,
{
    m.function(
        "listen",
        move |eng: &Engine, (node, opts): (balaur_script::NodeId, Option<Value>)| {
            let handler = handler_of(&Value::Node(node.0), opts.as_ref(), "on_event", verb)?
                .ok_or_else(|| anyhow!("{verb} needs a node"))?;
            eng.resource::<S>()
                .borrow_mut()
                .as_mut()
                .listeners
                .push(handler);
            Ok(())
        },
    );
}
