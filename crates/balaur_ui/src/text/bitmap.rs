//! AngelCode bitmap fonts: a pixel font drawn as the artist drew it.
//!
//! A `.fnt` descriptor names a page image and gives every glyph's box on it,
//! its bearings and its advance. Nothing here is shaped — a bitmap font has
//! one glyph per character and no contextual forms — so this lays out by
//! advance and kerning, and hands back the same [`super::Shaped`] the vector
//! path does. The page is copied into the atlas once, so a bitmap glyph and a
//! rasterised one come from the same texture.

use std::collections::HashMap;

use egui::{Rect, Vec2, pos2, vec2};

/// One character's box on the page, in page pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glyph {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    /// Where the box sits relative to the pen, down being positive.
    pub(crate) offset: Vec2,
    /// How far the pen moves after it.
    pub(crate) advance: f32,
}

/// A parsed `.fnt` and the page it draws from.
#[derive(Clone, Debug, Default)]
pub struct BitmapFont {
    /// Project-relative path to the page image.
    pub page: String,
    /// The size the font was authored at, in pixels.
    pub size: f32,
    /// Distance between baselines, in page pixels.
    pub line_height: f32,
    pub(crate) glyphs: HashMap<char, Glyph>,
    pub(crate) kerning: HashMap<(char, char), f32>,
}

/// One `key=value` pair off a `.fnt` line.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    for part in line.split_whitespace() {
        if let Some(value) = part.strip_prefix(key)
            && value.starts_with('=')
        {
            return Some(value[1..].trim_matches('"'));
        }
    }
    None
}

fn number(line: &str, key: &str) -> Option<f32> {
    field(line, key)?.parse().ok()
}

/// Parse the text form of a `.fnt`. The binary form is not read: every tool
/// that writes one writes this too, and it is the form a person can diff.
///
/// # Errors
/// If the descriptor names no page or declares no glyph.
pub fn parse(source: &str) -> anyhow::Result<BitmapFont> {
    let mut font = BitmapFont {
        size: 16.0,
        ..BitmapFont::default()
    };
    for line in source.lines() {
        let line = line.trim();
        let Some(kind) = line.split_whitespace().next() else {
            continue;
        };
        match kind {
            "info" => {
                if let Some(size) = number(line, "size") {
                    font.size = size.abs().max(1.0);
                }
            }
            "common" => {
                font.line_height = number(line, "lineHeight").unwrap_or(font.size);
            }
            // Only the first page: a font spread over several is a font whose
            // glyphs cannot share one atlas region, and none of ours are.
            "page" => {
                if font.page.is_empty()
                    && let Some(file) = field(line, "file")
                {
                    font.page = file.to_string();
                }
            }
            "char" => {
                let Some(id) = number(line, "id") else {
                    continue;
                };
                let Some(character) = u32::try_from(id as i64).ok().and_then(char::from_u32) else {
                    continue;
                };
                font.glyphs.insert(
                    character,
                    Glyph {
                        x: number(line, "x").unwrap_or(0.0),
                        y: number(line, "y").unwrap_or(0.0),
                        width: number(line, "width").unwrap_or(0.0),
                        height: number(line, "height").unwrap_or(0.0),
                        offset: vec2(
                            number(line, "xoffset").unwrap_or(0.0),
                            number(line, "yoffset").unwrap_or(0.0),
                        ),
                        advance: number(line, "xadvance").unwrap_or(0.0),
                    },
                );
            }
            "kerning" => {
                let first = number(line, "first")
                    .and_then(|v| u32::try_from(v as i64).ok())
                    .and_then(char::from_u32);
                let second = number(line, "second")
                    .and_then(|v| u32::try_from(v as i64).ok())
                    .and_then(char::from_u32);
                if let (Some(first), Some(second)) = (first, second) {
                    font.kerning
                        .insert((first, second), number(line, "amount").unwrap_or(0.0));
                }
            }
            _ => {}
        }
    }
    if font.page.is_empty() {
        anyhow::bail!("a bitmap font needs a `page` line naming its image");
    }
    if font.glyphs.is_empty() {
        anyhow::bail!("a bitmap font declares no `char` lines");
    }
    if font.line_height <= 0.0 {
        font.line_height = font.size;
    }
    Ok(font)
}

