//! Text in the widget layer: labels and captions through the shaper, and
//! the one line the player types into.

use egui::vec2;

use crate::widget_arrange::box_of;
use crate::widget_layer::{Edit, Painting, Widget};

/// What a widget's text asks the shaper for, at this scale.
pub(crate) fn text_request(
    widget: &Widget,
    caption: &str,
    scale: f32,
    width: Option<f32>,
) -> crate::text::Request {
    crate::text::Request {
        text: caption.to_string(),
        size: widget.font_size * scale,
        weight: widget.font_weight.clamp(100.0, 900.0) as u16,
        italic: widget.font_style == "italic",
        width,
        align: match widget.text_align.as_str() {
            "center" => crate::text::Align::Center,
            "end" => crate::text::Align::End,
            _ => crate::text::Align::Start,
        },
        markup: widget.markup,
    }
}

/// A caption shaped on one line, with the atlas it draws from; `None` until
/// the fonts are installed, when egui's own layout stands in.
pub(crate) fn shaped_caption(
    ui: &egui::Ui,
    at: &Painting<'_>,
    widget: &Widget,
    caption: &str,
) -> Option<(std::rc::Rc<crate::text::Shaped>, Option<egui::TextureId>)> {
    let state = crate::text::state(at.eng)?;
    let mut state = state.borrow_mut();
    let request = text_request(widget, caption, at.scale, None);
    let shaped = state.shape(ui.ctx(), &request);
    Some((shaped, state.texture()))
}

/// Draw a label through the shaper. Answers false when the shaper is not
/// up yet, so the caller may fall back to egui's own text.
pub(crate) fn shaped_label(
    ui: &mut egui::Ui,
    at: &Painting<'_>,
    widget: &Widget,
    caption: &str,
    color: egui::Color32,
) -> bool {
    let Some(state) = crate::text::state(at.eng) else {
        return false;
    };
    let room = ui.available_width();
    let width = widget.wrap.then_some(room.max(1.0));
    let (shaped, texture) = {
        let mut state = state.borrow_mut();
        let request = text_request(widget, caption, at.scale, width);
        (state.shape(ui.ctx(), &request), state.texture())
    };
    // An aligned line takes the width it is aligned in; a wrapped block
    // already did, and aligned its own lines.
    let take = if width.is_none() && widget.text_align != "start" {
        room.max(shaped.size.x)
    } else {
        shaped.size.x
    };
    let (rect, _) = ui.allocate_exact_size(vec2(take, shaped.size.y), egui::Sense::hover());
    let slack = (take - shaped.size.x).max(0.0);
    let shift = match widget.text_align.as_str() {
        "center" if width.is_none() => slack / 2.0,
        "end" if width.is_none() => slack,
        _ => 0.0,
    };
    let origin = rect.min + vec2(shift, 0.0);
    crate::text::paint(ui.painter(), texture, &shaped, origin, color, at.eng.time());
    for picture in &shaped.pictures {
        if let Ok(handle) = crate::widgets::texture_of(at.eng, ui.ctx(), &picture.path) {
            ui.painter().image(
                handle.id(),
                picture.rect.translate(origin.to_vec2()),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
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
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let entity = placed.entity;
    let key = format!("widget:{}", entity.to_bits());
    let state = at.eng.resource::<crate::UiState>();
    let mut buffer = {
        let mut state = state.borrow_mut();
        if state.text_seeds.get(&key) != Some(&widget.text) {
            state.text_seeds.insert(key.clone(), widget.text.clone());
            state.text_buffers.insert(key.clone(), widget.text.clone());
        }
        state.text_buffers.get(&key).cloned().unwrap_or_default()
    };
    let want = box_of(widget, at.assigned, at.scale);
    let mut edit = egui::TextEdit::singleline(&mut buffer)
        .id(egui::Id::new(&key))
        .font(font.clone())
        .text_color(color)
        .hint_text(widget.placeholder.clone())
        .password(widget.secret)
        .desired_width(if want.x > 0.0 {
            want.x
        } else {
            ui.available_width()
        });
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
