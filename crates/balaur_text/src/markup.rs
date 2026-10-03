//! Inline marks in a string: the small tag set a localized string carries.
//!
//! `[b]`, `[i]`, `[u]`, `[s]`, `[o]`, `[color=#rrggbb]`, `[size=24]`,
//! `[font=mono]`, `[wave amp=8 freq=4]`, `[url=target]` and `[hint=text]` wrap
//! text; `[center]`, `[left]` and `[right]` set the block's alignment;
//! `[img=path width=32 height=32]` stands alone. Anything else in brackets is
//! text, so a string that was never markup still reads as it was written.

use egui::Color32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Span {
    pub(crate) text: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) lines: Lines,
    pub(crate) color: Option<Color32>,
    /// A size in the request's pixels, over the block's.
    pub(crate) size: Option<f32>,
    /// A named chain, over the block's.
    pub(crate) font: Option<String>,
    /// Amplitude in pixels and frequency in cycles per second.
    pub(crate) wave: Option<(f32, f32)>,
    /// An inline picture the span stands in for, with its box.
    pub(crate) image: Option<Inline>,
    /// Which of the block's link targets this span reports when clicked.
    pub(crate) link: Option<u16>,
    /// Which of the block's hints this span shows on hover.
    pub(crate) hint: Option<u16>,
}

/// The lines a span draws with its glyphs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Lines {
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
    pub(crate) overline: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Inline {
    pub(crate) path: String,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// Where lines sit in their block: start and end follow the text's
/// direction, left and right do not, and justify stretches every wrapped
/// line but the last to the width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Start,
    Center,
    End,
    Left,
    Right,
    Justify,
}

