//! The kinds that hold a number between two ends: the slider, the number
//! field and the progress bar.

use egui::{Color32, pos2, vec2};

use crate::vocabulary::words as w;
use crate::widget::arrange::solved_of;
use crate::widget::drag::{DragNumber, Spelling};
use crate::widget::layer::{Edit, Painting};
use crate::widget::slide::{Clamping, Scale, Slide};
use crate::widget::theme::rich;

/// How much of a `number_field`'s box its two arrows take, gap included.
const ARROWS: f32 = 18.0;

/// A number dragged between `min` and `max`. egui draws it; the value it
/// reports lands on the widget next tick, which is also when `on_change`
/// hears it.
pub(crate) fn slider(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let o = &widget.egui;
    let (low, high) = (widget.min, widget.max.max(widget.min));
    // Held in range up front only where it always is: under `edits` or
    // `never` a number the scene set outside is shown as set.
    let mut value = if o.clamp == w::EDITS || o.clamp == w::NEVER {
        widget.value
    } else {
        widget.value.clamp(low, high)
    };
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    let upright = widget.axis == w::VERTICAL;
    let along = if upright { want.y } else { want.x };
    let length = if along > 0.0 {
        along
    } else if upright {
        ui.available_height().min(160.0)
    } else {
        ui.available_width().min(160.0)
    };
    // The shown number sits after the track inside the same box.
    let shown = value_room(ui.painter(), widget);
    ui.spacing_mut().slider_width = (length - shown).max(16.0);
    let decimals = usize::try_from(o.decimals).ok();
    let (prefix, suffix) = affixes(&o.prefix, &widget.suffix);
    let mut number = f64::from(value);
    let response = Slide {
        value: &mut number,
        range: f64::from(low)..=f64::from(high),
        scale: Scale {
            logarithmic: o.logarithmic,
            smallest_positive: if o.smallest_positive > 0.0 {
                f64::from(o.smallest_positive)
            } else {
                1e-6
            },
            largest_finite: if o.largest_finite > 0.0 {
                f64::from(o.largest_finite)
            } else {
                f64::INFINITY
            },
        },
        clamping: match o.clamp.as_str() {
            w::EDITS => Clamping::Edits,
            w::NEVER => Clamping::Never,
            _ => Clamping::Always,
        },
        smart_aim: o.smart_aim,
        show_value: o.show_value,
        upright,
        prefix: &prefix,
        suffix: &suffix,
        step: (widget.step > 0.0).then(|| f64::from(widget.step)),
        drag_speed: (o.drag_speed > 0.0).then(|| f64::from(o.drag_speed)),
        min_decimals: decimals.unwrap_or(0),
        max_decimals: decimals,
        spelling: Spelling::of(&o.number_format),
        trailing_fill: Some(o.trailing_fill),
        handle: if o.handle == w::RECT {
            Some(o.handle_aspect)
        } else {
            match ui.style().visuals.handle_shape {
                egui::style::HandleShape::Rect { aspect_ratio } => Some(aspect_ratio),
                egui::style::HandleShape::Circle => None,
            }
        },
        update_while_editing: o.update_while_editing,
    }
    .show(ui);
    value = number as f32;
    if response.changed() {
        at.edits.push((placed.entity, Edit::Value(value)));
    }
    // A drag lets go, or a key moved it with no drag at all.
    if response.drag_stopped() || (response.changed() && !response.dragged()) {
        at.edits.push((placed.entity, Edit::Committed(value)));
    }
}

/// What a `slider`'s shown number takes along it, the space before it
/// included: its widest end with `prefix` and `suffix` across, a control's
/// height down. Nothing where it shows none.
pub(crate) fn value_room(painter: &egui::Painter, widget: &crate::widget::node::Widget) -> f32 {
    let o = &widget.egui;
    if !o.show_value {
        return 0.0;
    }
    let style = painter.ctx().global_style();
    let spacing = &style.spacing;
    if widget.axis == w::VERTICAL {
        return spacing.interact_size.y + spacing.item_spacing.y;
    }
    let digits = |n: f32| match usize::try_from(o.decimals) {
        Ok(decimals) => format!("{n:.decimals$}"),
        Err(_) => format!("{n}"),
    };
    let pieces = [o.prefix.is_empty(), widget.suffix.is_empty()]
        .iter()
        .filter(|empty| !**empty)
        .count() as f32;
    // The prefix and suffix are pieces of their own, with egui's gap before
    // or after the number.
    let widest = [widget.min, widget.max]
        .map(|end| format!("{} {} {}", o.prefix, digits(end), widget.suffix))
        .into_iter()
        .map(|text| {
            let font = egui::TextStyle::Body.resolve(&style);
            painter.layout_no_wrap(text, font, Color32::WHITE).size().x
        })
        .fold(0.0, f32::max)
        + pieces * spacing.icon_spacing;
    (widest + spacing.button_padding.x * 2.0).max(spacing.interact_size.x) + spacing.item_spacing.x
}

