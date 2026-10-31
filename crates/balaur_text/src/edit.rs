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
    /// Which of [`Clusters::clusters`] sit on it, a run of them.
    pub clusters: (usize, usize),
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
        let from = out.clusters.len();
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
            clusters: (from, out.clusters.len()),
        });
    }
    if out.lines.is_empty() {
        out.lines.push(LineBox {
            top: 0.0,
            height: line_height,
            bytes: (0, 0),
            rtl: false,
            clusters: (0, 0),
        });
    }
    out
}

/// A text laid out for editing: the glyphs a label would draw for it, and
/// the boxes its caret, clicks and selection are measured against, both from
/// one layout so they cannot disagree.
pub struct Laid {
    pub shaped: crate::Shaped,
    pub clusters: Clusters,
}

impl Clusters {
    /// The line `offset` falls on: the last one starting at or before it.
    fn line_of(&self, offset: usize) -> usize {
        self.lines
            .partition_point(|l| l.bytes.0 <= offset)
            .saturating_sub(1)
    }

    /// The clusters a caret before `offset` can touch: its line's and the
    /// line's before it, where a cluster ending there may sit. A whole file
    /// is laid out at once, so a caret never walks every cluster in it.
    fn near(&self, offset: usize) -> &[Cluster] {
        let line = self.line_of(offset);
        let from = self.lines[line.saturating_sub(1)].clusters.0;
        &self.clusters[from..self.lines[line].clusters.1]
    }

    fn on_line(&self, line: usize) -> &[Cluster] {
        let (from, to) = self.lines[line].clusters;
        &self.clusters[from..to]
    }

    /// Where a caret before byte `offset` stands: its x and its line. The
    /// leading edge of the cluster starting there, else the trailing edge of
    /// the one ending there, which is where a right-to-left run puts it.
    #[must_use]
    pub fn caret(&self, offset: usize) -> (f32, usize) {
        let near = self.near(offset);
        if let Some(c) = near.iter().find(|c| c.bytes.0 == offset) {
            return (if c.rtl { c.x + c.width } else { c.x }, c.line);
        }
        if let Some(c) = near.iter().rev().find(|c| c.bytes.1 == offset) {
            return (if c.rtl { c.x } else { c.x + c.width }, c.line);
        }
        // Inside a ligature: the cluster's box shared out by its bytes.
        if let Some(c) = near
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
        let rtl = self.lines[line].rtl;
        let at = self
            .on_line(line)
            .iter()
            .map(|c| if rtl { c.x + c.width } else { c.x })
            .reduce(|a, b| if rtl { a.max(b) } else { a.min(b) });
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
            .on_line(line)
            .iter()
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
        self.stops_in(c.bytes.0, c.bytes.1)
            .iter()
            .copied()
            .min_by(|a, b| {
                #[allow(clippy::cast_precision_loss, reason = "byte offsets in one text")]
                let (da, db) = ((*a as f32 - target).abs(), (*b as f32 - target).abs());
                da.total_cmp(&db)
            })
            .unwrap_or(c.bytes.0)
    }

    /// The caret stops from byte `from` to byte `to`, both ends included.
    fn stops_in(&self, from: usize, to: usize) -> &[usize] {
        let start = self.stops.partition_point(|s| *s < from);
        let end = self.stops.partition_point(|s| *s <= to);
        &self.stops[start..end.max(start)]
    }

