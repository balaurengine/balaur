//! The button: what it paints inside itself, and the box it paints on.
//!
//! Split from `widget_layer` because that file is the component and the walk
//! over the world, and this is one kind's drawing.

use egui::{Color32, Stroke, pos2, vec2};

use crate::theme::family;
use crate::vocabulary::words as w;
use crate::widget::layer::Painting;
use crate::widget::node::Widget;
use crate::widget::theme::Style;

/// What a button paints inside itself: a picture, the icon glyph, the
/// caption, the trailing text, and the box they need between them.
struct Face {
    picture: Option<(egui::TextureId, egui::Vec2)>,
    /// Around the picture when the role puts it on a disc.
    plate: f32,
    icon: Option<std::sync::Arc<egui::Galley>>,
    shaped: Option<(std::rc::Rc<balaur_text::Shaped>, Option<egui::TextureId>)>,
    plain: Option<std::sync::Arc<egui::Galley>>,
    trailing: Option<std::sync::Arc<egui::Galley>>,
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
    style: &Style,
) -> Face {
    let widget = &at.arena[index].widget;
    // As tall as the caption's type, keeping the picture's own aspect.
    let picture = (!widget.source.is_empty())
        .then(|| crate::images::texture_of(at.eng, ui.ctx(), &widget.source).ok())
        .flatten()
        .map(|texture| {
            let native = texture.size_vec2();
            let aspect = if native.y > 0.0 {
                native.x / native.y
            } else {
                1.0
            };
            (texture.id(), vec2(font.size * aspect, font.size))
        });
    let plate = if picture.is_some() && style.plate.is_some() {
        2.0 * at.scale
    } else {
        0.0
    };
    let trailing = (!widget.trailing.is_empty()).then(|| {
        ui.painter().layout_no_wrap(
            widget.trailing.to_string(),
            font.clone(),
            Color32::PLACEHOLDER,
        )
    });
    let icon = (!widget.icon.is_empty()).then(|| {
        let mark = egui::FontId::new(font.size, family(w::ICON));
        ui.painter()
            .layout_no_wrap(widget.icon.to_string(), mark, Color32::PLACEHOLDER)
    });
    let shaped = crate::widget::text::shaped_caption(ui, at, index, widget, caption, font);
    let plain = (shaped.is_none() && !caption.is_empty()).then(|| {
        ui.painter()
            .layout_no_wrap(caption.to_owned(), font.clone(), Color32::PLACEHOLDER)
    });
    let text = shaped.as_ref().map_or_else(
        || plain.as_ref().map_or(egui::Vec2::ZERO, |g| g.size()),
        |(shaped, _)| shaped.size,
    );
    let mark = icon.as_ref().map_or(egui::Vec2::ZERO, |g| g.size());
    let pic = picture.map_or(egui::Vec2::ZERO, |(_, s)| {
        s + egui::Vec2::splat(plate * 2.0)
    });
    let tail = trailing.as_ref().map_or(egui::Vec2::ZERO, |g| g.size());
    let gap = font.size * 0.5;
    // One gap between each pair of parts that are there, and a wider one
    // before the trailing text, which belongs to the far edge.
    let parts = [pic.x, mark.x, text.x].iter().filter(|w| **w > 0.0).count();
    let between = gap * parts.saturating_sub(1) as f32;
    let tail_gap = if tail.x > 0.0 { font.size } else { 0.0 };
    let size = vec2(
        pic.x + mark.x + text.x + between + tail_gap + tail.x,
        pic.y.max(mark.y).max(text.y).max(tail.y),
    );
    Face {
        picture,
        plate,
        icon,
        shaped,
        plain,
        trailing,
        size,
        gap,
    }
}

/// The face in the rect the button took: centred, or from the left edge for a
/// role that says `align = "left"`, with the trailing text on the far edge.
fn paint_face(
    ui: &egui::Ui,
    at: &Painting<'_>,
    face: &Face,
    rect: egui::Rect,
    ink: Color32,
    style: &Style,
    pad_x: f32,
) {
    let left = style.align.as_deref() == Some(w::LEFT);
    let mut at_x = if left {
        rect.min.x + pad_x
    } else {
        rect.center().x - face.size.x / 2.0
    };
    if let Some(trailing) = &face.trailing {
        let x = if left {
            rect.max.x - pad_x - trailing.size().x
        } else {
            at_x + face.size.x - trailing.size().x
        };
        let y = rect.center().y - trailing.size().y / 2.0;
        ui.painter().galley(
            pos2(x, y),
            std::sync::Arc::clone(trailing),
            ink.gamma_multiply(0.55),
        );
    }
    if let Some((texture, size)) = face.picture {
        let disc = egui::Rect::from_min_size(
            pos2(at_x, rect.center().y - size.y / 2.0 - face.plate),
            size + egui::Vec2::splat(face.plate * 2.0),
        );
        if let Some(plate) = style.plate {
            ui.painter().rect_filled(disc, disc.height() / 2.0, plate);
        }
        let inner = egui::Rect::from_center_size(disc.center(), size);
        egui::Image::new((texture, size)).paint_at(ui, inner);
        at_x = disc.max.x + face.gap;
    }
    if let Some(icon) = &face.icon {
        let y = rect.center().y - icon.size().y / 2.0;
        ui.painter()
            .galley(pos2(at_x, y), std::sync::Arc::clone(icon), ink);
        at_x += icon.size().x + face.gap;
    }
    if let Some((shaped, texture)) = &face.shaped {
        let origin = pos2(at_x, rect.center().y - shaped.size.y / 2.0);
        balaur_text::paint(ui.painter(), *texture, shaped, origin, ink, at.eng.time());
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
) -> egui::Response {
    let (entity, widget) = {
        let placed = &at.arena[index];
        (placed.entity, placed.widget.clone())
    };
    // Resting: a button is not always as wide as the box it was given, so it
    // reads the state off its own response rather than off that box.
    let base = at.resting(index).style.clone();
    let (scale, focused) = (at.scale, at.focused);
    let face = face_of(ui, at, index, caption, font, &base);
    let pad_x = base
        .padding_x
        .map_or(ui.spacing().button_padding.x, |p| p * scale);
    let floor = vec2(
        base.width.unwrap_or(0.0) * scale,
        base.height.unwrap_or(0.0) * scale,
    );
    // The box the layout handed it too: a button in a column fills its width
    // rather than hugging its caption, as it does in Godot and in CSS.
    let given = crate::widget::arrange::box_of(&widget, at.assigned, scale);
    let min = (face.size + vec2(pad_x, ui.spacing().button_padding.y) * 2.0)
        .max(vec2(widget.width, widget.height) * scale)
        .max(floor)
        .max(given);
    let plate = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(
        egui::Button::new("")
            .min_size(min)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE),
    );
    // A checked button is a toggle held down, and wears its pressed look.
    let down = response.is_pointer_button_down_on() || widget.checked;
    let style = base.in_state(response.hovered(), down);
    let radius = corner(&style, &widget, scale, response.rect.height());
    match style.image.as_ref() {
        Some(path) => crate::widget::kinds::nine_patch_plate(
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
    // A theme that dresses no state still lights the button up: every control
    // answers the pointer, and a theme refines what that looks like.
    if (response.hovered() || widget.checked) && base.hover.is_none() && base.active.is_none() {
        ui.painter()
            .rect_filled(response.rect, radius, crate::immediate::wash(ui, down));
    }
    let ink = if widget.text_color[3] > 0.0 {
        color
    } else {
        style.text_color.unwrap_or(color)
    };
    paint_face(ui, at, &face, response.rect, ink, &style, pad_x);
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
    response
}
