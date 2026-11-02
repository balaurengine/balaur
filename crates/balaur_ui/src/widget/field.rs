//! The kinds a reader types a line or a block into, `text_field` and
//! `text_area`: drawn by [`crate::widget::typing`] into the box the layout
//! gave them, and measured by the same arithmetic the draw sizes them by.

use std::rc::Rc;

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use egui::{Rect, Vec2, vec2};

use crate::widget::arrange::{Pad, padding_of, padding_only, solved_of};
use crate::widget::layer::{Edit, Painting};
use crate::widget::node::Widget;
use crate::widget::theme::Style;
use crate::widget::typing::{Field, Typesetter, Typing};

/// The air round a field's text where it states no padding: egui's text
/// edit margin.
const MARGIN: Vec2 = vec2(4.0, 2.0);

/// The lines a `text_area` has room for at least, as egui's multiline edit.
const AREA_ROWS: f32 = 4.0;

/// What a field holds room for before a container or a `width` says
/// otherwise: a dozen wide letters.
const ROOM: &str = "MMMMMMMMMMMM";

/// The space between a field's edge and its text: its padding and border,
/// and egui's margin round the text where it states no padding.
pub(crate) fn margin_of(widget: &Widget, style: &Style) -> Pad {
    let pad = padding_of(widget, style);
    if padding_only(widget, style).taken() != Vec2::ZERO {
        return pad;
    }
    Pad::of([
        pad.left + MARGIN.x,
        pad.top + MARGIN.y,
        pad.right + MARGIN.x,
        pad.bottom + MARGIN.y,
    ])
}

/// What a field's text is shaped from: the widget, whose faces it draws in,
/// its face and its look.
pub(crate) struct Face<'a> {
    pub(crate) eng: &'a Engine,
    pub(crate) ctx: &'a egui::Context,
    pub(crate) widget: &'a Widget,
    pub(crate) project: balaur_text::FontProject,
    pub(crate) font: &'a egui::FontId,
    pub(crate) style: &'a Style,
}

impl Face<'_> {
    /// What the text is laid out from, as a label's, plain and uncut, broken
    /// at `width` when there is one.
    pub(crate) fn request(&self, width: Option<f32>) -> balaur_text::RequestRef<'_> {
        balaur_text::RequestRef {
            markup: false,
            truncate: false,
            align: balaur_text::Align::Start,
            ..crate::widget::text::text_request(
                (self.widget, self.project),
                "",
                width,
                self.font,
                self.style,
            )
        }
    }

    /// `text` shaped as a label of it is, at `width`; `None` for no text or
    /// while the shaper is not up.
    pub(crate) fn shape(&self, text: &str, width: Option<f32>) -> Option<Rc<balaur_text::Shaped>> {
        if text.is_empty() {
            return None;
        }
        let state = balaur_text::state(self.eng)?;
        let request = balaur_text::RequestRef {
            text,
            ..self.request(width)
        };
        let shaped = state.borrow_mut().shape_for_egui(self.ctx, &request);
        Some(shaped)
    }

    fn size_of(&self, text: &str, width: Option<f32>) -> Vec2 {
        self.shape(text, width)
            .map_or(Vec2::ZERO, |shaped| shaped.size)
    }

    /// What the text before and after the typing takes, gaps included.
    fn affixes(&self) -> f32 {
        [
            self.widget.egui.prefix.as_str(),
            self.widget.suffix.as_str(),
        ]
        .into_iter()
        .map(|affix| self.size_of(affix, None).x)
        .filter(|width| *width > 0.0)
        .map(|width| width + crate::widget::typing::AFFIX_GAP)
        .sum()
    }

    /// The box the field asks for, padding included: room for a dozen wide
    /// letters on a line, the whole text where `clip_text` is off, and a
    /// `text_area`'s text broken at that width, four lines at least.
    pub(crate) fn natural(&self, multiline: bool) -> Vec2 {
        let widget = self.widget;
        let margin = margin_of(widget, self.style).taken();
        let room = self.size_of(ROOM, None);
        let floor = self.ctx.global_style().spacing.interact_size.y;
        if multiline {
            let across = if widget.width > 0.0 {
                (widget.width - margin.x).max(1.0)
            } else {
                room.x
            };
            let text = self.size_of(&widget.text, Some(across));
            let down = text.y.max(room.y * AREA_ROWS);
            return vec2(across + margin.x, (down + margin.y).max(floor));
        }
        let mut across = room.x;
        if !widget.egui.clip_text {
            let shown = balaur_text::edit::Editing::new(&widget.text, false).shown(widget.secret);
            across = across.max(self.size_of(&shown.text, None).x);
        }
        vec2(
            across + self.affixes() + margin.x,
            (room.y + margin.y).max(floor),
        )
    }
}

