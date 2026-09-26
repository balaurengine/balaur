//! When the UI pass runs.
//!
//! Every frame by default: a HUD reads live state each pass. A script that
//! turns `ui.set_lazy` on gets a pass only when something asks for one:
//! input, `ui.request_repaint`, a log line, an asset reload, an egui
//! animation, or the idle tick between drags, and the frames between
//! re-present the last pass's shapes. The editor asks for it: on the web its
//! shell is most of the frame, and nothing in it moves while nobody touches
//! it.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use balaur_core::Engine;
use balaur_core::time::Instant;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::{UiConfig, UiState};

/// How long a lazy UI goes without a pass when nothing asks for one, and
/// no drag outside it holds the pointer.
const IDLE: Duration = Duration::from_millis(250);

/// What a pass is filed under in the profiler, and what the passes after it
/// in the same frame are: egui reruns the whole closure when a pass only
/// learned a size, so the shell is built twice and the rows say so.
const PASS: &str = "ui";
const RERUN: &str = "ui rerun";

/// Whether the next frame runs the UI pass.
#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches, each with its own writer: a script, the loop, egui's hook"
)]
pub struct Pacing {
    lazy: bool,
    /// Set by the windowed loop. Offscreen runs stay per frame, so a capture
    /// never shows a stale shell.
    honoured: bool,
    requested: bool,
    /// A request's claim on the next frame: `requested` is spent by this
    /// frame's pass, and a sleeping loop has to see the ask after it.
    owed: bool,
    /// Passes this frame, counted by [`note_pass`] and zeroed by
    /// [`wants_pass`], which the loop calls once a frame.
    passes: u32,
    /// When the last pass drew; `None` until one has.
    last_pass: Option<Instant>,
    logs_seen: u64,
    assets_seen: u64,
    /// When passes are owed: egui's delayed requests, which it reports from
    /// any thread, and a script's `request_repaint(#{ after })`. Every one is
    /// kept, so one asked for as another comes due is not lost behind it.
    due: Arc<Mutex<BTreeSet<Instant>>>,
    /// Whether egui reports its requests into `due` yet.
    heard: bool,
}

/// What a low-processor loop does before its next frame.
#[derive(Debug, PartialEq, Eq)]
pub enum NextFrame {
    /// Something is owed a frame now.
    Now,
    /// Nothing is; sleep until input, a wake, or this long if it says.
    Sleep(Option<Duration>),
}

/// Owe a pass at `at`.
fn owe(due: &Mutex<BTreeSet<Instant>>, at: Instant) {
    due.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(at);
}

/// Have egui report every repaint it asks for, with its delay, into `due`.
#[allow(
    clippy::disallowed_methods,
    reason = "egui's delays are wall-clock time; nothing simulated reads them"
)]
fn hear_egui(pacing: &mut Pacing, ctx: &egui::Context) {
    if pacing.heard {
        return;
    }
    pacing.heard = true;
    let due = Arc::clone(&pacing.due);
    ctx.set_request_repaint_callback(move |info| {
        owe(&due, Instant::now() + info.delay);
        if info.delay.is_zero() {
            balaur_core::wake::wake();
        }
    });
}

/// Whether a low-processor loop owes a frame now, and otherwise how long it
/// may sleep before one is due. Input and `balaur_core::wake`, which a log
/// line calls, are the loop's own to check; this answers for the UI: a
/// request, a theme still settling, and every repaint egui or a script
/// scheduled.
#[allow(
    clippy::disallowed_methods,
    reason = "a sleeping loop is paced against the wall clock; nothing simulated reads it"
)]
pub fn next_frame(eng: &Engine, ctx: &egui::Context) -> NextFrame {
    let Some(pacing) = eng.try_resource::<Pacing>() else {
        return NextFrame::Now;
    };
    let mut pacing = pacing.borrow_mut();
    hear_egui(&mut pacing, ctx);
    if std::mem::take(&mut pacing.owed) || pacing.requested || settling(eng) {
        return NextFrame::Now;
    }
    let due = pacing
        .due
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .first()
        .copied();
    match due {
        None => NextFrame::Sleep(None),
        Some(at) => match at.checked_duration_since(Instant::now()) {
            Some(left) if !left.is_zero() => NextFrame::Sleep(Some(left)),
            _ => NextFrame::Now,
        },
    }
}

/// Fonts, a theme swap and a forgotten scene each take a pass to settle.
fn settling(eng: &Engine) -> bool {
    eng.try_resource::<UiState>().is_some_and(|state| {
        let state = state.borrow();
        !state.fonts_installed || state.forget_egui
    }) || eng
        .try_resource::<UiConfig>()
        .is_some_and(|config| config.borrow().changed)
}

/// Whether a repaint egui or a script scheduled has come due, clearing what has.
#[allow(
    clippy::disallowed_methods,
    reason = "a scheduled repaint is paced against the wall clock; nothing simulated reads it"
)]
fn take_due(pacing: &Pacing) -> bool {
    let now = Instant::now();
    let mut due = pacing.due.lock().unwrap_or_else(PoisonError::into_inner);
    let before = due.len();
    due.retain(|at| *at > now);
    due.len() < before
}

