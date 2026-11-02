//! A number dragged sideways, or typed into after a click: what a
//! `number_field` is.
//!
//! Ported from egui 0.36's `DragValue` (MIT OR Apache-2.0, Emil Ernerfeldt
//! and the egui contributors), so the drag, the typing and the keys behave as
//! they did. The box is painted from egui's visuals, which the theme dresses,
//! and the number typed is shaped as a label is (`widget::typing`).

use std::ops::RangeInclusive;

use egui::{Key, Modifiers, Sense};

use crate::vocabulary::words as w;
use crate::widget::paint::Brush;
use crate::widget::typing::{Field, Typesetter, Typing};

/// How a number is spelled: the decimals a drag settles on, or a base.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spelling {
    Decimal,
    Binary,
    Octal,
    Hex,
}

impl Spelling {
    /// The `number_format` word, decimal for anything else.
    pub(crate) fn of(word: &str) -> Self {
        match word {
            w::BINARY => Self::Binary,
            w::OCTAL => Self::Octal,
            w::HEX => Self::Hex,
            _ => Self::Decimal,
        }
    }

    fn radix(self) -> Option<u32> {
        match self {
            Self::Decimal => None,
            Self::Binary => Some(2),
            Self::Octal => Some(8),
            Self::Hex => Some(16),
        }
    }

    /// The number in this spelling, a minus sign before the digits of a base.
    pub(crate) fn format(
        self,
        style: &egui::Style,
        n: f64,
        decimals: RangeInclusive<usize>,
    ) -> String {
        let sign = if n < 0.0 { "\u{2212}" } else { "" };
        let whole = n.abs() as i64;
        match self {
            Self::Decimal => style.number_formatter.format(n, decimals),
            Self::Binary => format!("{sign}{whole:b}"),
            Self::Octal => format!("{sign}{whole:o}"),
            Self::Hex => format!("{sign}{whole:x}"),
        }
    }

    /// What a reader typed, as a number: spaces and thousands gaps ignored,
    /// and the typographic minus taken as a minus.
    fn parse(self, text: &str) -> Option<f64> {
        if let Some(radix) = self.radix() {
            return i64::from_str_radix(text, radix).map(|n| n as f64).ok();
        }
        let text: String = text
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| if c == '\u{2212}' { '-' } else { c })
            .collect();
        text.parse().ok()
    }
}

/// What a dragged number is told.
pub(crate) struct DragNumber<'a> {
    pub(crate) value: &'a mut f64,
    /// Units a point of drag moves it.
    pub(crate) speed: f64,
    pub(crate) range: RangeInclusive<f64>,
    /// Hold a number set outside the range inside it, rather than show it.
    pub(crate) clamp_existing: bool,
    pub(crate) prefix: &'a str,
    pub(crate) suffix: &'a str,
    pub(crate) min_decimals: usize,
    pub(crate) max_decimals: Option<usize>,
    pub(crate) spelling: Spelling,
    /// Take every keystroke while typing, rather than the text left behind.
    pub(crate) update_while_editing: bool,
    pub(crate) eng: &'a balaur_core::Engine,
    /// What the typed number is shaped from: the widget's face.
    pub(crate) setter: Typesetter<'a>,
}

/// The value a typed number was typed over: a value changed by something
/// else meanwhile makes the text stale. Kept under the field's id with this.
const TYPED_OVER: &str = "typed-over";

/// What one pass of a dragged number knows before it draws.
#[allow(
    clippy::struct_excessive_bools,
    reason = "the drag's speed and the field's focus, each read in one place"
)]
struct Moment {
    /// One id whether it is typed into or dragged.
    id: egui::Id,
    /// Shift held while dragging: a tenth of the speed.
    slow: bool,
    /// How far the pointer may be off, in points.
    aim: f64,
    /// The decimals a drag at this speed settles on, and the most it shows.
    auto: usize,
    most: usize,
    typing: bool,
    gained: bool,
    lost: bool,
}

