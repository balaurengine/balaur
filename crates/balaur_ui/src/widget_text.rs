//! Text in the widget layer: labels and captions through the shaper, and
//! the one line the player types into.

use egui::vec2;

use crate::vocabulary::words as w;
use crate::widget_arrange::box_of;
use crate::widget_layer::{Edit, Painting, Widget, weight_of};

/// What a widget's text asks the shaper for, at this scale.
///
/// Borrowed, not owned: this is built twice a widget a frame and the shaper
/// keeps nothing from it unless the cache misses.
pub(crate) fn text_request<'a>(
    widget: &'a Widget,
    caption: &'a str,
    width: Option<f32>,
    font: &egui::FontId,
    style: &'a crate::widget_theme::Style,
) -> crate::text::RequestRef<'a> {
    crate::text::RequestRef {
        text: caption,
        // The face the caller already resolved, so a role's `size` and
        // `strong` reach the shaper the way they reach egui's own text.
        size: font.size,
        weight: weight_of(style, widget).clamp(100.0, 900.0) as u16,
        italic: widget.font_style == w::ITALIC,
        width,
        align: match widget.text_align.as_str() {
            w::CENTER => crate::text::Align::Center,
            w::END => crate::text::Align::End,
            _ => crate::text::Align::Start,
        },
        markup: widget.markup,
        // A widget names no bitmap font yet; the world's text is where a
        // pixel face is asked for.
        font: "",
        family: crate::widget_layer::family_of(style, widget),
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
) -> Option<(std::rc::Rc<crate::text::Shaped>, Option<egui::TextureId>)> {
    let state = crate::text::state(at.eng)?;
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
    at: &Painting<'_>,
    index: usize,
    widget: &Widget,
    caption: &str,
    color: egui::Color32,
    font: &egui::FontId,
) -> bool {
    let Some(state) = crate::text::state(at.eng) else {
        return false;
    };
    let look = at.look(index);
    let style = &look.style;
    let room = ui.available_width();
    let width = widget.wrap.then_some(room.max(1.0));
    // A stated width is a column, so a long line is cut off at its edge
    // rather than run into whatever sits beside it.
    let column = (!widget.wrap && widget.width > 0.0).then_some(widget.width * at.scale);
    let (shaped, texture) = {
        let mut state = state.borrow_mut();
        let request = text_request(widget, caption, width, font, style);
        (state.shape_for_egui(ui.ctx(), &request), state.texture())
    };
    // An aligned line takes the width it is aligned in; a wrapped block
    // already did, and aligned its own lines.
    let take = if width.is_none() && widget.text_align != w::START {
        room.max(shaped.size.x)
    } else {
        shaped.size.x
    };
    let take = column.unwrap_or(take);
    let (rect, _) = ui.allocate_exact_size(vec2(take, shaped.size.y), egui::Sense::hover());
    let held = ui.clip_rect();
    if column.is_some() {
        ui.set_clip_rect(held.intersect(rect));
    }
    let slack = (take - shaped.size.x).max(0.0);
    let shift = match widget.text_align.as_str() {
        w::CENTER if width.is_none() => slack / 2.0,
        w::END if width.is_none() => slack,
        _ => 0.0,
    };
    let origin = rect.min + vec2(shift, 0.0);
    crate::text::paint(ui.painter(), texture, &shaped, origin, color, at.eng.time());
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
    ui.set_clip_rect(held);
    true
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

/// A `field` that keeps its newlines: Godot's `TextEdit`. `height` sizes it,
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
    let want = box_of(widget, at.assigned, at.scale);
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
