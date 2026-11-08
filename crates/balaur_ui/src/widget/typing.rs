//! Text a reader types: a `text_field`, a `text_area`, a `code` node and a
//! number being typed. Laid out by the shaper a label is laid out by, so
//! Arabic, Hindi or Thai typed into a field draws as it does in a label, and
//! painted here: the caret through text running either way, the selection,
//! the keys, the clipboard and an input method's composition
//! (docs/PLAN-ui-gaps.md §1).

use std::hash::{Hash as _, Hasher as _};
use std::ops::Range;
use std::rc::Rc;

use balaur_core::Engine;
use balaur_text::edit::{Change, Clusters, Editing, Laid, Motion};
use egui::{Color32, Key, Rect, Sense, Vec2, pos2, vec2};

use crate::widget::arrange::Pad;
use crate::widget::paint::Brush;

/// A text laid out for typing: its glyphs in blocks, one a line where a file
/// is laid out a line at a time, and the boxes its caret, clicks and
/// selection are measured against.
pub(crate) struct Typeset {
    pub(crate) blocks: Vec<Block>,
    pub(crate) clusters: Clusters,
    pub(crate) size: Vec2,
}

/// One block of glyphs: where its top-left sits in the text, and the bytes of
/// the text it draws.
pub(crate) struct Block {
    pub(crate) laid: Rc<Laid>,
    pub(crate) at: Vec2,
    pub(crate) bytes: Range<usize>,
}

/// What a typesetter keeps between passes, so a text that did not move is not
/// laid out again: what it was laid out under, the typeset, and a file's
/// lines, each laid out apart.
#[derive(Default)]
pub(crate) struct Kept {
    face: u64,
    text: u64,
    typeset: Option<Rc<Typeset>>,
    lines: Vec<(String, Rc<Laid>)>,
}

/// How a text is laid out: the request a label of it would make, its text
/// left out.
#[derive(Clone, Copy)]
pub(crate) enum Typesetter<'a> {
    /// As a label is: one block, wrapped at the request's width.
    Plain(balaur_text::RequestRef<'a>),
    /// A line at a time and never wrapped: a file, where typing on one line
    /// lays out that line alone.
    Lines(balaur_text::RequestRef<'a>),
}

fn hash_of(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl Typesetter<'_> {
    fn request(&self) -> &balaur_text::RequestRef<'_> {
        match self {
            Self::Plain(request) | Self::Lines(request) => request,
        }
    }

    /// `text` laid out, from `kept` when it was laid out the same before.
    pub(crate) fn set(
        &self,
        eng: &Engine,
        ctx: &egui::Context,
        text: &str,
        kept: &mut Kept,
    ) -> Rc<Typeset> {
        let Some(state) = balaur_text::state(eng) else {
            return Rc::new(Typeset {
                blocks: Vec::new(),
                clusters: Clusters::default(),
                size: Vec2::ZERO,
            });
        };
        let mut state = state.borrow_mut();
        let scale = ctx.pixels_per_point();
        let bare = balaur_text::RequestRef {
            text: "",
            ..*self.request()
        };
        let face = bare.fingerprint() ^ state.generation() ^ u64::from(scale.to_bits());
        let hash = hash_of(text);
        if kept.face != face {
            *kept = Kept {
                face,
                ..Kept::default()
            };
        }
        if kept.text == hash
            && let Some(typeset) = &kept.typeset
        {
            return Rc::clone(typeset);
        }
        let lay = |state: &mut balaur_text::TextState, line: &str| {
            let request = balaur_text::RequestRef { text: line, ..bare };
            Rc::new(state.editable(&request, scale))
        };
        let typeset = match self {
            Self::Plain(_) => {
                let laid = lay(&mut state, text);
                Typeset {
                    size: laid.shaped.size,
                    clusters: laid.clusters.clone(),
                    blocks: vec![Block {
                        laid,
                        at: Vec2::ZERO,
                        bytes: 0..text.len(),
                    }],
                }
            }
            Self::Lines(_) => {
                kept.lines = relaid(&kept.lines, text, |line| lay(&mut state, line));
                stacked(&kept.lines)
            }
        };
        state.flush_egui(ctx);
        let typeset = Rc::new(typeset);
        kept.text = hash;
        kept.typeset = Some(Rc::clone(&typeset));
        typeset
    }
}