impl DragNumber<'_> {
    /// The text the number is held in while it is typed into, which the
    /// field keeps between passes.
    fn field(&self, id: egui::Id) -> Option<Field> {
        self.eng
            .resource::<crate::UiState>()
            .borrow_mut()
            .fields
            .remove(&id)
    }

    fn keep(&self, id: egui::Id, field: Field) {
        self.eng
            .resource::<crate::UiState>()
            .borrow_mut()
            .fields
            .insert(id, field);
    }

    #[allow(clippy::float_cmp, reason = "a value written, not one measured")]
    pub(crate) fn show(mut self, ui: &mut egui::Ui) -> egui::Response {
        let moment = self.moment(ui);
        let before = *self.value;
        self.step(ui, &moment);
        let shown = self
            .spelling
            .format(ui.style(), *self.value, moment.auto..=moment.most);
        self.keep_typed(ui, &moment, before);
        let mut response = if moment.typing {
            self.typed(ui, &moment, &shown, before)
        } else {
            self.dragged(ui, &moment, &shown)
        };
        if *self.value != before {
            response.mark_changed();
        }
        let (enabled, value) = (ui.is_enabled(), *self.value);
        response.widget_info(|| egui::WidgetInfo::drag_value(enabled, value));
        response
    }

    fn moment(&self, ui: &mut egui::Ui) -> Moment {
        let id = ui.next_auto_id();
        let shift = ui.input(|i| i.modifiers.shift_only());
        let slow = shift && ui.ctx().is_being_dragged(id);
        // Focus shows the field at once, not a pass later.
        let typing = ui.is_enabled()
            && ui.memory_mut(|mem| {
                mem.interested_in_focus(id, ui.layer_id());
                mem.has_focus(id)
            });
        // Whether the field held focus last pass, so this one knows it was
        // just given or just left.
        let held_focus = id.with("typing");
        let was = ui
            .data(|data| data.get_temp::<bool>(held_focus))
            .unwrap_or(false);
        let has = ui.memory(|mem| mem.has_focus(id));
        ui.data_mut(|data| data.insert_temp(held_focus, has));
        let (gained, lost) = (has && !was, was && !has);
        if gained {
            self.field(id);
        }
        let aim = f64::from(ui.input(egui::InputState::aim_radius));
        let (auto, most) = decimals(
            aim,
            self.speed,
            slow,
            (self.min_decimals, self.max_decimals),
        );
        Moment {
            id,
            slow,
            aim,
            auto,
            most,
            typing,
            gained,
            lost,
        }
    }

    /// Up and down step it while typing; left and right move the caret.
    #[allow(clippy::float_cmp, reason = "a value written, not one measured")]
    fn step(&mut self, ui: &mut egui::Ui, moment: &Moment) {
        let change = if moment.typing {
            ui.input_mut(|input| {
                input.count_and_consume_key(Modifiers::NONE, Key::ArrowUp) as f64
                    - input.count_and_consume_key(Modifiers::NONE, Key::ArrowDown) as f64
            })
        } else {
            0.0
        };
        let before = *self.value;
        let mut value = before;
        if self.clamp_existing {
            value = clamp(value, &self.range);
        }
        if change != 0.0 {
            value = egui::emath::round_to_decimals(value + self.speed * change, moment.auto);
        }
        if before != value {
            *self.value = value;
            self.field(moment.id);
        }
    }

    /// Left with Enter or a click elsewhere: what was typed is kept, unless
    /// the value moved under it.
    #[allow(clippy::float_cmp, reason = "a value written, not one measured")]
    fn keep_typed(&mut self, ui: &mut egui::Ui, moment: &Moment, before: f64) {
        if !moment.lost || ui.input(|i| i.key_pressed(Key::Escape)) {
            return;
        }
        let over = ui.data(|data| data.get_temp::<f64>(moment.id.with(TYPED_OVER)));
        if let Some(field) = self.field(moment.id)
            && over == Some(before)
            && let Some(parsed) = self.spelling.parse(field.editing.text())
        {
            *self.value = clamp(parsed, &self.range);
        }
    }

    /// The field typed into, keeping last pass's text unless the value
    /// moved under it, in the box the number rests in.
    #[allow(clippy::float_cmp, reason = "a value written, not one measured")]
    fn typed(
        &mut self,
        ui: &mut egui::Ui,
        moment: &Moment,
        shown: &str,
        before: f64,
    ) -> egui::Response {
        let id = moment.id;
        let over = ui.data(|data| data.get_temp::<f64>(id.with(TYPED_OVER)));
        let mut field = self
            .field(id)
            .unwrap_or_else(|| Field::new(shown, false, true));
        if over.is_some_and(|over| over != before) {
            field.editing.set_text(shown);
        }
        // The whole number as the field opens, so typing replaces it.
        if moment.gained {
            field.editing.set_text(shown);
            field.editing.select_all();
        }
        let pieces = faces(ui.ctx(), ui.style(), (self.prefix, shown, self.suffix));
        let size = face_size(ui.spacing(), &pieces);
        let (_, rect) = ui.allocate_space(size);
        let pad = ui.spacing().button_padding;
        let typed = Typing {
            id,
            multiline: false,
            secret: false,
            editable: true,
            clip: true,
            frame: true,
            tab_inserts: false,
            numeric: false,
            max_chars: 0,
            submit: Some(egui::KeyboardShortcut::new(Modifiers::NONE, Key::Enter)),
            align: ui.layout().horizontal_align(),
            margin: crate::widget::arrange::Pad::of([pad.x, pad.y, pad.x, pad.y]),
            color: ui.visuals().text_color(),
            placeholder: None,
            prefix: None,
            suffix: None,
            colors: None,
            underline: None,
        }
        .show(ui, self.eng, rect, &mut field, &self.setter);
        let take = if self.update_while_editing {
            typed.changed
        } else {
            typed.response.lost_focus() && !ui.input(|i| i.key_pressed(Key::Escape))
        };
        if take && let Some(parsed) = self.spelling.parse(field.editing.text()) {
            *self.value = clamp(parsed, &self.range);
        }
        let value = *self.value;
        ui.data_mut(|data| data.insert_temp(id.with(TYPED_OVER), value));
        self.keep(id, field);
        typed.response
    }

    /// The resting box: a click starts typing, a drag moves the number.
    fn dragged(&mut self, ui: &mut egui::Ui, moment: &Moment, shown: &str) -> egui::Response {
        let id = moment.id;
        let value = *self.value;
        let response = face(ui, (self.prefix, shown, self.suffix));
        let cursor = if value <= *self.range.start() {
            egui::CursorIcon::ResizeEast
        } else if value < *self.range.end() {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeWest
        };
        let response = response.on_hover_cursor(cursor);
        if ui.input(|i| i.pointer.any_pressed() || i.pointer.any_released()) {
            ui.data_mut(|data| data.remove::<f64>(id));
        }
        if response.clicked() {
            let mut field = Field::new(shown, false, true);
            field.editing.select_all();
            self.keep(id, field);
            ui.memory_mut(|mem| mem.request_focus(id));
            return response;
        }
        if !response.dragged() {
            return response;
        }
        ui.set_cursor_icon(cursor);
        let moved = response.drag_delta();
        let speed = if moment.slow {
            self.speed / 10.0
        } else {
            self.speed
        };
        let delta = f64::from(moved.x - moved.y) * speed;
        if delta == 0.0 {
            return response;
        }
        // The drag is kept at full precision; what shows is rounded.
        let precise = ui
            .data_mut(|data| data.get_temp::<f64>(id))
            .unwrap_or(value)
            + delta;
        let reach = moment.aim * speed;
        let aimed = egui::emath::smart_aim::best_in_range_f64(precise - reach, precise + reach);
        *self.value = clamp(
            egui::emath::round_to_decimals(aimed, moment.auto),
            &self.range,
        );
        ui.data_mut(|data| data.insert_temp::<f64>(id, precise));
        response
    }
}

