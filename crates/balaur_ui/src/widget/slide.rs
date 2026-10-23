//! A number dragged along a rail between two ends, with the number beside it:
//! what a `slider` is.
//!
//! Ported from egui 0.36's `Slider` (MIT OR Apache-2.0, Emil Ernerfeldt and
//! the egui contributors), so the pointer, the keys, the logarithmic scale and
//! the smart aim behave as they did. The rail and the handle are painted from
//! egui's visuals, which the theme dresses.

use std::ops::RangeInclusive;

use egui::emath::{Rangef, lerp, remap, remap_clamp};
use egui::{Key, Pos2, Rect, Sense, pos2, vec2};

use crate::widget::drag::{DragNumber, Spelling, clamp};

/// When a number is held inside the range.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clamping {
    /// Never: a number past either end is shown and kept.
    Never,
    /// Only what the reader drags, types or steps.
    Edits,
    /// Always, a number the scene set included.
    Always,
}

/// How the rail maps to the range.
#[derive(Clone, Copy)]
pub(crate) struct Scale {
    pub(crate) logarithmic: bool,
    /// The smallest positive end of a logarithmic range that reaches zero.
    pub(crate) smallest_positive: f64,
    /// The largest end of a logarithmic range that reaches infinity.
    pub(crate) largest_finite: f64,
}

/// What a slider is told.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches of the `widget` component, each read once"
)]
pub(crate) struct Slide<'a> {
    pub(crate) value: &'a mut f64,
    pub(crate) range: RangeInclusive<f64>,
    pub(crate) scale: Scale,
    pub(crate) clamping: Clamping,
    pub(crate) smart_aim: bool,
    pub(crate) show_value: bool,
    pub(crate) upright: bool,
    pub(crate) prefix: &'a str,
    pub(crate) suffix: &'a str,
    pub(crate) step: Option<f64>,
    pub(crate) drag_speed: Option<f64>,
    pub(crate) min_decimals: usize,
    pub(crate) max_decimals: Option<usize>,
    pub(crate) spelling: Spelling,
    pub(crate) trailing_fill: Option<bool>,
    /// A rectangle this wide for each unit tall, rather than a disc.
    pub(crate) handle: Option<f32>,
    pub(crate) update_while_editing: bool,
}

