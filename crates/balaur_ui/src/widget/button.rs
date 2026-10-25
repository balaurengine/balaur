//! The button: what it paints inside itself, and the box it paints on.
//!
//! Split from `widget_layer` because that file is the component and the walk
//! over the world, and this is one kind's drawing.

use egui::{Color32, Stroke, pos2, vec2};

use crate::theme::family;
use crate::vocabulary::words as w;
use crate::widget::arrange::{Pad, padding_of};
use crate::widget::layer::Painting;
use crate::widget::node::Widget;
use crate::widget::paint::{self, Brush};
use crate::widget::theme::{Pointer, Style, WidgetState};

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
    effects: balaur_text::Effects,
}

/// The icon and the caption, measured but not yet painted. `width` is the
/// box the layout gave the button, inside its padding: a `wrap` or `truncate`
/// caption is shaped to what the other parts leave of it.
fn face_of(
    ui: &egui::Ui,
    at: &Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    style: &Style,
    width: f32,
) -> Face {
    let widget = &at.arena[index].widget;
    // As tall as the caption's type, keeping the picture's own aspect.
    let picture = (!widget.source.is_empty())
        .then(|| crate::images::texture_of(at.eng, ui.ctx(), &widget.source).ok())
        .flatten()
        .map(|texture| {
            let native = crate::images::native_size(at.eng, &widget.source, &texture);
            let aspect = if native.y > 0.0 {
                native.x / native.y
            } else {
                1.0
            };
            (texture.id(), vec2(font.size * aspect, font.size))
        });
    let plate = if picture.is_some() && style.plate.is_some() {
        2.0
    } else {
        0.0
    };
    // A shortcut draws itself the way the platform writes it, so a row that
    // says `cmd+s` needs no second spelling of it beside the caption.
    let shown = if widget.trailing.is_empty() {
        crate::chord::chord_shown(ui.ctx(), &widget.shortcut)
    } else {
        Some(widget.trailing.to_string())
    };
    let trailing =
        shown.map(|text| Brush::of(ui).line_of(text, font.clone(), Color32::PLACEHOLDER));
    let icon = (!widget.icon.is_empty()).then(|| {
        let mark = egui::FontId::new(
            crate::widget::text::icon_px(widget, font.size),
            family(w::ICON),
        );
        Brush::of(ui).line_of(widget.icon.to_string(), mark, Color32::PLACEHOLDER)
    });
    let mark = icon.as_ref().map_or(egui::Vec2::ZERO, |g| g.size());
    let pic = picture.map_or(egui::Vec2::ZERO, |(_, s)| {
        s + egui::Vec2::splat(plate * 2.0)
    });
    let tail = trailing.as_ref().map_or(egui::Vec2::ZERO, |g| g.size());
    let gap = gap_of(widget, style, font.size);
    let mut shaped = crate::widget::text::shaped_caption(ui, at, index, caption, font, None);
    if let Some((natural, _)) = &shaped {
        let before = [pic.x, mark.x].iter().filter(|w| **w > 0.0).count() as f32;
        let tail_gap = if tail.x > 0.0 { font.size } else { 0.0 };
        let taken = pic.x + mark.x + gap * before + tail_gap + tail.x;
        let room = crate::widget::text::caption_room(widget, natural.size.x, width, taken);
        if room.is_some() {
            shaped = crate::widget::text::shaped_caption(ui, at, index, caption, font, room);
        }
    }
    let plain = (shaped.is_none() && !caption.is_empty())
        .then(|| Brush::of(ui).line_of(caption.to_owned(), font.clone(), Color32::PLACEHOLDER));
    let text = shaped.as_ref().map_or_else(
        || plain.as_ref().map_or(egui::Vec2::ZERO, |g| g.size()),
        |(shaped, _)| shaped.size,
    );
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
        effects: widget.text_look.effects,
    }
}

/// The space between a button's picture, icon and caption: the first of its
/// `gap`, else its theme's, else half its font size.
pub(crate) fn gap_of(widget: &Widget, style: &Style, font_size: f32) -> f32 {
    if widget.gap[0] >= 0.0 {
        widget.gap[0]
    } else {
        style.gap.unwrap_or(font_size * 0.5)
    }
}

