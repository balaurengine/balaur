//! When the UI pass runs.
//!
//! Every frame by default: a HUD reads live state each pass. A script that
//! turns `ui.set_lazy` on gets a pass only when something asks for one —
//! input, `ui.request_repaint`, a log line, an asset reload, an egui
//! animation, or the idle tick — and the frames between re-present the last
//! pass's shapes. The editor asks for it: on the web its shell is most of the
//! frame, and nothing in it moves while nobody touches it.

use std::time::Duration;

use balaur_core::Engine;
use balaur_core::time::Instant;
use balaur_script::{Bindings, BindingsExt};

use crate::{UiConfig, UiState};

/// How long a lazy UI goes without a pass when nothing asks for one.
const IDLE: Duration = Duration::from_millis(250);

/// Whether the next frame runs the UI pass.
#[derive(Default)]
pub struct Pacing {
    lazy: bool,
    /// Set by the windowed loop. Offscreen runs stay per frame, so a capture
    /// never shows a stale shell.
    honoured: bool,
    requested: bool,
    /// When the last pass drew; `None` until one has.
    last_pass: Option<Instant>,
    logs_seen: u64,
    assets_seen: u64,
}

/// Let `ui.set_lazy` take effect: the loop calling this re-presents the last
/// pass's shapes on a frame that skips one.
pub fn honour_lazy(eng: &Engine) {
    if let Some(pacing) = eng.try_resource::<Pacing>() {
        pacing.borrow_mut().honoured = true;
    }
}

/// Whether this frame runs the UI pass; `input_seen` is whether the window
/// delivered any event since the last one.
pub fn wants_pass(eng: &Engine, ctx: &egui::Context, input_seen: bool) -> bool {
    let Some(pacing) = eng.try_resource::<Pacing>() else {
        return true;
    };
    let mut pacing = pacing.borrow_mut();
    if !(pacing.lazy && pacing.honoured) {
        return true;
    }
    let logs = balaur_core::logbuf::total();
    let assets = balaur_core::assets::generation(eng);
    let changed = logs != pacing.logs_seen || assets != pacing.assets_seen;
    pacing.logs_seen = logs;
    pacing.assets_seen = assets;
    // Fonts, a theme swap and a forgotten scene each take a pass to settle.
    let settling = eng.try_resource::<UiState>().is_some_and(|state| {
        let state = state.borrow();
        !state.fonts_installed || state.forget_egui
    }) || eng
        .try_resource::<UiConfig>()
        .is_some_and(|config| config.borrow().changed);
    let idle = pacing.last_pass.is_none_or(|at| at.elapsed() >= IDLE);
    input_seen
        || std::mem::take(&mut pacing.requested)
        || changed
        || settling
        || idle
        || ctx.has_requested_repaint()
}

/// Note that a pass drew, for the idle tick.
#[allow(
    clippy::disallowed_methods,
    reason = "paces the UI pass against the wall clock; nothing simulated reads it"
)]
pub(crate) fn mark_pass(eng: &Engine) {
    if let Some(pacing) = eng.try_resource::<Pacing>() {
        pacing.borrow_mut().last_pass = Some(Instant::now());
    }
}

/// `ui.*` bindings: pacing.
pub(crate) fn install(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_lazy", &[], "", "Run the UI pass only when something asks for it — input, `request_repaint`, a log line, an asset reload, an egui animation, or the idle tick every 250 ms — and re-present the last pass in between. Off by default: a HUD that reads live state each frame should leave it off. Ignored offscreen."),
        ("request_repaint", &[], "", "Run the UI pass this frame even when lazy; call it every frame something on screen moves without input, such as while the scene plays."),
    ]);
    m.function("set_lazy", |eng: &Engine, on: bool| {
        eng.resource::<Pacing>().borrow_mut().lazy = on;
        Ok(())
    });
    m.function("request_repaint", |eng: &Engine, ()| {
        eng.resource::<Pacing>().borrow_mut().requested = true;
        Ok(())
    });
}
