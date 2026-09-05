//! The widget kinds past the first nine: the controls a settings screen is
//! made of, the containers a shop is laid out with, and the nine-patch
//! that dresses both. Each is egui's own widget where egui has one, drawn
//! from the scene's values and reporting back through the frame's edits.

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use egui::{Color32, Rect, Sense, Stroke, TextureId, pos2, vec2};

use crate::widget_arrange::{Axis, box_of, lay_out, padding_of, record_measure, record_rect};
use crate::widget_layer::{Edit, Painting, draw_one, rgba_color};
use crate::widget_measure::Measure;

/// A ticked box with a caption. The tick lives on the widget: the click is
/// reported like a button's and the next tick flips `checked`.
pub(crate) fn check(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let mut on = widget.checked;
    let label = egui::RichText::new(caption).font(font.clone()).color(color);
    let response = ui.add(egui::Checkbox::new(&mut on, label));
    if response.clicked() {
        at.clicked.push(placed.entity);
    }
    if at.focused == Some(placed.entity) {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            4.0,
            Stroke::new(2.0, color),
            egui::StrokeKind::Outside,
        );
    }
}

/// One of the widget's `options`, chosen from a list that drops down.
pub(crate) fn dropdown(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let entity = placed.entity;
    let want = box_of(widget, at.assigned, at.scale);
    let mut chosen = widget.text.clone();
    let mut combo = egui::ComboBox::from_id_salt(("balaur-dropdown", entity))
        .selected_text(egui::RichText::new(&chosen).font(font.clone()).color(color));
    if want.x > 0.0 {
        combo = combo.width(want.x);
    }
    combo.show_ui(ui, |ui| {
        for option in &widget.options {
            let label = egui::RichText::new(option).font(font.clone()).color(color);
            ui.selectable_value(&mut chosen, option.clone(), label);
        }
    });
    if chosen != widget.text {
        at.edits.push((entity, Edit::Choice(chosen)));
    }
}

/// A number dragged between `min` and `max`. egui draws it; the value it
/// reports lands on the widget next tick, which is also when `on_change`
/// hears it.
pub(crate) fn slider(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let (low, high) = (widget.min, widget.max.max(widget.min));
    let mut value = widget.value.clamp(low, high);
    let want = box_of(widget, at.assigned, at.scale);
    let width = if want.x > 0.0 {
        want.x
    } else {
        ui.available_width().min(160.0 * at.scale)
    };
    ui.spacing_mut().slider_width = width;
    let mut slider = egui::Slider::new(&mut value, low..=high).show_value(false);
    if widget.step > 0.0 {
        slider = slider.step_by(f64::from(widget.step));
    }
    let response = ui.add(slider);
    if response.changed() {
        at.edits.push((placed.entity, Edit::Value(value)));
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
    let want = box_of(widget, at.assigned, at.scale);
    let mut bar = egui::ProgressBar::new(fraction).desired_width(if want.x > 0.0 {
        want.x
    } else {
        ui.available_width().min(160.0 * at.scale)
    });
    if want.y > 0.0 {
        bar = bar.desired_height(want.y);
    }
    let style = at.theme.style(&widget.kind);
    if let Some(fill) = style.fill {
        bar = bar.fill(fill);
    }
    if !caption.is_empty() {
        bar = bar.text(egui::RichText::new(caption).font(font.clone()).color(color));
    }
    ui.add(bar);
}

/// A line across the parent's direction, in the theme's stroke.
pub(crate) fn separator(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let widget = &at.arena[index].widget;
    let style = at.theme.style(&widget.kind);
    if let Some(color) = style.stroke {
        ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::new(style.stroke_width, color);
    }
    ui.add(egui::Separator::default().spacing(6.0 * at.scale));
}

/// A header that shows or hides the children under it. The header is a
/// button by another shape: clicking it reports an `Open` edit, and focus
/// lands on it as on a button.
pub(crate) fn fold(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let (entity, open) = (placed.entity, placed.widget.open);
    let widget = placed.widget.clone();
    let style = at.theme.style(&widget.kind);
    let scale = at.scale;
    let pad = padding_of(&widget, &style, scale);
    let mark = if open { "▾" } else { "▸" };
    let text = egui::RichText::new(format!("{mark} {caption}"))
        .font(font.clone())
        .color(color);
    let header = ui.add(egui::Label::new(text).sense(Sense::click()));
    if header.clicked() {
        at.edits.push((entity, Edit::Open(!open)));
    }
    if at.focused == Some(entity) {
        ui.painter().rect_stroke(
            header.rect.expand(2.0),
            4.0,
            Stroke::new(2.0, color),
            egui::StrokeKind::Outside,
        );
    }
    if !open {
        return;
    }
    let room = ui.available_rect_before_wrap();
    let body = Rect::from_min_max(pos2(room.min.x + pad, room.min.y), room.max);
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body));
    let held = std::mem::replace(&mut at.bounds, egui::Vec2::ZERO);
    lay_out(&mut inner, at, index, Axis::Column);
    at.bounds = held;
    ui.advance_cursor_after_rect(inner.min_rect());
}

