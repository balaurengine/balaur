//! The button: what it paints inside itself, and the box it paints on.
//!
//! Split from `widget_layer` because that file is the component and the walk
//! over the world, and this is one kind's drawing.

use egui::{Color32, Stroke, pos2, vec2};

use crate::theme::family;
use crate::vocabulary::words as w;
use crate::widget_layer::{Painting, Widget};
use crate::widget_theme::Style;

/// What a button paints inside itself: the icon glyph, the caption, and the
/// box the two of them need.
struct Face {
    icon: Option<std::sync::Arc<egui::Galley>>,
    shaped: Option<(std::rc::Rc<crate::text::Shaped>, Option<egui::TextureId>)>,
    plain: Option<std::sync::Arc<egui::Galley>>,
    size: egui::Vec2,
    gap: f32,
}

/// The icon and the caption, measured but not yet painted.
fn face_of(
    ui: &egui::Ui,
    at: &Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
) -> Face {
    let widget = &at.arena[index].widget;
    let icon = (!widget.icon.is_empty()).then(|| {
        let mark = egui::FontId::new(font.size, family(w::ICON));
        ui.painter()
            .layout_no_wrap(widget.icon.to_string(), mark, Color32::WHITE)
    });
    let shaped = crate::widget_text::shaped_caption(ui, at, index, widget, caption, font);
    let plain = (shaped.is_none() && !caption.is_empty()).then(|| {
        ui.painter()
            .layout_no_wrap(caption.to_owned(), font.clone(), Color32::WHITE)
    });
    let text = shaped.as_ref().map_or_else(
        || plain.as_ref().map_or(egui::Vec2::ZERO, |g| g.size()),
        |(shaped, _)| shaped.size,
    );
    let mark = icon.as_ref().map_or(egui::Vec2::ZERO, |g| g.size());
    let gap = if mark.x > 0.0 && text.x > 0.0 {
        font.size * 0.5
    } else {
        0.0
    };
    let size = vec2(mark.x + gap + text.x, mark.y.max(text.y));
    Face {
        icon,
        shaped,
        plain,
        size,
        gap,
    }
}

/// The icon and the caption, centred together in the rect the button took.
fn paint_face(ui: &egui::Ui, at: &Painting<'_>, face: &Face, rect: egui::Rect, ink: Color32) {
    let mut at_x = rect.center().x - face.size.x / 2.0;
    if let Some(icon) = &face.icon {
        let y = rect.center().y - icon.size().y / 2.0;
        ui.painter()
            .galley(pos2(at_x, y), std::sync::Arc::clone(icon), ink);
        at_x += icon.size().x + face.gap;
    }
    if let Some((shaped, texture)) = &face.shaped {
        let origin = pos2(at_x, rect.center().y - shaped.size.y / 2.0);
        crate::text::paint(ui.painter(), *texture, shaped, origin, ink, at.eng.time());
        return;
    }
    if let Some(plain) = &face.plain {
        let y = rect.center().y - plain.size().y / 2.0;
        ui.painter()
            .galley(pos2(at_x, y), std::sync::Arc::clone(plain), ink);
    }
}

/// The corner a button is drawn with: what the theme says, else as round as
/// its text is tall, which is the pill the layer has always drawn.
fn corner(style: &Style, widget: &Widget, scale: f32, height: f32) -> egui::CornerRadius {
    if style.round == Some(true) {
        return egui::CornerRadius::same((height / 2.0).min(120.0) as u8);
    }
    let stated = style.radius.unwrap_or({
        if widget.font_size > 0.0 {
            widget.font_size
        } else {
            16.0
        }
    });
    egui::CornerRadius::same((stated * scale).min(120.0) as u8)
}

/// A pill that reports its click.
///
/// The background is painted into a slot reserved before the button rather
/// than handed to `egui::Button`: a widget that states a fill of its own
/// otherwise keeps it under the pointer, and the theme's `hover` never shows.
pub(crate) fn button(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: egui::Color32,
) {
    let (entity, widget) = {
        let placed = &at.arena[index];
        (placed.entity, placed.widget.clone())
    };
    let base = at.look(index).style.clone();
    let (scale, focused) = (at.scale, at.focused);
    let face = face_of(ui, at, index, caption, font);
    let pad_x = base
        .padding_x
        .map_or(ui.spacing().button_padding.x, |p| p * scale);
    let floor = vec2(
        base.width.unwrap_or(0.0) * scale,
        base.height.unwrap_or(0.0) * scale,
    );
    let min = (face.size + vec2(pad_x, ui.spacing().button_padding.y) * 2.0)
        .max(vec2(widget.width, widget.height) * scale)
        .max(floor);
    let plate = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(
        egui::Button::new("")
            .min_size(min)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE),
    );
    let style = base.in_state(response.hovered(), response.is_pointer_button_down_on());
    let radius = corner(&style, &widget, scale, response.rect.height());
    match style.image.as_ref() {
        Some(path) => crate::widget_kinds::nine_patch_plate(
            ui,
            at.eng,
            plate,
            path,
            style.slice,
            response.rect,
            scale,
        ),
        None => ui.painter().set(
            plate,
            egui::epaint::RectShape::new(
                response.rect,
                radius,
                style.fill.unwrap_or(Color32::TRANSPARENT),
                style
                    .stroke
                    .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_px(), c)),
                egui::StrokeKind::Inside,
            ),
        ),
    }
    let ink = if widget.text_color[3] > 0.0 {
        color
    } else {
        style.text_color.unwrap_or(color)
    };
    paint_face(ui, at, &face, response.rect, ink);
    if response.clicked() {
        at.clicked.push(entity);
    }
    if focused == Some(entity) {
        // Drawn rather than egui's own focus ring: the ring follows
        // egui's keyboard focus, and this follows the scene's.
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            radius,
            Stroke::new(2.0, style.stroke.unwrap_or(ink)),
            egui::StrokeKind::Outside,
        );
    }
}
