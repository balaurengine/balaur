//! The `image` kind: a picture drawn at its own size, a stated one, fitted
//! into its box, or nine-sliced, and the box it takes.

use egui::vec2;

use crate::vocabulary::words as w;
use crate::widget::arrange::solved_of;
use crate::widget::layer::Painting;
use crate::widget::paint::{self, Brush};
use crate::widget::theme::Style;

/// The box an image takes: what it states, else its own size, keeping the
/// aspect where only one axis is given.
/// Where a picture with a `fit` sits in the box it was given: its own shape
/// kept inside it, kept over it, stretched to it, or its own size centred.
fn fitted(fit: &str, box_rect: egui::Rect, native: egui::Vec2) -> egui::Rect {
    if fit == w::FILL || native.x <= 0.0 || native.y <= 0.0 {
        return box_rect;
    }
    let scale = match fit {
        w::CONTAIN => (box_rect.width() / native.x).min(box_rect.height() / native.y),
        w::COVER => (box_rect.width() / native.x).max(box_rect.height() / native.y),
        _ => 1.0,
    };
    egui::Rect::from_center_size(box_rect.center(), native * scale)
}

pub(crate) fn image_size(stated: egui::Vec2, native: egui::Vec2) -> egui::Vec2 {
    let aspect = if native.y > 0.0 {
        native.x / native.y
    } else {
        1.0
    };
    match (stated.x > 0.0, stated.y > 0.0) {
        (true, true) => stated,
        (true, false) => vec2(stated.x, stated.x / aspect),
        (false, true) => vec2(stated.y * aspect, stated.y),
        (false, false) => native,
    }
}

/// Draw a project image. A source that will not load is reported once and
/// draws its `alt_text`, or nothing: a missing picture must not take the
/// frame down.
pub(crate) fn image(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    if widget.source.is_empty() {
        return;
    }
    // A picture that names a handler is a button made of a picture: it senses
    // the click and `settle_clicks` calls `on_click` as it does for any other.
    let entity = placed.entity;
    let sense = if widget.on_click.is_empty() {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let ctx = ui.ctx().clone();
    let texture = match crate::images::texture_of(at.eng, &ctx, &widget.source) {
        Ok(texture) => texture,
        Err(err) => {
            warn_once(&widget.source, &err);
            if !widget.egui.alt_text.is_empty() {
                let look = at.look(index);
                crate::theme::asked_of_egui(&widget.egui.alt_text);
                let words = egui::RichText::new(widget.egui.alt_text.as_str())
                    .font(look.font.clone())
                    .color(look.ink);
                let galley = egui::WidgetText::from(words).into_galley(
                    ui,
                    None,
                    ui.available_width(),
                    egui::TextStyle::Body,
                );
                let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                Brush::of(ui).galley(rect.min, galley, look.ink);
            }
            return;
        }
    };
    let style = at.style(index);
    let o = &widget.egui;
    let tint = crate::widget::node::rgba_color(o.tint);
    let (uv, native) = crate::images::region_uv(
        crate::images::native_size(at.eng, &widget.source, &texture),
        o.region,
    );
    let size = image_size(solved_of(widget, &style, at.assigned), native);
    let clicked = if widget.slice.iter().any(|v| *v > 0.0) {
        // The borders stay the picture's own size; only the middle
        // stretches to the box.
        let (rect, response) = ui.allocate_exact_size(size, sense);
        if let Some(fill) = style.fill {
            Brush::of(ui).fill(rect, 0.0, fill);
        }
        let shapes =
            crate::widget::kinds::nine_patch_tinted(texture.id(), native, rect, widget.slice, tint);
        Brush::of(ui).add(paint::group(shapes));
        response.clicked()
    } else if widget.fit.is_empty() {
        let mut picture = picture_of(&texture, size, uv, o, &style);
        if let Some(fill) = style.fill {
            picture = picture.bg_fill(fill);
        }
        ui.add(picture.sense(sense)).clicked()
    } else {
        let box_size = solved_of(widget, &style, at.assigned).max(size);
        let (rect, response) = ui.allocate_exact_size(box_size, sense);
        if let Some(fill) = style.fill {
            Brush::of(ui).fill(rect, 0.0, fill);
        }
        let held = fitted(&widget.fit, rect, native);
        picture_of(&texture, held.size(), uv, o, &style).paint_at(ui, held);
        response.clicked()
    };
    if clicked {
        at.clicked.push(entity);
    }
}

/// The picture with what the widget asks of egui's image: the region, the
/// tint, the corners, and the turn, which drops the corners.
fn picture_of<'a>(
    texture: &egui::TextureHandle,
    size: egui::Vec2,
    uv: egui::Rect,
    o: &crate::widget::node::EguiOptions,
    style: &Style,
) -> egui::Image<'a> {
    let mut picture = egui::Image::new((texture.id(), size))
        .uv(uv)
        .tint(crate::widget::node::rgba_color(o.tint));
    if let Some(radius) = style.radius {
        picture = picture.corner_radius(egui::CornerRadius::same(radius.clamp(0.0, 255.0) as u8));
    }
    if o.angle_degrees != 0.0 {
        let origin = vec2(o.angle_origin[0], o.angle_origin[1]);
        picture = picture.rotate(o.angle_degrees.to_radians(), origin);
    }
    picture
}

/// Report a source once. Repeating it sixty times a second buries everything
/// else in the log.
fn warn_once(source: &str, err: &anyhow::Error) {
    if balaur_core::logbuf::first_time("widget image", source) {
        tracing::warn!("widget image '{source}': {err:#}");
    }
}
