//! The boxes the controls egui lays out are drawn in, worked out once for
//! both the draw and the measure, so a row sized from one holds the other
//! (docs/PLAN-ui-gaps.md §2).

use egui::{Color32, vec2};

use crate::vocabulary::words as w;
use crate::widget::node::Widget;
use crate::widget::theme::{Style, rich};

/// A caption laid out as egui lays a control's out: on one line, in the
/// button text style where the text names no face.
pub(crate) fn control_galley(
    ctx: &egui::Context,
    style: &egui::Style,
    text: egui::RichText,
) -> std::sync::Arc<egui::Galley> {
    egui::WidgetText::from(text).into_galley_impl(
        ctx,
        style,
        egui::text::TextWrapping::no_max_width(),
        egui::TextStyle::Button.into(),
        egui::Align::Center,
    )
}

/// A checkbox's box: the tick's square, the caption after it, never shorter
/// than a control.
pub(crate) fn tick_box_size(
    spacing: &egui::style::Spacing,
    text: Option<egui::Vec2>,
) -> egui::Vec2 {
    let side = spacing.icon_width;
    let tall = side.max(spacing.interact_size.y);
    let wide = text.map_or(side, |text| side + spacing.icon_spacing + text.x);
    vec2(
        wide.max(spacing.interact_size.y),
        text.map_or(tall, |text| tall.max(text.y)),
    )
}

/// A switch's track: as tall as its role asks, and most of twice that
/// across.
pub(crate) fn switch_size(style: &Style) -> egui::Vec2 {
    let height = style.height.unwrap_or(18.0);
    vec2(height * 1.75, height)
}

/// A dropdown's box as `kinds::picker` draws it with nothing sizing it: the widest
/// of its text and its options, the arrow after it, in a button's padding,
/// never narrower than egui's combo box nor shorter than a control.
pub(crate) fn dropdown_size(
    ctx: &egui::Context,
    style: &egui::Style,
    widget: &Widget,
    (font, color, slant): (&egui::FontId, Color32, bool),
) -> egui::Vec2 {
    let spacing = &style.spacing;
    let margin = spacing.button_padding;
    let widest = std::iter::once(widget.text.as_str())
        .chain(widget.options.iter().map(smol_str::SmolStr::as_str))
        .map(|text| {
            let text = rich(text, font, color, slant, &widget.text_look);
            control_galley(ctx, style, text).size()
        })
        .fold(egui::Vec2::ZERO, egui::Vec2::max);
    let mark = spacing.icon_width;
    let wide = (widest.x + spacing.icon_spacing + mark).max(spacing.combo_width - 2.0 * margin.x);
    let tall = widest.y.max(mark);
    vec2(
        wide + 2.0 * margin.x,
        (tall + 2.0 * margin.y).max(spacing.interact_size.y),
    )
}

/// The box a menu of strings asks for: its caption as egui's button lays it
/// out, in the button's padding.
pub(crate) fn menu_size(
    ctx: &egui::Context,
    style: &egui::Style,
    widget: &Widget,
    caption: &str,
    (font, color, slant): (&egui::FontId, Color32, bool),
) -> egui::Vec2 {
    let text = rich(caption, font, color, slant, &widget.text_look);
    control_galley(ctx, style, text).size() + style.spacing.button_padding * 2.0
}

/// How a picker treats alpha, by the widget's `alpha` word.
pub(crate) fn alpha_of(widget: &Widget) -> egui::color_picker::Alpha {
    match widget.egui.alpha.as_str() {
        w::NONE => egui::color_picker::Alpha::Opaque,
        w::ADDITIVE => egui::color_picker::Alpha::BlendOrAdditive,
        _ => egui::color_picker::Alpha::OnlyBlend,
    }
}
