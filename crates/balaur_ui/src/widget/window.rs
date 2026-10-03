//! The `window` kind: a panel with a title bar, moved by dragging the bar and
//! shut by its cross. What Godot's `Window` is when the project embeds its
//! subwindows, which it does unless told otherwise. Its switches are the ones
//! `ui.window` takes, drawn by Balaur rather than by `egui::Window`.

use std::cell::RefCell;

use balaur_core::hecs::Entity;
use egui::{Color32, Sense, Stroke, pos2, vec2};
use rustc_hash::FxHashMap;

use crate::widget::arrange::{Axis, hold_to, lay_out, padding_of, solved_of};
use crate::widget::layer::{Edit, Painting};
use crate::widget::measure::Measure;
use crate::widget::node::Widget;

/// The cross on the title bar.
const CLOSE: &str = "×";
/// The arrow a `collapsible` window folds by, open and folded.
const UNFOLDED: &str = "▾";
const FOLDED_MARK: &str = "▸";
/// How big a square the resize grip takes in the bottom-right corner.
const GRIP: f32 = 12.0;

thread_local! {
    /// Which windows are folded to their bar, by entity: the window's own
    /// state, as egui keeps a collapsing header's, which no property says.
    static FOLDED: RefCell<FxHashMap<u64, bool>> = RefCell::new(FxHashMap::default());
}

/// Whether a window shows only its title bar. A `collapsible` window starts
/// as `default_open` says and keeps what its arrow last made it.
pub(crate) fn folded(entity: Entity, widget: &Widget) -> bool {
    if !widget.egui.collapsible || !widget.header {
        return false;
    }
    FOLDED.with(|held| {
        *held
            .borrow_mut()
            .entry(entity.to_bits().get())
            .or_insert(!widget.egui.default_open)
    })
}

fn set_folded(entity: Entity, now: bool) {
    FOLDED.with(|held| {
        held.borrow_mut().insert(entity.to_bits().get(), now);
    });
    // The layout keeps a folded window's children out of its tree.
    crate::widget::arena::widget_changed(entity);
}

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
    at.shown.push(entity);
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style);
    let folded_now = folded(entity, &widget);
    let mut box_size = solved_of(&widget, &at.style_of(&widget), at.assigned);
    if folded_now {
        box_size.y = 0.0;
    }
    let plate = ui.painter().add(egui::Shape::Noop);
    let min = (box_size - pad.taken()).max(egui::Vec2::ZERO);
    // Top down whatever the parent runs, as a panel's is.
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(pad.inside(ui.max_rect()))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    hold_to(&mut inner, min);
    let bar = {
        let measure = Measure::new(at.eng, at.arena, ui);
        measure.title_bar(index, &at.theme)
    };
    if widget.header {
        let rect = egui::Rect::from_min_size(
            inner.max_rect().min,
            vec2(inner.max_rect().width().max(bar.x), bar.y),
        );
        title_bar(&mut inner, at, index, (caption, font, color), rect);
        inner.advance_cursor_after_rect(rect);
    }
    if !folded_now {
        let held = std::mem::replace(&mut at.bounds, min);
        lay_out(&mut inner, at, index, Axis::Column);
        at.bounds = held;
    }
    let background = pad.around(inner.min_rect());
    ui.painter().set(
        plate,
        crate::widget::theme::frame_shape(
            background,
            egui::CornerRadius::same(style.radius.unwrap_or(8.0) as u8),
            style.fill.unwrap_or(Color32::from_black_alpha(160)),
            &style,
        ),
    );
    if widget.egui.resizable && !folded_now {
        grip(ui, at, entity, &widget, background, color);
    }
    ui.advance_cursor_after_rect(background);
}

