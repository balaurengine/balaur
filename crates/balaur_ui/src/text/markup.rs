//! Inline marks in a string: the small tag set a localized string carries.
//!
//! `[b]`, `[i]`, `[color=#rrggbb]`, `[wave amp=8 freq=4]` wrap text;
//! `[center]` and `[right]` set the block's alignment; `[img=path width=32
//! height=32]` stands alone. Anything else in brackets is text, so a string
//! that was never markup still reads as it was written.

use egui::Color32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Span {
    pub(crate) text: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) color: Option<Color32>,
    /// Amplitude in pixels and frequency in cycles per second.
    pub(crate) wave: Option<(f32, f32)>,
    /// An inline picture the span stands in for, with its box.
    pub(crate) image: Option<Inline>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Inline {
    pub(crate) path: String,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Align {
    Start,
    Center,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Markup {
    pub(crate) spans: Vec<Span>,
    pub(crate) align: Option<Align>,
}

#[derive(Clone, Default)]
struct Style {
    bold: u32,
    italic: u32,
    colors: Vec<Color32>,
    waves: Vec<(f32, f32)>,
}

impl Style {
    fn span(&self, text: String) -> Span {
        Span {
            text,
            bold: self.bold > 0,
            italic: self.italic > 0,
            color: self.colors.last().copied(),
            wave: self.waves.last().copied(),
            image: None,
        }
    }
}

/// Split a string into styled runs. Adjacent text under one style is one
/// span, so a plain string is a single span.
pub(crate) fn parse(source: &str) -> Markup {
    let mut spans: Vec<Span> = Vec::new();
    let mut align = None;
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
            Some(Tag::Bold(on)) => {
                text.push_str(before);
                flush(&mut text, &mut spans, &style);
                style.bold = if on {
                    style.bold + 1
                } else {
                    style.bold.saturating_sub(1)
                };
            }
            Some(Tag::Italic(on)) => {
                text.push_str(before);
                flush(&mut text, &mut spans, &style);
                style.italic = if on {
                    style.italic + 1
                } else {
                    style.italic.saturating_sub(1)
                };
            }
            Some(Tag::Color(color)) => {
                text.push_str(before);
                flush(&mut text, &mut spans, &style);
                match color {
                    Some(color) => style.colors.push(color),
                    None => {
                        style.colors.pop();
                    }
                }
            }
            Some(Tag::Wave(wave)) => {
                text.push_str(before);
                flush(&mut text, &mut spans, &style);
                match wave {
                    Some(wave) => style.waves.push(wave),
                    None => {
                        style.waves.pop();
                    }
                }
            }
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
        }
        rest = after;
    }
    text.push_str(rest);
    flush(&mut text, &mut spans, &style);
    Markup { spans, align }
}

enum Tag {
    Bold(bool),
    Italic(bool),
    Color(Option<Color32>),
    Wave(Option<(f32, f32)>),
    /// `None` closes; the alignment stays what the opener set.
    Align(Option<Align>),
    Image(Inline),
}

fn tag_of(tag: &str) -> Option<Tag> {
    let (closing, body) = match tag.strip_prefix('/') {
        Some(body) => (true, body.trim()),
        None => (false, tag.trim()),
    };
    let (name, args) = body.split_once(['=', ' ']).unwrap_or((body, ""));
    match (name, closing) {
        ("b", on) => Some(Tag::Bold(!on)),
        ("i", on) => Some(Tag::Italic(!on)),
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
        ("right", false) => Some(Tag::Align(Some(Align::End))),
        ("left", false) => Some(Tag::Align(Some(Align::Start))),
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
        assert_eq!(parse("[right]x").align, Some(Align::End));
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
