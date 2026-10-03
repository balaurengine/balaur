//! The kinds that hold a number between two ends: the slider, the number
//! field and the progress bar, each egui's own widget with the options the
//! `widget` component states.

use egui::{Color32, pos2, vec2};

use crate::vocabulary::words as w;
use crate::widget::arrange::solved_of;
use crate::widget::layer::{Edit, Painting};
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
    let mut slider = egui::Slider::new(&mut value, low..=high)
        .show_value(o.show_value)
        .logarithmic(o.logarithmic)
        .smart_aim(o.smart_aim)
        .trailing_fill(o.trailing_fill)
        .update_while_editing(o.update_while_editing)
        .clamping(match o.clamp.as_str() {
            w::EDITS => egui::SliderClamping::Edits,
            w::NEVER => egui::SliderClamping::Never,
            _ => egui::SliderClamping::Always,
        });
    if upright {
        slider = slider.vertical();
    }
    if widget.step > 0.0 {
        slider = slider.step_by(f64::from(widget.step));
    }
    if o.smallest_positive > 0.0 {
        slider = slider.smallest_positive(f64::from(o.smallest_positive));
    }
    if o.largest_finite > 0.0 {
        slider = slider.largest_finite(f64::from(o.largest_finite));
    }
    if o.drag_speed > 0.0 {
        slider = slider.drag_value_speed(f64::from(o.drag_speed));
    }
    if let Ok(decimals) = usize::try_from(o.decimals) {
        slider = slider.fixed_decimals(decimals);
    }
    if o.handle == w::RECT {
        slider = slider.handle_shape(egui::style::HandleShape::Rect {
            aspect_ratio: o.handle_aspect,
        });
    }
    if !o.prefix.is_empty() {
        slider = slider.prefix(format!("{} ", o.prefix));
    }
    if !widget.suffix.is_empty() {
        slider = slider.suffix(format!(" {}", widget.suffix));
    }
    slider = match o.number_format.as_str() {
        w::BINARY => slider.binary(1, false),
        w::OCTAL => slider.octal(1, false),
        w::HEX => slider.hexadecimal(1, false, false),
        _ => slider,
    };
    let response = ui.add(slider);
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
    let mut drag = egui::DragValue::new(&mut value).update_while_editing(o.update_while_editing);
    // The letter a vector row puts before each number, which is the one thing
    // a drag value shows that is not the number itself.
    if !o.prefix.is_empty() {
        drag = drag.prefix(format!("{} ", o.prefix));
    }
    if !widget.suffix.is_empty() {
        drag = drag.suffix(format!(" {}", widget.suffix));
    }
    let bounded = widget.max > widget.min && (widget.min, widget.max) != (0.0, 1.0);
    if bounded {
        drag = drag
            .range(widget.min..=widget.max)
            .clamp_existing_to_range(o.clamp_existing);
    }
    if o.drag_speed > 0.0 {
        drag = drag.speed(o.drag_speed);
    }
    if let Ok(decimals) = usize::try_from(o.decimals) {
        drag = drag.fixed_decimals(decimals);
    }
    drag = match o.number_format.as_str() {
        w::BINARY => drag.binary(1, false),
        w::OCTAL => drag.octal(1, false),
        w::HEX => drag.hexadecimal(1, false, false),
        _ => drag,
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
        let mark = |glyph: &str| egui::Button::new(egui::RichText::new(glyph).size(each - 2.0));
        let up = egui::Rect::from_min_size(steps.min, vec2(wide, each));
        let down = egui::Rect::from_min_size(
            pos2(steps.left(), steps.top() + each + 1.0),
            vec2(wide, each),
        );
        if ui.put(up, mark("⏶")).clicked() {
            stepped = Some(1.0);
        }
        if ui.put(down, mark("⏷")).clicked() {
            stepped = Some(-1.0);
        }
        ui.advance_cursor_after_rect(egui::Rect::from_min_size(
            full.min,
            vec2(full.width(), high),
        ));
        inner
    });
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
    // egui repaints an animated bar while it is on screen and short of full,
    // which a sleeping loop hears through its repaint callback.
    let mut bar = egui::ProgressBar::new(fraction)
        .desired_width(if want.x > 0.0 {
            want.x
        } else {
            ui.available_width().min(160.0)
        })
        .desired_height(height)
        .animate(widget.egui.animate);
    let style = at.style_of(widget);
    if let Some(fill) = style.fill {
        bar = bar.fill(fill);
    }
    if let Some(radius) = style.radius {
        bar = bar.corner_radius(egui::CornerRadius::same(radius.clamp(0.0, 255.0) as u8));
    }
    if widget.egui.show_percentage {
        bar = bar.show_percentage();
    } else if !caption.is_empty() {
        bar = bar.text(rich(
            caption,
            font,
            color,
            at.slant(index),
            &widget.text_look,
        ));
    }
    ui.add(bar);
}