/// The bar: the arrow a collapsible window folds by, the title a drag moves
/// it by, and the cross that shuts it.
fn title_bar(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    (caption, font, color): (&str, &egui::FontId, Color32),
    rect: egui::Rect,
) {
    let (entity, widget) = (at.arena[index].entity, at.arena[index].widget.clone());
    let line = rect.height();
    let mut left = rect.min.x;
    let mut right = rect.max.x;
    if widget.egui.collapsible {
        let arrow = egui::Rect::from_min_size(rect.min, vec2(line, line));
        let shut = folded(entity, &widget);
        let mark = if shut { FOLDED_MARK } else { UNFOLDED };
        ui.painter().text(
            arrow.center(),
            egui::Align2::CENTER_CENTER,
            mark,
            font.clone(),
            color,
        );
        if ui
            .interact(arrow, ui.id().with(("fold", entity)), Sense::click())
            .clicked()
        {
            set_folded(entity, !shut);
        }
        left = arrow.max.x;
    }
    if widget.egui.closable {
        let cross = egui::Rect::from_min_max(pos2(rect.max.x - line, rect.min.y), rect.max);
        ui.painter().text(
            cross.center(),
            egui::Align2::CENTER_CENTER,
            CLOSE,
            font.clone(),
            color,
        );
        if ui
            .interact(cross, ui.id().with(("close", entity)), Sense::click())
            .clicked()
        {
            at.edits.push((entity, Edit::CloseRequested));
            if widget.hide_on_close {
                at.edits.push((entity, Edit::Open(false)));
            }
        }
        right = cross.min.x;
    }
    let slant = at.slant(index);
    let words = crate::widget::theme::galley(ui.painter(), caption, font, slant);
    let y = rect.center().y - words.size().y / 2.0;
    ui.painter().galley(pos2(left, y), words, color);
    let handle = egui::Rect::from_min_max(pos2(left, rect.min.y), pos2(right, rect.max.y));
    if widget.egui.movable {
        let title = ui.interact(handle, ui.id().with(("title", entity)), Sense::drag());
        if title.dragged() {
            let moved = constrained(at, &widget, ui.max_rect(), title.drag_delta());
            at.edits.push((entity, Edit::Moved([moved.x, moved.y])));
        }
    }
}

/// A drag of `rect` by `delta`, held inside the surface where the widget asks
/// to be `constrain`ed.
pub(crate) fn constrained(
    at: &Painting<'_>,
    widget: &Widget,
    rect: egui::Rect,
    delta: egui::Vec2,
) -> egui::Vec2 {
    if !widget.egui.constrain {
        return delta;
    }
    let room = at.surface;
    let axis = |delta: f32, low: f32, high: f32| {
        // Already past an edge: let it come back, never further out.
        delta.max(low.min(0.0)).min(high.max(0.0))
    };
    vec2(
        axis(delta.x, room.min.x - rect.min.x, room.max.x - rect.max.x),
        axis(delta.y, room.min.y - rect.min.y, room.max.y - rect.max.y),
    )
}

/// The corner a drag resizes a `resizable` window by, writing its `width` and
/// `height` inside its minimums and maximums.
fn grip(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    entity: Entity,
    widget: &Widget,
    background: egui::Rect,
    color: Color32,
) {
    let corner = egui::Rect::from_min_max(background.max - vec2(GRIP, GRIP), background.max);
    let ink = color.gamma_multiply(0.5);
    for step in 1..=3 {
        let inset = GRIP * step as f32 / 4.0;
        ui.painter().line_segment(
            [
                pos2(corner.max.x - inset, corner.max.y - 2.0),
                pos2(corner.max.x - 2.0, corner.max.y - inset),
            ],
            Stroke::new(1.0, ink),
        );
    }
    let held = ui.interact(corner, ui.id().with(("grip", entity)), Sense::drag());
    if held.hovered() || held.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
    }
    if !held.dragged() {
        return;
    }
    let moved = held.drag_delta();
    let bound = |now: f32, low: f32, high: f32| {
        let now = now.max(low.max(1.0));
        if high > 0.0 { now.min(high) } else { now }
    };
    let layout = &widget.layout;
    if moved.x != 0.0 {
        let now = bound(
            background.width() + moved.x,
            widget.min_width,
            layout.max_width.get(),
        );
        at.edits.push((entity, Edit::Width(now)));
    }
    if moved.y != 0.0 {
        let now = bound(
            background.height() + moved.y,
            widget.min_height,
            layout.max_height.get(),
        );
        at.edits.push((entity, Edit::Height(now)));
    }
}

/// Which way `x` and `y` run for an anchor, so a drag moves the window the
/// way the pointer went: an offset from a right or bottom edge runs inward.
pub(crate) fn drag_signs(anchor: &str) -> (f32, f32) {
    use crate::vocabulary::words as w;
    let x = if matches!(
        anchor,
        w::TOP_RIGHT | w::BOTTOM_RIGHT | w::CENTER_RIGHT | w::FILL_RIGHT
    ) {
        -1.0
    } else {
        1.0
    };
    let y = if matches!(
        anchor,
        w::BOTTOM_LEFT | w::BOTTOM_RIGHT | w::CENTER_BOTTOM | w::FILL_BOTTOM
    ) {
        -1.0
    } else {
        1.0
    };
    (x, y)
}
