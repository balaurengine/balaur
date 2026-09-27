//! Marks at the right of a `list` or `tree` row: glyphs from the icon face,
//! each reported by name when clicked, and never a pick of the row.

use egui::{Color32, Rect, Sense, pos2};

use crate::widget::rows::{Ink, SEP};

/// One mark as a pass saw it: its name, its glyph, and what the pointer did.
pub(super) type Mark<'a> = (&'a str, &'a str, egui::Response);

/// A row's marks: its sixth field, `name=glyph` pairs joined on U+001E, left
/// to right as they are drawn.
fn of(item: &str) -> impl DoubleEndedIterator<Item = (&str, &str)> {
    item.trim_start_matches('\t')
        .split(SEP)
        .nth(5)
        .unwrap_or_default()
        .split('\u{1e}')
        .filter_map(|mark| mark.split_once('='))
}

/// A row's marks with the hit box each answers in, from the right edge in:
/// the last mark named is the rightmost. Called after the row is allocated,
/// so a click on a mark is the mark's, as one on the caret is the caret's.
pub(super) fn sense<'a>(ui: &egui::Ui, item: &'a str, slot: usize, rect: Rect) -> Vec<Mark<'a>> {
    let step = rect.height();
    of(item)
        .rev()
        .enumerate()
        .map(|(n, (name, glyph))| {
            let right = rect.max.x - step * n as f32;
            let box_ = Rect::from_min_max(pos2(right - step, rect.min.y), pos2(right, rect.max.y));
            let sensed = ui.interact(box_, ui.id().with(("mark", slot, name)), Sense::click());
            (name, glyph, sensed)
        })
        .collect()
}

/// Paint the marks in `ink`, washed under the pointer, and answer the name of
/// the one clicked.
pub(super) fn draw(
    ui: &egui::Ui,
    marks: &[Mark<'_>],
    size: f32,
    (ink, wash): (Color32, &Ink),
) -> Option<String> {
    let glyph_font = egui::FontId::new(size, crate::theme::family("icon"));
    let mut clicked = None;
    for (name, glyph, sensed) in marks {
        if sensed.hovered() {
            let radius = egui::CornerRadius::same((sensed.rect.height() * 0.2) as u8);
            ui.painter().rect_filled(
                sensed.rect.shrink(sensed.rect.height() * 0.1),
                radius,
                wash.wash(sensed.is_pointer_button_down_on()),
            );
        }
        ui.painter().text(
            sensed.rect.center(),
            egui::Align2::CENTER_CENTER,
            *glyph,
            glyph_font.clone(),
            ink,
        );
        if sensed.clicked() {
            clicked = Some((*name).to_owned());
        }
    }
    clicked
}
