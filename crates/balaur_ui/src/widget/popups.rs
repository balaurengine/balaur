//! A popup kept inside the viewport its widget sits in. egui keeps a menu, a
//! dropdown's list and a tooltip inside the window alone, choosing which side
//! of what it hangs off to open on against the window; a widget in a viewport
//! asks for that choice against the viewport.

use std::cell::Cell;

use egui::containers::Tooltip;
use egui::{Rect, RectAlign, Vec2};

thread_local! {
    /// The viewport the root being drawn sits in; none for one on the window.
    static SCREEN: Cell<Option<Rect>> = const { Cell::new(None) };
    /// What the popup about to show hangs off, for `scrolled` to size it by.
    static ANCHOR: Cell<Option<(Rect, f32)>> = const { Cell::new(None) };
}

/// Begin a root's drawing: what it draws is cut at `screen`, the viewport it
/// sits in, and its popups open inside it. None for a root on the window.
pub(crate) fn confine(ui: &mut egui::Ui, screen: Option<Rect>) {
    SCREEN.set(screen);
    if let Some(screen) = screen {
        ui.set_clip_rect(ui.clip_rect().intersect(screen));
    }
}

/// The viewport the root being drawn sits in; none on the window.
pub(crate) fn screen() -> Option<Rect> {
    SCREEN.get()
}

/// `popup` on the first side of `asked`, then of `alternatives` or egui's own
/// where those are none, that keeps it inside the viewport being drawn; as
/// it is on the window. `gap` is the room the popup keeps from what it hangs
/// off, as it was built with.
pub(crate) fn kept_inside<'a>(
    popup: egui::Popup<'a>,
    asked: RectAlign,
    alternatives: &[RectAlign],
    gap: f32,
) -> egui::Popup<'a> {
    ANCHOR.set(popup.get_anchor_rect().map(|anchor| (anchor, gap)));
    let size = popup.get_expected_size();
    sized_inside(popup, (asked, alternatives, gap), size)
}

// The same with the size it last drew at, which a tooltip keeps under an id
// of its own.
fn sized_inside<'a>(
    popup: egui::Popup<'a>,
    (asked, alternatives, gap): (RectAlign, &[RectAlign], f32),
    size: Option<Vec2>,
) -> egui::Popup<'a> {
    let Some(screen) = SCREEN.get() else {
        return popup;
    };
    // Its first frame has no size yet: egui measures it, and the next one fits.
    let (Some(anchor), Some(size)) = (popup.get_anchor_rect(), size) else {
        return popup;
    };
    let egui_own: Vec<RectAlign> = asked
        .symmetries()
        .into_iter()
        .chain(RectAlign::MENU_ALIGNS)
        .collect();
    let others = if alternatives.is_empty() {
        &egui_own[..]
    } else {
        alternatives
    };
    let mut tried = std::iter::once(asked).chain(others.iter().copied());
    if let Some(best) =
        tried.find(|align| screen.contains_rect(align.align_rect(&anchor, size, gap)))
    {
        return popup.align(best).align_alternatives(&[]);
    }
    // No side has the room: the asked one, slid inside, as egui slides a
    // popup inside the window.
    let rect = moved_inside(asked.align_rect(&anchor, size, gap), screen);
    popup
        .at_position(rect.min - egui::vec2(0.0, gap))
        .align(RectAlign::BOTTOM_START)
        .align_alternatives(&[])
}

/// A popup's rows, scrolling past the larger room either side of what it hangs
/// off, so it still opens beside that and reaches its last row in a short
/// window. A submenu, which hangs off a row, scrolls at the screen's foot.
pub(crate) fn scrolled<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let screen = SCREEN.get().unwrap_or_else(|| ui.ctx().viewport_rect());
    let room = match ANCHOR.take() {
        Some((anchor, gap)) => {
            (screen.bottom() - anchor.bottom()).max(anchor.top() - screen.top()) - gap
        }
        None => screen.bottom() - ui.cursor().top(),
    };
    // The frame round the rows, top and bottom, as the popup last drew it.
    let frame = ui
        .ctx()
        .memory(|m| m.area_rect(ui.layer_id().id))
        .map_or(ui.spacing().menu_margin.sum().y, |area| {
            2.0 * (ui.cursor().top() - area.top()).max(0.0)
        });
    let room = room - frame;
    egui::ScrollArea::vertical()
        .max_height(room.max(ui.spacing().interact_size.y))
        .show(ui, add)
        .inner
}

/// `rect` moved the least that puts it inside `screen`, its top left kept
/// where it is larger.
fn moved_inside(rect: Rect, screen: Rect) -> Rect {
    let shift = |low: f32, high: f32, from: f32, to: f32| {
        if high > to {
            (to - high).max(from - low)
        } else {
            (from - low).max(0.0)
        }
    };
    rect.translate(egui::vec2(
        shift(rect.min.x, rect.max.x, screen.min.x, screen.max.x),
        shift(rect.min.y, rect.max.y, screen.min.y, screen.max.y),
    ))
}

/// A tooltip with `text` while the widget is hovered, kept inside the
/// viewport being drawn: egui's own `on_hover_text` where the root is on the
/// window.
pub(crate) fn hover_text(response: &egui::Response, text: &str) {
    if SCREEN.get().is_none() {
        response.clone().on_hover_text(text);
        return;
    }
    show(response, Tooltip::for_enabled(response), text);
}

/// The same while a finger holds the widget: shown whatever the pointer does.
pub(crate) fn held_text(response: &egui::Response, text: &str) {
    if SCREEN.get().is_none() {
        response.show_tooltip_text(text.to_owned());
        return;
    }
    show(response, Tooltip::for_widget(response), text);
}

/// The same over a widget that is disabled.
pub(crate) fn disabled_text(response: &egui::Response, text: &str) {
    if SCREEN.get().is_none() {
        response.clone().on_disabled_hover_text(text);
        return;
    }
    show(response, Tooltip::for_disabled(response), text);
}

fn show(response: &egui::Response, mut tip: Tooltip<'_>, text: &str) {
    let drew = response
        .ctx
        .memory(|m| m.area_rect(Tooltip::tooltip_id(response.id, 0)))
        .map(|rect| rect.size());
    tip.popup = sized_inside(tip.popup, (RectAlign::BOTTOM_START, &[], TOOLTIP_GAP), drew);
    let width = response.ctx.global_style().spacing.tooltip_width;
    tip.show(|ui| {
        ui.set_max_width(width);
        ui.add(egui::Label::new(text));
    });
}

/// The room egui's tooltip keeps from its widget.
const TOOLTIP_GAP: f32 = 4.0;