/// A `text_field` or a `text_area`, typed into. The text lives on the
/// widget: a script writing a new one refills the field, and what the reader
/// types is reported for the next tick to write.
pub(crate) fn edit(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    (font, color): (&egui::FontId, egui::Color32),
    multiline: bool,
) {
    let placed = &at.arena[index];
    let (widget, entity) = (&placed.widget, placed.entity);
    let look = at.look(index);
    crate::widget::theme::dress(ui, &look.style, color);
    let ctx = ui.ctx().clone();
    let face = Face {
        eng: at.eng,
        ctx: &ctx,
        widget,
        project: placed.project,
        font,
        style: &look.style,
    };
    let natural = face.natural(multiline);
    let want = solved_of(widget, &at.style(index), at.assigned);
    let pick = |want: f32, natural: f32| if want > 0.0 { want } else { natural };
    let mut size = vec2(pick(want.x, natural.x), pick(want.y, natural.y));
    if !multiline && !widget.egui.clip_text {
        size.x = size.x.max(natural.x);
    }
    let rect = Rect::from_min_size(ui.max_rect().min, size);
    let submit = crate::chord::chord(&widget.egui.submit_key).map_or(
        egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Enter),
        |(modifiers, key)| egui::KeyboardShortcut::new(modifiers, key),
    );
    let typing = Typing {
        id: egui::Id::unique(("balaur-field", entity)),
        multiline,
        secret: widget.secret,
        editable: widget.egui.editable,
        clip: true,
        frame: true,
        tab_inserts: widget.egui.tab_inserts,
        numeric: widget.numeric,
        max_chars: widget.max_length as usize,
        submit: Some(submit),
        align: crate::widget::layer::across(crate::widget::theme::text_align_of(
            &look.style,
            widget,
        )),
        margin: margin_of(widget, &look.style),
        color,
        placeholder: face.shape(&widget.placeholder, None),
        prefix: face.shape(&widget.egui.prefix, None),
        suffix: face.shape(&widget.suffix, None),
        colors: None,
        underline: None,
    };
    let wrap = multiline.then(|| typing.inner(rect).width().max(1.0));
    let setter = Typesetter::Plain(face.request(wrap));
    let state = at.eng.resource::<crate::UiState>();
    let mut field = state
        .borrow_mut()
        .fields
        .remove(&typing.id)
        .unwrap_or_else(|| Field::new(&widget.text, multiline, widget.egui.caret_at_end));
    field.refill(&widget.text);
    // Only on the pass focus was put here: asking every frame would take the
    // caret back from whatever the reader clicked next. Focus given rather
    // than clicked into selects what the field holds, so typing replaces it.
    if at.taking && at.focused == Some(entity) {
        ui.memory_mut(|mem| mem.request_focus(typing.id));
        field.editing.select_all();
    }
    let typed = typing.show(ui, at.eng, rect, &mut field, &setter);
    ui.advance_cursor_after_rect(rect);
    // A click into the field is focus arriving, as Tab reaching it is.
    if typed.response.gained_focus()
        && let Some(focus) = at.eng.try_resource::<crate::UiFocus>()
    {
        focus.borrow_mut().focused = Some(entity);
    }
    if typed.changed {
        at.edits
            .push((entity, Edit::Text(field.editing.text().to_string())));
    }
    if typed.response.lost_focus() {
        left(ui, at, entity, field.editing.text(), submit);
    }
    field.owner = Some(entity);
    state.borrow_mut().fields.insert(typing.id, field);
}

/// A field focus left: the submit chord and a click away both submit, and
/// only the click away is a blur. Escape is a blur that keeps nothing, as it
/// is on a number.
fn left(
    ui: &egui::Ui,
    at: &mut Painting<'_>,
    entity: Entity,
    text: &str,
    submit: egui::KeyboardShortcut,
) {
    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        at.edits.push((entity, Edit::Submit(text.to_owned())));
    }
    if ui.input(|i| i.key_pressed(submit.logical_key)) {
        return;
    }
    at.edits.push((entity, Edit::Blurred));
    if let Some(focus) = at.eng.try_resource::<crate::UiFocus>() {
        let mut focus = focus.borrow_mut();
        if focus.focused == Some(entity) {
            focus.focused = None;
        }
    }
}
