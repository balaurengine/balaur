//! A kind's style told to the egui widgets it is drawn from.

use super::Style;
use egui::Color32;

/// Dress the egui widgets a kind is drawn from in that kind's own style.
///
/// A `number_field`, a `slider`, a `text_field`, a `checkbox`, a `dropdown`
/// and a `menu` are egui's widgets, so they wear egui's palette unless the
/// theme is told to them here: a screen whose
/// theme names a `[number_field]` would otherwise show egui's grey beside the
/// buttons it dressed itself. A theme that names nothing changes nothing.
pub(crate) fn dress(ui: &mut egui::Ui, style: &Style, ink: Color32) {
    let radius = style
        .radius
        .map(|r| egui::CornerRadius::same(r.clamp(0.0, 255.0) as u8));
    let edge = |style: &Style| {
        style
            .stroke
            .map(|color| egui::Stroke::new(style.stroke_px(), color))
    };
    let visuals = ui.visuals_mut();
    for (state, look) in [
        (&mut visuals.widgets.inactive, Some(style)),
        (
            &mut visuals.widgets.hovered,
            style.hover.as_deref().or(Some(style)),
        ),
        (
            &mut visuals.widgets.active,
            style
                .active
                .as_deref()
                .or(style.hover.as_deref())
                .or(Some(style)),
        ),
    ] {
        let Some(look) = look else {
            continue;
        };
        if let Some(fill) = look.fill {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
        }
        if let Some(stroke) = edge(look) {
            state.bg_stroke = stroke;
        }
        if let Some(radius) = radius {
            state.corner_radius = radius;
        }
        // What egui strokes in the widget's foreground: a checkbox's tick, a
        // dropdown's arrow. Its `icon_color`, the mark a theme names apart
        // from the caption, else its text's.
        if let Some(color) = look.icon_color.or(look.text_color) {
            state.fg_stroke.color = color;
        }
    }
    // What a `text_field` draws its line on, which is not a widget state.
    if let Some(fill) = style.fill {
        visuals.extreme_bg_color = fill;
    }
    // The frame a focused `text_field` wears: egui draws it in the selection
    // stroke, so a role's `focus` table reaches it there.
    if let Some(stroke) = style.focus.as_deref().and_then(edge) {
        visuals.selection.stroke = stroke;
    }
    visuals.override_text_color = Some(ink);
}
