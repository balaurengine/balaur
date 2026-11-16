//! A menu's rows as a sheet along the foot of a phone's screen, and the
//! frame round what a menu opens, which a dropdown and a sheet share.

use balaur_core::Engine;
use egui::{Color32, Rect, vec2};

use crate::vocabulary::words as w;
use crate::widget::arrange::style_padding;
use crate::widget::layer::Painting;
use crate::widget::paint::Brush;
use crate::widget::theme::Style;

/// A menu's rows as a sheet along the foot of the screen, over a dim: what a
/// phone shows for a list too long to hang off a button (daisyUI's `modal
/// modal-bottom`). Its button opens and shuts it, and so do the dim, Escape
/// and a row that does not keep it open; `showing` holds it up. As wide as
/// the screen, its foot square, and no taller than the screen less 5em, as
/// daisyUI's modal box: past that its rows scroll. Its frame is the menu's
/// `body` table, else the dialog's look; its dim the dialog's. Answers
/// whether it is up.
pub(crate) fn sheet(
    ui: &egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    response: &egui::Response,
    rows: impl FnOnce(&mut egui::Ui, &mut Painting<'_>) -> bool,
) -> bool {
    let placed = &at.arena[index];
    let (entity, showing) = (placed.entity, placed.widget.showing);
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique(("balaur-sheet", entity));
    let was = ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    if !(showing || was != response.clicked()) {
        ctx.data_mut(|d| d.insert_temp(id, false));
        return false;
    }
    let look = match at.style(index).body.clone() {
        Some(body) => body,
        None => at.theme.resolved(w::DIALOG, ""),
    };
    let pad = style_padding(&look, egui::Vec2::ZERO);
    // The dialog's dim, as daisyUI's modal backdrop is one.
    let dim = at
        .theme
        .resolved(w::DIALOG, "")
        .backdrop
        .unwrap_or(crate::widget::layer::DIM);
    let screen = ctx.content_rect();
    let tallest = screen.height() - 5.0 * at.theme.size(crate::vocabulary::tokens::FONT_SIZE);
    let rows_most = (tallest - pad.top - pad.bottom).max(0.0);
    let area = egui::Modal::default_area(id.with(w::SHEET))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::Vec2::ZERO);
    let shown = egui::Modal::new(id.with(w::SHEET))
        .area(area)
        .frame(egui::Frame::NONE)
        .backdrop_color(dim)
        .show(&ctx, |ui| {
            // Its own room, the screen's width and its tallest: an area held
            // by its foot is offered only what it measured last frame, so
            // its rows would never grow past their first height.
            let room = Rect::from_min_size(ui.max_rect().min, vec2(screen.width(), tallest));
            let mut whole = ui.new_child(egui::UiBuilder::new().max_rect(room));
            let shut = on_plate(&mut whole, at.eng, Some(&look), true, |ui| {
                ui.set_min_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt(("balaur-sheet-rows", entity))
                    .max_height(rows_most)
                    .show(ui, |ui| rows(ui, at))
                    .inner
            });
            ui.advance_cursor_after_rect(whole.min_rect());
            shut
        });
    let shut = shown.inner || shown.should_close();
    ctx.data_mut(|d| d.insert_temp(id, !shut));
    true
}

/// What an open menu shows, on the frame its `body` table draws round it,
/// a fold's way: the fill, the edge, the corners and the shadows, and the
/// padding inside them. With no `body` it is drawn as it stands, in egui's
/// own popup frame. A sheet's foot is square, as it sits on the screen's.
pub(crate) fn on_plate<R>(
    ui: &mut egui::Ui,
    eng: &Engine,
    body: Option<&Style>,
    square_foot: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let Some(look) = body else {
        return add(ui);
    };
    let pad = style_padding(look, egui::Vec2::ZERO);
    let slot = Brush::of(ui).slot();
    let room = ui.available_rect_before_wrap();
    let inside = Rect::from_min_max(
        room.min + vec2(pad.left, pad.top),
        room.max - vec2(pad.right, pad.bottom),
    );
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(inside));
    let out = add(&mut inner);
    let used = inner.min_rect();
    let rect = Rect::from_min_max(
        used.min - vec2(pad.left, pad.top),
        used.max + vec2(pad.right, pad.bottom),
    );
    let round = look.radius.unwrap_or(0.0).clamp(0.0, 255.0) as u8;
    let foot = if square_foot { 0 } else { round };
    let corners = egui::CornerRadius {
        nw: round,
        ne: round,
        sw: foot,
        se: foot,
    };
    if let Some(path) = look.image.as_ref() {
        crate::widget::kinds::nine_patch_plate(ui, eng, slot, path, look.slice, rect);
    } else {
        let fill = look.fill.unwrap_or(Color32::TRANSPARENT);
        Brush::of(ui).set(
            slot,
            crate::widget::frame::frame_shape(rect, corners, fill, look),
        );
    }
    ui.advance_cursor_after_rect(rect);
    out
}
