//! What a widget's text is shaped and dressed with past its face and size:
//! the keys `text2d` and `text3d` take, read into the shaper's own types.

use balaur_text::vocabulary::words as tw;
use egui::Color32;
use smol_str::SmolStr;

use crate::vocabulary::{keys as k, options};
use crate::widget::options::Read;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextLook {
    pub options: balaur_text::Options,
    /// Baseline to baseline as a multiple of the size; zero takes the default.
    pub line_height: f32,
    /// Extra space between glyphs, in the caption's pixels.
    pub letter_spacing: f32,
    /// A project-relative AngelCode `.fnt`; empty shapes with the vector chain.
    pub bitmap_font: SmolStr,
    pub effects: balaur_text::Effects,
    /// A wash behind the glyphs of the kinds egui draws; alpha 0 is none.
    pub background: Color32,
}

impl Default for TextLook {
    fn default() -> Self {
        Self {
            options: balaur_text::Options::default(),
            line_height: 0.0,
            letter_spacing: 0.0,
            bitmap_font: SmolStr::default(),
            effects: balaur_text::Effects {
                outline_size: 0.0,
                outline_color: Color32::BLACK,
                shadow_offset: egui::Vec2::ZERO,
                shadow_color: Color32::from_black_alpha(128),
            },
            background: Color32::TRANSPARENT,
        }
    }
}

/// The channels a scene writes, as the widget's other colour keys read them.
fn color_of(quad: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(quad[0]), byte(quad[1]), byte(quad[2]), byte(quad[3]))
}

fn quad_of(color: Color32) -> [f32; 4] {
    color.to_srgba_unmultiplied().map(|v| f32::from(v) / 255.0)
}

/// A decoration colour as the shaper takes it: alpha 0 follows the glyphs.
fn line_color(quad: [f32; 4]) -> Option<Color32> {
    (quad[3] > 0.0).then(|| color_of(quad))
}

