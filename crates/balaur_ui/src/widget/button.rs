//! The button: what it paints inside itself, and the box it paints on.
//!
//! Split from `widget_layer` because that file is the component and the walk
//! over the world, and this is one kind's drawing.

use egui::{Color32, Stroke, pos2, vec2};

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
    /// A `loading` button's spinner, this wide; 0 for none.
    spinner: f32,
    icon: Option<std::sync::Arc<egui::Galley>>,
    shaped: Option<(std::rc::Rc<balaur_text::Shaped>, Option<egui::TextureId>)>,
    plain: Option<std::sync::Arc<egui::Galley>>,
    trailing: Option<std::sync::Arc<egui::Galley>>,
    /// The trailing part is a glyph (`trailing_icon`), drawn in the icon's
    /// ink a gap after the caption, not dimmed text against the far edge.
    tail_mark: bool,
    /// The trailing part worn as a chip (`trailing_role`), in `trailing`'s
    /// place.
    chip: Option<Chip>,
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
    // As tall as the button's icon (`icon_size`, else the caption's type),
    // keeping the picture's own aspect: a brand's mark beside a glyph's.
    let edge = crate::widget::text::icon_px(widget, style, font.size);
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
            (texture.id(), vec2(edge * aspect, edge))
        });
    let plate = if picture.is_some() && style.plate.is_some() {
        2.0
    } else {
        0.0
    };
    let look = at.look(index);
    let (trailing, chip, tail_mark) = tail_of(ui, at, &look, widget, font);
    let spinner = if widget.loading {
        crate::widget::text::icon_px(widget, &look.style, font.size)
    } else {
        0.0
    };
    let icon = (!widget.loading
        && !widget.icon.is_empty()
        && crate::widget::text::shows_icon(&look.style))
    .then(|| crate::widget::icons::Icons::of(at.eng, &look).glyph(&widget.icon))
    .flatten()
    .map(|glyph| {
        let mark = egui::FontId::new(
            crate::widget::text::icon_px(widget, &look.style, font.size),
            look.icon.clone(),
        );
        Brush::of(ui).line_of(glyph, mark, Color32::PLACEHOLDER)
    });
    let mark = icon
        .as_ref()
        .map_or(egui::Vec2::splat(spinner), |g| g.size());
    let pic = picture.map_or(egui::Vec2::ZERO, |(_, s)| {
        s + egui::Vec2::splat(plate * 2.0)
    });
    let tail = chip.as_ref().map_or_else(
        || trailing.as_ref().map_or(egui::Vec2::ZERO, |g| g.size()),
        |c| c.size,
    );
    let gap = gap_of(widget, style, font.size);
    let tail_gap = tail_gap_of(tail.x, tail_mark, gap, font.size);
    let mut shaped = crate::widget::text::shaped_caption(ui, at, index, caption, font, None);
    if let Some((natural, _)) = &shaped {
        let before = [pic.x, mark.x].iter().filter(|w| **w > 0.0).count() as f32;
        // Stacked, the caption has the whole width under the icon.
        let taken = if style.stacked() {
            0.0
        } else {
            pic.x + mark.x + gap * before + tail_gap + tail.x
        };
        let room = crate::widget::text::caption_room((widget, style), natural.size.x, width, taken);
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
    let size = if style.stacked() {
        stacked_size(&[pic, mark, text], gap)
    } else {
        vec2(
            pic.x + mark.x + text.x + between + tail_gap + tail.x,
            pic.y.max(mark.y).max(text.y).max(tail.y),
        )
    };
    Face {
        picture,
        plate,
        spinner,
        icon,
        shaped,
        plain,
        trailing,
        tail_mark,
        chip,
        size,
        gap,
        effects: widget.text_look.effects,
    }
}

