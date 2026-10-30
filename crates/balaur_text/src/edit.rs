//! Text a person types: what a `field` or a `text_area` holds and where its
//! caret is, laid out by the same shaper as a label, so a field in Arabic,
//! Hindi or Thai draws as its label does (docs/PLAN-ui-gaps.md §1).

use std::ops::Range;

use cosmic_text::Buffer;
use egui::{Pos2, Rect, pos2, vec2};
use unicode_segmentation::UnicodeSegmentation;

/// One cluster of laid-out text: the bytes it shows and its advance box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cluster {
    pub bytes: (usize, usize),
    pub x: f32,
    pub width: f32,
    pub line: usize,
    pub rtl: bool,
}

/// One laid-out line, soft-wrapped or ended by a line break.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineBox {
    pub top: f32,
    pub height: f32,
    /// The bytes of its paragraph it starts and ends at.
    pub bytes: (usize, usize),
    pub rtl: bool,
}

/// Where every cluster of a text sits: what a caret, a click and a selection
/// are measured against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clusters {
    pub clusters: Vec<Cluster>,
    pub lines: Vec<LineBox>,
    /// Every grapheme boundary of the text: where a caret may stand, inside
    /// a ligature too.
    pub stops: Vec<usize>,
}

/// Each paragraph's first byte: cosmic-text numbers a glyph's bytes from the
/// start of its own paragraph.
fn paragraph_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect()
}

pub(crate) fn clusters(buffer: &Buffer, text: &str, line_height: f32) -> Clusters {
    let starts = paragraph_starts(text);
    let mut out = Clusters {
        stops: text
            .grapheme_indices(true)
            .map(|(at, _)| at)
            .chain(std::iter::once(text.len()))
            .collect(),
        ..Clusters::default()
    };
    for run in buffer.layout_runs() {
        let base = starts.get(run.line_i).copied().unwrap_or(text.len());
        let line = out.lines.len();
        let mut first = usize::MAX;
        let mut last = 0;
        for glyph in run.glyphs {
            let bytes = (base + glyph.start, base + glyph.end);
            if bytes.0 == bytes.1 {
                continue;
            }
            first = first.min(bytes.0);
            last = last.max(bytes.1);
            // A cluster drawn as several glyphs (a base and its marks) is one
            // box from the leftmost to the rightmost of them.
            match out.clusters.last_mut() {
                Some(c) if c.bytes == bytes && c.line == line => {
                    let right = (c.x + c.width).max(glyph.x + glyph.w);
                    c.x = c.x.min(glyph.x);
                    c.width = right - c.x;
                }
                _ => out.clusters.push(Cluster {
                    bytes,
                    x: glyph.x,
                    width: glyph.w,
                    line,
                    rtl: glyph.level.is_rtl(),
                }),
            }
        }
        let (start, end) = if first == usize::MAX {
            (base, base)
        } else {
            (first, last)
        };
        out.lines.push(LineBox {
            top: run.line_top,
            height: run.line_height,
            bytes: (start, end),
            rtl: run.rtl,
        });
    }
    if out.lines.is_empty() {
        out.lines.push(LineBox {
            top: 0.0,
            height: line_height,
            bytes: (0, 0),
            rtl: false,
        });
    }
    out
}

impl Clusters {
    fn line_of(&self, offset: usize) -> usize {
        self.lines
            .iter()
            .rposition(|l| l.bytes.0 <= offset)
            .unwrap_or(0)
    }