/// A number dragged sideways, or typed into after a click. `SpinBox` in a
/// Godot scene; the control an inspector row is mostly made of.
///
/// `min` and `max` are the slider's, and so default to 0 and 1. A position or
/// a scale is neither, and most of what an inspector shows runs free, so that
/// default pair reads here as no bounds at all: any other pair binds.
///
/// A range binds what the reader drags, types or steps to. A number outside
/// it is shown as the scene holds it, never clamped and reported as an edit.
pub(crate) fn drag_value(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let mut value = widget.value;
    let o = &widget.egui;
    let bounded = widget.max > widget.min && (widget.min, widget.max) != (0.0, 1.0);
    let decimals = usize::try_from(o.decimals).ok();
    let (prefix, suffix) = affixes(&o.prefix, &widget.suffix);
    let mut number = f64::from(value);
    let drag = DragNumber {
        value: &mut number,
        speed: if o.drag_speed > 0.0 {
            f64::from(o.drag_speed)
        } else {
            1.0
        },
        range: if bounded {
            f64::from(widget.min)..=f64::from(widget.max)
        } else {
            f64::NEG_INFINITY..=f64::INFINITY
        },
        clamp_existing: bounded && o.clamp_existing,
        prefix: &prefix,
        suffix: &suffix,
        min_decimals: decimals.unwrap_or(0),
        max_decimals: decimals,
        spelling: Spelling::of(&o.number_format),
        update_while_editing: o.update_while_editing,
    };
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    if want.x > 0.0 {
        // With arrows, the number is told what is left of the stated box, so
        // the pair sits inside it rather than in the next widget's.
        ui.spacing_mut().interact_size.x = if widget.arrows {
            (want.x - ARROWS).max(24.0)
        } else {
            want.x
        };
    }
    let mut stepped = None;
    let look = at.look(index);
    let response = ui.scope(|ui| {
        ui.style_mut().override_font_id = Some(font.clone());
        crate::widget::theme::dress(ui, &look.style, color);
        if !widget.arrows {
            return ui.add(drag);
        }
        // The box split by hand: egui's layouts size a `DragValue` from the
        // room they have, so in a row the steps land in the next widget's.
        let full = ui.available_rect_before_wrap();
        let high = full.height().min(want.y.max(18.0));
        let wide = ARROWS - 4.0;
        let steps =
            egui::Rect::from_min_size(pos2(full.right() - wide, full.top()), vec2(wide, high));
        let number =
            egui::Rect::from_min_max(full.min, pos2(steps.left() - 4.0, full.top() + high));
        let inner = ui.put(number, drag);
        let each = (high - 1.0) / 2.0;
        let up = egui::Rect::from_min_size(steps.min, vec2(wide, each));
        let down = egui::Rect::from_min_size(
            pos2(steps.left(), steps.top() + each + 1.0),
            vec2(wide, each),
        );
        if arrow(ui, up, "⏶", each - 2.0) {
            stepped = Some(1.0);
        }
        if arrow(ui, down, "⏷", each - 2.0) {
            stepped = Some(-1.0);
        }
        ui.advance_cursor_after_rect(egui::Rect::from_min_size(
            full.min,
            vec2(full.width(), high),
        ));
        inner
    });
    value = number as f32;
    if let Some(way) = stepped {
        let step = if widget.step > 0.0 { widget.step } else { 1.0 };
        let moved = value + way * step;
        value = if bounded {
            moved.clamp(widget.min, widget.max)
        } else {
            moved
        };
        at.edits.push((placed.entity, Edit::Value(value)));
        at.edits.push((placed.entity, Edit::Committed(value)));
    } else if response.inner.changed() {
        at.edits.push((placed.entity, Edit::Value(value)));
    }
    // A drag lets go, or the number typed in is left.
    if response.inner.drag_stopped() || response.inner.lost_focus() {
        at.edits.push((placed.entity, Edit::Committed(value)));
    }
}