/// A box that answers the pointer the way egui's button does, with nothing
/// painted in it: at least `min`, its padding and a control's height, under
/// the next automatic id, and named to a screen reader as `kind`.
pub(crate) fn sensor(
    ui: &mut egui::Ui,
    min: egui::Vec2,
    sense: egui::Sense,
    kind: egui::Role,
    label: &str,
) -> egui::Response {
    let spacing = ui.spacing();
    let mut size = min.max(spacing.button_padding * 2.0);
    size.y = size.y.max(spacing.interact_size.y);
    let (id, rect) = ui.allocate_space(size);
    let response = ui.interact(rect, id, sense);
    if let Some(cursor) = ui.visuals().interact_cursor
        && response.hovered()
    {
        ui.ctx().set_cursor_icon(cursor);
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(kind, enabled, label));
    response
}

/// A button of one line of text, painted as egui's: the frame the pointer's
/// state dresses, the text at the layout's side. Inside a menu the menu's
/// style leaves it flat until the pointer is on it. `selected` makes it a
/// row of a list, framed only when picked or under the pointer.
pub(crate) fn text_button(
    ui: &mut egui::Ui,
    text: egui::RichText,
    selected: Option<bool>,
) -> egui::Response {
    let pad = ui.spacing().button_padding;
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        None,
        ui.available_width() - 2.0 * pad.x,
        egui::TextStyle::Button,
    );
    let plate = Brush::of(ui).slot();
    let label = galley.text().to_owned();
    let response = sensor(
        ui,
        galley.size() + pad * 2.0,
        egui::Sense::click(),
        egui::Role::Button,
        &label,
    );
    if let Some(on) = selected {
        let enabled = ui.is_enabled();
        response
            .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, on, &label));
    }
    if ui.is_rect_visible(response.rect) {
        let look = ui.style().interact(&response);
        let resting =
            !(response.hovered() || response.is_pointer_button_down_on() || response.has_focus());
        let fill = if selected == Some(true) {
            ui.visuals().selection.bg_fill
        } else {
            look.weak_bg_fill
        };
        if !(selected == Some(false) && resting) {
            Brush::of(ui).set(
                plate,
                paint::rect(
                    response.rect.expand(look.expansion),
                    look.corner_radius,
                    fill,
                    look.bg_stroke,
                    egui::StrokeKind::Inside,
                ),
            );
        }
        let side = egui::Align2([ui.layout().horizontal_align(), egui::Align::Center]);
        let at = side
            .align_size_within_rect(galley.size(), response.rect.shrink2(pad))
            .min;
        Brush::of(ui).galley(at, galley, look.text_color());
    }
    response
}

/// What a button's `sense` lets the pointer do to it.
fn sense_of(word: &str) -> egui::Sense {
    match word {
        w::CLICK_AND_DRAG => egui::Sense::click_and_drag(),
        w::DRAG => egui::Sense::drag(),
        w::HOVER => egui::Sense::hover(),
        _ => egui::Sense::click(),
    }
}