/// Let `ui.set_lazy` take effect: the loop calling this re-presents the last
/// pass's shapes on a frame that skips one.
pub fn honour_lazy(eng: &Engine) {
    if let Some(pacing) = eng.try_resource::<Pacing>() {
        pacing.borrow_mut().honoured = true;
    }
}

/// Whether the pointer belongs to a drag the UI is no part of — a camera
/// being orbited over the scene, with `camera_enabled` saying the camera
/// still has its drag buttons.
///
/// A button is down and egui took no candidate from the press, so the press
/// missed every widget: nothing in the shell can change until it comes up,
/// and a pass would rebuild the same picture. The press ran one, which is
/// where both facts come from.
#[must_use]
pub fn pointer_is_dragging_elsewhere(ctx: &egui::Context, camera_enabled: bool) -> bool {
    camera_enabled && ctx.input(|i| i.pointer.any_down()) && !ctx.egui_is_using_pointer()
}

/// Whether this frame runs the UI pass; `input_seen` is whether the window
/// delivered an event the UI could act on since the last one, and `dragging`
/// whether the pointer belongs to a drag the UI is no part of
/// ([`pointer_is_dragging_elsewhere`]).
pub fn wants_pass(eng: &Engine, ctx: &egui::Context, input_seen: bool, dragging: bool) -> bool {
    let Some(pacing) = eng.try_resource::<Pacing>() else {
        return true;
    };
    let mut pacing = pacing.borrow_mut();
    pacing.passes = 0;
    let requested = std::mem::take(&mut pacing.requested);
    let due = take_due(&pacing);
    if !(pacing.lazy && pacing.honoured) {
        return true;
    }
    hear_egui(&mut pacing, ctx);
    let logs = balaur_core::logbuf::total();
    let assets = balaur_core::assets::generation(eng);
    let changed = logs != pacing.logs_seen || assets != pacing.assets_seen;
    pacing.logs_seen = logs;
    pacing.assets_seen = assets;
    // The tick is for state that moves without input; a drag the UI is no
    // part of moves none of it, and the pass it forces stalls that drag.
    let idle = !dragging && pacing.last_pass.is_none_or(|at| at.elapsed() >= IDLE);
    input_seen || requested || changed || settling(eng) || idle || due
}

/// File what a pass cost, under a name that says whether egui had already
/// built the same shell this frame.
pub(crate) fn note_pass(eng: &Engine, elapsed: std::time::Duration) {
    let first = match eng.try_resource::<Pacing>() {
        Some(pacing) => {
            let mut pacing = pacing.borrow_mut();
            pacing.passes += 1;
            pacing.passes <= 1
        }
        None => true,
    };
    balaur_core::timings::record(eng, if first { PASS } else { RERUN }, elapsed);
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

/// The instant `seconds` from now; a negative or absurd count is now.
#[allow(
    clippy::disallowed_methods,
    reason = "a scheduled repaint is paced against the wall clock; nothing simulated reads it"
)]
fn later(seconds: f64) -> Instant {
    Instant::now() + Duration::try_from_secs_f64(seconds).unwrap_or_default()
}

/// `ui.*` bindings: pacing.
pub(crate) fn install(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_lazy", &[], "", "Run the UI pass only when something asks for it (input, `request_repaint`, a log line, an asset reload, an egui animation, or the idle tick every 250 ms, which waits out a drag the UI is no part of) and re-present the last pass in between. Off by default: a HUD that reads live state each frame should leave it off. Ignored offscreen."),
        ("request_repaint", &[], "(opts: map?)", "Run the UI pass this frame even when lazy, and the frame itself under `[window] low_processor`; call it every frame something on screen moves without input, such as while the scene plays. `#{ after: seconds }` asks for one that far ahead instead."),
    ]);
    m.function("set_lazy", |eng: &Engine, on: bool| {
        eng.resource::<Pacing>().borrow_mut().lazy = on;
        Ok(())
    });
    m.function("request_repaint", |eng: &Engine, opts: Option<Value>| {
        let after = match &opts {
            Some(Value::Map(entries)) => entries
                .iter()
                .find(|(key, _)| key == "after")
                .map(|(_, v)| v),
            _ => None,
        };
        let pacing = eng.resource::<Pacing>();
        let mut pacing = pacing.borrow_mut();
        match after {
            None | Some(Value::Nil) => {
                pacing.requested = true;
                pacing.owed = true;
            }
            Some(Value::Num(seconds)) => owe(&pacing.due, later(*seconds)),
            #[allow(clippy::cast_precision_loss, reason = "a count of seconds")]
            Some(Value::Int(seconds)) => owe(&pacing.due, later(*seconds as f64)),
            Some(other) => anyhow::bail!("`after` is seconds, not {}", other.type_name()),
        }
        Ok(())
    });
}