    /// The boxes a selection of `bytes` paints, a cluster at a time.
    #[must_use]
    pub fn selection(&self, bytes: Range<usize>) -> Vec<Rect> {
        let first = self.line_of(bytes.start);
        let last = self.line_of(bytes.end);
        let (from, to) = (self.lines[first].clusters.0, self.lines[last].clusters.1);
        self.clusters[from..to]
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

    /// The next caret stop to the right or left of `offset` as its line is
    /// drawn, whatever direction the text runs in there; past the line's
    /// end, the next line's start, and `None` past the text's.
    #[must_use]
    pub fn beside(&self, offset: usize, right: bool) -> Option<usize> {
        let (x, line) = self.caret(offset);
        let (from, to) = self.lines[line].bytes;
        let ahead = |s: usize| {
            let (sx, on) = self.caret(s);
            let gap = if right { sx - x } else { x - sx };
            (on == line && gap > 0.01).then_some(gap)
        };
        let next = self
            .stops_in(from, to)
            .iter()
            .filter_map(|s| ahead(*s).map(|gap| (gap, *s)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, s)| s);
        if next.is_some() {
            return next;
        }
        // Off the end of the line: on to the next line's start, or back to
        // the previous line's end, in the order the lines read.
        let rtl = self.lines[line].rtl;
        let onward = right != rtl;
        if onward {
            self.lines.get(line + 1).map(|l| l.bytes.0)
        } else {
            line.checked_sub(1).map(|l| self.lines[l].bytes.1)
        }
    }

    /// Whether the text at `offset` runs right to left: the cluster starting
    /// there, else the one ending there, else its line's direction.
    #[must_use]
    pub fn rtl_at(&self, offset: usize) -> bool {
        let near = self.near(offset);
        near.iter()
            .find(|c| c.bytes.0 == offset)
            .or_else(|| near.iter().rev().find(|c| c.bytes.1 == offset))
            .map_or_else(|| self.lines[self.line_of(offset)].rtl, |c| c.rtl)
    }

    /// `line`'s boxes put after these, its bytes from `base` and its top at
    /// `top`: a file laid out a line at a time, each line apart, read as one.
    pub fn append(&mut self, line: &Self, base: usize, top: f32) {
        let lines = self.lines.len();
        let clusters = self.clusters.len();
        self.clusters.extend(line.clusters.iter().map(|c| Cluster {
            bytes: (c.bytes.0 + base, c.bytes.1 + base),
            line: c.line + lines,
            ..*c
        }));
        self.lines.extend(line.lines.iter().map(|l| LineBox {
            top: l.top + top,
            bytes: (l.bytes.0 + base, l.bytes.1 + base),
            clusters: (l.clusters.0 + clusters, l.clusters.1 + clusters),
            ..*l
        }));
        let from = usize::from(self.stops.last() == Some(&base));
        self.stops
            .extend(line.stops.iter().skip(from).map(|s| s + base));
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
    /// One caret stop left or right as the line is drawn: what the arrow
    /// keys do, through text running either way.
    Left,
    Right,
    WordBack,
    WordForward,
    /// A word left or right: back through text running left to right, on
    /// through text running right to left.
    WordLeft,
    WordRight,
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

    /// The word around `offset` selected, or the spaces or the mark there:
    /// what a double click takes.
    pub fn select_word(&mut self, offset: usize) {
        let offset = self.boundary_at(offset);
        let (start, end) = self
            .text
            .split_word_bound_indices()
            .map(|(at, word)| (at, at + word.len()))
            .find(|(start, end)| *start <= offset && offset < *end)
            .unwrap_or((offset, offset));
        self.anchor = start;
        self.caret = end;
        self.typing = false;
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
        let collapsed = !extend && !selection.is_empty();
        let to = match motion {
            Motion::Back if collapsed => selection.start,
            Motion::Forward if collapsed => selection.end,
            Motion::Left | Motion::Right if collapsed => {
                let (start, end) = (layout.caret(selection.start), layout.caret(selection.end));
                let start_first = (start.1, start.0) <= (end.1, end.0);
                if start_first == (motion == Motion::Left) {
                    selection.start
                } else {
                    selection.end
                }
            }
            Motion::Back => self.grapheme_back(at),
            Motion::Forward => self.grapheme_forward(at),
            Motion::Left => layout.beside(at, false).unwrap_or(at),
            Motion::Right => layout.beside(at, true).unwrap_or(at),
            Motion::WordBack => self.word_back(at),
            Motion::WordForward => self.word_forward(at),
            Motion::WordLeft | Motion::WordRight => {
                let onward = (motion == Motion::WordRight) ^ layout.rtl_at(at);
                if onward {
                    self.word_forward(at)
                } else {
                    self.word_back(at)
                }
            }
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

    /// The byte of the text a byte of what [`Self::shown`] drew stands for:
    /// where a click on the drawing puts the caret. A dot is its grapheme,
    /// and a click inside a composition lands where the composition is.
    #[must_use]
    pub fn from_shown(&self, offset: usize, secret: bool) -> usize {
        let mut offset = offset;
        if let Some(preedit) = &self.composing {
            let (_, _, selected) = self.uncomposed(secret);
            offset = if offset <= selected.start {
                offset
            } else if offset < selected.start + preedit.len() {
                selected.start
            } else {
                offset - preedit.len() + selected.len()
            };
        }
        if !secret {
            return self.boundary_at(offset);
        }
        let graphemes = offset / SECRET.len_utf8();
        self.text
            .grapheme_indices(true)
            .map(|(at, _)| at)
            .nth(graphemes)
            .unwrap_or(self.text.len())
    }

    /// Whether an input method is composing.
    #[must_use]
    pub fn composing(&self) -> bool {
        self.composing.is_some()
    }

    /// The text as a field draws it. `secret` shows a dot for each grapheme,
    /// so neither the text nor its length by bytes is on screen.
    #[must_use]
    pub fn shown(&self, secret: bool) -> Shown {
        let (mut text, mut caret, mut range) = self.uncomposed(secret);
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

    /// The text drawn, its caret and its selection, before any composition.
    fn uncomposed(&self, secret: bool) -> (String, usize, Range<usize>) {
        let selection = self.selection();
        if !secret {
            return (self.text.clone(), self.caret, selection);
        }
        let dots = |end: usize| self.text[..end].graphemes(true).count() * SECRET.len_utf8();
        (
            SECRET.to_string().repeat(self.text.graphemes(true).count()),
            dots(self.caret),
            dots(selection.start)..dots(selection.end),
        )
    }
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
