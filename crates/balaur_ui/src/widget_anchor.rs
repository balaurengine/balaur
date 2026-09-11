//! Where a root widget goes on its surface: a corner, an edge, the middle,
//! the whole of it, or one axis of it. A widget inside a container is placed
//! by the container, so only a root is ever asked.

use egui::{Align2, pos2, vec2};

use crate::vocabulary::words as w;
use crate::widget_layer::Widget;

/// Where a root goes and what box it is handed: `fill` takes the surface
/// less its insets so a container at the root fills the screen, a dialog
/// sits in the middle over the dimmed screen, the rest anchor as before.
pub(crate) fn root_frame(
    widget: &Widget,
    area: egui::Rect,
    scale: f32,
) -> (egui::Pos2, Align2, egui::Vec2, egui::Order) {
    if widget.anchor == w::FILL {
        let inset = widget.inset.map(|v| v * scale);
        let rect = egui::Rect::from_min_max(
            area.min + vec2(inset[0], inset[1]),
            area.max - vec2(inset[2], inset[3]),
        );
        return (
            rect.min,
            Align2::LEFT_TOP,
            rect.size().max(egui::Vec2::ZERO),
            egui::Order::Middle,
        );
    }
    if let Some((pos, align, size)) = wide(widget, area, scale) {
        return (pos, align, size, egui::Order::Middle);
    }
    if widget.kind == w::DIALOG {
        return (
            area.center(),
            Align2::CENTER_CENTER,
            egui::Vec2::ZERO,
            egui::Order::Foreground,
        );
    }
    let (pos, align) = root_placement(widget, area, scale);
    (pos, align, egui::Vec2::ZERO, egui::Order::Middle)
}

/// Where a root's own `anchor`, `x` and `y` put it inside its surface. Only a
/// root is placed this way; every other widget is placed by its container.
fn root_placement(widget: &Widget, area: egui::Rect, scale: f32) -> (egui::Pos2, Align2) {
    let align = match widget.anchor.as_str() {
        w::TOP_RIGHT => Align2::RIGHT_TOP,
        w::BOTTOM_LEFT => Align2::LEFT_BOTTOM,
        w::BOTTOM_RIGHT => Align2::RIGHT_BOTTOM,
        w::CENTER => Align2::CENTER_CENTER,
        w::CENTER_LEFT => Align2::LEFT_CENTER,
        w::CENTER_RIGHT => Align2::RIGHT_CENTER,
        w::CENTER_TOP => Align2::CENTER_TOP,
        w::CENTER_BOTTOM => Align2::CENTER_BOTTOM,
        _ => Align2::LEFT_TOP,
    };
    // The offset runs inward from whichever edge the anchor names, so the
    // position falls out of the alignment rather than repeating it per anchor.
    let inward = |edge, min: f32, mid: f32, max: f32, offset: f32| match edge {
        egui::Align::Min => min + offset,
        egui::Align::Center => mid + offset,
        egui::Align::Max => max - offset,
    };
    let (centre, ox, oy) = (area.center(), widget.x * scale, widget.y * scale);
    let pos = pos2(
        inward(align.x(), area.min.x, centre.x, area.max.x, ox),
        inward(align.y(), area.min.y, centre.y, area.max.y, oy),
    );
    (pos, align)
}

/// A root that fills one axis of its surface and is placed on the other, as
/// Godot's six wide presets are: `fill_top` spans the width along the top,
/// `fill_left` the height down the left, `fill_across` and `fill_down` the
/// middle. The spanning axis is the surface less `inset`; the other is the
/// widget's stated size, or what it measures when it states none.
fn wide(widget: &Widget, area: egui::Rect, scale: f32) -> Option<(egui::Pos2, Align2, egui::Vec2)> {
    let [left, top, right, bottom] = widget.inset.map(|v| v * scale);
    let (ox, oy) = (widget.x * scale, widget.y * scale);
    let across = (area.width() - left - right).max(0.0);
    let down = (area.height() - top - bottom).max(0.0);
    let (tall, broad) = (widget.height * scale, widget.width * scale);
    let centre = area.center();
    Some(match widget.anchor.as_str() {
        w::FILL_TOP => (pos2(area.min.x + left, area.min.y + oy), Align2::LEFT_TOP, vec2(across, tall)),
        w::FILL_BOTTOM => (pos2(area.min.x + left, area.max.y - oy), Align2::LEFT_BOTTOM, vec2(across, tall)),
        w::FILL_ACROSS => (pos2(area.min.x + left, centre.y + oy), Align2::LEFT_CENTER, vec2(across, tall)),
        w::FILL_LEFT => (pos2(area.min.x + ox, area.min.y + top), Align2::LEFT_TOP, vec2(broad, down)),
        w::FILL_RIGHT => (pos2(area.max.x - ox, area.min.y + top), Align2::RIGHT_TOP, vec2(broad, down)),
        w::FILL_DOWN => (pos2(centre.x + ox, area.min.y + top), Align2::CENTER_TOP, vec2(broad, down)),
        _ => return None,
    })
}