/// Children in rows of `columns`, every cell as big as the biggest child
/// and, given a width, sharing it equally.
pub(crate) fn grid(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let children = placed.children.clone();
    if children.is_empty() {
        return;
    }
    let widget = placed.widget.clone();
    let scale = at.scale;
    let columns = (widget.columns.max(1)) as usize;
    let gap = widget.gap * scale;
    let style = at.theme.style(&widget.kind);
    let pad = padding_of(&widget, &style, scale);
    let box_size = box_of(&widget, at.assigned, scale);
    let mut cell = egui::Vec2::ZERO;
    {
        let mut measure = Measure::new(at.eng, at.arena, ui, scale);
        for child in &children {
            cell = cell.max(measure.of(*child, &at.theme));
        }
    }
    if box_size.x > 0.0 {
        let shared = (box_size.x - 2.0 * pad - gap * (columns as f32 - 1.0)) / columns as f32;
        cell.x = shared.max(0.0);
    }
    let origin = ui.available_rect_before_wrap().min + egui::Vec2::splat(pad);
    let mut extent = egui::Vec2::ZERO;
    for (slot, child) in children.iter().enumerate() {
        let (column, row) = ((slot % columns) as f32, (slot / columns) as f32);
        let min = origin + vec2(column * (cell.x + gap), row * (cell.y + gap));
        let rect = Rect::from_min_size(min, cell);
        place_child(ui, at, *child, rect, cell);
        extent = extent.max(rect.max - origin);
    }
    let taken = Rect::from_min_size(
        origin - egui::Vec2::splat(pad),
        extent + egui::Vec2::splat(pad * 2.0),
    );
    ui.allocate_rect(taken, Sense::hover());
}

/// Children left to right at their own size, wrapping to a new line when
/// the next one would run past the box.
pub(crate) fn flow(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let children = placed.children.clone();
    if children.is_empty() {
        return;
    }
    let widget = placed.widget.clone();
    let scale = at.scale;
    let gap = widget.gap * scale;
    let style = at.theme.style(&widget.kind);
    let pad = padding_of(&widget, &style, scale);
    let box_size = box_of(&widget, at.assigned, scale);
    let room = ui.available_rect_before_wrap();
    let width = if box_size.x > 0.0 {
        box_size.x
    } else {
        room.width()
    } - 2.0 * pad;
    let sizes: Vec<egui::Vec2> = {
        let mut measure = Measure::new(at.eng, at.arena, ui, scale);
        children
            .iter()
            .map(|child| measure.of(*child, &at.theme))
            .collect()
    };
    let origin = room.min + egui::Vec2::splat(pad);
    let mut cursor = egui::Vec2::ZERO;
    let mut line_height = 0.0f32;
    let mut extent = egui::Vec2::ZERO;
    for (child, size) in children.iter().zip(sizes) {
        if size == egui::Vec2::ZERO {
            continue;
        }
        if cursor.x > 0.0 && cursor.x + size.x > width {
            cursor = vec2(0.0, cursor.y + line_height + gap);
            line_height = 0.0;
        }
        let rect = Rect::from_min_size(origin + cursor, size);
        place_child(ui, at, *child, rect, egui::Vec2::ZERO);
        cursor.x += size.x + gap;
        line_height = line_height.max(size.y);
        extent = extent.max(rect.max - origin);
    }
    let taken = Rect::from_min_size(room.min, extent + egui::Vec2::splat(pad * 2.0));
    ui.allocate_rect(taken, Sense::hover());
}

/// Draw one child of a grid or a flow in the rect it was given.
fn place_child(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    child: usize,
    rect: Rect,
    assigned: egui::Vec2,
) {
    let entity = at.arena[child].entity;
    let restore = at.assigned;
    at.assigned = assigned;
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    draw_one(&mut child_ui, at, child);
    at.assigned = restore;
    record_measure(entity, child_ui.min_rect().size());
    record_rect(entity, rect);
}