/// The face in the rect the button took, inside its padding: centred, or
/// against the edge a `text_align` of `start` or `end` names, with the
/// trailing text on the far edge.
fn paint_face(
    ui: &egui::Ui,
    at: &Painting<'_>,
    face: &Face,
    rect: egui::Rect,
    ink: Color32,
    style: &Style,
    pad: Pad,
) {
    // A caption that neither wraps nor truncates is cut at the box rather
    // than painted over whatever sits beside it.
    let painter = if face.size.x + pad.taken().x > rect.width() {
        Brush::clipped(ui, rect)
    } else {
        Brush::of(ui)
    };
    let inner = pad.inside(rect);
    let align = style.align.as_deref();
    let left = align == Some(w::START);
    let mut at_x = match align {
        Some(w::START) => inner.min.x,
        Some(w::END) => inner.max.x - face.size.x,
        _ => inner.center().x - face.size.x / 2.0,
    };
    let middle = inner.center().y;
    if let Some(trailing) = &face.trailing {
        let x = if left {
            inner.max.x - trailing.size().x
        } else {
            at_x + face.size.x - trailing.size().x
        };
        let y = middle - trailing.size().y / 2.0;
        painter.galley(
            pos2(x, y),
            std::sync::Arc::clone(trailing),
            ink.gamma_multiply(0.55),
        );
    }
    if let Some((texture, size)) = face.picture {
        let disc = egui::Rect::from_min_size(
            pos2(at_x, middle - size.y / 2.0 - face.plate),
            size + egui::Vec2::splat(face.plate * 2.0),
        );
        if let Some(plate) = style.plate {
            painter.fill(disc, disc.height() / 2.0, plate);
        }
        let inner = egui::Rect::from_center_size(disc.center(), size);
        // A theme that names an `icon_color` tints the picture with it, the
        // way Godot's `icon_normal_color` dresses a button's icon.
        painter.add(paint::picture(
            texture,
            inner,
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            style.icon_color.unwrap_or(Color32::WHITE),
        ));
        at_x = disc.max.x + face.gap;
    }
    if let Some(icon) = &face.icon {
        let y = middle - icon.size().y / 2.0;
        // A glyph icon answers `icon_color` as a picture does, so a row that
        // tints its mark per component does not have to draw itself.
        let tint = style.icon_color.unwrap_or(ink);
        painter.galley(pos2(at_x, y), std::sync::Arc::clone(icon), tint);
        at_x += icon.size().x + face.gap;
    }
    if let Some((shaped, texture)) = &face.shaped {
        let origin = pos2(at_x, middle - shaped.size.y / 2.0);
        painter.glyphs(
            *texture,
            shaped,
            origin,
            ink,
            None,
            &face.effects,
            at.eng.time(),
        );
        return;
    }
    if let Some(plain) = &face.plain {
        let y = middle - plain.size().y / 2.0;
        painter.galley(pos2(at_x, y), std::sync::Arc::clone(plain), ink);
    }
}

/// The corner a button is drawn with: what the theme says, else as round as
/// its text is tall, which is the pill the layer has always drawn.
fn corner(style: &Style, widget: &Widget, height: f32) -> egui::CornerRadius {
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
    egui::CornerRadius::same((stated).min(120.0) as u8)
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
    let focused = at.focused;
    let pad = padding_of(&widget, &base);
    let floor = vec2(base.width.unwrap_or(0.0), base.height.unwrap_or(0.0));
    // The box the layout handed it too: a button in a column fills its width
    // rather than hugging its caption, as it does in Godot and in CSS.
    let given = crate::widget::arrange::solved_of(&widget, &at.style_of(&widget), at.assigned);
    let inside = if given.x > 0.0 {
        given.x - pad.taken().x
    } else {
        0.0
    };
    let face = face_of(ui, at, index, caption, font, &base, inside);
    let min = (face.size + pad.taken())
        .max(vec2(widget.width, widget.height))
        .max(floor)
        .max(given);
    let plate = Brush::of(ui).slot();
    let response = sensor(
        ui,
        min,
        sense_of(&widget.egui.sense),
        egui::Role::Button,
        caption,
    );
    let pointer = Pointer::of(&response);
    let down = pointer == Pointer::Held || widget.checked;
    let style = base.in_states(WidgetState {
        pointer,
        ..at.state
    });
    let radius = corner(&style, &widget, response.rect.height());
    match style.image.as_ref() {
        Some(path) => crate::widget::kinds::nine_patch_plate(
            ui,
            at.eng,
            plate,
            path,
            style.slice,
            response.rect,
        ),
        None => Brush::of(ui).set(
            plate,
            crate::widget::theme::frame_shape(
                response.rect,
                radius,
                style.fill.unwrap_or(Color32::TRANSPARENT),
                &style,
            ),
        ),
    }
    // A theme that dresses no state still lights the button up: every control
    // answers the pointer, and a theme refines what that looks like.
    let plain = base.hover.is_none() && base.active.is_none() && base.checked.is_none();
    if (response.hovered() || widget.checked) && plain {
        Brush::of(ui).fill(response.rect, radius, crate::widget::theme::wash(ui, down));
    }
    let ink = if widget.text_color[3] > 0.0 {
        color
    } else {
        style.text_color.unwrap_or(color)
    };
    paint_face(ui, at, &face, response.rect, ink, &style, pad);
    if response.clicked() {
        at.clicked.push(entity);
    }
    if focused == Some(entity) {
        // Drawn rather than egui's own focus ring: the ring follows
        // egui's keyboard focus, and this follows the scene's.
        Brush::of(ui).outline(
            response.rect.expand(2.0),
            radius,
            Stroke::new(2.0, style.stroke.unwrap_or(ink)),
            egui::StrokeKind::Outside,
        );
    }
    response
}
