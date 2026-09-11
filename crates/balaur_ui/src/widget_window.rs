//! The `window` kind: a panel with a title bar, moved by dragging the bar and
//! shut by its cross. What Godot's `Window` is when the project embeds its
//! subwindows, which it does unless told otherwise.

use egui::{Color32, Sense, Stroke};

use crate::widget_arrange::{Axis, box_of, hold_to, lay_out, padding_of};
use crate::widget_layer::{Edit, Painting};

/// The cross on the title bar.
const CLOSE: &str = "×";

pub(crate) fn window(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let entity = placed.entity;
    let widget = placed.widget.clone();
    if !widget.open {
        return;
    }
    let scale = at.scale;
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style, scale);
    let box_size = box_of(&widget, at.assigned, scale);
    let plate = ui.painter().add(egui::Shape::Noop);
    let min = (box_size - egui::Vec2::splat(pad * 2.0)).max(egui::Vec2::ZERO);
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(ui.max_rect().shrink(pad)));
    hold_to(&mut inner, min);
    let (title, close) = inner
        .horizontal(|bar| {
            let text = egui::RichText::new(caption).font(font.clone()).color(color);
            let title = bar.add(egui::Label::new(text).sense(Sense::drag()));
            let cross = egui::RichText::new(CLOSE).font(font.clone()).color(color);
            // On the far edge, where a window keeps it.
            let close = bar
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |edge| {
                    edge.add(egui::Button::new(cross).frame(false))
                })
                .inner;
            (title, close)
        })
        .inner;
    if title.dragged() {
        let moved = title.drag_delta() / scale;
        at.edits.push((entity, Edit::Moved([moved.x, moved.y])));
    }
    if close.clicked() {
        at.edits.push((entity, Edit::Open(false)));
    }
    let held = std::mem::replace(&mut at.bounds, min);
    lay_out(&mut inner, at, index, Axis::Column);
    at.bounds = held;
    let background = inner.min_rect().expand(pad);
    ui.painter().set(
        plate,
        egui::epaint::RectShape::new(
            background,
            egui::CornerRadius::same(style.radius.map_or(8.0, |r| r * scale) as u8),
            style.fill.unwrap_or(Color32::from_black_alpha(160)),
            style
                .stroke
                .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_px(), c)),
            egui::StrokeKind::Inside,
        ),
    );
    ui.advance_cursor_after_rect(background);
}

/// Which way `x` and `y` run for an anchor, so a drag moves the window the
/// way the pointer went: an offset from a right or bottom edge runs inward.
pub(crate) fn drag_signs(anchor: &str) -> (f32, f32) {
    use crate::vocabulary::words as w;
    let x = if matches!(anchor, w::TOP_RIGHT | w::BOTTOM_RIGHT | w::CENTER_RIGHT | w::FILL_RIGHT) {
        -1.0
    } else {
        1.0
    };
    let y = if matches!(anchor, w::BOTTOM_LEFT | w::BOTTOM_RIGHT | w::CENTER_BOTTOM | w::FILL_BOTTOM) {
        -1.0
    } else {
        1.0
    };
    (x, y)
}