impl egui::Widget for DragNumber<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        self.show(ui)
    }
}

/// The decimals a drag at `speed` settles on and the most it shows, with
/// `aim` how far the pointer may be off and `slow` a shift held through it.
pub(crate) fn decimals(
    aim: f64,
    speed: f64,
    slow: bool,
    (least, most): (usize, Option<usize>),
) -> (usize, usize) {
    let auto = libm::log10(aim / speed.abs()).ceil().clamp(0.0, 15.0) as usize + usize::from(slow);
    let most = most.unwrap_or(auto + 2).max(least);
    (auto.clamp(least, most), most)
}

/// The prefix, the number and the suffix as three runs of text side by side,
/// as egui lays a button's parts out: the number in egui's drag value style.
pub(crate) fn faces(
    ctx: &egui::Context,
    style: &egui::Style,
    (prefix, value, suffix): (&str, &str, &str),
) -> Vec<std::sync::Arc<egui::Galley>> {
    let number = style.drag_value_text_style.clone();
    [(prefix, false), (value, true), (suffix, false)]
        .into_iter()
        .filter(|(text, _)| !text.is_empty())
        .map(|(text, is_number)| {
            crate::theme::asked_of_egui(text);
            let mut rich = egui::RichText::new(text);
            if is_number {
                rich = rich.text_style(number.clone());
            }
            egui::WidgetText::from(rich).into_galley_impl(
                ctx,
                style,
                egui::text::TextWrapping::no_max_width(),
                egui::TextStyle::Button.into(),
                egui::Align::Center,
            )
        })
        .collect()
}