/// The dimmed, deaf screen under a dialog: one full-surface area that takes
/// every click so nothing behind the dialog hears them.
pub(crate) fn dialog_backdrop(ctx: &egui::Context, entity: Entity, area: Rect) {
    egui::Area::new(egui::Id::new(("balaur-dialog-backdrop", entity)))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .interactable(true)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(area.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(140));
        });
}

/// A picture over a rect with its borders kept at their own size: nine
/// quads, the corners as they are, the edges stretched one way and the
/// middle both. `slice` is in the picture's pixels; the borders are drawn
/// at design scale.
pub(crate) fn nine_patch(
    texture: TextureId,
    native: egui::Vec2,
    rect: Rect,
    slice: [f32; 4],
    scale: f32,
) -> Vec<egui::Shape> {
    let [left, top, right, bottom] = slice;
    let xs = [
        rect.min.x,
        rect.min.x + left * scale,
        rect.max.x - right * scale,
        rect.max.x,
    ];
    let ys = [
        rect.min.y,
        rect.min.y + top * scale,
        rect.max.y - bottom * scale,
        rect.max.y,
    ];
    let us = [
        0.0,
        left / native.x.max(1.0),
        1.0 - right / native.x.max(1.0),
        1.0,
    ];
    let vs = [
        0.0,
        top / native.y.max(1.0),
        1.0 - bottom / native.y.max(1.0),
        1.0,
    ];
    let mut shapes = Vec::with_capacity(9);
    for row in 0..3 {
        for column in 0..3 {
            let piece =
                Rect::from_min_max(pos2(xs[column], ys[row]), pos2(xs[column + 1], ys[row + 1]));
            if piece.width() <= 0.0 || piece.height() <= 0.0 {
                continue;
            }
            let uv =
                Rect::from_min_max(pos2(us[column], vs[row]), pos2(us[column + 1], vs[row + 1]));
            shapes.push(egui::Shape::image(texture, piece, uv, Color32::WHITE));
        }
    }
    shapes
}

/// Fill a reserved plate with a themed nine-patch; a picture that will not
/// load leaves the plate empty rather than taking the frame down.
pub(crate) fn nine_patch_plate(
    ui: &egui::Ui,
    eng: &Engine,
    plate: egui::layers::ShapeIdx,
    path: &str,
    slice: [f32; 4],
    rect: Rect,
    scale: f32,
) {
    let ctx = ui.ctx().clone();
    if let Ok(texture) = crate::widgets::texture_of(eng, &ctx, path) {
        let shapes = nine_patch(texture.id(), texture.size_vec2(), rect, slice, scale);
        ui.painter().set(plate, egui::Shape::Vec(shapes));
    }
}

/// The offset a finger past a scroll's deadzone asks for, or `None` while
/// nothing is dragging that far. The press is remembered with the offset
/// the scroll had then, so the content follows the finger from there.
pub(crate) fn deadzone_drag(
    ui: &egui::Ui,
    eng: &Engine,
    entity: Entity,
    dead: f32,
) -> Option<egui::Vec2> {
    let (down, origin, latest) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
        )
    });
    let state = eng.resource::<crate::UiState>();
    let mut state = state.borrow_mut();
    let key = entity.to_bits().get();
    if !down {
        state.scroll_drags.remove(&key);
        return None;
    }
    let (origin, latest) = (origin?, latest?);
    if let std::collections::hash_map::Entry::Vacant(slot) = state.scroll_drags.entry(key) {
        let inside =
            crate::widget_arrange::drawn_at(entity).is_some_and(|rect| rect.contains(origin));
        if !inside {
            return None;
        }
        let id = ui.make_persistent_id(("balaur-scroll", entity));
        let offset =
            egui::scroll_area::State::load(ui.ctx(), id).map_or(egui::Vec2::ZERO, |s| s.offset);
        slot.insert((origin, offset));
    }
    let (start, base) = state.scroll_drags[&key];
    let travelled = latest - start;
    if travelled.length() < dead {
        return None;
    }
    Some((base - travelled).max(egui::Vec2::ZERO))
}

/// The colour a kind's text is drawn in, for a control egui paints itself.
#[allow(dead_code, reason = "kept beside the kinds that will take a tint")]
pub(crate) fn tint_of(widget: &crate::widget_layer::Widget) -> Color32 {
    rgba_color(widget.text_color)
}