/// A file's lines laid out again: the run at the start and the run at the
/// end that did not change kept as they were, the lines between laid out.
fn relaid(
    old: &[(String, Rc<Laid>)],
    text: &str,
    mut lay: impl FnMut(&str) -> Rc<Laid>,
) -> Vec<(String, Rc<Laid>)> {
    let wanted: Vec<&str> = text.split('\n').collect();
    let head = old
        .iter()
        .zip(&wanted)
        .take_while(|((had, _), want)| had == *want)
        .count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(wanted[head..].iter().rev())
        .take_while(|((had, _), want)| had == *want)
        .count();
    let mut lines = Vec::with_capacity(wanted.len());
    lines.extend(old[..head].iter().cloned());
    for line in &wanted[head..wanted.len() - tail] {
        lines.push(((*line).to_string(), lay(line)));
    }
    lines.extend(old[old.len() - tail..].iter().cloned());
    lines
}

/// The lines one under the next, a row each, read as one text.
fn stacked(lines: &[(String, Rc<Laid>)]) -> Typeset {
    // Every row as tall as a line with text in it: an empty one is measured
    // without a face, a hair off the rest.
    let row = lines
        .iter()
        .find(|(line, _)| !line.is_empty())
        .or_else(|| lines.first())
        .map_or(0.0, |(_, laid)| laid.clusters.lines[0].height);
    let mut clusters = Clusters::default();
    let mut blocks = Vec::with_capacity(lines.len());
    let (mut base, mut width) = (0, 0.0f32);
    for (row_index, (line, laid)) in lines.iter().enumerate() {
        let top = row_index as f32 * row;
        clusters.append(&laid.clusters, base, top);
        blocks.push(Block {
            laid: Rc::clone(laid),
            at: vec2(0.0, top),
            bytes: base..base + line.len(),
        });
        width = width.max(laid.shaped.size.x);
        base += line.len() + 1;
    }
    Typeset {
        blocks,
        clusters,
        size: vec2(width, row * lines.len() as f32),
    }
}

/// What a field keeps between passes.
#[derive(Default)]
pub(crate) struct Field {
    /// The node it is kept for, let go with it; a row of a tool has none.
    pub(crate) owner: Option<balaur_core::hecs::Entity>,
    pub(crate) editing: Editing,
    /// What it was last filled from: a script writing a new text refills it,
    /// and the reader's own typing coming back as the same text does not.
    pub(crate) seed: Option<String>,
    /// How far the text is scrolled inside its box.
    offset: Vec2,
    /// When the caret last moved, which the blink counts from.
    moved_at: f64,
    pub(crate) kept: Kept,
}

impl Field {
    /// A field holding `text`, its caret at the end or the start.
    pub(crate) fn new(text: &str, multiline: bool, caret_at_end: bool) -> Self {
        let mut editing = Editing::new(text, multiline);
        if !caret_at_end {
            editing.place(0, false);
        }
        Self {
            editing,
            seed: Some(text.to_string()),
            ..Self::default()
        }
    }

    /// Filled from `text` when it is not what the field was last filled from:
    /// a script's write. The caret stays where it was when it still fits.
    pub(crate) fn refill(&mut self, text: &str) {
        if self.seed.as_deref() != Some(text) {
            self.editing.set_text(text);
            self.seed = Some(text.to_string());
        }
    }
}

/// Each line's colours by the byte each starts at.
pub(crate) type LineColors<'a> = &'a dyn Fn(&str) -> Vec<(u32, Color32)>;

