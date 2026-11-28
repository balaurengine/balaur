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
use crate::widget::paint::Brush;
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

/// A field's ids: its sensor's, one per layer, and its kept state's.
const FIELD: &str = "balaur-field";

/// The id of a field's menu of `options`, under its sensor's.
const OPTIONS: &str = "balaur-field-options";

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
            // What is typed is shown as typed, or the caret would stand
            // among letters of another length.
            case: balaur_text::Case::AsWritten,
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
    let submit = submit_of(widget);
    // Mirrored, the line starts at the right and the icon sits there.
    let flip = crate::widget::direction::mirrored(at.arena, index);
    let mut typing = Typing {
        id: crate::widget::layer::sensor_id(ui, FIELD, entity),
        multiline,
        secret: widget.secret,
        editable: widget.egui.editable,
        clip: true,
        frame: true,
        tab_inserts: widget.egui.tab_inserts,
        keeps_escape: false,
        numeric: widget.numeric,
        max_chars: widget.max_length as usize,
        submit: Some(submit),
        align: crate::widget::direction::physical_align(
            crate::widget::theme::text_align_of(&look.style, widget),
            flip,
        ),
        mirrored: flip,
        margin: margin_of(widget, &look.style),
        color,
        placeholder: face.shape(&placeholder_of(at.eng, widget), None),
        icon: lead_icon(ui, at, index, font),
        prefix: face.shape(&widget.egui.prefix, None),
        suffix: face.shape(&widget.suffix, None),
        colors: None,
        underline: None,
    };
    let wrap = multiline.then(|| typing.inner(rect).width().max(1.0));
    let setter = Typesetter::Plain(face.request(wrap));
    // The node's, whatever layer draws it: a moved field keeps its caret.
    let key = egui::Id::unique((FIELD, entity));
    let state = at.eng.resource::<crate::UiState>();
    let mut field = state
        .borrow_mut()
        .fields
        .remove(&key)
        .unwrap_or_else(|| Field::new(&widget.text, multiline, widget.egui.caret_at_end));
    field.refill(&widget.text);
    field.carry_focus(ui, typing.id);
    // Only on the pass focus was put here: asking every frame would take the
    // caret back from whatever the reader clicked next. Focus given rather
    // than clicked into selects what the field holds, so typing replaces it.
    if at.taking && at.focused == Some(entity) {
        ui.memory_mut(|mem| mem.request_focus(typing.id));
        field.editing.select_all();
    }
    let offers = !multiline && widget.egui.editable && !widget.options.is_empty();
    let walked = offers
        .then(|| walk_options(ui, typing.id, widget.options.len(), &mut field.offered))
        .flatten();
    // Escape is the menu's to fold while it hangs, not egui's to leave by.
    typing.keeps_escape = offers && !field.offered.folded;
    let keep = offers && field.offered.pressed_on(ui);
    let typed = keeping_focus(ui, keep, |ui| {
        typing.show(ui, at.eng, rect, &mut field, &setter)
    });
    ui.advance_cursor_after_rect(rect);
    if offers {
        let faced = (&*look, font, color, at.slant(index));
        offer(
            ui,
            at,
            index,
            ((rect, &typed), faced),
            (&mut field.offered, walked),
        );
    }
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
    state.borrow_mut().fields.insert(key, field);
}

/// The chord that submits a field: its `submit_key`, else Enter.
fn submit_of(widget: &Widget) -> egui::KeyboardShortcut {
    crate::chord::chord(&widget.egui.submit_key).map_or(
        egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Enter),
        |(modifiers, key)| egui::KeyboardShortcut::new(modifiers, key),
    )
}

/// `show` with egui keeping the keyboard where `keep` says, as a press on a
/// field's menu asks: egui would let go of it in the field's own sensor,
/// for a press elsewhere, before the menu is drawn.
fn keeping_focus<R>(ui: &mut egui::Ui, keep: bool, show: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if !keep {
        return show(ui);
    }
    let was = ui.ctx().options_mut(|options| {
        std::mem::replace(
            &mut options.input_options.surrender_focus_on,
            egui::SurrenderFocusOn::Never,
        )
    });
    let shown = show(ui);
    ui.ctx()
        .options_mut(|options| options.input_options.surrender_focus_on = was);
    shown
}

/// The keys a field's menu of `options` takes before the field reads them,
/// held alone: the arrows walk its rows, Escape folds it, and Enter picks
/// the row walked to, which it answers. Tab folds it and moves on, so the
/// keyboard reaches the next control rather than a row.
fn walk_options(
    ui: &mut egui::Ui,
    id: egui::Id,
    rows: usize,
    offered: &mut crate::widget::typing::Offered,
) -> Option<usize> {
    use egui::{Key, Modifiers};
    if !offered.hangs(ui, id) {
        return None;
    }
    let last = rows - 1;
    ui.input_mut(|i| {
        if i.key_pressed(Key::Tab) {
            offered.folded = true;
        }
        if !i.modifiers.is_none() {
            return None;
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            offered.pointed = false;
            offered.walked = Some(offered.walked.map_or(0, |row| (row + 1).min(last)));
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            offered.pointed = false;
            offered.walked = offered.walked.and_then(|row| row.checked_sub(1));
        }
        if i.consume_key(Modifiers::NONE, Key::Escape) {
            offered.folded = true;
        }
        offered
            .walked
            .filter(|row| *row <= last)
            .filter(|_| i.consume_key(Modifiers::NONE, Key::Enter))
    })
}

