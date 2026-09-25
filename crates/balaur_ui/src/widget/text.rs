//! Text in the widget layer: labels and captions through the shaper, and
//! the one line the player types into.

use egui::vec2;

use crate::vocabulary::words as w;
use crate::widget::arrange::solved_of;
use crate::widget::layer::{Edit, Painting};
use crate::widget::node::Widget;
use crate::widget::theme::weight_of;

/// What a widget's text asks the shaper for, at this scale.
///
/// Borrowed, not owned: this is built twice a widget a frame and the shaper
/// keeps nothing from it unless the cache misses.
pub(crate) fn text_request<'a>(
    widget: &'a Widget,
    caption: &'a str,
    width: Option<f32>,
    font: &egui::FontId,
    style: &'a crate::widget::theme::Style,
) -> balaur_text::RequestRef<'a> {
    balaur_text::RequestRef {
        text: caption,
        // The face the caller already resolved, so a role's `font_size` and
        // `font_weight` reach the shaper the way they reach egui's own text.
        size: font.size,
        weight: weight_of(style, widget).clamp(100.0, 900.0) as u16,
        italic: widget.font_style == w::ITALIC,
        width,
        // A grown child was cut at its box above; `truncate` says end that
        // cut with an ellipsis rather than mid-glyph.
        truncate: widget.truncate,
        align: match crate::widget::theme::text_align_of(style, widget) {
            w::CENTER => balaur_text::Align::Center,
            w::END => balaur_text::Align::End,
            _ => balaur_text::Align::Start,
        },
        markup: widget.markup,
        // A widget names no bitmap font yet; the world's text is where a
        // pixel face is asked for.
        font: "",
        family: crate::widget::theme::family_of(style, widget),
        line_height: 0.0,
        letter_spacing: 0.0,
    }
}

/// A caption shaped on one line, with the atlas it draws from; `None` until
/// the fonts are installed, when egui's own layout stands in.
pub(crate) fn shaped_caption(
    ui: &egui::Ui,
    at: &Painting<'_>,
    index: usize,
    widget: &Widget,
    caption: &str,
    font: &egui::FontId,
) -> Option<(std::rc::Rc<balaur_text::Shaped>, Option<egui::TextureId>)> {
    let state = balaur_text::state(at.eng)?;
    let look = at.look(index);
    let mut state = state.borrow_mut();
    let request = text_request(widget, caption, None, font, &look.style);
    let shaped = state.shape_for_egui(ui.ctx(), &request);
    Some((shaped, state.texture()))
}