/// How a field types and looks: what a kind tells [`Typing::show`].
#[allow(
    clippy::struct_excessive_bools,
    reason = "a switch a row of `widget_field`, each read in one place"
)]
pub(crate) struct Typing<'a> {
    pub(crate) id: egui::Id,
    pub(crate) multiline: bool,
    pub(crate) secret: bool,
    pub(crate) editable: bool,
    /// Kept to its box, the text scrolled to the caret; off, the box is the
    /// text's and whatever holds it scrolls.
    pub(crate) clip: bool,
    /// The box behind the text, in egui's text edit look.
    pub(crate) frame: bool,
    pub(crate) tab_inserts: bool,
    pub(crate) numeric: bool,
    /// The most characters it holds; 0 is no limit.
    pub(crate) max_chars: usize,
    /// The chord that ends a single line: the field gives up focus.
    pub(crate) submit: Option<egui::KeyboardShortcut>,
    /// Where a single line sits across its box.
    pub(crate) align: egui::Align,
    pub(crate) margin: Pad,
    pub(crate) color: Color32,
    /// Shown while it is empty.
    pub(crate) placeholder: Option<Rc<balaur_text::Shaped>>,
    /// Drawn before and after the text, never typed into.
    pub(crate) prefix: Option<Rc<balaur_text::Shaped>>,
    pub(crate) suffix: Option<Rc<balaur_text::Shaped>>,
    /// Each line's colours by byte, a code editor's highlighting.
    pub(crate) colors: Option<LineColors<'a>>,
    /// The colour a line, counted from 1, is underlined in: a problem on it.
    pub(crate) underline: Option<&'a dyn Fn(usize) -> Option<Color32>>,
}

/// What one pass of a field did.
pub(crate) struct Typed {
    pub(crate) response: egui::Response,
    pub(crate) changed: bool,
    /// Where the caret is on screen and before which byte, while it has
    /// focus.
    pub(crate) caret: Option<(Rect, usize)>,
}

/// The room egui's affixes keep from the text, as its atoms do.
pub(crate) const AFFIX_GAP: f32 = 4.0;