/// The runs laid end to end.
fn runs_size(pieces: &[std::sync::Arc<egui::Galley>]) -> egui::Vec2 {
    pieces.iter().fold(egui::Vec2::ZERO, |sum, piece| {
        egui::vec2(sum.x + piece.size().x, sum.y.max(piece.size().y))
    })
}

/// The box a number rests in, typed into or not: its runs in a button's
/// padding, never smaller than a control.
pub(crate) fn face_size(
    spacing: &egui::style::Spacing,
    pieces: &[std::sync::Arc<egui::Galley>],
) -> egui::Vec2 {
    (runs_size(pieces) + spacing.button_padding * 2.0).max(spacing.interact_size)
}

/// The resting box: the number in egui's button frame, dragged or clicked.
fn face(ui: &mut egui::Ui, (prefix, value, suffix): (&str, &str, &str)) -> egui::Response {
    let pieces = faces(ui.ctx(), ui.style(), (prefix, value, suffix));
    let inner = runs_size(&pieces);
    let pad = ui.spacing().button_padding;
    let (id, rect) = ui.allocate_space(face_size(ui.spacing(), &pieces));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    if ui.is_rect_visible(rect) {
        let look = ui.style().interact(&response);
        Brush::of(ui).plate(
            rect.expand(look.expansion),
            look.corner_radius,
            look.weak_bg_fill,
            look.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let placed = egui::Align2([ui.layout().horizontal_align(), ui.layout().vertical_align()]);
        let content = placed.align_size_within_rect(inner, rect.shrink2(pad));
        let mut x = content.min.x;
        for piece in pieces {
            let at = egui::pos2(x, content.center().y - piece.size().y / 2.0);
            x += piece.size().x;
            Brush::of(ui).galley(at, piece, look.text_color());
        }
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, value));
    response
}

/// Clamp with care for negative zero and a range given backwards.
pub(crate) fn clamp(x: f64, range: &RangeInclusive<f64>) -> f64 {
    use std::cmp::Ordering;
    let (mut low, mut high) = (*range.start(), *range.end());
    if low.total_cmp(&high) == Ordering::Greater {
        (low, high) = (high, low);
    }
    match x.total_cmp(&low) {
        Ordering::Less | Ordering::Equal => low,
        Ordering::Greater => match x.total_cmp(&high) {
            Ordering::Greater | Ordering::Equal => high,
            Ordering::Less => x,
        },
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "a clamp answers one of its ends exactly")]
mod tests {
    use super::{Spelling, clamp};

    #[test]
    fn a_clamp_keeps_the_sign_of_zero_and_takes_a_range_backwards() {
        assert!(clamp(-0.0, &(0.0..=f64::MAX)).is_sign_positive());
        assert!(clamp(0.0, &(-1.0..=-0.0)).is_sign_negative());
        assert_eq!(clamp(-25.0, &(-1.0..=1.0)), -1.0);
        assert_eq!(clamp(5.0, &(10.0..=1.0)), 5.0);
        assert_eq!(clamp(15.0, &(5.0..=1.0)), 5.0);
    }

    #[test]
    fn a_typed_number_ignores_gaps_and_takes_the_typographic_minus() {
        assert_eq!(Spelling::Decimal.parse(" 1 234.5 "), Some(1234.5));
        assert_eq!(Spelling::Decimal.parse("\u{2212}2"), Some(-2.0));
        assert_eq!(Spelling::Hex.parse("ff"), Some(255.0));
        assert_eq!(Spelling::Binary.parse("102"), None);
    }
}