/// A bar's track and its fill to `fraction`, with egui's shimmer and
/// spinner while `animate` holds and it is short of full.
fn paint_bar(
    ui: &egui::Ui,
    outer: egui::Rect,
    fraction: f32,
    style: &crate::widget::theme::Style,
    animate: bool,
) {
    // Animated while short of full: egui is asked for the next pass, which a
    // sleeping loop hears through its repaint callback.
    let animate = animate && fraction < 1.0;
    if animate {
        ui.ctx().request_repaint();
    }
    let visuals = ui.visuals().clone();
    let half = outer.height() / 2.0;
    let radius = style.radius.map_or_else(
        || egui::CornerRadius::from(half),
        |radius| egui::CornerRadius::same(radius.clamp(0.0, 255.0) as u8),
    );
    let painter = ui.painter();
    painter.rect_filled(outer, radius, visuals.extreme_bg_color);
    let least = 2.0 * f32::from(radius.sw.max(radius.nw)).min(half);
    let filled = egui::Rect::from_min_size(
        outer.min,
        vec2((outer.width() * fraction).max(least), outer.height()),
    );
    let shade = if animate {
        let time = ui.input(|i| i.time);
        egui::lerp(0.7..=1.0, time.cos().abs()) as f32
    } else {
        1.0
    };
    let fill = style.fill.unwrap_or(visuals.selection.bg_fill);
    painter.rect_filled(
        filled,
        radius,
        Color32::from(egui::Rgba::from(fill) * shade),
    );
    if animate && style.radius.is_none() {
        let time = ui.input(|i| i.time);
        let start = time * std::f64::consts::TAU;
        let end = start + 240f64.to_radians() * time.sin();
        let reach = half - 2.0;
        let points: Vec<egui::Pos2> = (0..20)
            .map(|i| {
                let angle = egui::lerp(start..=end, f64::from(i) / 20.0);
                let (sin, cos) = angle.sin_cos();
                filled.right_center() + reach * vec2(cos as f32, sin as f32) + vec2(-half, 0.0)
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(2.0, visuals.text_color()),
        ));
    }
}

/// The words before and after the number, a space between each and it.
fn affixes(prefix: &str, suffix: &str) -> (String, String) {
    let before = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix} ")
    };
    let after = if suffix.is_empty() {
        String::new()
    } else {
        format!(" {suffix}")
    };
    (before, after)
}

/// One of a `number_field`'s two steps: a glyph in `rect` that lights up
/// under the pointer, true on the pass it was clicked.
fn arrow(ui: &mut egui::Ui, rect: egui::Rect, glyph: &str, size: f32) -> bool {
    // Put as egui puts a button: centred on the rect, and as tall as a
    // control even where the rect is shorter, so the pair overlaps.
    let response = ui.put(rect, |ui: &mut egui::Ui| {
        crate::widget::button::sensor(
            ui,
            egui::Vec2::ZERO,
            egui::Sense::click(),
            egui::WidgetType::Button,
            glyph,
        )
    });
    if ui.is_rect_visible(response.rect) {
        let look = ui.style().interact(&response);
        let painter = ui.painter();
        painter.rect(
            response.rect.expand(look.expansion),
            look.corner_radius,
            look.weak_bg_fill,
            look.bg_stroke,
            egui::StrokeKind::Inside,
        );
        // In the field's own face, at the arrow's size, as egui sized a
        // button's text under the field's font.
        let family = ui
            .style()
            .override_font_id
            .as_ref()
            .map_or(egui::FontFamily::Proportional, |font| font.family.clone());
        painter.text(
            response.rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::new(size, family),
            look.text_color(),
        );
    }
    response.clicked()
}

/// A bar filled to `value`, with the caption over it.
pub(crate) fn progress(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let widget = &at.arena[index].widget;
    let span = (widget.max - widget.min).abs().max(f32::EPSILON);
    let fraction = ((widget.value - widget.min) / span).clamp(0.0, 1.0);
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    // A line of the caption's face with a button's air, as it measured; egui's
    // own height ignores the face and cut the caption off.
    let height = if want.y > 0.0 {
        want.y
    } else {
        ui.fonts_mut(|f| f.row_height(font)) + ui.spacing().button_padding.y * 2.0
    };
    let width = if want.x > 0.0 {
        want.x
    } else {
        ui.available_width().min(160.0)
    };
    let (outer, response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    let shown = if widget.egui.show_percentage {
        Some(format!("{}%", (fraction * 100.0) as usize))
    } else {
        (!caption.is_empty()).then(|| caption.to_owned())
    };
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        let mut info = match &shown {
            Some(text) if !widget.egui.show_percentage => {
                egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, enabled, text)
            }
            _ => egui::WidgetInfo::new(egui::WidgetType::ProgressIndicator),
        };
        info.value = Some((f64::from(fraction) * 100.0).floor());
        info
    });
    if !ui.is_rect_visible(outer) {
        return;
    }
    let style = at.style_of(widget);
    paint_bar(ui, outer, fraction, &style, widget.egui.animate);
    let Some(text) = shown else {
        return;
    };
    let text = if widget.egui.show_percentage {
        egui::WidgetText::from(text)
    } else {
        egui::WidgetText::from(rich(&text, font, color, at.slant(index), &widget.text_look))
    };
    let galley = text.into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let place = outer.left_center() - vec2(0.0, galley.size().y / 2.0)
        + vec2(ui.spacing().item_spacing.x, 0.0);
    let visuals = ui.visuals();
    let ink = visuals
        .override_text_color
        .unwrap_or(visuals.selection.stroke.color);
    ui.painter()
        .with_clip_rect(outer)
        .galley(place, galley, ink);
}
