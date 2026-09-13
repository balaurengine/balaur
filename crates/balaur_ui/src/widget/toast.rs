//! Messages that arrive, stack in a corner and leave on their own.
//!
//! A toast is a root like any other, so the scene holds it and a script
//! spawns it; what this file adds is the clock. Its age is the engine's own
//! time, so a replay shows the same toasts for the same frames.

use crate::vocabulary::words as w;
use crate::widget::arena::Placed;
use crate::widget::node::Widget;
use balaur_core::{Engine, hecs::Entity};
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::cell::RefCell;

/// How long a toast takes to fade out, at the end of its time.
const FADE: f64 = 0.5;

/// Space between two toasts stacked at the same anchor.
const GAP: f32 = 8.0;

thread_local! {
    /// When each toast was first drawn, in engine time.
    static BORN: RefCell<FxHashMap<u64, f64>> = RefCell::new(FxHashMap::default());
}

/// The toasts drawn so far this pass, per anchor, and the ones whose time is
/// up. One of these lives for the length of a draw.
#[derive(Default)]
pub(crate) struct Stack {
    stacked: FxHashMap<SmolStr, f32>,
    expired: Vec<Entity>,
    /// Every toast seen this pass, so the ones that are gone are forgotten.
    seen: Vec<u64>,
}

impl Stack {
    /// Where this root draws and how solid it is: a toast is pushed past the
    /// toasts already at its anchor, and fades over its last half second.
    /// Every other kind is handed back what it came with.
    pub(crate) fn place(
        &mut self,
        eng: &Engine,
        placed: &Placed,
        area: egui::Rect,
    ) -> (egui::Rect, f32) {
        let widget = &placed.widget;
        if widget.kind != w::TOAST {
            return (area, 1.0);
        }
        let key = placed.entity.to_bits().get();
        self.seen.push(key);
        let now = eng.time();
        let born = BORN.with(|born| *born.borrow_mut().entry(key).or_insert(now));
        let age = now - born;
        if widget.duration > 0.0 && age >= f64::from(widget.duration) {
            self.expired.push(placed.entity);
        }
        let offset = self.stacked.get(&widget.anchor).copied().unwrap_or(0.0);
        (shifted(&widget.anchor, area, offset), fade(widget, age))
    }

    /// Count what a toast took, so the next one at that anchor clears it.
    pub(crate) fn drew(&mut self, placed: &Placed) {
        if placed.widget.kind != w::TOAST {
            return;
        }
        let Some(rect) = crate::widget::arrange::placing_at(placed.entity) else {
            return;
        };
        *self.stacked.entry(placed.widget.anchor.clone()).or_default() += rect.height() + GAP;
    }

    /// The toasts to free, with the rest of the clock tidied.
    pub(crate) fn expired(self) -> Vec<Entity> {
        BORN.with(|born| {
            born.borrow_mut()
                .retain(|key, _| self.seen.contains(key));
        });
        self.expired
    }
}

/// A toast's own share of the screen, past the ones already stacked there:
/// down from a top anchor, up from a bottom one.
fn shifted(anchor: &str, area: egui::Rect, offset: f32) -> egui::Rect {
    if offset == 0.0 {
        return area;
    }
    let down = !matches!(
        anchor,
        w::BOTTOM_LEFT | w::BOTTOM_RIGHT | w::CENTER_BOTTOM
    );
    area.translate(egui::vec2(0.0, if down { offset } else { -offset }))
}

/// How solid a toast is at this age: whole until its last half second, then
/// out. A toast with no `duration` stays as it is.
fn fade(widget: &Widget, age: f64) -> f32 {
    if widget.duration <= 0.0 {
        return 1.0;
    }
    let left = f64::from(widget.duration) - age;
    if left >= FADE {
        return 1.0;
    }
    (left / FADE).clamp(0.0, 1.0) as f32
}

/// Ask for the toasts whose time is up to go.
///
/// Queued, as a script's own `queue_free` is: the frame frees them together
/// at its end, past the pass that is still walking an arena naming them.
pub(crate) fn clear(eng: &Engine, expired: &[Entity]) {
    for entity in expired {
        eng.push_command(balaur_core::Command::Free(*entity));
    }
}