/// A button's trailing part, shaped: worn as a chip (`trailing_role`), else
/// its glyph (`trailing_icon`), else its text or the shortcut it answers;
/// and whether it sits a gap from the caption (a glyph, a chip) rather than
/// against the far edge.
fn tail_of(
    ui: &egui::Ui,
    at: &Painting<'_>,
    look: &crate::widget::arena::Look,
    widget: &Widget,
    font: &egui::FontId,
) -> (Option<std::sync::Arc<egui::Galley>>, Option<Chip>, bool) {
    let icons = crate::widget::icons::Icons::of(at.eng, look);
    if let Some(chip) = chip_of(&Brush::of(ui), &icons, &at.theme, look, widget) {
        return (None, Some(chip), true);
    }
    if let Some(glyph) = tail_glyph(ui, at, look, widget, font) {
        return (Some(glyph), None, true);
    }
    // A shortcut draws itself the way the platform writes it, so a row that
    // says `cmd+KeyS` needs no second spelling of it beside the caption.
    let shown = if widget.trailing.is_empty() {
        crate::chord::chord_shown(ui.ctx(), &widget.shortcut)
    } else {
        Some(widget.trailing.to_string())
    };
    let Some(text) = shown else {
        return (None, None, false);
    };
    let galley = Brush::of(ui).line_of(text, font.clone(), Color32::PLACEHOLDER);
    (Some(galley), None, false)
}

/// A button's `trailing_icon` shaped in the icon font at its icon's size,
/// or `None` where it names none the font has.
fn tail_glyph(
    ui: &egui::Ui,
    at: &Painting<'_>,
    look: &crate::widget::arena::Look,
    widget: &Widget,
    font: &egui::FontId,
) -> Option<std::sync::Arc<egui::Galley>> {
    if widget.trailing_icon.is_empty() {
        return None;
    }
    let glyph = crate::widget::icons::Icons::of(at.eng, look).glyph(&widget.trailing_icon)?;
    let mark = egui::FontId::new(
        crate::widget::text::icon_px(widget, &look.style, font.size),
        look.icon.clone(),
    );
    Some(Brush::of(ui).line_of(glyph, mark, Color32::PLACEHOLDER))
}

/// A button's trailing part worn as a chip (`trailing_role`): its glyph
/// (`trailing_icon`) and its words (`trailing`) side by side inside the
/// role's padding, on its `fill`: a price in coins on a Start button.
pub(crate) struct Chip {
    glyph: Option<std::sync::Arc<egui::Galley>>,
    words: Option<std::sync::Arc<egui::Galley>>,
    style: std::rc::Rc<Style>,
    pad: egui::Vec2,
    gap: f32,
    /// Its box, padding included.
    pub(crate) size: egui::Vec2,
}

/// The chip a button's `trailing_role` dresses its trailing part in,
/// measured; `None` where it names no role or there is nothing to wear it.
pub(crate) fn chip_of(
    brush: &Brush,
    icons: &crate::widget::icons::Icons,
    theme: &crate::widget::theme::WidgetTheme,
    look: &crate::widget::arena::Look,
    widget: &Widget,
) -> Option<Chip> {
    if widget.trailing_role.is_empty() {
        return None;
    }
    let style = theme.resolved(w::LABEL, &widget.trailing_role);
    let size = style.font_size.unwrap_or(look.font.size);
    let mark = egui::FontId::new(style.icon_size.unwrap_or(size), look.icon.clone());
    let glyph = (!widget.trailing_icon.is_empty())
        .then(|| icons.glyph(&widget.trailing_icon))
        .flatten()
        .map(|glyph| brush.line_of(glyph, mark, Color32::PLACEHOLDER));
    let words = (!widget.trailing.is_empty()).then(|| {
        let font = egui::FontId::new(size, look.font.family.clone());
        brush.line_of(widget.trailing.to_string(), font, Color32::PLACEHOLDER)
    });
    if glyph.is_none() && words.is_none() {
        return None;
    }
    let pad = vec2(
        style.padding_x.or(style.padding).unwrap_or(0.0),
        style.padding_y.or(style.padding).unwrap_or(0.0),
    );
    let gap = style.gap.unwrap_or(size / 4.0);
    let mark = glyph.as_ref().map_or(egui::Vec2::ZERO, |g| g.size());
    let text = words.as_ref().map_or(egui::Vec2::ZERO, |g| g.size());
    let between = if mark.x > 0.0 && text.x > 0.0 {
        gap
    } else {
        0.0
    };
    let size = vec2(
        mark.x + between + text.x + pad.x * 2.0,
        mark.y.max(text.y) + pad.y * 2.0,
    );
    Some(Chip {
        glyph,
        words,
        style,
        pad,
        gap,
        size,
    })
}