impl BitmapFont {
    /// Lay `text` out at `size` pixels, over a page already placed in the
    /// atlas at `region` and `page_size` pixels across.
    ///
    /// One glyph per character, advanced and kerned: a bitmap font has no
    /// contextual forms, so there is nothing to shape.
    pub(crate) fn layout(
        &self,
        text: &str,
        size: f32,
        region: Rect,
        page_size: Vec2,
    ) -> super::Shaped {
        // Authored at `self.size`; asked for at `size`.
        let scale = size / self.size;
        let mut quads = Vec::new();
        let mut pen = 0.0f32;
        let mut widest = 0.0f32;
        let mut top = 0.0f32;
        let mut previous: Option<char> = None;
        for character in text.chars() {
            if character == '\n' {
                widest = widest.max(pen);
                pen = 0.0;
                top += self.line_height * scale;
                previous = None;
                continue;
            }
            let Some(glyph) = self.glyphs.get(&character) else {
                previous = Some(character);
                continue;
            };
            if let Some(before) = previous
                && let Some(amount) = self.kerning.get(&(before, character))
            {
                pen += amount * scale;
            }
            if glyph.width > 0.0 && glyph.height > 0.0 {
                let at = pos2(pen + glyph.offset.x * scale, top + glyph.offset.y * scale);
                // The page's own pixels, as a fraction of the atlas.
                let uv_min = pos2(
                    region.min.x + glyph.x / page_size.x * region.width(),
                    region.min.y + glyph.y / page_size.y * region.height(),
                );
                let uv_max = pos2(
                    region.min.x + (glyph.x + glyph.width) / page_size.x * region.width(),
                    region.min.y + (glyph.y + glyph.height) / page_size.y * region.height(),
                );
                quads.push(super::Quad {
                    rect: Rect::from_min_size(at, vec2(glyph.width * scale, glyph.height * scale)),
                    uv: Rect::from_min_max(uv_min, uv_max),
                    color: None,
                    // The page carries its own colour, as an emoji does: the
                    // label's tint would otherwise wash the art out.
                    colored: true,
                    wave: None,
                });
            }
            pen += glyph.advance * scale;
            previous = Some(character);
        }
        widest = widest.max(pen);
        super::Shaped {
            size: vec2(widest, top + self.line_height * scale),
            quads,
            pictures: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
info face="Pixel" size=16
common lineHeight=18 base=14 scaleW=128 scaleH=128 pages=1
page id=0 file="pixel_0.png"
chars count=2
char id=65 x=0 y=0 width=8 height=10 xoffset=0 yoffset=2 xadvance=9
char id=66 x=8 y=0 width=8 height=10 xoffset=0 yoffset=2 xadvance=9
kernings count=1
kerning first=65 second=66 amount=-1
"#;

    #[test]
    fn a_descriptor_reads_its_page_glyphs_and_kerning() {
        let font = parse(SAMPLE).expect("the sample parses");
        assert_eq!(font.page, "pixel_0.png");
        assert_eq!(font.size, 16.0);
        assert_eq!(font.line_height, 18.0);
        assert_eq!(font.glyphs.len(), 2);
        assert_eq!(font.kerning.get(&('A', 'B')), Some(&-1.0));
    }

    #[test]
    fn a_descriptor_with_no_page_is_an_error() {
        assert!(parse("info face=\"x\" size=16\nchar id=65 x=0 y=0").is_err());
    }

    /// Kerning pulls the pair together, so `AB` is narrower than twice `A`.
    #[test]
    fn kerning_narrows_the_pair() {
        let font = parse(SAMPLE).expect("the sample parses");
        let region = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let page = vec2(128.0, 128.0);
        let one = font.layout("A", 16.0, region, page);
        let pair = font.layout("AB", 16.0, region, page);
        assert!(pair.size.x < one.size.x * 2.0);
        assert_eq!(pair.quads.len(), 2);
    }

    /// Asked for at twice the authored size, everything doubles.
    #[test]
    fn a_bigger_size_scales_the_boxes() {
        let font = parse(SAMPLE).expect("the sample parses");
        let region = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let page = vec2(128.0, 128.0);
        let small = font.layout("A", 16.0, region, page);
        let large = font.layout("A", 32.0, region, page);
        assert!((large.size.x - small.size.x * 2.0).abs() < 0.01);
    }
}
