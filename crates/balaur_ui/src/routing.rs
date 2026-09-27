//! Whose the pointer is: the UI's, layer by layer from the top, or the
//! world's under it.
//!
//! An open dialog takes every point. Otherwise a point is the UI's while egui
//! drags something, or where the last pass drew something that takes the
//! pointer: a popup or window, an overlay or side panel, a widget that does not
//! let the pointer through, or a seam between boxes. What is left is the
//! world's, the immediate-mode central panel included.
//!
//! A press is routed once and keeps its answer until every button is up, so a
//! drag begun in the world stays the world's across a panel, and one begun on
//! a panel never reaches the world.

use balaur_core::Engine;
use balaur_input::InputSnapshot;

/// What the last UI pass drew that takes the pointer, in egui points: kept
/// from pass to pass, because egui forgets what was visible over a frame the
/// pacing skipped, and the picture on screen is still that pass's.
#[derive(Default)]
pub(crate) struct UiHits {
    /// Widgets that do not let the pointer through, the seams between boxes,
    /// and this crate's own overlays and side panels, clipped to what shows.
    taking: Vec<egui::Rect>,
    /// The layers this crate drew, which answer for themselves above.
    own: Vec<egui::LayerId>,
    /// Every other window or popup egui showed, which takes all it covers.
    foreign: Vec<egui::Rect>,
    modal: bool,
    per_point: f32,
    ctx: Option<egui::Context>,
}

/// Where the press being held went: `Some(true)` for the UI.
#[derive(Default)]
pub(crate) struct PointerRoute {
    held: Option<bool>,
}

fn with_hits(eng: &Engine, f: impl FnOnce(&mut UiHits)) {
    if let Some(hits) = eng.try_resource::<UiHits>() {
        f(&mut hits.borrow_mut());
    }
}

/// Forget the last pass's records: this one is about to draw its own.
pub(crate) fn pass_begins(eng: &Engine) {
    with_hits(eng, |hits| {
        hits.taking.clear();
        hits.own.clear();
    });
}

/// File what the widget layer drew: what takes the pointer, and the layers
/// its roots are on.
pub(crate) fn widgets_drew(eng: &Engine, taking: Vec<egui::Rect>, roots: Vec<egui::LayerId>) {
    with_hits(eng, |hits| {
        hits.taking.extend(taking);
        hits.own.extend(roots);
    });
}

/// File a piece of immediate-mode chrome as it is drawn: the layer it is on,
/// when it has one of its own, and its rect when it takes the pointer.
pub(crate) fn chrome_drew(eng: &Engine, layer: Option<egui::LayerId>, taking: Option<egui::Rect>) {
    with_hits(eng, |hits| {
        hits.own.extend(layer);
        hits.taking.extend(taking);
    });
}

/// Close the pass's records with what egui itself shows: any window or popup
/// this crate did not draw, and whether a dialog holds the screen.
pub(crate) fn pass_drew(eng: &Engine, ctx: &egui::Context) {
    with_hits(eng, |hits| {
        let (foreign, modal) = ctx.memory(|m| {
            let foreign = m
                .areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|layer| {
                    matches!(layer.order, egui::Order::Middle | egui::Order::Foreground)
                })
                .filter(|layer| !hits.own.contains(layer))
                .filter_map(|layer| m.area_rect(layer.id))
                .collect();
            (foreign, m.top_modal_layer().is_some())
        });
        hits.foreign = foreign;
        hits.modal = modal;
        hits.per_point = ctx.pixels_per_point();
        hits.ctx = Some(ctx.clone());
    });
}

/// Whether the UI takes the pointer at `at`, in egui points: the design
/// pixels a layout is written in. False with no UI pass yet.
#[must_use]
pub fn takes_point(eng: &Engine, at: egui::Pos2) -> bool {
    let Some(hits) = eng.try_resource::<UiHits>() else {
        return false;
    };
    let hits = hits.borrow();
    let Some(ctx) = hits.ctx.as_ref() else {
        return false;
    };
    hits.modal
        || ctx.dragged_id().is_some()
        || hits.foreign.iter().any(|rect| rect.contains(at))
        || hits.taking.iter().any(|rect| rect.contains(at))
}

/// Whether the UI has the pointer: what the press answered while a button is
/// held, and whatever is under the pointer otherwise.
#[must_use]
pub fn pointer_is_ui(eng: &Engine) -> bool {
    let Some(input) = eng.try_resource::<InputSnapshot>() else {
        return false;
    };
    let (pressed, down, released, (x, y)) = {
        let input = input.borrow();
        (
            input.any_mouse_just_pressed(),
            input.any_mouse_down(),
            input.any_mouse_just_released(),
            input.mouse_pos(),
        )
    };
    let per_point = eng
        .try_resource::<UiHits>()
        .map(|hits| hits.borrow().per_point)
        .filter(|scale| *scale > 0.0)
        .unwrap_or(1.0);
    let at = egui::pos2(x / per_point, y / per_point);
    let Some(route) = eng.try_resource::<PointerRoute>() else {
        return takes_point(eng, at);
    };
    let mut route = route.borrow_mut();
    // Kept through the frame the button comes up, so the release goes where
    // the press did.
    if pressed || (down && route.held.is_none()) {
        route.held = Some(takes_point(eng, at));
    } else if !down && !released {
        route.held = None;
    }
    route.held.unwrap_or_else(|| takes_point(eng, at))
}
