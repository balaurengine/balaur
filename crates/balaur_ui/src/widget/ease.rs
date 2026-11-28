//! A widget's look in its states and between them: which tables apply, what
//! a theme's `transition` eases, and the blend between two styles. The site's
//! buttons ease their colours over 0.2 s, its cards rise over 0.12 s.

use std::rc::Rc;

use egui::Color32;

use crate::widget::layer::Painting;
use crate::widget::theme::{Pointer, Shadow, Style, WidgetState};

/// Which pointer a widget's easing follows: the box the layout gave it, or
/// the response a kind drew for itself.
pub(crate) const BY_BOX: u8 = 0;
const BY_RESPONSE: u8 = 1;

/// The ring a focused widget whose theme states an `outline` draws outside
/// what it drew, as CSS's `outline`, `outline-width` and `outline-offset`;
/// one a state table (`checked`, `hover`) states draws in that state too.
pub(crate) fn focus_ring(ui: &egui::Ui, style: &Style) {
    let Some(colour) = style.outline else {
        return;
    };
    let drawn = ui.min_rect();
    let rect = if drawn.area() > 0.0 {
        drawn
    } else {
        ui.max_rect()
    };
    let radius = if style.round == Some(true) {
        rect.height() / 2.0
    } else {
        style.radius.unwrap_or(0.0)
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a corner in whole points, capped"
    )]
    let corner = egui::CornerRadius::same(radius.clamp(0.0, 120.0) as u8);
    let (gap, width) = style.ring();
    crate::widget::paint::Brush::of(ui).outline(
        rect.expand(gap),
        corner,
        egui::Stroke::new(width, colour),
        egui::StrokeKind::Outside,
    );
}

/// One state a widget moves in and out of, each eased on its own, as CSS
/// runs every property's transition from where it is.
#[derive(Clone, Copy)]
enum Turn {
    Checked,
    Disabled,
    Focused,
    Over,
    Held,
}

impl Turn {
    /// In the order they blend, outermost first: a checked widget is a
    /// different base, the pointer's two are the innermost.
    const ALL: [Self; 5] = [
        Self::Checked,
        Self::Disabled,
        Self::Focused,
        Self::Over,
        Self::Held,
    ];

    fn of(self, state: WidgetState) -> bool {
        match self {
            Self::Checked => state.checked,
            Self::Disabled => state.disabled,
            Self::Focused => state.focused,
            Self::Over => state.pointer != Pointer::Away,
            Self::Held => state.pointer == Pointer::Held,
        }
    }

    /// `state` with this turn set to `on`. The pointer is over a widget it
    /// holds, so `Held` brings `Over` along and losing `Over` lets go.
    fn set(self, state: WidgetState, on: bool) -> WidgetState {
        let pointer = |over: bool, held: bool| match (over, held) {
            (_, true) => Pointer::Held,
            (true, false) => Pointer::Over,
            (false, false) => Pointer::Away,
        };
        match self {
            Self::Checked => WidgetState {
                checked: on,
                ..state
            },
            Self::Disabled => WidgetState {
                disabled: on,
                ..state
            },
            Self::Focused => WidgetState {
                focused: on,
                ..state
            },
            Self::Over => WidgetState {
                pointer: pointer(on, state.pointer == Pointer::Held && on),
                ..state
            },
            Self::Held => WidgetState {
                pointer: pointer(state.pointer != Pointer::Away, on),
                ..state
            },
        }
    }
}

/// How far one turn has gone, kept between passes: 0 off, 1 on, linear,
/// and the clock it was last read at.
#[derive(Clone, Copy)]
struct Progress {
    linear: f32,
    at: f64,
}

impl Painting<'_> {
    /// `base` in `state` for the widget at `index`, which answers with the
    /// response it drew, eased from where it last was when the style states
    /// a `transition`.
    pub(crate) fn dressed(&self, index: usize, base: &Style, state: WidgetState) -> Style {
        self.easing(index, base, state, BY_RESPONSE)
            .unwrap_or_else(|| base.in_states(state))
    }

    /// `base` partway through a change of state, `None` once every turn has
    /// settled or where it states no `transition`. `by` keeps apart the box's
    /// pointer and a response's, which a kind narrower than its box may
    /// disagree on.
    pub(crate) fn easing(
        &self,
        index: usize,
        base: &Style,
        state: WidgetState,
        by: u8,
    ) -> Option<Style> {
        let time = base.transition?;
        let id = egui::Id::unique(("balaur-ease", self.arena[index].entity, by));
        let mut turns = [(Turn::Checked, 0.0); 5];
        let mut moving = false;
        for (slot, (n, turn)) in turns.iter_mut().zip(Turn::ALL.into_iter().enumerate()) {
            let on = turn.of(state);
            let linear = self.progress(id.with(n), on, time);
            moving |= if on { linear < 1.0 } else { linear > 0.0 };
            *slot = (turn, egui::emath::easing::cubic_out(linear));
        }
        if !moving {
            return None;
        }
        self.ctx.request_repaint();
        Some(blend(base, state, &turns))
    }

    /// The clock an easing runs on: the engine's when every frame is one set
    /// step, so a picture taken mid-change is the same every run, else the
    /// pass's own, which a scaled or paused game leaves running.
    pub(crate) fn ui_now(&self) -> f64 {
        if self.eng.steps_fixed() {
            self.eng.time()
        } else {
            self.ctx.input(|input| input.time)
        }
    }

    /// One turn's linear progress toward `on`, starting where it is: a turn
    /// reversed halfway runs back from halfway. The time since the last pass
    /// counts toward the state this pass found, so the frame after a change
    /// already shows it begun. A widget seen for the first time starts
    /// settled.
    fn progress(&self, id: egui::Id, on: bool, seconds: f32) -> f32 {
        let now = self.ui_now();
        let settled = if on { 1.0 } else { 0.0 };
        self.ctx.data_mut(|data| {
            let kept = data.get_temp_mut_or_insert_with(id, || Progress {
                linear: settled,
                at: now,
            });
            #[allow(
                clippy::cast_possible_truncation,
                reason = "seconds between two passes"
            )]
            let step = ((now - kept.at) as f32 / seconds).max(0.0);
            kept.at = now;
            kept.linear = if on {
                (kept.linear + step).min(1.0)
            } else {
                (kept.linear - step).max(0.0)
            };
            kept.linear
        })
    }
}