impl Align {
    /// The alignment a word names; anything else starts.
    #[must_use]
    pub fn of(word: &str) -> Self {
        use crate::vocabulary::words as w;
        match word {
            w::CENTER => Self::Center,
            w::END => Self::End,
            w::LEFT => Self::Left,
            w::RIGHT => Self::Right,
            w::JUSTIFY => Self::Justify,
            _ => Self::Start,
        }
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        use crate::vocabulary::words as w;
        match self {
            Self::Start => w::START,
            Self::Center => w::CENTER,
            Self::End => w::END,
            Self::Left => w::LEFT,
            Self::Right => w::RIGHT,
            Self::Justify => w::JUSTIFY,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Markup {
    pub(crate) spans: Vec<Span>,
    pub(crate) align: Option<Align>,
    /// What each `[url]` points at, in the order they opened.
    pub(crate) links: Vec<String>,
    /// What each `[hint]` says, in the order they opened.
    pub(crate) hints: Vec<String>,
}

#[derive(Clone, Default)]
struct Style {
    bold: u32,
    italic: u32,
    underline: u32,
    strikethrough: u32,
    overline: u32,
    colors: Vec<Color32>,
    sizes: Vec<f32>,
    fonts: Vec<String>,
    waves: Vec<(f32, f32)>,
    links: Vec<u16>,
    hints: Vec<u16>,
}

impl Style {
    fn span(&self, text: String) -> Span {
        Span {
            text,
            bold: self.bold > 0,
            italic: self.italic > 0,
            lines: Lines {
                underline: self.underline > 0,
                strikethrough: self.strikethrough > 0,
                overline: self.overline > 0,
            },
            color: self.colors.last().copied(),
            size: self.sizes.last().copied(),
            font: self.fonts.last().cloned(),
            wave: self.waves.last().copied(),
            image: None,
            link: self.links.last().copied(),
            hint: self.hints.last().copied(),
        }
    }
}

impl Style {
    /// Open or close a tag that styles the text after it. The block keeps a
    /// link's target and a hint's text once, and the spans carry the index,
    /// the way a colour rides on a span.
    fn apply(&mut self, tag: Tag, links: &mut Vec<String>, hints: &mut Vec<String>) {
        match tag {
            Tag::Toggle(mark, on) => {
                let count = match mark {
                    Mark::Bold => &mut self.bold,
                    Mark::Italic => &mut self.italic,
                    Mark::Underline => &mut self.underline,
                    Mark::Strikethrough => &mut self.strikethrough,
                    Mark::Overline => &mut self.overline,
                };
                *count = if on {
                    *count + 1
                } else {
                    count.saturating_sub(1)
                };
            }
            Tag::Size(size) => push_or_pop(&mut self.sizes, size),
            Tag::Font(chain) => push_or_pop(&mut self.fonts, chain),
            Tag::Color(color) => push_or_pop(&mut self.colors, color),
            Tag::Wave(wave) => push_or_pop(&mut self.waves, wave),
            Tag::Link(target) => scoped(links, &mut self.links, target),
            Tag::Hint(said) => scoped(hints, &mut self.hints, said),
            Tag::Align(_) | Tag::Image(_) => {}
        }
    }
}

/// Open or close one of the values the block keeps once and the spans point
/// at: a link's target, a hint's text.
fn scoped(pool: &mut Vec<String>, open: &mut Vec<u16>, value: Option<String>) {
    match value {
        Some(value) => {
            pool.push(value);
            open.push(u16::try_from(pool.len() - 1).unwrap_or(0));
        }
        None => {
            open.pop();
        }
    }
}

/// Split a string into styled runs. Adjacent text under one style is one
/// span, so a plain string is a single span.
pub(crate) fn parse(source: &str) -> Markup {
    let mut spans: Vec<Span> = Vec::new();
    let mut align = None;
    let mut links: Vec<String> = Vec::new();
    let mut hints: Vec<String> = Vec::new();
    let mut style = Style::default();
    let mut text = String::new();
    let mut rest = source;

    let flush = |text: &mut String, spans: &mut Vec<Span>, style: &Style| {
        if !text.is_empty() {
            spans.push(style.span(std::mem::take(text)));
        }
    };

    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        let tag = &rest[open + 1..open + close];
        let before = &rest[..open];
        let after = &rest[open + close + 1..];
        match tag_of(tag) {
            Some(Tag::Align(set)) => {
                text.push_str(before);
                if let Some(set) = set {
                    align = Some(set);
                }
            }
            Some(Tag::Image(inline)) => {
                text.push_str(before);
                flush(&mut text, &mut spans, &style);
                let mut span = style.span(String::from('\u{a0}'));
                span.image = Some(inline);
                spans.push(span);
            }
            None => {
                text.push_str(before);
                text.push('[');
                text.push_str(tag);
                text.push(']');
            }
            Some(styled) => {
                text.push_str(before);
                flush(&mut text, &mut spans, &style);
                style.apply(styled, &mut links, &mut hints);
            }
        }
        rest = after;
    }
    text.push_str(rest);
    flush(&mut text, &mut spans, &style);
    Markup {
        spans,
        align,
        links,
        hints,
    }
}

/// A mark that is on or off for the text it wraps, and nests.
#[derive(Clone, Copy)]
enum Mark {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Overline,
}

/// Open a scoped value, or close the innermost one.
fn push_or_pop<T>(stack: &mut Vec<T>, value: Option<T>) {
    match value {
        Some(value) => stack.push(value),
        None => {
            stack.pop();
        }
    }
}

enum Tag {
    /// A mark, and whether this tag opens it.
    Toggle(Mark, bool),
    Color(Option<Color32>),
    /// A size in the request's pixels; `None` closes.
    Size(Option<f32>),
    /// A named chain; `None` closes.
    Font(Option<String>),
    Wave(Option<(f32, f32)>),
    /// `None` closes; the alignment stays what the opener set.
    Align(Option<Align>),
    Image(Inline),
    /// What a span reports when clicked; `None` closes.
    Link(Option<String>),
    /// What a span says on hover; `None` closes.
    Hint(Option<String>),
}

fn tag_of(tag: &str) -> Option<Tag> {
    let (closing, body) = match tag.strip_prefix('/') {
        Some(body) => (true, body.trim()),
        None => (false, tag.trim()),
    };
    let (name, args) = body.split_once(['=', ' ']).unwrap_or((body, ""));
    match (name, closing) {
        ("b", on) => Some(Tag::Toggle(Mark::Bold, !on)),
        ("i", on) => Some(Tag::Toggle(Mark::Italic, !on)),
        ("u", on) => Some(Tag::Toggle(Mark::Underline, !on)),
        ("s", on) => Some(Tag::Toggle(Mark::Strikethrough, !on)),
        ("o", on) => Some(Tag::Toggle(Mark::Overline, !on)),
        ("size", true) => Some(Tag::Size(None)),
        ("size", false) => args
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|size| *size > 0.0)
            .map(|size| Tag::Size(Some(size))),
        ("font", true) => Some(Tag::Font(None)),
        ("font", false) => {
            let chain = args.trim();
            (!chain.is_empty()).then(|| Tag::Font(Some(chain.to_string())))
        }
        ("color", true) => Some(Tag::Color(None)),
        ("color", false) => color_of(args.trim()).map(|c| Tag::Color(Some(c))),
        ("wave", true) => Some(Tag::Wave(None)),
        ("wave", false) => {
            let amp = number_arg(args, "amp").unwrap_or(4.0);
            let freq = number_arg(args, "freq").unwrap_or(2.0);
            Some(Tag::Wave(Some((amp, freq))))
        }
        ("center" | "right" | "left", true) => Some(Tag::Align(None)),
        ("center", false) => Some(Tag::Align(Some(Align::Center))),
        ("right", false) => Some(Tag::Align(Some(Align::Right))),
        ("left", false) => Some(Tag::Align(Some(Align::Left))),
        ("url", true) => Some(Tag::Link(None)),
        ("url", false) => {
            let target = args.trim();
            (!target.is_empty()).then(|| Tag::Link(Some(target.to_string())))
        }
        ("hint", true) => Some(Tag::Hint(None)),
        ("hint", false) => {
            let said = args.trim();
            (!said.is_empty()).then(|| Tag::Hint(Some(said.to_string())))
        }
        ("img", false) => {
            let (path, extra) = args.split_once(' ').unwrap_or((args, ""));
            let path = path.trim();
            if path.is_empty() {
                return None;
            }
            let width = number_arg(extra, "width").unwrap_or(0.0);
            let height = number_arg(extra, "height").unwrap_or(width);
            Some(Tag::Image(Inline {
                path: path.to_string(),
                width: if width > 0.0 { width } else { height },
                height,
            }))
        }
        _ => None,
    }
}

fn number_arg(args: &str, name: &str) -> Option<f32> {
    args.split_whitespace()
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
        .and_then(|v| v.parse().ok())
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa`.
fn color_of(text: &str) -> Option<Color32> {
    let hex = text.strip_prefix('#')?;
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => {
            let short = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|v| v * 17);
            Some(Color32::from_rgb(short(0)?, short(1)?, short(2)?))
        }
        6 => Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            channel(0)?,
            channel(2)?,
            channel(4)?,
            channel(6)?,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(markup: &Markup) -> Vec<&str> {
        markup.spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn a_plain_string_is_one_span() {
        let parsed = parse("hello world");
        assert_eq!(texts(&parsed), ["hello world"]);
        assert!(!parsed.spans[0].bold);
        assert_eq!(parsed.align, None);
    }

    #[test]
    fn emphasis_splits_the_string_into_runs() {
        let parsed = parse("a [b]bold[/b] and [i]slanted[/i] word");
        assert_eq!(texts(&parsed), ["a ", "bold", " and ", "slanted", " word"]);
        assert!(parsed.spans[1].bold && !parsed.spans[1].italic);
        assert!(parsed.spans[3].italic && !parsed.spans[3].bold);
        assert!(!parsed.spans[4].italic);
    }

    #[test]
    fn a_colour_nests_and_pops_back_to_the_one_outside() {
        let parsed = parse("[color=#ff0000]red [color=#00ff00]green[/color] red[/color] plain");
        assert_eq!(texts(&parsed), ["red ", "green", " red", " plain"]);
        assert_eq!(parsed.spans[0].color, Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(parsed.spans[1].color, Some(Color32::from_rgb(0, 255, 0)));
        assert_eq!(parsed.spans[2].color, Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(parsed.spans[3].color, None);
    }

    #[test]
    fn a_wave_carries_its_amplitude_and_frequency() {
        let parsed = parse("[wave amp=8 freq=3]hi[/wave]");
        assert_eq!(parsed.spans[0].wave, Some((8.0, 3.0)));
        assert_eq!(parse("[wave]hi[/wave]").spans[0].wave, Some((4.0, 2.0)));
    }

    #[test]
    fn alignment_is_a_block_property_not_a_span() {
        let parsed = parse("[center]title[/center]");
        assert_eq!(texts(&parsed), ["title"]);
        assert_eq!(parsed.align, Some(Align::Center));
        assert_eq!(parse("[right]x").align, Some(Align::Right));
    }

    #[test]
    fn an_image_is_a_span_of_its_own_with_a_box() {
        let parsed = parse("coin [img=icons/coin.png width=24] each");
        assert_eq!(parsed.spans.len(), 3);
        let image = parsed.spans[1].image.as_ref().unwrap();
        assert_eq!(image.path, "icons/coin.png");
        assert_eq!((image.width, image.height), (24.0, 24.0));
    }

    #[test]
    fn underline_strike_and_overline_each_mark_their_own_run() {
        let parsed = parse("a[u]b[s]c[/s][/u][o]d[/o]e");
        assert_eq!(texts(&parsed), ["a", "b", "c", "d", "e"]);
        let marks: Vec<(bool, bool, bool)> = parsed
            .spans
            .iter()
            .map(|s| (s.lines.underline, s.lines.strikethrough, s.lines.overline))
            .collect();
        assert_eq!(
            marks,
            [
                (false, false, false),
                (true, false, false),
                (true, true, false),
                (false, false, true),
                (false, false, false)
            ]
        );
    }

    #[test]
    fn a_size_and_a_font_nest_and_pop_back() {
        let parsed = parse("[size=30]big [font=mono]code[/font][/size] [size=x]plain");
        assert_eq!(texts(&parsed), ["big ", "code", " [size=x]plain"]);
        assert_eq!(parsed.spans[0].size, Some(30.0));
        assert_eq!(parsed.spans[1].font.as_deref(), Some("mono"));
        assert_eq!(parsed.spans[1].size, Some(30.0));
        assert_eq!(parsed.spans[2].size, None);
        assert_eq!(parsed.spans[2].font, None);
    }

    #[test]
    fn brackets_that_are_not_a_tag_stay_in_the_text() {
        assert_eq!(
            texts(&parse("press [Space] to jump")),
            ["press [Space] to jump"]
        );
        assert_eq!(texts(&parse("a [ b")), ["a [ b"]);
    }

    #[test]
    fn short_and_long_hex_colours_both_parse() {
        assert_eq!(color_of("#fff"), Some(Color32::from_rgb(255, 255, 255)));
        assert_eq!(
            color_of("#10203040"),
            Some(Color32::from_rgba_unmultiplied(16, 32, 48, 64))
        );
        assert_eq!(color_of("red"), None);
    }
}