    /// Where a caret before byte `offset` stands: its x and its line. The
    /// leading edge of the cluster starting there, else the trailing edge of
    /// the one ending there, which is where a right-to-left run puts it.
    #[must_use]
    pub fn caret(&self, offset: usize) -> (f32, usize) {
        if let Some(c) = self.clusters.iter().find(|c| c.bytes.0 == offset) {
            return (if c.rtl { c.x + c.width } else { c.x }, c.line);
        }
        if let Some(c) = self.clusters.iter().rev().find(|c| c.bytes.1 == offset) {
            return (if c.rtl { c.x } else { c.x + c.width }, c.line);
        }
        // Inside a ligature: the cluster's box shared out by its bytes.
        if let Some(c) = self
            .clusters
            .iter()
            .find(|c| c.bytes.0 < offset && offset < c.bytes.1)
        {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a share of a cluster's few bytes"
            )]
            let part = (offset - c.bytes.0) as f32 / (c.bytes.1 - c.bytes.0) as f32;
            let reach = if c.rtl { 1.0 - part } else { part };
            return (c.x + c.width * reach, c.line);
        }
        // An empty line, or the one after a line break.
        let line = self.line_of(offset);
        let at = self
            .clusters
            .iter()
            .filter(|c| c.line == line)
            .map(|c| {
                if self.lines[line].rtl {
                    c.x + c.width
                } else {
                    c.x
                }
            })
            .reduce(|a, b| {
                if self.lines[line].rtl {
                    a.max(b)
                } else {
                    a.min(b)
                }
            });
        (at.unwrap_or(0.0), line)
    }

    /// The caret's box, `width` wide.
    #[must_use]
    pub fn caret_rect(&self, offset: usize, width: f32) -> Rect {
        let (x, line) = self.caret(offset);
        let l = self.lines[line];
        Rect::from_min_size(pos2(x - width / 2.0, l.top), vec2(width, l.height))
    }

    /// The byte a click at `point` puts the caret before: the nearer edge of
    /// the cluster under it, on the line under it.
    #[must_use]
    pub fn offset_at(&self, point: Pos2) -> usize {
        let line = self
            .lines
            .iter()
            .position(|l| point.y < l.top + l.height)
            .unwrap_or(self.lines.len() - 1);
        let nearest = self
            .clusters
            .iter()
            .filter(|c| c.line == line)
            .min_by(|a, b| distance(a, point.x).total_cmp(&distance(b, point.x)));
        let Some(c) = nearest else {
            return self.lines[line].bytes.0;
        };
        // How far into the cluster the click is, in reading order, as bytes:
        // the nearest stop to that, which inside a ligature may be neither end.
        let across = ((point.x - c.x) / c.width.max(f32::EPSILON)).clamp(0.0, 1.0);
        let read = if c.rtl { 1.0 - across } else { across };
        #[allow(
            clippy::cast_precision_loss,
            reason = "a share of a cluster's few bytes"
        )]
        let target = c.bytes.0 as f32 + read * (c.bytes.1 - c.bytes.0) as f32;
        self.stops
            .iter()
            .copied()
            .filter(|s| (c.bytes.0..=c.bytes.1).contains(s))
            .min_by(|a, b| {
                #[allow(clippy::cast_precision_loss, reason = "byte offsets in one text")]
                let (da, db) = ((*a as f32 - target).abs(), (*b as f32 - target).abs());
                da.total_cmp(&db)
            })
            .unwrap_or(c.bytes.0)
    }

    /// The boxes a selection of `bytes` paints, a cluster at a time.
    #[must_use]
    pub fn selection(&self, bytes: Range<usize>) -> Vec<Rect> {
        self.clusters
            .iter()
            .filter(|c| bytes.start <= c.bytes.0 && c.bytes.1 <= bytes.end)
            .map(|c| {
                let l = self.lines[c.line];
                Rect::from_min_size(pos2(c.x, l.top), vec2(c.width, l.height))
            })
            .collect()
    }

    /// The first and last byte of the line `offset` sits on: where Home and
    /// End go.
    #[must_use]
    pub fn line_bytes(&self, offset: usize) -> (usize, usize) {
        self.lines[self.caret(offset).1].bytes
    }

    /// The byte straight above or below `offset`, at the same x, or `None`
    /// at the first or last line.
    #[must_use]
    pub fn vertical(&self, offset: usize, down: bool) -> Option<usize> {
        let (x, line) = self.caret(offset);
        let next = if down { line + 1 } else { line.checked_sub(1)? };
        let l = self.lines.get(next)?;
        Some(self.offset_at(pos2(x, l.top + l.height / 2.0)))
    }
}

fn distance(c: &Cluster, x: f32) -> f32 {
    if x < c.x {
        c.x - x
    } else if x > c.x + c.width {
        x - c.x - c.width
    } else {
        0.0
    }
}

/// A move of the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// One grapheme back or on, in the order the text is stored.
    Back,
    Forward,
    WordBack,
    WordForward,
    LineStart,
    LineEnd,
    Up,
    Down,
    TextStart,
    TextEnd,
}