impl Chip {
    /// The chip with its top left at `min`: its box, then its glyph and its
    /// words, each in the role's ink, else the button's.
    fn paint(&self, painter: &Brush, min: egui::Pos2, ink: Color32) {
        let rect = egui::Rect::from_min_size(min, self.size);
        if let Some(fill) = self.style.fill {
            painter.fill(rect, self.style.radius.unwrap_or(rect.height() / 2.0), fill);
        }
        let middle = rect.center().y;
        let mut x = rect.min.x + self.pad.x;
        if let Some(glyph) = &self.glyph {
            let y = middle - glyph.size().y / 2.0;
            let tint = self.style.icon_color.unwrap_or(ink);
            painter.galley(pos2(x, y), std::sync::Arc::clone(glyph), tint);
            x += glyph.size().x + if self.words.is_some() { self.gap } else { 0.0 };
        }
        if let Some(words) = &self.words {
            let y = middle - words.size().y / 2.0;
            let tint = self.style.text_color.unwrap_or(ink);
            painter.galley(pos2(x, y), std::sync::Arc::clone(words), tint);
        }
    }
}

/// The space before a button's trailing part: its gap before a glyph, which
/// sits by the caption, and a font's size before text, which belongs to the
/// far edge; none where there is no trailing part.
pub(crate) fn tail_gap_of(tail: f32, mark: bool, gap: f32, font_size: f32) -> f32 {
    if tail <= 0.0 {
        0.0
    } else if mark {
        gap
    } else {
        font_size
    }
}

/// The space between a button's picture, icon and caption: its `face_gap`,
/// else its theme's, else half its font size.
pub(crate) fn gap_of(widget: &Widget, style: &Style, font_size: f32) -> f32 {
    if widget.face_gap >= 0.0 {
        widget.face_gap
    } else {
        style.gap.unwrap_or(font_size * 0.5)
    }
}