impl TextLook {
    /// The schema lines, in the words `text2d` uses for the same keys.
    pub(crate) fn schema() -> Vec<(&'static str, String)> {
        let colour = |what: &str| {
            format!(
                r#"{{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The {what}'s colour; alpha 0 takes the glyphs' own", group = "type" }}"#
            )
        };
        vec![
            (k::LINE_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Baseline to baseline as a multiple of the size; zero takes the default", group = "type" }"#.into()),
            (k::LETTER_SPACING, r#"{ type = "float", default = 0.0, description = "Extra space between glyphs, in the caption's pixels", group = "type" }"#.into()),
            (k::BITMAP_FONT, r#"{ type = "string", default = "", description = "A project-relative AngelCode .fnt naming a bitmap face for a shaped label or caption; empty shapes with the project's vector fonts", group = "type" }"#.into()),
            (k::OUTLINE_SIZE, r#"{ type = "float", default = 0.0, min = 0.0, description = "Pixels the outline reaches round a shaped label's glyphs; zero draws none", group = "type" }"#.into()),
            (k::OUTLINE_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 1.0], description = "The outline's colour", group = "type" }"#.into()),
            (k::SHADOW_OFFSET_X, r#"{ type = "float", default = 0.0, description = "Pixels a shaped label's shadow is moved along x; zero with y draws none", group = "type" }"#.into()),
            (k::SHADOW_OFFSET_Y, r#"{ type = "float", default = 0.0, description = "Pixels the shadow is moved along y", group = "type" }"#.into()),
            (k::SHADOW_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.5], description = "The shadow's colour", group = "type" }"#.into()),
            (k::TEXT_BACKGROUND, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "A wash behind the text of the kinds egui draws; alpha 0 is none", group = "type" }"#.into()),
            (k::FONT_STRETCH, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How wide a face is picked among the faces its family ships", group = "type" }}"#, tw::NORMAL, options(tw::FONT_STRETCHES))),
            (k::FONT_NAME, r#"{ type = "string", default = "", description = "A face by family name, tried before the chain", group = "type" }"#.into()),
            (k::FONT_FEATURES, r#"{ type = "list", of = { type = "string" }, default = [], description = "OpenType features: `smcp` turns one on, `liga=0` turns one off", group = "type" }"#.into()),
            (k::UNDERLINE, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "A line under the text, once or twice; egui's own kinds draw one either way", group = "type" }}"#, tw::NONE, options(tw::UNDERLINES))),
            (k::UNDERLINE_COLOR, colour("underline")),
            (k::STRIKETHROUGH, r#"{ type = "bool", default = false, description = "A line through the text", group = "type" }"#.into()),
            (k::STRIKETHROUGH_COLOR, colour("strikethrough")),
            (k::OVERLINE, r#"{ type = "bool", default = false, description = "A line over a shaped label's text; egui's own kinds have none", group = "type" }"#.into()),
            (k::OVERLINE_COLOR, colour("overline")),
            (k::LINE_BREAK, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where a wrapped line may break", group = "type" }}"#, tw::WORD_OR_GLYPH, options(tw::LINE_BREAKS))),
            (k::TRUNCATE_AT, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Which part of a cut caption the ellipsis stands in for", group = "type" }}"#, tw::END, options(tw::TRUNCATE_ATS))),
            (k::MAX_LINES, r#"{ type = "int", default = 0, min = 0, description = "The most lines a wrapped label keeps; zero keeps every line", group = "type" }"#.into()),
            (k::SHAPING, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Complex shaping joins scripts that need it and falls back to another face; simple does neither", group = "type" }}"#, tw::COMPLEX, options(tw::SHAPINGS))),
            (k::SNAP_ADVANCES, r#"{ type = "bool", default = false, description = "Round each glyph's advance to a whole pixel", group = "type" }"#.into()),
            (k::HINTING, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Snap outlines to the pixel grid; auto takes the face's import setting", group = "type" }}"#, tw::AUTO, options(tw::HINTINGS))),
            (k::PIXEL_SNAP, r#"{ type = "bool", default = false, description = "Rasterise on whole pixels, for a pixel face", group = "type" }"#.into()),
            (k::MONOSPACE_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "Pixels a monospace face's advance is set to; zero keeps its own", group = "type" }"#.into()),
            (k::TAB_WIDTH, r#"{ type = "int", default = 8, min = 1, max = 64, description = "Spaces between tab stops", group = "type" }"#.into()),
        ]
    }

    /// What a widget's params say, over the defaults.
    pub(crate) fn read(params: &toml::Value) -> Self {
        let r = Read(params);
        let base = Self::default();
        let positive = |v: f32| (v > 0.0).then_some(v);
        let features = crate::widget::options::strings(params, k::FONT_FEATURES)
            .iter()
            .filter_map(|f| balaur_text::Feature::parse(f))
            .collect();
        let tab = r.num(k::TAB_WIDTH);
        Self {
            options: balaur_text::Options {
                stretch: balaur_text::Stretch::of(&r.str(k::FONT_STRETCH)),
                font_name: r.str(k::FONT_NAME).to_string(),
                features,
                decoration: balaur_text::Decoration {
                    underline: balaur_text::Underline::of(&r.str(k::UNDERLINE)),
                    underline_color: line_color(r.quad(k::UNDERLINE_COLOR)),
                    strikethrough: r.flag(k::STRIKETHROUGH),
                    strikethrough_color: line_color(r.quad(k::STRIKETHROUGH_COLOR)),
                    overline: r.flag(k::OVERLINE),
                    overline_color: line_color(r.quad(k::OVERLINE_COLOR)),
                },
                line_break: balaur_text::LineBreak::of(&r.str(k::LINE_BREAK)),
                truncate_at: balaur_text::TruncateAt::of(&r.str(k::TRUNCATE_AT)),
                max_lines: r.num(k::MAX_LINES).max(0.0) as u32,
                max_height: None,
                shaping: balaur_text::Shaping::of(&r.str(k::SHAPING)),
                snap_advances: r.flag(k::SNAP_ADVANCES),
                hinting: balaur_text::Hinting::of(&r.str(k::HINTING)),
                pixel_snap: r.flag(k::PIXEL_SNAP),
                monospace_width: positive(r.num(k::MONOSPACE_WIDTH)),
                tab_width: if tab >= 1.0 {
                    tab.min(64.0) as u16
                } else {
                    base.options.tab_width
                },
            },
            line_height: r.num(k::LINE_HEIGHT).max(0.0),
            letter_spacing: r.num(k::LETTER_SPACING),
            bitmap_font: r.str(k::BITMAP_FONT),
            effects: balaur_text::Effects {
                outline_size: r.num(k::OUTLINE_SIZE).max(0.0),
                outline_color: color_of(r.quad(k::OUTLINE_COLOR)),
                shadow_offset: egui::vec2(r.num(k::SHADOW_OFFSET_X), r.num(k::SHADOW_OFFSET_Y)),
                shadow_color: color_of(r.quad(k::SHADOW_COLOR)),
            },
            background: color_of(r.quad(k::TEXT_BACKGROUND)),
        }
    }

    /// The look back into the table a scene file would have written.
    pub(crate) fn put(&self, map: &mut toml::map::Map<String, toml::Value>) {
        use toml::Value as V;
        let o = &self.options;
        let d = &o.decoration;
        let colour = |c: Option<Color32>| crate::widget::options::four(c.map_or([0.0; 4], quad_of));
        let pairs = [
            (k::LINE_HEIGHT, V::Float(f64::from(self.line_height))),
            (k::LETTER_SPACING, V::Float(f64::from(self.letter_spacing))),
            (k::BITMAP_FONT, V::String(self.bitmap_font.to_string())),
            (
                k::OUTLINE_SIZE,
                V::Float(f64::from(self.effects.outline_size)),
            ),
            (
                k::OUTLINE_COLOR,
                crate::widget::options::four(quad_of(self.effects.outline_color)),
            ),
            (
                k::SHADOW_OFFSET_X,
                V::Float(f64::from(self.effects.shadow_offset.x)),
            ),
            (
                k::SHADOW_OFFSET_Y,
                V::Float(f64::from(self.effects.shadow_offset.y)),
            ),
            (
                k::SHADOW_COLOR,
                crate::widget::options::four(quad_of(self.effects.shadow_color)),
            ),
            (
                k::TEXT_BACKGROUND,
                crate::widget::options::four(quad_of(self.background)),
            ),
            (k::FONT_STRETCH, V::String(o.stretch.word().into())),
            (k::FONT_NAME, V::String(o.font_name.clone())),
            (
                k::FONT_FEATURES,
                V::Array(
                    o.features
                        .iter()
                        .map(|f| V::String(feature_word(*f)))
                        .collect(),
                ),
            ),
            (k::UNDERLINE, V::String(d.underline.word().into())),
            (k::UNDERLINE_COLOR, colour(d.underline_color)),
            (k::STRIKETHROUGH, V::Boolean(d.strikethrough)),
            (k::STRIKETHROUGH_COLOR, colour(d.strikethrough_color)),
            (k::OVERLINE, V::Boolean(d.overline)),
            (k::OVERLINE_COLOR, colour(d.overline_color)),
            (k::LINE_BREAK, V::String(o.line_break.word().into())),
            (k::TRUNCATE_AT, V::String(o.truncate_at.word().into())),
            (k::MAX_LINES, V::Integer(i64::from(o.max_lines))),
            (k::SHAPING, V::String(o.shaping.word().into())),
            (k::SNAP_ADVANCES, V::Boolean(o.snap_advances)),
            (k::HINTING, V::String(o.hinting.word().into())),
            (k::PIXEL_SNAP, V::Boolean(o.pixel_snap)),
            (
                k::MONOSPACE_WIDTH,
                V::Float(f64::from(o.monospace_width.unwrap_or(0.0))),
            ),
            (k::TAB_WIDTH, V::Integer(i64::from(o.tab_width))),
        ];
        for (key, value) in pairs {
            map.insert(key.to_string(), value);
        }
    }

    /// The same look on the text of a kind egui draws: what `RichText` can
    /// carry of it. egui draws one underline and no overline.
    pub(crate) fn dress(&self, text: egui::RichText, size: f32) -> egui::RichText {
        let d = &self.options.decoration;
        let mut text = text;
        if d.underline != balaur_text::Underline::None {
            text = text.underline();
        }
        if d.strikethrough {
            text = text.strikethrough();
        }
        if self.letter_spacing != 0.0 {
            text = text.extra_letter_spacing(self.letter_spacing);
        }
        if self.line_height > 0.0 {
            text = text.line_height(Some(self.line_height * size));
        }
        if self.background.a() > 0 {
            text = text.background_color(self.background);
        }
        text
    }
}

/// A feature back as a scene spells it: the tag, then `=n` unless it is on.
fn feature_word(feature: balaur_text::Feature) -> String {
    let tag = String::from_utf8_lossy(&feature.tag).into_owned();
    if feature.value == 1 {
        tag
    } else {
        format!("{tag}={}", feature.value)
    }
}