/// A change to the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Insert(String),
    /// The grapheme before the caret, or the selection.
    DeleteBack,
    /// The grapheme after the caret, or the selection.
    DeleteForward,
    DeleteWordBack,
    DeleteWordForward,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    caret: usize,
    anchor: usize,
}

/// What a field holds while a person edits it: the text, the caret and the
/// end of the selection, what an input method is composing, and undo.
#[derive(Clone, Debug, Default)]
pub struct Editing {
    text: String,
    caret: usize,
    anchor: usize,
    composing: Option<String>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Typing on from the last insert joins its undo step.
    typing: bool,
    multiline: bool,
}

/// What a field shows: its text with any composition in place, and where
/// the caret and the selection fall in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    pub caret: usize,
    pub selection: Range<usize>,
    /// The bytes being composed, underlined.
    pub composing: Option<Range<usize>>,
}

const SECRET: char = '•';

impl Editing {
    #[must_use]
    pub fn new(text: &str, multiline: bool) -> Self {
        let text = Self::clean(text, multiline);
        let end = text.len();
        Self {
            text,
            caret: end,
            anchor: end,
            multiline,
            ..Self::default()
        }
    }

    fn clean(text: &str, multiline: bool) -> String {
        let text = text.replace("\r\n", "\n");
        if multiline {
            text
        } else {
            text.replace('\n', " ")
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    #[must_use]
    pub fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    #[must_use]
    pub fn selected(&self) -> &str {
        &self.text[self.selection()]
    }

    /// The text set from outside, a script's write: the caret stays where it
    /// was when it still fits, and undo starts over.
    pub fn set_text(&mut self, text: &str) {
        let text = Self::clean(text, self.multiline);
        if text == self.text {
            return;
        }
        self.text = text;
        self.caret = self.boundary_at(self.caret);
        self.anchor = self.caret;
        self.composing = None;
        self.undo.clear();
        self.redo.clear();
        self.typing = false;
    }

    /// The nearest grapheme boundary at or before `offset`.
    fn boundary_at(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        self.text
            .grapheme_indices(true)
            .map(|(at, _)| at)
            .chain(std::iter::once(self.text.len()))
            .take_while(|at| *at <= offset)
            .last()
            .unwrap_or(0)
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
        self.typing = false;
    }

    /// The caret put at `offset`, a click's; `extend` keeps the selection's
    /// other end, a shift-click's or a drag's.
    pub fn place(&mut self, offset: usize, extend: bool) {
        self.caret = self.boundary_at(offset);
        if !extend {
            self.anchor = self.caret;
        }
        self.typing = false;
    }

    fn grapheme_back(&self, from: usize) -> usize {
        self.text[..from]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    fn grapheme_forward(&self, from: usize) -> usize {
        self.text[from..]
            .graphemes(true)
            .next()
            .map_or(from, |g| from + g.len())
    }

    // A word's start back from `from`, past any spaces and punctuation.
    fn word_back(&self, from: usize) -> usize {
        self.text[..from]
            .split_word_bound_indices()
            .rev()
            .find(|(_, w)| w.chars().any(char::is_alphanumeric))
            .map_or(0, |(at, _)| at)
    }

    // The end of the next word on from `from`.
    fn word_forward(&self, from: usize) -> usize {
        self.text[from..]
            .split_word_bound_indices()
            .find(|(_, w)| w.chars().any(char::is_alphanumeric))
            .map_or(self.text.len(), |(at, w)| from + at + w.len())
    }

    /// Moves the caret; `extend` grows the selection instead of dropping it.
    /// A move with no `extend` from a selection lands at its near end.
    pub fn motion(&mut self, motion: Motion, extend: bool, layout: &Clusters) {
        let selection = self.selection();
        let at = self.caret;
        let to = match motion {
            Motion::Back if !extend && !selection.is_empty() => selection.start,
            Motion::Forward if !extend && !selection.is_empty() => selection.end,
            Motion::Back => self.grapheme_back(at),
            Motion::Forward => self.grapheme_forward(at),
            Motion::WordBack => self.word_back(at),
            Motion::WordForward => self.word_forward(at),
            Motion::LineStart => layout.line_bytes(at).0,
            Motion::LineEnd => layout.line_bytes(at).1,
            Motion::Up => layout.vertical(at, false).unwrap_or(0),
            Motion::Down => layout.vertical(at, true).unwrap_or(self.text.len()),
            Motion::TextStart => 0,
            Motion::TextEnd => self.text.len(),
        };
        self.place(to, extend);
    }

    fn remember(&mut self, typing: bool) {
        if !(typing && self.typing) {
            self.undo.push(Snapshot {
                text: self.text.clone(),
                caret: self.caret,
                anchor: self.anchor,
            });
        }
        self.redo.clear();
        self.typing = typing;
    }

    fn replace(&mut self, bytes: Range<usize>, with: &str) {
        self.text.replace_range(bytes.clone(), with);
        self.caret = bytes.start + with.len();
        self.anchor = self.caret;
    }

    /// Applies `change`; true when the text changed.
    pub fn change(&mut self, change: &Change) -> bool {
        let selection = self.selection();
        let at = self.caret;
        let (bytes, with, typing) = match change {
            Change::Insert(text) => {
                let text = Self::clean(text, self.multiline);
                let typed = text.chars().count() == 1 && !text.chars().any(char::is_whitespace);
                (selection, text, typed)
            }
            _ if !selection.is_empty() => (selection, String::new(), false),
            Change::DeleteBack => (self.grapheme_back(at)..at, String::new(), false),
            Change::DeleteForward => (at..self.grapheme_forward(at), String::new(), false),
            Change::DeleteWordBack => (self.word_back(at)..at, String::new(), false),
            Change::DeleteWordForward => (at..self.word_forward(at), String::new(), false),
        };
        if bytes.is_empty() && with.is_empty() {
            return false;
        }
        self.remember(typing);
        self.replace(bytes, &with);
        true
    }

    /// What an input method composes before it commits: shown at the
    /// caret, in place of any selection, and not yet in the text.
    pub fn compose(&mut self, preedit: Option<&str>) {
        self.composing = preedit.filter(|p| !p.is_empty()).map(str::to_string);
    }

    /// What an input method commits: the composition ends and `text` is typed.
    pub fn commit(&mut self, text: &str) -> bool {
        self.composing = None;
        self.change(&Change::Insert(text.to_string()))
    }

    pub fn undo(&mut self) -> bool {
        self.step(true)
    }

    pub fn redo(&mut self) -> bool {
        self.step(false)
    }

    fn step(&mut self, back: bool) -> bool {
        let taken = if back {
            self.undo.pop()
        } else {
            self.redo.pop()
        };
        let Some(s) = taken else {
            return false;
        };
        let now = Snapshot {
            text: std::mem::replace(&mut self.text, s.text),
            caret: self.caret,
            anchor: self.anchor,
        };
        if back {
            self.redo.push(now);
        } else {
            self.undo.push(now);
        }
        self.caret = s.caret;
        self.anchor = s.anchor;
        self.typing = false;
        true
    }

    /// What is cut: the selection, taken out as one undo step.
    pub fn cut(&mut self) -> Option<String> {
        let taken = self.selected().to_string();
        if taken.is_empty() {
            return None;
        }
        self.change(&Change::DeleteBack);
        Some(taken)
    }

    /// The text as a field draws it. `secret` shows a dot for each grapheme,
    /// so neither the text nor its length by bytes is on screen.
    #[must_use]
    pub fn shown(&self, secret: bool) -> Shown {
        let selection = self.selection();
        let (mut text, mut caret, mut range) = if secret {
            let dots = |end: usize| self.text[..end].graphemes(true).count() * SECRET.len_utf8();
            (
                SECRET.to_string().repeat(self.text.graphemes(true).count()),
                dots(self.caret),
                dots(selection.start)..dots(selection.end),
            )
        } else {
            (self.text.clone(), self.caret, selection)
        };
        let mut composing = None;
        if let Some(preedit) = &self.composing {
            let at = range.start;
            text.replace_range(range.clone(), preedit);
            composing = Some(at..at + preedit.len());
            caret = at + preedit.len();
            range = caret..caret;
        }
        Shown {
            text,
            caret,
            selection: range,
            composing,
        }
    }
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
