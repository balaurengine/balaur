//! How a widget's look moves between states: what a theme's `transition`
//! eases, and the blend between two styles it eases through. The site's
//! buttons ease their colours over 0.2 s, its cards rise over 0.12 s.

use egui::Color32;

use crate::widget::layer::Painting;
use crate::widget::theme::{Pointer, Shadow, Style, WidgetState};

/// Which pointer a widget's easing follows: the box the layout gave it, or
/// the response a kind drew for itself.
pub(crate) const BY_BOX: u8 = 0;
const BY_RESPONSE: u8 = 1;

impl Painting<'_> {
    /// `base` in `state` for the widget at `index`, which answers with the
    /// response it drew, eased from where the pointer last had it when the
    /// style states a `transition`.
    pub(crate) fn dressed(&self, index: usize, base: &Style, state: WidgetState) -> Style {
        self.easing(index, base, state, BY_RESPONSE)
            .unwrap_or_else(|| base.in_states(state))
    }

    /// `base` partway through a change of the pointer's state, `None` once
    /// it has settled or where it states no `transition`. `by` keeps apart the
    /// box's pointer and a response's, which a kind narrower than its box
    /// may disagree on.
    pub(crate) fn easing(
        &self,
        index: usize,
        base: &Style,
        state: WidgetState,
        by: u8,
    ) -> Option<Style> {
        let time = base.transition?;
        let id = egui::Id::unique(("balaur-ease", self.arena[index].entity, by));
        let eased = |which: u8, on: bool| {
            let ease = egui::emath::easing::cubic_out;
            self.ctx
                .animate_bool_with_time_and_easing(id.with(which), on, time, ease)
        };
        let over = eased(0, state.pointer != Pointer::Away);
        let held = eased(1, state.pointer == Pointer::Held);
        let moving = |t: f32| t > 0.0 && t < 1.0;
        if !moving(over) && !moving(held) {
            return None;
        }
        let at = |pointer| base.in_states(WidgetState { pointer, ..state });
        let over_it = at(Pointer::Away).toward(&at(Pointer::Over), over);
        Some(over_it.toward(&at(Pointer::Held), held))
    }
}

impl Style {
    /// The style `t` of the way from `self` to `to`, 0 to 1, as a state
    /// change eases over `transition`. Colours, the shadow and `offset` run
    /// between the two; a fill, an outline or a shadow only one states runs
    /// from clear, an ink only one states steps, as does everything else, at
    /// halfway.
    #[must_use]
    pub fn toward(&self, to: &Self, t: f32) -> Self {
        let clear = Color32::TRANSPARENT;
        let paint = |from: Option<Color32>, into: Option<Color32>| match (from, into) {
            (None, None) => None,
            (from, into) => Some(
                from.unwrap_or(clear)
                    .lerp_to_gamma(into.unwrap_or(clear), t),
            ),
        };
        let ink = |from: Option<Color32>, into: Option<Color32>| match (from, into) {
            (Some(from), Some(into)) => Some(from.lerp_to_gamma(into, t)),
            _ if t < 0.5 => from,
            _ => into,
        };
        let shadow = match (self.shadow, to.shadow) {
            (None, None) => None,
            (Some(from), None) => Some(from.toward(from.cleared(), t)),
            (None, Some(into)) => Some(into.cleared().toward(into, t)),
            (Some(from), Some(into)) => Some(from.toward(into, t)),
        };
        let offset = match (self.offset, to.offset) {
            (None, None) => None,
            (from, into) => {
                let ([ax, ay], [bx, by]) = (from.unwrap_or_default(), into.unwrap_or_default());
                Some([ax + (bx - ax) * t, ay + (by - ay) * t])
            }
        };
        Self {
            fill: paint(self.fill, to.fill),
            stroke: paint(self.stroke, to.stroke),
            text_color: ink(self.text_color, to.text_color),
            icon_color: ink(self.icon_color, to.icon_color),
            shadow,
            offset,
            ..if t < 0.5 { self.clone() } else { to.clone() }
        }
    }
}

impl Shadow {
    /// The same shadow with nothing to show, for one easing in or out.
    #[must_use]
    pub fn cleared(self) -> Self {
        Self {
            color: Color32::TRANSPARENT,
            ..self
        }
    }

    /// The shadow `t` of the way to `to`, 0 to 1.
    #[must_use]
    pub fn toward(self, to: Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            x: mix(self.x, to.x),
            y: mix(self.y, to.y),
            blur: mix(self.blur, to.blur),
            spread: mix(self.spread, to.spread),
            color: self.color.lerp_to_gamma(to.color, t),
        }
    }
}
