//! A menu's rows as a sheet along the foot of a phone's screen, and the
//! frame round what a menu opens, which a dropdown and a sheet share.

use std::cell::Cell;

use balaur_core::Engine;
use egui::{Color32, Rect, vec2};

use crate::vocabulary::words as w;
use crate::widget::arrange::style_padding;
use crate::widget::layer::Painting;
use crate::widget::paint::Brush;
use crate::widget::theme::Style;

thread_local! {
    /// The menu whose rows a sheet is laying out now, by its arena index:
    /// only then is its head row (`title_bar`) one of them.
    static SHEETING: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Whether the menu at `index` is laying its rows out as a sheet this
/// moment, which is when its `title_bar` head is drawn with them.
pub(crate) fn sheeting(index: usize) -> bool {
    SHEETING.with(|now| now.get() == Some(index))
}

/// A menu's rows as a sheet along the foot of its surface, over a dim: what
/// a phone shows for a list too long to hang off a button (daisyUI's `modal
/// modal-bottom`). Its button opens and shuts it, and so do Escape, the dim
/// and a row that does not keep it open, as `close_on` says: `never` leaves
/// the dim to Escape, `click` shuts it on any row; `showing` holds it up.
/// As wide as the surface, its foot square and clear of the home bar, and
/// no taller than the surface less the notch and 5em, as daisyUI's modal
/// box: past that its rows scroll. Opening it shuts any other popup. Its
/// frame is the menu's `body` table, else the dialog's look; its dim the
/// dialog's. A sheet not drawn for a pass (its menu hidden or gone, or hung
/// under its button at a wider class) comes back shut. Answers whether it
/// is up.
pub(crate) fn sheet(
    ui: &egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    response: &egui::Response,
    rows: impl FnOnce(&mut egui::Ui, &mut Painting<'_>) -> bool,
) -> bool {
    let placed = &at.arena[index];
    let (entity, showing) = (placed.entity, placed.widget.showing);
    let close_on = placed.widget.egui.close_on.clone();
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique(("balaur-sheet", entity));
    let pass = ctx.cumulative_pass_nr();
    let was = ctx
        .data(|d| d.get_temp::<(bool, u64)>(id))
        .is_some_and(|(up, drawn)| up && drawn + 1 >= pass);
    if !(showing || was != response.clicked()) {
        if was {
            ctx.data_mut(|d| d.remove::<(bool, u64)>(id));
        }
        return false;
    }
    if !was {
        egui::Popup::close_all(&ctx);
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
        .unwrap_or(crate::widget::window::DIM);
    // The surface the root sits on, to its edge, less what the notch and
    // the home bar cover: the sheet's plate reaches the foot, its rows stop
    // above the bar.
    let edge = at.edge_to_edge;
    let safe = crate::widget::layer::inside_safe_area(&at.facts, edge, [true; 4]);
    let (notch, foot) = (
        (safe.top() - edge.top()).max(0.0),
        (edge.bottom() - safe.bottom()).max(0.0),
    );
    let tallest = edge.height() - notch - 5.0 * at.theme.size(crate::vocabulary::tokens::FONT_SIZE);
    let rows_most = (tallest - pad.top - pad.bottom - foot).max(0.0);
    let area = egui::Modal::default_area(id.with(w::SHEET))
        .constrain_to(edge)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::Vec2::ZERO);
    let before = at.clicked.len();
    SHEETING.with(|now| now.set(Some(index)));
    let shown = egui::Modal::new(id.with(w::SHEET))
        .area(area)
        .frame(egui::Frame::NONE)
        .backdrop_color(dim)
        .show(&ctx, |ui| {
            // Its own room, the surface's width and its tallest: an area
            // held by its foot is offered only what it measured last frame,
            // so its rows would never grow past their first height.
            let room = Rect::from_min_size(ui.max_rect().min, vec2(edge.width(), tallest));
            let mut whole = ui.new_child(egui::UiBuilder::new().max_rect(room));
            let shut = on_plate(&mut whole, at.eng, Some(&look), Some(foot), |ui| {
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
    SHEETING.with(|now| now.set(None));
    let any_row = at.clicked.len() > before;
    let on_dim = shown.backdrop_response.clicked();
    let escape = !on_dim && shown.should_close();
    let shut = escape
        || match close_on.as_str() {
            w::NEVER => shown.inner,
            w::CLICK => any_row || shown.inner || on_dim,
            _ => shown.inner || on_dim,
        };
    ctx.data_mut(|d| d.insert_temp(id, (!shut, pass)));
    true
}

/// What an open menu shows, on the frame its `body` table draws round it,
/// a fold's way: the fill, the edge, the corners and the shadows, and the
/// padding inside them. With no `body` it is drawn as it stands, in egui's
/// own popup frame. A sheet's `foot` is how far the home bar reaches up its
/// surface: its foot is square, as it sits on the screen's, and its rows
/// keep that much more above it; `None` is a popup's, rounded all round.
pub(crate) fn on_plate<R>(
    ui: &mut egui::Ui,
    eng: &Engine,
    body: Option<&Style>,
    foot: Option<f32>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let Some(look) = body else {
        return add(ui);
    };
    let mut pad = style_padding(look, egui::Vec2::ZERO);
    pad.bottom += foot.unwrap_or(0.0);
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
    let square = if foot.is_some() { 0 } else { round };
    let corners = egui::CornerRadius {
        nw: round,
        ne: round,
        sw: square,
        se: square,
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
