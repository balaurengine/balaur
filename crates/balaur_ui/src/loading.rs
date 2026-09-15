//! What a held splash reports: how far the load is, and whether it is done.
//!
//! A resource rather than a setting, because it is the run's own state: a
//! script reports progress as it goes and says when it is through, and the
//! splash reads both. Inserted on the first report, so a game that never
//! reports has no resource and its splash goes when its seconds are up.

use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt};

#[derive(Default, Clone, Debug)]
pub struct Loading {
    /// Zero to one, as last reported.
    pub progress: f32,
    /// What is loading, drawn under the bar; empty draws the bar alone.
    pub label: String,
    /// Set by `ui.loaded()`: the splash may go.
    pub done: bool,
}

fn report(eng: &Engine, progress: f32, label: String) {
    let progress = progress.clamp(0.0, 1.0);
    if let Some(loading) = eng.try_resource::<Loading>() {
        let mut loading = loading.borrow_mut();
        loading.progress = progress;
        loading.label = label;
        return;
    }
    eng.insert_resource(Loading {
        progress,
        label,
        done: false,
    });
}

/// `ui.*` bindings: the load a splash waits on.
pub(crate) fn install(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_loading", &[], "(progress: float, label: string?)", "Report how far a load is, zero to one. This is what holds `[application] splash` past its seconds, and what draws the bar under it."),
        ("loaded", &[], "()", "Say the load is through, which lets a held splash go once `splash_seconds` has also passed. The editor calls it once its shell has settled."),
    ]);
    m.function(
        "set_loading",
        |eng: &Engine, (progress, label): (f32, Option<String>)| {
            report(eng, progress, label.unwrap_or_default());
            Ok(())
        },
    );
    m.function("loaded", |eng: &Engine, ()| {
        report(eng, 1.0, String::new());
        if let Some(loading) = eng.try_resource::<Loading>() {
            loading.borrow_mut().done = true;
        }
        Ok(())
    });
}