/// A box that answers the pointer the way egui's button does, with nothing
/// painted in it: at least `min` and a control's height, under the next
/// automatic id, and named to a screen reader as `kind`. The caller counts
/// the padding, which a one-glyph button leaves off its sides.
pub(crate) fn sensor(
    ui: &mut egui::Ui,
    min: egui::Vec2,
    sense: egui::Sense,
    kind: egui::Role,
    label: &str,
) -> egui::Response {
    let spacing = ui.spacing();
    let mut size = min;
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

/// Parts stacked one over the next, the gap between each two that are there:
/// as wide as the widest, as tall as all of them.
pub(crate) fn stacked_size(parts: &[egui::Vec2], gap: f32) -> egui::Vec2 {
    let there: Vec<&egui::Vec2> = parts.iter().filter(|part| part.x > 0.0).collect();
    let down: f32 = there.iter().map(|part| part.y).sum();
    vec2(
        there.iter().map(|part| part.x).fold(0.0, f32::max),
        down + gap * there.len().saturating_sub(1) as f32,
    )
}

/// A stacked face: the icon centred over the caption, the pair in the middle
/// of the box.
fn paint_stacked(
    painter: &Brush,
    at: &Painting<'_>,
    face: &Face,
    inner: egui::Rect,
    ink: Color32,
    style: &Style,
) {
    let mut y = inner.center().y - face.size.y / 2.0;
    let middle = inner.center().x;
    if let Some(icon) = &face.icon {
        let tint = style.icon_color.unwrap_or(ink);
        painter.galley(
            pos2(middle - icon.size().x / 2.0, y),
            std::sync::Arc::clone(icon),
            tint,
        );
        y += icon.size().y + face.gap;
    }
    if let Some((shaped, texture)) = &face.shaped {
        let origin = pos2(middle - shaped.size.x / 2.0, y);
        painter.glyphs(
            *texture,
            shaped,
            origin,
            ink,
            None,
            &face.effects,
            at.eng.time(),
        );
    } else if let Some(plain) = &face.plain {
        painter.galley(
            pos2(middle - plain.size().x / 2.0, y),
            std::sync::Arc::clone(plain),
            ink,
        );
    }
}

/// The face in the rect the button took, inside its padding: centred, or
/// against the edge a `text_align` of `start` or `end` names, with the
/// trailing text on the far edge. Mirrored, it is laid out the same and
/// drawn as its mirror image, each part's own drawing kept: the icon at the
/// right of the caption, the trailing text at the left edge, and an icon the
/// theme says points a way turned round.
fn paint_face(
    ui: &egui::Ui,
    (at, index): (&Painting<'_>, usize),
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
    let flip = crate::widget::direction::mirrored(at.arena, index);
    let widget = &at.arena[index].widget;
    let inner = pad.turned(flip).inside(rect);
    let across =
        |x: f32, width: f32| crate::widget::direction::placed_x(flip, x, width, inner.x_range());
    // An icon the theme says points a way, turned round where the face is.
    let turns = |name: &str| flip && at.theme.mirrors_icon(name);
    if style.stacked() {
        paint_stacked(&painter, at, face, inner, ink, style);
        return;
    }
    let align = style.align.as_deref();
    let left = align == Some(w::START);
    let mut at_x = match align {
        Some(w::START) => inner.min.x,
        Some(w::END) => inner.max.x - face.size.x,
        _ => inner.center().x - face.size.x / 2.0,
    };
    let middle = inner.center().y;
    // The trailing part and the chip end at the box's far edge where the
    // face is against its start, else just past the caption.
    let end = if left {
        inner.max.x
    } else {
        at_x + face.size.x
    };
    if let Some(trailing) = &face.trailing {
        let y = middle - trailing.size().y / 2.0;
        let (tint, named) = if face.tail_mark {
            (
                style.icon_color.unwrap_or(ink),
                widget.trailing_icon.as_str(),
            )
        } else {
            (ink.gamma_multiply(0.55), "")
        };
        let x = across(end - trailing.size().x, trailing.size().x);
        painter.icon(pos2(x, y), trailing, tint, turns(named));
    }
    if let Some(chip) = &face.chip {
        let x = across(end - chip.size.x, chip.size.x);
        chip.paint(&painter, pos2(x, middle - chip.size.y / 2.0), ink);
    }
    if let Some(picture) = face.picture {
        at_x += paint_picture(
            &painter,
            picture,
            face,
            (at_x, middle),
            (inner, flip),
            style,
        );
    }
    if face.spinner > 0.0 {
        let rect = egui::Rect::from_min_size(
            pos2(across(at_x, face.spinner), middle - face.spinner / 2.0),
            vec2(face.spinner, face.spinner),
        );
        // The theme's `[spinner]` turns it, so one table sets both.
        let spinner = at.theme.resolved(w::SPINNER, "");
        let tint = style.icon_color.unwrap_or(ink);
        crate::widget::numbers::spin(ui, &painter, (rect, tint), &spinner, at.ui_now());
        at_x += face.spinner + face.gap;
    }
    if let Some(icon) = &face.icon {
        let y = middle - icon.size().y / 2.0;
        // A glyph icon answers `icon_color` as a picture does, so a row that
        // tints its mark per component does not have to draw itself.
        let tint = style.icon_color.unwrap_or(ink);
        let place = pos2(across(at_x, icon.size().x), y);
        painter.icon(place, icon, tint, turns(&widget.icon));
        at_x += icon.size().x + face.gap;
    }
    if let Some((shaped, texture)) = &face.shaped {
        let origin = pos2(across(at_x, shaped.size.x), middle - shaped.size.y / 2.0);
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
        painter.galley(
            pos2(across(at_x, plain.size().x), y),
            std::sync::Arc::clone(plain),
            ink,
        );
    }
}

/// A face's picture on its round plate, the plate's left edge at `x`, the
/// pair mirrored in `inner` where `flip` says: how far across it took, its
/// gap to the caption with it.
fn paint_picture(
    painter: &Brush,
    (texture, size): (egui::TextureId, egui::Vec2),
    face: &Face,
    (at_x, middle): (f32, f32),
    (inner, flip): (egui::Rect, bool),
    style: &Style,
) -> f32 {
    let disc = egui::Rect::from_min_size(
        pos2(at_x, middle - size.y / 2.0 - face.plate),
        size + egui::Vec2::splat(face.plate * 2.0),
    );
    let disc = if flip {
        crate::widget::direction::reflect(disc, inner)
    } else {
        disc
    };
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
    disc.width() + face.gap
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
        let nodes = at.arena;
        let placed = &nodes[index];
        (placed.entity, &placed.widget)
    };
    // Resting: a button is not always as wide as the box it was given, so it
    // reads the state off its own response rather than off that box.
    let base = at.resting(index).style.clone();
    let focused = at.focused;
    let pad = padding_of(widget, &base);
    let floor = vec2(base.width.unwrap_or(0.0), base.height.unwrap_or(0.0));
    // The box the layout handed it too: a button in a column fills its width
    // rather than hugging its caption, as it does in Godot and in CSS.
    let given = crate::widget::arrange::solved_of(widget, &at.style(index), at.assigned);
    let inside = if given.x > 0.0 {
        given.x - pad.taken().x
    } else {
        0.0
    };
    let face = face_of(ui, at, index, caption, font, &base, inside);
    // One glyph takes no side padding, which is a caption's air: its box is
    // the square the measure gave it, as `measure::button_in` says.
    let one_glyph = caption.is_empty()
        && !widget.icon.is_empty()
        && crate::widget::text::shows_icon(&base)
        && widget.source.is_empty()
        && widget.trailing.is_empty();
    let air = if one_glyph {
        vec2(0.0, pad.taken().y)
    } else {
        pad.taken()
    };
    let mut min = (face.size + air)
        .max(vec2(widget.width, widget.height))
        .max(floor)
        .max(given);
    if one_glyph && base.width.is_none() {
        min.x = min.x.max(min.y);
    }
    let plate = Brush::of(ui).slot();
    let sense = widget.sense(sense_of(&widget.egui.sense));
    let response = sensor(ui, min, sense, egui::Role::Button, caption);
    let pointer = at.pointer(index, &response);
    let down = pointer == Pointer::Held || widget.checked;
    let style = at.dressed(
        index,
        &base,
        WidgetState {
            pointer,
            ..at.state
        },
    );
    let radius = corner(&style, widget, response.rect.height());
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
            crate::widget::frame::frame_shape(
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
    paint_face(ui, (at, index), &face, response.rect, ink, &style, pad);
    if response.clicked() {
        at.clicked.push(entity);
    }
    let held_focus = widget.state == crate::vocabulary::states::FOCUS;
    if (focused == Some(entity) || held_focus) && style.outline.is_none() {
        // Drawn rather than egui's own focus ring: the ring follows egui's
        // keyboard focus, and this follows the scene's. A theme's `outline`
        // is drawn round every kind instead, by the layer.
        let (gap, width) = style.ring();
        Brush::of(ui).outline(
            response.rect.expand(gap),
            radius,
            Stroke::new(width, style.stroke.unwrap_or(ink)),
            egui::StrokeKind::Outside,
        );
    }
    response
}