impl Slide<'_> {
    fn get(&self) -> f64 {
        if self.clamping == Clamping::Always {
            clamp(*self.value, &self.range)
        } else {
            *self.value
        }
    }

    fn set(&mut self, mut value: f64) {
        if self.clamping != Clamping::Never {
            value = clamp(value, &self.range);
        }
        if let Some(step) = self.step {
            let start = *self.range.start();
            value = start + ((value - start) / step).round() * step;
        }
        if let Some(most) = self.max_decimals {
            value = egui::emath::round_to_decimals(value, most);
        }
        *self.value = value;
    }

    fn value_at(&self, position: f32, along: Rangef) -> f64 {
        let normalized = f64::from(remap_clamp(position, along, 0.0..=1.0));
        from_normalized(normalized, self.range.clone(), &self.scale)
    }

    fn position_of(&self, value: f64, along: Rangef) -> f32 {
        let normalized = to_normalized(value, self.range.clone(), &self.scale);
        lerp(along, normalized as f32)
    }

    fn handle_radius(&self, rect: &Rect) -> f32 {
        (if self.upright {
            rect.width()
        } else {
            rect.height()
        }) / 2.5
    }

    /// Where the handle's centre can travel: the rail less half the handle.
    fn travel(&self, rect: &Rect) -> Rangef {
        let radius = self.handle_radius(rect) * self.handle.unwrap_or(1.0);
        if self.upright {
            // The largest value is at the top, the lowest y.
            rect.y_range().shrink(radius).flip()
        } else {
            rect.x_range().shrink(radius)
        }
    }

    pub(crate) fn show(mut self, ui: &mut egui::Ui) -> egui::Response {
        let upright = self.upright;
        let inner = if upright {
            ui.vertical(|ui| self.contents(ui))
        } else {
            ui.horizontal(|ui| self.contents(ui))
        };
        inner.inner | inner.response
    }

    #[allow(clippy::float_cmp, reason = "a value written, not one measured")]
    fn contents(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let before = self.get();
        if self.clamping == Clamping::Always {
            self.set(before);
        }
        let thick = ui
            .text_style_height(&egui::TextStyle::Body)
            .max(ui.spacing().interact_size.y);
        let length = ui.spacing().slider_width;
        let size = if self.upright {
            vec2(thick, length)
        } else {
            vec2(length, thick)
        };
        let mut response = ui.allocate_response(size, Sense::drag());
        self.rail(ui, &response);
        let value = self.get();
        if value != before {
            response.mark_changed();
        }
        let enabled = ui.is_enabled();
        response.widget_info(|| egui::WidgetInfo::slider(enabled, value, ""));
        if self.show_value {
            let along = self.travel(&response.rect);
            let shown = self.number(ui, along);
            response = if shown.gained_focus() || shown.has_focus() || shown.lost_focus() {
                shown.union(response)
            } else {
                response.union(shown)
            };
        }
        response
    }

    /// The pointer and the keys on the rail, then the rail and its handle.
    fn rail(&mut self, ui: &egui::Ui, response: &egui::Response) {
        let rect = response.rect;
        let along = self.travel(&rect);
        if let Some(pointer) = response.interact_pointer_pos() {
            let at = if self.upright { pointer.y } else { pointer.x };
            let value = if self.smart_aim {
                let aim = ui.input(egui::InputState::aim_radius);
                egui::emath::smart_aim::best_in_range_f64(
                    self.value_at(at - aim, along),
                    self.value_at(at + aim, along),
                )
            } else {
                self.value_at(at, along)
            };
            self.set(value);
        }
        if response.has_focus() {
            self.keys(ui, response, along);
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        let value = self.get();
        let look = ui.style().interact(response);
        let widgets = &ui.visuals().widgets;
        let half = (ui.spacing().slider_rail_height / 2.0).max(0.0);
        let bar = if self.upright {
            Rect::from_min_max(
                pos2(rect.center().x - half, rect.top()),
                pos2(rect.center().x + half, rect.bottom()),
            )
        } else {
            Rect::from_min_max(
                pos2(rect.left(), rect.center().y - half),
                pos2(rect.right(), rect.center().y + half),
            )
        };
        let corner = widgets.inactive.corner_radius;
        let painter = ui.painter();
        painter.rect_filled(bar, corner, widgets.inactive.bg_fill);
        let at = self.position_of(value, along);
        let centre: Pos2 = if self.upright {
            pos2(bar.center().x, at)
        } else {
            pos2(at, bar.center().y)
        };
        if self
            .trailing_fill
            .unwrap_or_else(|| ui.visuals().slider_trailing_fill)
        {
            let mut trail = bar;
            if self.upright {
                trail.min.y = centre.y - f32::from(corner.se);
            } else {
                trail.max.x = centre.x + f32::from(corner.nw);
            }
            painter.rect_filled(trail, corner, ui.visuals().selection.bg_fill);
        }
        let radius = self.handle_radius(&rect);
        match self.handle {
            None => {
                painter.add(egui::epaint::CircleShape {
                    center: centre,
                    radius: radius + look.expansion,
                    fill: look.bg_fill,
                    stroke: look.fg_stroke,
                });
            }
            Some(aspect) => {
                let half = if self.upright {
                    vec2(radius, radius * aspect)
                } else {
                    vec2(radius * aspect, radius)
                } + egui::Vec2::splat(look.expansion);
                painter.rect(
                    Rect::from_center_size(centre, 2.0 * half),
                    look.corner_radius,
                    look.bg_fill,
                    look.fg_stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    /// The arrows along the rail step it, by `step` or by a point's worth.
    fn keys(&mut self, ui: &egui::Ui, response: &egui::Response, along: Rangef) {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: !self.upright,
                    vertical_arrows: self.upright,
                    ..Default::default()
                },
            );
        });
        let (less, more) = if self.upright {
            (Key::ArrowUp, Key::ArrowDown)
        } else {
            (Key::ArrowLeft, Key::ArrowRight)
        };
        let step =
            ui.input(|input| input.num_presses(more) as f32 - input.num_presses(less) as f32);
        if step == 0.0 {
            return;
        }
        let was = self.get();
        let from = self.position_of(was, along) + step;
        let mut value = match self.step {
            Some(each) => was + f64::from(step) * each,
            None if self.smart_aim => egui::emath::smart_aim::best_in_range_f64(
                self.value_at(from - 0.49, along),
                self.value_at(from + 0.49, along),
            ),
            None => self.value_at(from, along),
        };
        // Past the next number the decimals can say, or rounding undoes it.
        if let Some(most) = self.max_decimals {
            let least = 1.0 / 10.0_f64.powi(i32::try_from(most).unwrap_or(i32::MAX)) * 1.001;
            value = if value > was {
                value.max(was + least)
            } else if value < was {
                value.min(was - least)
            } else {
                value
            };
        }
        self.set(value);
    }

    /// The number beside the rail, dragged at the rail's own rate.
    #[allow(clippy::float_cmp, reason = "a value written, not one measured")]
    fn number(&mut self, ui: &mut egui::Ui, along: Rangef) -> egui::Response {
        let keyed = ui.input(|input| {
            input.num_presses(Key::ArrowUp) + input.num_presses(Key::ArrowRight)
                != input.num_presses(Key::ArrowDown) + input.num_presses(Key::ArrowLeft)
        });
        let speed = match (self.step, keyed) {
            (Some(step), true) => step,
            _ => self.drag_speed.unwrap_or_else(|| self.gradient(along)),
        };
        let mut value = self.get();
        let (range, clamp_existing) = match self.clamping {
            Clamping::Never => (f64::NEG_INFINITY..=f64::INFINITY, false),
            Clamping::Edits => (self.range.clone(), false),
            Clamping::Always => (self.range.clone(), true),
        };
        let response = DragNumber {
            value: &mut value,
            speed,
            range,
            clamp_existing,
            prefix: self.prefix,
            suffix: self.suffix,
            min_decimals: self.min_decimals,
            max_decimals: self.max_decimals,
            spelling: self.spelling,
            update_while_editing: self.update_while_editing,
        }
        .show(ui);
        if value != self.get() {
            self.set(value);
        }
        response
    }

    /// How much the value moves for a point of rail where the handle is.
    fn gradient(&self, along: Rangef) -> f64 {
        let at = self.position_of(self.get(), along);
        self.value_at(at + 0.5, along) - self.value_at(at - 0.5, along)
    }
}