impl Typing<'_> {
    /// The box the text runs in: `rect` without the margin and the affixes.
    pub(crate) fn inner(&self, rect: Rect) -> Rect {
        let mut inner = self.margin.inside(rect);
        if let Some(prefix) = &self.prefix {
            inner.min.x += prefix.size.x + AFFIX_GAP;
        }
        if let Some(suffix) = &self.suffix {
            inner.max.x -= suffix.size.x + AFFIX_GAP;
        }
        inner
    }

    /// Where the text's top-left corner is drawn: a single line across its
    /// box by `align` and down its middle, scrolled by `offset`.
    fn origin(&self, inner: Rect, size: Vec2, offset: Vec2) -> egui::Pos2 {
        if self.multiline {
            return inner.min - offset;
        }
        let slack = (inner.width() - size.x).max(0.0);
        let across = match self.align {
            egui::Align::Min => 0.0,
            egui::Align::Center => slack / 2.0,
            egui::Align::Max => slack,
        };
        pos2(
            inner.min.x + across - offset.x,
            inner.center().y - size.y / 2.0,
        )
    }

    /// The field in `rect`: the reader's clicks and keys taken, then drawn.
    pub(crate) fn show(
        &self,
        ui: &mut egui::Ui,
        eng: &Engine,
        rect: Rect,
        field: &mut Field,
        setter: &Typesetter<'_>,
    ) -> Typed {
        let id = self.id;
        let had_focus = ui.memory(|mem| mem.has_focus(id));
        let drag = ui.input(|i| !i.has_touch_screen()) || had_focus;
        let sense = match (self.editable, drag) {
            (false, _) => Sense::hover(),
            (true, true) => Sense::click_and_drag(),
            (true, false) => Sense::click(),
        };
        let mut response = ui.interact(rect, id, sense);
        let inner = self.inner(rect);
        let now = ui.input(|i| i.time);
        let before = (field.editing.caret(), field.editing.selection());
        let shown = field.editing.shown(self.secret);
        let typeset = setter.set(eng, ui.ctx(), &shown.text, &mut field.kept);
        let origin = self.origin(inner, typeset.size, field.offset);
        if self.editable {
            self.point(ui, &response, field, &typeset, origin);
            if response.hovered() {
                ui.set_cursor_icon(egui::CursorIcon::Text);
            }
        }
        let mut changed = false;
        if self.editable && ui.memory(|mem| mem.has_focus(id)) {
            changed = self.keys(ui, field, &mut |field: &mut Field| {
                let shown = field.editing.shown(self.secret);
                setter.set(eng, ui.ctx(), &shown.text, &mut field.kept)
            });
        }
        let focused = self.editable && ui.memory(|mem| mem.has_focus(id));
        let shown = field.editing.shown(self.secret);
        let typeset = setter.set(eng, ui.ctx(), &shown.text, &mut field.kept);
        let moved = changed || before != (field.editing.caret(), field.editing.selection());
        if moved {
            field.moved_at = now;
        }
        if changed {
            response.mark_changed();
        }
        let caret_at = typeset.clusters.caret_rect(shown.caret, 1.0);
        self.scroll(
            ui,
            &response,
            field,
            (&typeset, caret_at),
            (inner, focused, moved),
        );
        let origin = self.origin(inner, typeset.size, field.offset);
        let caret = caret_at.translate(origin.to_vec2());
        if ui.is_rect_visible(rect) {
            let texture = balaur_text::state(eng).and_then(|state| state.borrow().texture());
            self.paint(
                ui,
                texture,
                rect,
                (inner, origin),
                (&shown, &typeset),
                focused,
            );
        }
        if focused {
            if moved && !self.clip {
                ui.scroll_to_rect(caret, None);
            }
            Self::paint_caret(ui, caret, now - field.moved_at, shown.composing.is_some());
            self.ime(ui, inner, caret);
        }
        let hint = self.placeholder.as_ref().map_or("", |p| p.text.as_str());
        let enabled = ui.is_enabled();
        response
            .widget_info(|| egui::WidgetInfo::text_edit(enabled, &shown.text, &shown.text, hint));
        Typed {
            response,
            changed,
            caret: focused.then(|| (caret, field.editing.caret())),
        }
    }

    /// A press puts the caret under the pointer and takes focus; a drag
    /// stretches the selection; a double click takes a word, a triple the
    /// whole text.
    fn point(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        field: &mut Field,
        typeset: &Typeset,
        origin: egui::Pos2,
    ) {
        let Some(pos) = response.interact_pointer_pos() else {
            return;
        };
        let drawn = typeset.clusters.offset_at((pos - origin).to_pos2());
        let at = field.editing.from_shown(drawn, self.secret);
        let (pressed, shift) = ui.input(|i| (i.pointer.any_pressed(), i.modifiers.shift));
        if response.triple_clicked() {
            field.editing.select_all();
        } else if response.double_clicked() {
            field.editing.select_word(at);
        } else if pressed && response.is_pointer_button_down_on() {
            field.editing.place(at, shift);
        } else if response.dragged() {
            field.editing.place(at, true);
        }
        if pressed || response.clicked() {
            ui.memory_mut(|mem| mem.request_focus(self.id));
        }
    }

    /// The text kept in sight: a single line scrolled across to the caret,
    /// a clipped block down to it and by the wheel over it.
    fn scroll(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        field: &mut Field,
        (typeset, caret): (&Typeset, Rect),
        (inner, focused, moved): (Rect, bool, bool),
    ) {
        if !self.clip {
            field.offset = Vec2::ZERO;
            return;
        }
        let room = (typeset.size - inner.size()).max(Vec2::ZERO);
        if !self.multiline {
            let slack = inner.width() - typeset.size.x;
            let x = if focused { caret.min.x } else { 0.0 };
            let mut offset = field.offset.x;
            if slack >= 0.0 {
                offset = 0.0;
            } else if x < offset {
                offset = x;
            } else if x > offset + inner.width() {
                offset = x - inner.width();
            }
            field.offset = vec2(offset.clamp(0.0, room.x), 0.0);
            return;
        }
        let mut offset = field.offset.y;
        if response.hovered() && room.y > 0.0 {
            let wheel = ui
                .ctx()
                .input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y));
            offset -= wheel;
        }
        if focused && moved {
            if caret.min.y < offset {
                offset = caret.min.y;
            } else if caret.max.y > offset + inner.height() {
                offset = caret.max.y - inner.height();
            }
        }
        field.offset = vec2(0.0, offset.clamp(0.0, room.y));
    }

    /// The box, the affixes, the selection, the glyphs, the placeholder and a
    /// composition's underline, in that order.
    fn paint(
        &self,
        ui: &egui::Ui,
        texture: Option<egui::TextureId>,
        rect: Rect,
        (inner, origin): (Rect, egui::Pos2),
        (shown, typeset): (&balaur_text::edit::Shown, &Typeset),
        focused: bool,
    ) {
        if self.frame {
            frame(ui, rect, focused, self.editable);
        }
        let brush = Brush::of(ui);
        let ink = ui.visuals().text_color();
        if let Some(prefix) = &self.prefix {
            let at = pos2(
                rect.min.x + self.margin.left,
                rect.center().y - prefix.size.y / 2.0,
            );
            brush.shaped(texture, prefix, at, ink);
        }
        if let Some(suffix) = &self.suffix {
            let at = pos2(
                rect.max.x - self.margin.right - suffix.size.x,
                rect.center().y - suffix.size.y / 2.0,
            );
            brush.shaped(texture, suffix, at, ink);
        }
        let clip = if self.clip {
            inner.expand(1.0).intersect(ui.clip_rect())
        } else {
            ui.clip_rect()
        };
        let text = Brush::clipped(ui, clip);
        if focused && !shown.selection.is_empty() {
            let fill = ui.visuals().selection.bg_fill;
            for at in typeset.clusters.selection(shown.selection.clone()) {
                text.fill(at.translate(origin.to_vec2()), 0.0, fill);
            }
        }
        for (line, block) in typeset.blocks.iter().enumerate() {
            let at = origin + block.at;
            let drawn = Rect::from_min_size(at, block.laid.shaped.size.max(vec2(1.0, 1.0)));
            if !clip.intersects(drawn) {
                continue;
            }
            match self.colors {
                Some(colors) => {
                    let spans = colors(&shown.text[block.bytes.clone()]);
                    text.spans(texture, &block.laid.shaped, at, self.color, &spans);
                }
                None => text.shaped(texture, &block.laid.shaped, at, self.color),
            }
            if let Some(color) = self.underline.and_then(|of| of(line + 1)) {
                let y = at.y + block.laid.shaped.size.y - 1.0;
                let reach = at.x..=at.x + block.laid.shaped.size.x;
                text.hline(reach, y, egui::Stroke::new(1.0, color));
            }
        }
        if shown.text.is_empty()
            && let Some(placeholder) = &self.placeholder
        {
            let at = self.origin(inner, placeholder.size, Vec2::ZERO);
            text.shaped(texture, placeholder, at, ui.visuals().weak_text_color());
        }
        if let Some(composing) = &shown.composing {
            for at in typeset.clusters.selection(composing.clone()) {
                let at = at.translate(origin.to_vec2());
                text.hline(
                    at.x_range(),
                    at.max.y - 1.0,
                    egui::Stroke::new(1.0, self.color),
                );
            }
        }
    }
}