/// What a field's menu of `options` does once the field has drawn: typing,
/// or the keyboard coming or going, unfolds it with nothing walked; it
/// hangs; and the row `walked` picked by Enter or one a press picked folds
/// it and says so.
fn offer(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    ((rect, typed), faced): ((Rect, &crate::widget::typing::Typed), Faced<'_>),
    (offered, walked): (&mut crate::widget::typing::Offered, Option<usize>),
) {
    let response = &typed.response;
    if typed.changed || response.gained_focus() || response.lost_focus() {
        *offered = crate::widget::typing::Offered::default();
    }
    let pressed = hang_options(ui, at, index, (rect, response, faced), offered);
    if let Some(row) = walked.or(pressed) {
        offered.folded = true;
        let placed = &at.arena[index];
        let option = placed.widget.options[row].to_string();
        at.edits.push((placed.entity, Edit::OptionPicked(option)));
    }
}

/// The face a field's menu draws its rows in: the field's look, font, ink
/// and whether egui slants it.
type Faced<'a> = (
    &'a crate::widget::arena::Look,
    &'a egui::FontId,
    egui::Color32,
    bool,
);

/// A field's menu of `options`, hung under it over what sits below while the
/// field holds the keyboard, as wide as it and kept inside its screen as a
/// dropdown's list is, in the role's `body` plate where it states one; the
/// row a press picked. The pointer moving over a row walks to it, and off
/// the rows lets go of the row it walked to.
fn hang_options(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    (rect, response, (look, font, ink, slant)): (Rect, &egui::Response, Faced<'_>),
    offered: &mut crate::widget::typing::Offered,
) -> Option<usize> {
    let placed = &at.arena[index];
    let (widget, entity) = (&placed.widget, placed.entity);
    let id = response.id;
    offered.menu = None;
    if !offered.hangs(ui, id) {
        return None;
    }
    let body = look.style.body.as_deref();
    let mut popup = egui::Popup::from_response(response)
        .id(id.with(OPTIONS))
        .open(true)
        .width(rect.width())
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .style(egui::containers::menu::menu_style)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks);
    if body.is_some() {
        popup = popup.frame(egui::Frame::NONE);
    }
    let mut met = None;
    let shown = crate::widget::popups::kept_inside(popup, egui::RectAlign::BOTTOM_START, &[], 0.0)
        .show(|ui| {
            ui.set_min_width(rect.width());
            crate::widget::sheet::on_plate(ui, at.eng, body, None, |ui| {
                let height = crate::widget::kinds::list_height(widget, ui);
                egui::ScrollArea::vertical()
                    .max_height(height)
                    .show(ui, |ui| {
                        let lit = |row| offered.walked == Some(row);
                        let face = (font, ink, slant, &*widget.text_look);
                        met = Some(crate::widget::kinds::option_rows(
                            ui,
                            &widget.options,
                            face,
                            lit,
                        ));
                    });
            });
        });
    if let Some(shown) = shown {
        offered.menu = Some(shown.response.rect);
        at.shown.push(entity);
    }
    let met = met?;
    if ui.input(|i| i.pointer.delta() != Vec2::ZERO) {
        match met.hovered {
            Some(row) => {
                offered.walked = Some(row);
                offered.pointed = true;
            }
            None if offered.pointed => {
                offered.walked = None;
                offered.pointed = false;
            }
            None => {}
        }
    }
    met.clicked
}

/// The field's `icon` in the theme's icon font at its text's size, in its
/// role's `icon_color` or the placeholder's quiet ink; none without one.
fn lead_icon(
    ui: &egui::Ui,
    at: &Painting<'_>,
    index: usize,
    font: &egui::FontId,
) -> Option<(std::sync::Arc<egui::Galley>, egui::Color32)> {
    let widget = &at.arena[index].widget;
    if widget.icon.is_empty() {
        return None;
    }
    let look = at.look(index);
    let glyph = crate::widget::icons::Icons::of(at.eng, &look).glyph(&widget.icon)?;
    let face = egui::FontId::new(font.size, look.icon.clone());
    let ink = look
        .style
        .icon_color
        .unwrap_or_else(|| ui.visuals().weak_text_color());
    Some((
        Brush::of(ui).line_of(glyph, face, egui::Color32::WHITE),
        ink,
    ))
}

/// A field focus left: the submit chord and a click away both submit, and
/// only the click away is a blur. Escape is a blur that keeps nothing, as it
/// is on a number, and so is `ui::clear_focus`.
fn left(
    ui: &egui::Ui,
    at: &mut Painting<'_>,
    entity: Entity,
    text: &str,
    submit: egui::KeyboardShortcut,
) {
    let kept = ui.input(|i| i.key_pressed(egui::Key::Escape)) || crate::widget::focus::let_go();
    if !kept {
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

/// What a field shows while it is empty: its `placeholder_key` read in the
/// locale in force, else its `placeholder`.
pub(crate) fn placeholder_of(eng: &balaur_core::Engine, widget: &Widget) -> smol_str::SmolStr {
    if widget.placeholder_key.is_empty() {
        return widget.placeholder.clone();
    }
    balaur_core::strings::plain(eng, &widget.placeholder_key)
}
