//! What a verb that takes many frames looks like to a script: the event it
//! reports, and the pump that sends it.
//!
//! The work itself is elsewhere and differs per machine. What a script sees of
//! it *in flight* is the same everywhere and lives here once.
//!
//! [`crate::export_shared`] is this shape written out for export alone, from
//! before there was a second verb with it. It should move onto this, and is
//! left where it is until an export can be driven end to end in a test:
//! nothing but the editor exercises its events today.

use std::path::PathBuf;

use balaur::Engine;
use balaur::replay::ExternalIo;
use balaur_script::Value;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// One step of a job, as the named event a script listens for.
///
/// `Serialize` and `DeserializeOwned` are [`ExternalIo`]'s: what crossed into
/// a tick rides in the recording, so a replay hands a script the same steps.
pub(crate) trait Reported: Clone + Serialize + DeserializeOwned + 'static {
    /// What `events::listen` names to hear this verb, and the `on_<name>`
    /// method it calls.
    const EVENT: &'static str;
    /// The event's payload. One shape per verb, so a script reads `kind` and
    /// then the field that kind carries.
    fn value(&self) -> Value;
}

/// The project a job works on, and the channel it reports on.
///
/// Each plugin's own state holds one of these and adds what its machine needs.
pub(crate) struct Reporting<E> {
    pub(crate) io: ExternalIo<E>,
    pub(crate) project: PathBuf,
}

impl<E> Reporting<E> {
    pub(crate) fn new(project: PathBuf) -> Self {
        Self {
            io: ExternalIo::default(),
            project,
        }
    }
}

/// Send what a job reported as its named event.
///
/// Generic over the plugin's own state so each registers it with its own:
/// `reg.add_system(Stage::First, pump::<ImportState, ImportEvent>)`.
pub(crate) fn pump<S, E>(eng: &Engine, _: f32)
where
    S: AsMut<Reporting<E>> + 'static,
    E: Reported,
{
    let reports: Vec<Value> = {
        let state = eng.resource::<S>();
        let mut state = state.borrow_mut();
        let reports = state.as_mut().io.drain();
        reports.iter().map(Reported::value).collect()
    };
    for report in reports {
        balaur_core::events::emit(eng, E::EVENT, report);
    }
}