impl Typing<'_> {
    /// This pass's keys, the clipboard's and an input method's events, for a
    /// field with focus; true when the text changed. `relay` lays the text out
    /// again once it moved, for a key that moves by the drawing.
    fn keys(
        &self,
        ui: &egui::Ui,
        field: &mut Field,
        relay: &mut dyn FnMut(&mut Field) -> Rc<Typeset>,
    ) -> bool {
        let filter = egui::EventFilter {
            tab: self.tab_inserts,
            horizontal_arrows: true,
            vertical_arrows: true,
            escape: false,
        };
        ui.memory_mut(|mem| mem.set_focus_lock_filter(self.id, filter));
        let events = ui.input(|i| i.filtered_events(&filter));
        let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
        let mut typeset = relay(field);
        let mut changed = false;
        let mut cut = false;
        for event in &events {
            let done = match event {
                egui::Event::Text(text) if text != "\n" && text != "\r" => self.insert(field, text),
                egui::Event::Paste(text) => self.insert(field, text),
                egui::Event::Copy => {
                    self.copy(ui, field);
                    false
                }
                egui::Event::Cut if !cut => {
                    cut = true;
                    self.copy(ui, field);
                    field.editing.cut().is_some()
                }
                egui::Event::Ime(ime) => self.compose(field, ime),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let step = Step {
                        key: *key,
                        modifiers: *modifiers,
                        mac,
                    };
                    match self.key(ui, field, &typeset, step, &mut cut) {
                        Pressed::Changed => true,
                        Pressed::Moved | Pressed::Nothing => false,
                        Pressed::Left => break,
                    }
                }
                _ => false,
            };
            if done {
                changed = true;
                typeset = relay(field);
            }
        }
        changed
    }

    /// What `text` may add: digits, a sign and a point where it is numeric,
    /// and no more characters than the limit leaves room for.
    fn admit(&self, editing: &Editing, text: &str) -> String {
        let mut text: String = if self.numeric {
            text.chars()
                .filter(|c| c.is_ascii_digit() || matches!(c, '-' | '.'))
                .collect()
        } else {
            text.to_string()
        };
        if self.max_chars > 0 {
            let kept = editing.text().chars().count() - editing.selected().chars().count();
            let room = self.max_chars.saturating_sub(kept);
            if text.chars().count() > room {
                text = text.chars().take(room).collect();
            }
        }
        text
    }

    fn insert(&self, field: &mut Field, text: &str) -> bool {
        let text = self.admit(&field.editing, text);
        !text.is_empty() && field.editing.change(&Change::Insert(text))
    }

    /// The selection onto the clipboard; never a secret's.
    fn copy(&self, ui: &egui::Ui, field: &Field) {
        let selected = field.editing.selected();
        if !self.secret && !selected.is_empty() {
            ui.ctx().copy_text(selected.to_string());
        }
    }

    /// An input method's event: what it composes shown in place, and what it
    /// commits typed as one insert.
    fn compose(&self, field: &mut Field, ime: &egui::ImeEvent) -> bool {
        match ime {
            egui::ImeEvent::Preedit { text, .. } => {
                if !text.is_empty() || field.editing.composing() {
                    field.editing.compose(Some(text));
                }
                false
            }
            egui::ImeEvent::Commit(text) => {
                let text = self.admit(&field.editing, text);
                if text.is_empty() {
                    field.editing.compose(None);
                    return false;
                }
                field.editing.commit(&text)
            }
            _ => false,
        }
    }
}