/// Draw a label through the shaper. Answers false when the shaper is not
/// up yet, so the caller may fall back to egui's own text.
pub(crate) fn shaped_label(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    color: egui::Color32,
    font: &egui::FontId,
) -> bool {
    let Some(state) = balaur_text::state(at.eng) else {
        return false;
    };
    let look = at.look(index);
    let style = &look.style;
    let placed = &at.arena[index];
    let entity = placed.entity;
    let widget = &placed.widget;
    // A grown child was squeezed by the layout, so the box it was given is its
    // column: it cannot run past what taffy left beside it.
    let stated = if widget.width > 0.0 {
        widget.width
    } else if widget.grow > 0.0 {
        at.assigned.x
    } else {
        0.0
    };
    let (wrap, align, selectable) = (
        widget.wrap,
        crate::widget::theme::text_align_of(style, widget).to_owned(),
        widget.selectable,
    );
    let on_link = widget.on_link.clone();
    let room = ui.available_width();
    // A wrapping block takes the room; a truncating line takes its column, so
    // the shaper knows where to cut. Neither is the other.
    let width = if wrap {
        Some(room.max(1.0))
    } else if widget.truncate && stated > 0.0 {
        Some(stated)
    } else {
        None
    };
    // A stated width is a column, so a long line is cut off at its edge
    // rather than run into whatever sits beside it.
    let column = (!wrap && stated > 0.0).then_some(stated);
    let (shaped, texture) = {
        let mut state = state.borrow_mut();
        let request = text_request(widget, caption, width, font, style);
        (state.shape_for_egui(ui.ctx(), &request), state.texture())
    };
    // An aligned line takes the width it is aligned in; a wrapped block
    // already did, and aligned its own lines.
    let take = if width.is_none() && align != w::START {
        room.max(shaped.size.x)
    } else {
        shaped.size.x
    };
    let take = column.unwrap_or(take);
    // A block with links or one a player may select answers the pointer;
    // every other label is a picture of its text.
    let sense = if selectable {
        egui::Sense::click_and_drag()
    } else if shaped.links.is_empty() && shaped.hints.is_empty() {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(take, shaped.size.y), sense);
    let held = ui.clip_rect();
    if column.is_some() {
        ui.set_clip_rect(held.intersect(rect));
    }
    let slack = (take - shaped.size.x).max(0.0);
    let shift = match align.as_str() {
        w::CENTER if width.is_none() => slack / 2.0,
        w::END if width.is_none() => slack,
        _ => 0.0,
    };
    let origin = rect.min + vec2(shift, 0.0);
    if selectable {
        selecting(ui, &response, &shaped, origin, entity);
    }
    // A link wears the theme's primary ink where it has one, and egui's
    // otherwise, so a `[url]` never reads as plain text.
    let linked = (!shaped.links.is_empty()).then(|| {
        at.theme
            .token(crate::vocabulary::tokens::PRIMARY_TEXT)
            .unwrap_or(ui.visuals().hyperlink_color)
    });
    balaur_text::paint(
        ui.painter(),
        texture,
        &shaped,
        origin,
        color,
        linked,
        at.eng.time(),
    );
    if let Some(color) = linked {
        underline(ui, &shaped, origin, color);
    }
    for picture in &shaped.pictures {
        if let Ok(handle) = crate::images::texture_of(at.eng, ui.ctx(), &picture.path) {
            ui.painter().image(
                handle.id(),
                picture.rect.translate(origin.to_vec2()),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
    spans(ui, at, &response, &shaped, (origin, entity, &on_link));
    ui.set_clip_rect(held);
    true
}

/// What the pointer is over: the glyph under it, if any.
///
/// The rect is widened to the line: between two glyphs is still the word,
/// and a link half a pixel wide between letters is not a link.
fn glyph_at(
    shaped: &balaur_text::Shaped,
    origin: egui::Pos2,
    pos: egui::Pos2,
) -> Option<&balaur_text::Quad> {
    shaped
        .quads
        .iter()
        .min_by(|a, b| {
            let reach = |quad: &balaur_text::Quad| {
                quad.rect
                    .translate(origin.to_vec2())
                    .expand2(vec2(1.0, 4.0))
                    .distance_sq_to_pos(pos)
            };
            reach(a).total_cmp(&reach(b))
        })
        .filter(|quad| {
            quad.rect
                .translate(origin.to_vec2())
                .expand2(vec2(1.0, 4.0))
                .distance_sq_to_pos(pos)
                <= 0.0
        })
}

/// Where in the text a click at `pos` falls, in bytes: the near edge of the
/// glyph under the pointer, or the far edge when the pointer is past its
/// middle, so a drag from the right of a letter takes that letter.
fn offset_at(shaped: &balaur_text::Shaped, origin: egui::Pos2, pos: egui::Pos2) -> u32 {
    let Some(quad) = shaped.quads.iter().min_by(|a, b| {
        let reach = |quad: &balaur_text::Quad| {
            quad.rect
                .translate(origin.to_vec2())
                .distance_sq_to_pos(pos)
        };
        reach(a).total_cmp(&reach(b))
    }) else {
        return 0;
    };
    let box_ = quad.rect.translate(origin.to_vec2());
    if pos.x <= box_.center().x {
        return quad.start;
    }
    after(&shaped.text, quad.start)
}

/// The byte offset past the character starting at `start`.
fn after(text: &str, start: u32) -> u32 {
    let at = start as usize;
    text.get(at..)
        .and_then(|rest| rest.chars().next())
        .map_or(start, |c| start + u32::try_from(c.len_utf8()).unwrap_or(1))
}

/// A drag over a selectable label, the selection it leaves behind it painted,
/// and the copy that takes it.
///
/// The two offsets live in egui's own memory, keyed by the node: a selection
/// is this screen's, not the scene's, and nothing should write it back.
fn selecting(
    ui: &mut egui::Ui,
    response: &egui::Response,
    shaped: &balaur_text::Shaped,
    origin: egui::Pos2,
    entity: balaur_core::hecs::Entity,
) {
    let id = egui::Id::new(("balaur-selection", entity));
    let mut span = ui
        .data(|data| data.get_temp::<(u32, u32)>(id))
        .unwrap_or((0, 0));
    if let Some(pos) = response.interact_pointer_pos() {
        let at = offset_at(shaped, origin, pos);
        // The press is the anchor, not the frame egui calls it a drag: by
        // then the pointer has already moved off the letter it started on.
        if ui.input(|input| input.pointer.any_pressed()) {
            span = (at, at);
        } else if response.dragged() || response.is_pointer_button_down_on() {
            span.1 = at;
        }
        if response.double_clicked() {
            span = word_around(&shaped.text, at);
        }
        if response.triple_clicked() {
            span = (0, u32::try_from(shaped.text.len()).unwrap_or(u32::MAX));
        }
        ui.data_mut(|data| data.insert_temp(id, span));
    }
    let (from, to) = (span.0.min(span.1), span.0.max(span.1));
    if from == to {
        return;
    }
    let fill = ui.visuals().selection.bg_fill;
    for quad in &shaped.quads {
        if quad.start >= from && quad.start < to {
            let box_ = quad
                .rect
                .translate(origin.to_vec2())
                .expand2(vec2(0.0, 2.0));
            ui.painter().rect_filled(box_, 0.0, fill);
        }
    }
    // The platform's copy key, which is what `ui::shortcut` spells `cmd+c`.
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::C))
        && let Some(text) = shaped.text.get(from as usize..to as usize)
    {
        ui.ctx().copy_text(text.to_string());
    }
}

/// The word around an offset, for the double click that takes one.
fn word_around(text: &str, at: u32) -> (u32, u32) {
    let at = (at as usize).min(text.len());
    let before = text[..at]
        .rfind(|c: char| c.is_whitespace())
        .map_or(0, |i| i + 1);
    let after = text[at..]
        .find(|c: char| c.is_whitespace())
        .map_or(text.len(), |i| at + i);
    (
        u32::try_from(before).unwrap_or(0),
        u32::try_from(after).unwrap_or(u32::MAX),
    )
}

/// A line under every `[url]` run, drawn one run at a time so the gaps
/// between letters are under it rather than in it.
fn underline(
    ui: &egui::Ui,
    shaped: &balaur_text::Shaped,
    origin: egui::Pos2,
    color: egui::Color32,
) {
    let mut run: Option<(u16, egui::Rect)> = None;
    let draw = |(_, box_): (u16, egui::Rect)| {
        let y = box_.max.y + 1.0;
        ui.painter()
            .hline(box_.min.x..=box_.max.x, y, egui::Stroke::new(1.0, color));
    };
    for quad in &shaped.quads {
        let box_ = quad.rect.translate(origin.to_vec2());
        match (quad.link, run) {
            (Some(link), Some((held, union)))
                if held == link && (union.max.y - box_.max.y).abs() < 1.0 =>
            {
                run = Some((link, union.union(box_)));
            }
            (Some(link), held) => {
                if let Some(held) = held {
                    draw(held);
                }
                run = Some((link, box_));
            }
            (None, held) => {
                if let Some(held) = held {
                    draw(held);
                }
                run = None;
            }
        }
    }
    if let Some(held) = run {
        draw(held);
    }
}

/// What a `[url]` or a `[hint]` span does under the pointer: the hand, the
/// hover text, and the call a click makes.
fn spans(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    response: &egui::Response,
    shaped: &balaur_text::Shaped,
    what: (egui::Pos2, balaur_core::hecs::Entity, &str),
) {
    let (origin, entity, on_link) = what;
    let Some(pos) = response.hover_pos() else {
        return;
    };
    let Some(quad) = glyph_at(shaped, origin, pos) else {
        return;
    };
    if let Some(hint) = quad.hint.and_then(|i| shaped.hints.get(i as usize)) {
        let id = egui::Id::new(("balaur-hint", entity, quad.hint));
        let over = ui.interact(
            quad.rect.translate(origin.to_vec2()),
            id,
            egui::Sense::hover(),
        );
        crate::widget::theme::tip(&over, hint);
    }
    let Some(target) = quad.link.and_then(|i| shaped.links.get(i as usize)) else {
        return;
    };
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    if response.clicked() && !on_link.is_empty() {
        at.edits.push((entity, Edit::Link(target.clone())));
    }
}

/// One line of typing. egui owns the caret and the selection; the buffer
/// is seeded from the widget's `text` and re-seeded when a script changes
/// it, so a script may clear a field without fighting the player.
pub(crate) fn field(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: egui::Color32,
) {
    edit(ui, at, index, font, color, false);
}

/// A `text_field` that keeps its newlines: Godot's `TextEdit`. `height` sizes it,
/// and everything a single line reads is read here too.
pub(crate) fn text_area(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: egui::Color32,
) {
    edit(ui, at, index, font, color, true);
}

fn edit(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: egui::Color32,
    multiline: bool,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let entity = placed.entity;
    let key = format!("widget:{}", entity.to_bits());
    let state = at.eng.resource::<crate::UiState>();
    let mut buffer = {
        let mut state = state.borrow_mut();
        if state.text_seeds.get(&key).map(String::as_str) != Some(widget.text.as_str()) {
            state
                .text_seeds
                .insert(key.clone(), widget.text.to_string());
            state
                .text_buffers
                .insert(key.clone(), widget.text.to_string());
        }
        state.text_buffers.get(&key).cloned().unwrap_or_default()
    };
    let look = at.look(index);
    crate::widget::theme::dress(ui, &look.style, color);
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    let mut edit = if multiline {
        egui::TextEdit::multiline(&mut buffer)
    } else {
        egui::TextEdit::singleline(&mut buffer)
    }
    .id(egui::Id::new(&key))
    .font(font.clone())
    .text_color(color)
    .hint_text(widget.placeholder.to_string())
    .password(widget.secret)
    .desired_width(if want.x > 0.0 {
        want.x
    } else {
        ui.available_width()
    });
    if multiline && want.y > 0.0 {
        edit = edit.desired_rows(
            (want.y / ui.text_style_height(&egui::TextStyle::Body)).max(1.0) as usize,
        );
    }
    if widget.max_length > 0.0 {
        edit = edit.char_limit(widget.max_length as usize);
    }
    let response = ui.add(edit);
    // Only on the pass focus was put here: asking every frame would take the
    // caret back from whatever the reader clicked next.
    if at.taking && at.focused == Some(entity) {
        response.request_focus();
    }
    if widget.numeric {
        buffer.retain(|c| c.is_ascii_digit() || matches!(c, '-' | '.'));
    }
    if response.changed() {
        at.edits.push((entity, Edit::Text(buffer.clone())));
    }
    if response.lost_focus() {
        at.edits.push((entity, Edit::Submit(buffer.clone())));
    }
    state.borrow_mut().text_buffers.insert(key, buffer);
}
