//! Which way the UI runs: left to right, or mirrored for a language written
//! right to left, as a web page under `dir="rtl"` is.
//!
//! The whole UI follows the locale in force (`strings::right_to_left`); a
//! widget's `direction` turns its subtree one way or the other, and `auto`,
//! the default, takes its parent's. A mirrored container places its children
//! as the mirror image of the left-to-right layout: a row runs from the
//! right, `start` and `end` swap, and so do the left and right of a padding,
//! a margin and an `absolute` child's insets, as CSS's logical sides do.
//! Text is never mirrored, only where it sits; an icon is turned round where
//! its theme lists it under `[icons] mirrored`, as a chevron that points
//! forward points left in Arabic.

use std::cell::Cell;

use balaur_core::Engine;

use crate::vocabulary::words::direction as d;
use crate::widget::arena::Placed;

thread_local! {
    /// The direction the locale gives the UI, read once a pass.
    static UI_RIGHT_TO_LEFT: Cell<bool> = const { Cell::new(false) };
}

/// Read the locale's direction for this pass. Called once a pass, before
/// anything is laid out.
pub(crate) fn begin_pass(eng: &Engine) {
    UI_RIGHT_TO_LEFT.set(balaur_core::strings::right_to_left(eng));
}

/// Whether the locale in force runs the UI right to left.
pub(crate) fn ui_right_to_left() -> bool {
    UI_RIGHT_TO_LEFT.get()
}

/// What a `direction` word says: `Some(true)` mirrored, `Some(false)` left
/// to right, `None` for `auto`, which takes what is above it.
fn stated(word: &str) -> Option<bool> {
    match word {
        d::RIGHT_TO_LEFT => Some(true),
        d::LEFT_TO_RIGHT => Some(false),
        _ => None,
    }
}

/// Whether the widget at `index` runs mirrored: its own `direction`, else
/// the nearest one above it that states one, else the UI's.
pub(crate) fn mirrored(arena: &[Placed], index: usize) -> bool {
    let mut at = Some(index);
    while let Some(here) = at {
        if let Some(way) = stated(&arena[here].widget.layout.direction) {
            return way;
        }
        at = arena[here].parent;
    }
    ui_right_to_left()
}

/// Whether the widget at `index` is mirrored, given whether what holds it
/// is: its own word, else `above`.
pub(crate) fn mirrored_under(arena: &[Placed], index: usize, above: bool) -> bool {
    stated(&arena[index].widget.layout.direction).unwrap_or(above)
}

/// Whether what holds the widget at `index` runs mirrored: the UI's for a
/// root.
pub(crate) fn mirrored_above(arena: &[Placed], index: usize) -> bool {
    arena[index]
        .parent
        .map_or_else(ui_right_to_left, |parent| mirrored(arena, parent))
}

/// Where a box `width` wide that starts at `x` in `within` sits once
/// `within` is mirrored: as far from its right edge as it was from its left.
pub(crate) fn reflect_x(x: f32, width: f32, within: egui::Rangef) -> f32 {
    within.min + within.max - x - width
}

/// `rect` mirrored inside `within`, its size kept.
pub(crate) fn reflect(rect: egui::Rect, within: egui::Rect) -> egui::Rect {
    let x = reflect_x(rect.min.x, rect.width(), within.x_range());
    egui::Rect::from_min_size(egui::pos2(x, rect.min.y), rect.size())
}

/// A box's place in `within` on the way it runs: `x` where it is left to
/// right, mirrored where `flip` says it is not.
pub(crate) fn placed_x(flip: bool, x: f32, width: f32, within: egui::Rangef) -> f32 {
    if flip { reflect_x(x, width, within) } else { x }
}

/// A `text_align` word as it lands on the screen in a widget that runs
/// mirrored: `start` is its right edge, `end` its left.
pub(crate) fn physical_align(word: &str, flip: bool) -> egui::Align {
    use crate::vocabulary::words as w;
    match word {
        w::CENTER => egui::Align::Center,
        w::END if flip => egui::Align::Min,
        w::END => egui::Align::Max,
        _ if flip => egui::Align::Max,
        _ => egui::Align::Min,
    }
}

/// An egui alignment across, turned round where `flip` says: what a popup's
/// side and a root's anchor become in a mirrored UI.
pub(crate) fn flip_align2(align: egui::Align2, flip: bool) -> egui::Align2 {
    if flip { align.flip_x() } else { align }
}

/// A popup's side, turned round where `flip` says.
pub(crate) fn flip_side(side: egui::RectAlign, flip: bool) -> egui::RectAlign {
    if flip { side.flip_x() } else { side }
}

/// A `text_align` word as the shaper takes it for a block's lines. Left to
/// right, `start` and `end` stay the paragraph's own, so Arabic in an English
/// UI still starts at its right; mirrored, they are the UI's: `start` is the
/// right edge whatever the line is written in, as CSS's `text-align: start`
/// is under `dir="rtl"`.
pub(crate) fn shaped_align(word: &str, flip: bool) -> balaur_text::Align {
    use crate::vocabulary::words as w;
    match (word, flip) {
        (w::CENTER, _) => balaur_text::Align::Center,
        (w::END, false) => balaur_text::Align::End,
        (w::END, true) => balaur_text::Align::Left,
        (_, false) => balaur_text::Align::Start,
        (_, true) => balaur_text::Align::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::{physical_align, reflect, reflect_x};

    #[test]
    fn a_box_reflects_as_far_from_the_right_as_it_was_from_the_left() {
        let within = egui::Rangef::new(100.0, 300.0);
        assert!((reflect_x(110.0, 30.0, within) - 260.0).abs() < f32::EPSILON);
        let rect = egui::Rect::from_min_size(egui::pos2(110.0, 5.0), egui::vec2(30.0, 10.0));
        let room = egui::Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(300.0, 50.0));
        let turned = reflect(rect, room);
        assert_eq!(
            (turned.min.x, turned.max.x, turned.min.y),
            (260.0, 290.0, 5.0)
        );
    }

    #[test]
    fn start_is_the_right_edge_of_a_mirrored_box() {
        use egui::Align;
        assert_eq!(physical_align("start", false), Align::Min);
        assert_eq!(physical_align("start", true), Align::Max);
        assert_eq!(physical_align("end", true), Align::Min);
        assert_eq!(physical_align("center", true), Align::Center);
    }
}