/// `base` with each turn `t` of the way on, the first turn outermost.
fn blend(base: &Style, state: WidgetState, turns: &[(Turn, f32)]) -> Style {
    let Some((&(turn, t), rest)) = turns.split_first() else {
        return base.in_states(state);
    };
    if t <= 0.0 {
        return blend(base, turn.set(state, false), rest);
    }
    if t >= 1.0 {
        return blend(base, turn.set(state, true), rest);
    }
    let off = blend(base, turn.set(state, false), rest);
    off.toward(&blend(base, turn.set(state, true), rest), t)
}

impl Style {
    /// The outline as the table states it, to tell a state's own from rest's.
    #[must_use]
    pub fn states_ring(&self) -> (Option<Color32>, Option<f32>, Option<f32>) {
        (self.outline, self.outline_width, self.outline_offset)
    }

    /// The focus ring's gap from the edge and its width: the style's
    /// `outline_offset` and `outline_width`, 2 and 2 unset.
    #[must_use]
    pub fn ring(&self) -> (f32, f32) {
        (
            self.outline_offset.unwrap_or(2.0),
            self.outline_width.unwrap_or(2.0),
        )
    }

    /// The style with the tables for the widget's state over it: `checked`
    /// under everything, then disabled alone, or focus with the pointer's
    /// table over it.
    #[must_use]
    pub fn in_states(&self, state: WidgetState) -> Self {
        if state.checked && self.checked.is_none() && !state.disabled {
            // A theme with no `checked` table dresses a checked widget as held.
            let held = WidgetState {
                pointer: Pointer::Held,
                checked: false,
                ..state
            };
            return self.in_states(held);
        }
        let on = match (&self.checked, state.checked) {
            (Some(checked), true) => checked.over(self),
            _ => self.clone(),
        };
        if state.disabled {
            return on
                .disabled
                .as_ref()
                .map_or_else(|| on.clone(), |patch| patch.over(&on));
        }
        // `:focus` and `:hover` at once, as on the web; the pointer's table
        // wins what both state.
        let focused = match (&on.focus, state.focused) {
            (Some(focus), true) => focus.over(&on),
            _ => on,
        };
        let pointer = match state.pointer {
            Pointer::Held => focused.active.as_ref().or(focused.hover.as_ref()),
            Pointer::Over => focused.hover.as_ref(),
            Pointer::Away => None,
        };
        pointer.map_or_else(|| focused.clone(), |patch| patch.over(&focused))
    }

    /// The style `t` of the way from `self` to `to`, 0 to 1, as a state
    /// change eases over `transition`. Colours, the shadow, `offset` and
    /// `scale` run between the two; a fill, an edge, an outline or a shadow only one
    /// states runs from clear, an ink only one states steps, as does
    /// everything else, at halfway.
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
        let shadows = shadows_toward(
            self.shadows.as_deref().unwrap_or_default(),
            to.shadows.as_deref().unwrap_or_default(),
            t,
        );
        // `offset` and `scale` run between the two, an unset one from its
        // rest: no move, and its own size.
        let pair = |from: Option<[f32; 2]>, into: Option<[f32; 2]>, rest: [f32; 2]| {
            (from.is_some() || into.is_some()).then(|| {
                let ([ax, ay], [bx, by]) = (from.unwrap_or(rest), into.unwrap_or(rest));
                [ax + (bx - ax) * t, ay + (by - ay) * t]
            })
        };
        let offset = pair(self.offset, to.offset, [0.0, 0.0]);
        let scale = pair(self.scale, to.scale, [1.0, 1.0]);
        Self {
            fill: paint(self.fill, to.fill),
            stroke: paint(self.stroke, to.stroke),
            outline: paint(self.outline, to.outline),
            text_color: ink(self.text_color, to.text_color),
            icon_color: ink(self.icon_color, to.icon_color),
            shadows,
            offset,
            scale,
            // The weight the box was measured at, which is the state's it eases to.
            weight: to.weight,
            ..if t < 0.5 { self.clone() } else { to.clone() }
        }
    }
}

/// Two shadow lists `t` of the way, shadow by shadow, as CSS runs them: one
/// only one list has runs from or to clear, and lists inset at different
/// places step at halfway.
fn shadows_toward(from: &[Shadow], into: &[Shadow], t: f32) -> Option<Rc<[Shadow]>> {
    let paired = (0..from.len().max(into.len()))
        .map(|i| match (from.get(i), into.get(i)) {
            (Some(a), Some(b)) if a.inset != b.inset => None,
            (Some(&a), Some(&b)) => Some(a.toward(b, t)),
            (Some(&a), None) => Some(a.toward(a.cleared(), t)),
            (None, Some(&b)) => Some(b.cleared().toward(b, t)),
            (None, None) => None,
        })
        .collect::<Option<Vec<Shadow>>>();
    let list: Vec<Shadow> = paired.unwrap_or_else(|| if t < 0.5 { from } else { into }.to_vec());
    (!list.is_empty()).then(|| list.into())
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
            inset: self.inset,
        }
    }
}