/// How many orders of magnitude an unbounded logarithmic end spans.
const ENDLESS: f64 = 10.0;

#[allow(
    clippy::float_cmp,
    reason = "an empty range and the ends are exact cases"
)]
fn from_normalized(normalized: f64, range: RangeInclusive<f64>, scale: &Scale) -> f64 {
    let (min, max) = (*range.start(), *range.end());
    if min.is_nan() || max.is_nan() {
        f64::NAN
    } else if min == max {
        min
    } else if min > max {
        from_normalized(1.0 - normalized, max..=min, scale)
    } else if normalized <= 0.0 {
        min
    } else if normalized >= 1.0 {
        max
    } else if !scale.logarithmic {
        lerp(range, normalized.clamp(0.0, 1.0))
    } else if max <= 0.0 {
        -from_normalized(normalized, -min..=-max, scale)
    } else if 0.0 <= min {
        let (low, high) = log_ends(min, max, scale);
        10.0_f64.powf(lerp(low..=high, normalized))
    } else {
        let cut = zero_cut(min, max);
        if normalized < cut {
            from_normalized(remap(normalized, 0.0..=cut, 0.0..=1.0), min..=0.0, scale)
        } else {
            from_normalized(remap(normalized, cut..=1.0, 0.0..=1.0), 0.0..=max, scale)
        }
    }
}