/// A key pressed, with what was held and whose keyboard it is.
#[derive(Clone, Copy)]
struct Step {
    key: Key,
    modifiers: egui::Modifiers,
    mac: bool,
}

/// What a key did.
enum Pressed {
    Nothing,
    Moved,
    Changed,
    /// It ended the line and gave up focus: nothing after it is the field's.
    Left,
}

impl Typing<'_> {
    fn key(
        &self,
        ui: &egui::Ui,
        field: &mut Field,
        typeset: &Typeset,
        step: Step,
        cut: &mut bool,
    ) -> Pressed {
        let Step { key, modifiers, .. } = step;
        let command = modifiers.command;
        if let Some(chord) = self.submit.filter(|_| !self.multiline)
            && key == chord.logical_key
            && modifiers.matches_logically(chord.modifiers)
        {
            ui.memory_mut(|mem| mem.surrender_focus(self.id));
            return Pressed::Left;
        }
        let change = match key {
            Key::A if command => {
                field.editing.select_all();
                return Pressed::Moved;
            }
            Key::C if command => {
                self.copy(ui, field);
                return Pressed::Nothing;
            }
            Key::X if command => {
                if *cut {
                    return Pressed::Nothing;
                }
                *cut = true;
                self.copy(ui, field);
                return if field.editing.cut().is_some() {
                    Pressed::Changed
                } else {
                    Pressed::Nothing
                };
            }
            Key::Z if command && modifiers.shift => return done(field.editing.redo()),
            Key::Y if command => return done(field.editing.redo()),
            Key::Z if command => return done(field.editing.undo()),
            Key::Enter if self.multiline && !command => Change::Insert("\n".into()),
            Key::Tab if self.tab_inserts && !modifiers.shift => Change::Insert("\t".into()),
            Key::Backspace if modifiers.mac_cmd => return self.delete_line(field, typeset, false),
            Key::Delete if modifiers.mac_cmd => return self.delete_line(field, typeset, true),
            Key::Backspace if modifiers.alt || modifiers.ctrl => Change::DeleteWordBack,
            Key::Delete if modifiers.alt || modifiers.ctrl => Change::DeleteWordForward,
            Key::Backspace => Change::DeleteBack,
            Key::Delete => Change::DeleteForward,
            Key::H if modifiers.ctrl => Change::DeleteBack,
            Key::W if modifiers.ctrl => Change::DeleteWordBack,
            Key::K if modifiers.ctrl => return self.delete_line(field, typeset, true),
            Key::U if modifiers.ctrl => return self.delete_line(field, typeset, false),
            _ => {
                let Some(motion) = self.motion(field, typeset, step) else {
                    return Pressed::Nothing;
                };
                field
                    .editing
                    .motion(motion, modifiers.shift, &typeset.clusters);
                return Pressed::Moved;
            }
        };
        if let Change::Insert(text) = &change {
            return done(self.insert(field, text));
        }
        done(field.editing.change(&change))
    }

    /// The motion a key asks for. A secret's dots, and a text being composed,
    /// move by what is stored rather than by the drawing.
    fn motion(&self, field: &Field, typeset: &Typeset, step: Step) -> Option<Motion> {
        let Step {
            key,
            modifiers,
            mac,
        } = step;
        let word = if mac { modifiers.alt } else { modifiers.ctrl };
        let line = mac && modifiers.mac_cmd;
        let emacs = mac && modifiers.ctrl && !modifiers.shift;
        let rtl = typeset.clusters.rtl_at(field.editing.caret());
        let motion = match key {
            Key::ArrowLeft if line => {
                if rtl {
                    Motion::LineEnd
                } else {
                    Motion::LineStart
                }
            }
            Key::ArrowRight if line => {
                if rtl {
                    Motion::LineStart
                } else {
                    Motion::LineEnd
                }
            }
            Key::ArrowLeft if word => Motion::WordLeft,
            Key::ArrowRight if word => Motion::WordRight,
            Key::ArrowLeft => Motion::Left,
            Key::ArrowRight => Motion::Right,
            Key::ArrowUp if line => Motion::TextStart,
            Key::ArrowDown if line => Motion::TextEnd,
            Key::ArrowUp => Motion::Up,
            Key::ArrowDown => Motion::Down,
            Key::Home if modifiers.ctrl => Motion::TextStart,
            Key::End if modifiers.ctrl => Motion::TextEnd,
            Key::Home => Motion::LineStart,
            Key::End => Motion::LineEnd,
            Key::A if emacs => Motion::LineStart,
            Key::E if emacs => Motion::LineEnd,
            Key::B if emacs => Motion::Back,
            Key::F if emacs => Motion::Forward,
            Key::P if emacs => Motion::Up,
            Key::N if emacs => Motion::Down,
            _ => return None,
        };
        if !(self.secret || field.editing.composing()) {
            return Some(motion);
        }
        Some(match motion {
            Motion::Left | Motion::WordLeft => Motion::Back,
            Motion::Right | Motion::WordRight => Motion::Forward,
            Motion::Up | Motion::LineStart => Motion::TextStart,
            Motion::Down | Motion::LineEnd => Motion::TextEnd,
            other => other,
        })
    }

    /// Everything from the caret to its line's start or end taken out, or the
    /// selection where there is one: ⌘⌫ and Ctrl+K.
    fn delete_line(&self, field: &mut Field, typeset: &Typeset, forward: bool) -> Pressed {
        if field.editing.selection().is_empty() {
            let caret = field.editing.caret();
            let (start, end) = typeset.clusters.line_bytes(caret);
            let to = if forward { end } else { start };
            if self.secret || field.editing.composing() {
                let motion = if forward {
                    Motion::TextEnd
                } else {
                    Motion::TextStart
                };
                field.editing.motion(motion, true, &typeset.clusters);
            } else {
                field.editing.place(to, true);
            }
        }
        done(field.editing.change(&Change::DeleteBack))
    }

    /// The caret, on for the first part of each blink and off for the rest,
    /// a pass owed for each change; no blink at all while a composition is
    /// shown, which carries its own underline.
    fn paint_caret(ui: &egui::Ui, caret: Rect, since: f64, composing: bool) {
        if !ui.input(|i| i.focused) || composing {
            return;
        }
        let look = ui.visuals().text_cursor.clone();
        let line = [caret.center_top(), caret.center_bottom()];
        if !look.blink {
            Brush::of(ui).segment(line, look.stroke);
            return;
        }
        let cycle = f64::from(look.on_duration + look.off_duration);
        let into = (since % cycle) as f32;
        let wake = if into < look.on_duration {
            Brush::of(ui).segment(line, look.stroke);
            look.on_duration - into
        } else {
            look.on_duration + look.off_duration - into
        };
        ui.ctx().request_repaint_after_secs(wake);
    }

    /// Where the platform's input method puts its window: under the caret.
    fn ime(&self, ui: &egui::Ui, inner: Rect, caret: Rect) {
        let to_global = ui
            .ctx()
            .layer_transform_to_global(ui.layer_id())
            .unwrap_or_default();
        let purpose = if self.secret {
            egui::viewport::IMEPurpose::Password
        } else {
            egui::viewport::IMEPurpose::Normal
        };
        ui.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose,
                rect: to_global * inner,
                cursor_rect: to_global * caret,
                should_interrupt_composition: false,
            });
        });
    }
}

fn done(changed: bool) -> Pressed {
    if changed {
        Pressed::Changed
    } else {
        Pressed::Nothing
    }
}

/// egui's text edit box: its background, and its edge in the selection's
/// stroke while it has focus, as `theme::dress` set them up for the role.
fn frame(ui: &egui::Ui, rect: Rect, focused: bool, editable: bool) {
    let visuals = ui.visuals();
    let state = if ui.rect_contains_pointer(rect) {
        &visuals.widgets.hovered
    } else {
        &visuals.widgets.inactive
    };
    let (fill, stroke) = if !editable {
        (Color32::TRANSPARENT, visuals.widgets.inactive.bg_stroke)
    } else if focused {
        (visuals.text_edit_bg_color(), visuals.selection.stroke)
    } else {
        (visuals.text_edit_bg_color(), state.bg_stroke)
    };
    Brush::of(ui).plate(
        rect.expand(state.expansion),
        state.corner_radius,
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
}
