//! The controls that hold an on or off: a checkbox, the radio a checkbox in
//! a group is, and a switch. The tick lives on the widget: a click is
//! reported like a button's and the next tick flips `checked`.

use egui::{Color32, Rect, Sense, Stroke, pos2};

use crate::widget::layer::Painting;
use crate::widget::paint::Brush;
use crate::widget::theme::{WidgetState, rich};

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
    let slant = at.slant(index);
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let label = rich(caption, font, color, slant, &widget.text_look);
    let response = tick_box(
        ui,
        label,
        (widget.checked, widget.egui.indeterminate),
        caption,
        !widget.radio_group.is_empty(),
    );
    if response.clicked() {
        at.clicked.push(placed.entity);
    }
    if at.focused == Some(placed.entity) {
        Brush::of(ui).outline(
            response.rect.expand(2.0),
            4.0,
            Stroke::new(2.0, color),
            egui::StrokeKind::Outside,
        );
    }
}

/// A box, its tick and its caption, sized and coloured as egui's checkbox:
/// the box `icon_width` across, the tick `icon_width_inner`, the caption
/// `icon_spacing` after it, and the box's look the state the pointer leaves.
/// One in a radio group is `round`: a circle, and on, a dot inside a ring of
/// the tick's colour, as daisyUI's radio.
fn tick_box(
    ui: &mut egui::Ui,
    label: egui::RichText,
    (on, either): (bool, bool),
    caption: &str,
    round: bool,
) -> egui::Response {
    let spacing = ui.spacing().clone();
    let side = spacing.icon_width;
    let text = (!caption.is_empty())
        .then(|| crate::widget::sizing::control_galley(ui.ctx(), ui.style(), label));
    let want =
        crate::widget::sizing::tick_box_size(&spacing, text.as_ref().map(|text| text.size()));
    let (id, rect) = ui.allocate_space(want);
    let response = ui.interact(rect, id, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        let role = if round {
            egui::Role::RadioButton
        } else {
            egui::Role::CheckBox
        };
        if either {
            egui::WidgetInfo::labeled(role, enabled, caption)
        } else {
            egui::WidgetInfo::selected(role, enabled, on, caption)
        }
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let look = ui.style().interact(&response);
    let frame = egui::Rect::from_center_size(
        pos2(rect.left() + side / 2.0, rect.center().y),
        egui::Vec2::splat(side),
    );
    let mark =
        egui::Rect::from_center_size(frame.center(), egui::Vec2::splat(spacing.icon_width_inner));
    let painter = Brush::of(ui);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "half a box's side"
    )]
    let corner = if round {
        egui::CornerRadius::same((side / 2.0).round().clamp(0.0, 255.0) as u8)
    } else {
        look.corner_radius
    };
    painter.plate(
        frame,
        corner,
        look.bg_fill,
        look.bg_stroke,
        egui::StrokeKind::Inside,
    );
    if round {
        if on {
            let ring = side / 2.0 - look.bg_stroke.width.max(1.0);
            painter.disc(frame.center(), ring, look.fg_stroke.color);
            painter.disc(frame.center(), ring - side * 0.18, look.bg_fill);
        }
    } else if either {
        painter.hline(mark.x_range(), mark.center().y, look.fg_stroke);
    } else if on {
        painter.line(
            vec![
                pos2(mark.left(), mark.center().y),
                pos2(mark.center().x, mark.bottom()),
                pos2(mark.right(), mark.top()),
            ],
            look.fg_stroke,
        );
    }
    if let Some(text) = text {
        let at = pos2(
            frame.right() + spacing.icon_spacing,
            rect.center().y - text.size().y / 2.0,
        );
        painter.galley(at, text, look.text_color());
    }
    response
}

/// An on/off switch: a track the theme fills and a knob at one end of it,
/// and its caption after the track, as a checkbox's.
///
/// `checked` is what it holds: `[roles.x]` dresses it off and
/// `[roles.x.checked]` on, the held look standing in where a theme has none.
pub(crate) fn switch(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let slant = at.slant(index);
    let (entity, on) = {
        let placed = &at.arena[index];
        (placed.entity, placed.widget.checked)
    };
    let look = at.resting(index);
    let text = (!caption.is_empty()).then(|| {
        let label = rich(
            caption,
            font,
            color,
            slant,
            &at.arena[index].widget.text_look,
        );
        crate::widget::sizing::control_galley(ui.ctx(), ui.style(), label)
    });
    let gap = ui.spacing().icon_spacing;
    let track_size = crate::widget::sizing::switch_track(&look.style);
    let size =
        crate::widget::sizing::switch_size(&look.style, gap, text.as_ref().map(|text| text.size()));
    let height = track_size.y;
    let (whole, response) = ui.allocate_exact_size(size, Sense::click());
    let rect = Rect::from_min_size(
        pos2(whole.min.x, whole.center().y - height / 2.0),
        track_size,
    );
    if response.clicked() {
        at.clicked.push(entity);
    }
    let state = WidgetState {
        pointer: at.pointer(index, &response),
        ..at.state
    };
    let dressed = at.dressed(index, &look.style, state);
    let track = dressed.fill.unwrap_or(Color32::TRANSPARENT);
    let knob = dressed
        .text_color
        .unwrap_or(crate::widget::theme::DEFAULT_INK);
    Brush::of(ui).fill(rect, height / 2.0, track);
    let inset = height / 2.0;
    let x = if on {
        rect.max.x - inset
    } else {
        rect.min.x + inset
    };
    Brush::of(ui).disc(pos2(x, rect.center().y), height * 0.34, knob);
    if let Some(text) = text {
        let at = pos2(rect.right() + gap, whole.center().y - text.size().y / 2.0);
        Brush::of(ui).galley(at, text, color);
    }
}