#[allow(
    clippy::float_cmp,
    reason = "an empty range and the ends are exact cases"
)]
fn to_normalized(value: f64, range: RangeInclusive<f64>, scale: &Scale) -> f64 {
    let (min, max) = (*range.start(), *range.end());
    if min.is_nan() || max.is_nan() {
        f64::NAN
    } else if min == max {
        // An empty range shows its handle in the middle.
        0.5
    } else if min > max {
        1.0 - to_normalized(value, max..=min, scale)
    } else if value <= min {
        0.0
    } else if value >= max {
        1.0
    } else if !scale.logarithmic {
        remap_clamp(value, range, 0.0..=1.0)
    } else if max <= 0.0 {
        to_normalized(-value, -min..=-max, scale)
    } else if 0.0 <= min {
        let (low, high) = log_ends(min, max, scale);
        remap_clamp(value.log10(), low..=high, 0.0..=1.0)
    } else {
        let cut = zero_cut(min, max);
        if value < 0.0 {
            remap(to_normalized(value, min..=0.0, scale), 0.0..=1.0, 0.0..=cut)
        } else {
            remap(to_normalized(value, 0.0..=max, scale), 0.0..=1.0, cut..=1.0)
        }
    }
}

/// The ends of a non-negative logarithmic range, as powers of ten: a zero or
/// an infinity stands in at `smallest_positive` or `largest_finite`.
#[allow(clippy::float_cmp, reason = "zero and infinity are exact ends")]
fn log_ends(min: f64, max: f64, scale: &Scale) -> (f64, f64) {
    if min == 0.0 && max == f64::INFINITY {
        (scale.smallest_positive.log10(), ENDLESS)
    } else if min == 0.0 {
        if scale.smallest_positive < max {
            (scale.smallest_positive.log10(), max.log10())
        } else {
            (max.log10() - ENDLESS, max.log10())
        }
    } else if max == f64::INFINITY {
        if min < scale.largest_finite {
            (min.log10(), scale.largest_finite.log10())
        } else {
            (min.log10(), min.log10() + ENDLESS)
        }
    } else {
        (min.log10(), max.log10())
    }
}

/// Where zero sits on a logarithmic rail that crosses it: the two sides
/// share it by how many orders of magnitude each spans.
#[allow(clippy::float_cmp, reason = "an infinite end is an exact case")]
fn zero_cut(min: f64, max: f64) -> f64 {
    let below = if min == f64::NEG_INFINITY {
        ENDLESS
    } else {
        min.abs().log10().abs()
    };
    let above = if max == f64::INFINITY {
        ENDLESS
    } else {
        max.log10().abs()
    };
    below / (below + above)
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "the ends and the middle of a rail are exact"
)]
mod tests {
    use super::{Scale, from_normalized, to_normalized};

    const LINEAR: Scale = Scale {
        logarithmic: false,
        smallest_positive: 1e-6,
        largest_finite: f64::INFINITY,
    };
    const LOG: Scale = Scale {
        logarithmic: true,
        ..LINEAR
    };

    #[test]
    fn a_linear_rail_maps_its_ends_and_its_middle() {
        assert_eq!(from_normalized(0.5, 0.0..=10.0, &LINEAR), 5.0);
        assert_eq!(to_normalized(2.5, 0.0..=10.0, &LINEAR), 0.25);
        assert_eq!(to_normalized(4.0, 4.0..=4.0, &LINEAR), 0.5);
    }

    #[test]
    fn a_logarithmic_rail_gives_each_power_of_ten_the_same_length() {
        let at = |v| to_normalized(v, 1.0..=1000.0, &LOG);
        assert!((at(10.0) - 1.0 / 3.0).abs() < 1e-9);
        assert!((at(100.0) - 2.0 / 3.0).abs() < 1e-9);
        let back = from_normalized(
            to_normalized(42.0, -100.0..=1000.0, &LOG),
            -100.0..=1000.0,
            &LOG,
        );
        assert!((back - 42.0).abs() < 1e-6, "{back}");
    }
}
