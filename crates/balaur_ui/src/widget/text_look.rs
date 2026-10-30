//! What a widget's text is shaped and dressed with past its font and size:
//! how its lines break and are cut, the wash behind it, and what its
//! `text_style` says, read into the shaper's own types.

use balaur_text::text_style::TextStyle;
use egui::Color32;
use smol_str::SmolStr;

use crate::vocabulary::keys as k;
use crate::widget::options::Read;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextLook {
    pub options: balaur_text::Options,
    /// The `text_style` asset the widget names; empty draws it plain.
    pub text_style: SmolStr,
    /// Baseline to baseline as a multiple of the size; zero takes the
    /// default. This and the effects and the options' decoration are the
    /// style's, read in where the widget is applied.
    pub line_height: f32,
    /// Extra space between glyphs, in the caption's pixels.
    pub letter_spacing: f32,
    pub effects: balaur_text::Effects,
    /// A wash behind the glyphs of the kinds egui draws; alpha 0 is none.
    pub background: Color32,
}

impl Default for TextLook {
    fn default() -> Self {
        let plain = TextStyle::default();
        Self {
            options: balaur_text::Options::default(),
            text_style: SmolStr::default(),
            line_height: plain.line_height,
            letter_spacing: plain.letter_spacing,
            effects: plain.effects,
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

impl TextLook {
    /// What a widget's params say, over the defaults. The style it names is
    /// read in by [`Self::dress_with`], which needs the asset layer.
    pub(crate) fn read(params: &toml::Value) -> Self {
        let r = Read(params);
        let base = Self::default();
        let tab = r.num(k::TAB_WIDTH);
        Self {
            options: balaur_text::Options {
                line_break: balaur_text::LineBreak::of(&r.str(k::LINE_BREAK)),
                truncate_at: balaur_text::TruncateAt::of(&r.str(k::TRUNCATE_AT)),
                max_lines: r.num(k::MAX_LINES).max(0.0) as u32,
                max_height: None,
                tab_width: if tab >= 1.0 {
                    tab.min(64.0) as u16
                } else {
                    base.options.tab_width
                },
                ..base.options
            },
            text_style: r.str(k::TEXT_STYLE),
            background: color_of(r.quad(k::TEXT_BACKGROUND)),
            ..base
        }
    }

    /// Take what the widget's `text_style` says.
    pub(crate) fn dress_with(&mut self, style: TextStyle) {
        self.options.decoration = style.decoration;
        self.line_height = style.line_height;
        self.letter_spacing = style.letter_spacing;
        self.effects = style.effects;
    }

    /// The look back into the table a scene file would have written.
    pub(crate) fn put(&self, map: &mut toml::map::Map<String, toml::Value>) {
        use toml::Value as V;
        let o = &self.options;
        let pairs = [
            (k::TEXT_STYLE, V::String(self.text_style.to_string())),
            (
                k::TEXT_BACKGROUND,
                crate::widget::options::four(quad_of(self.background)),
            ),
            (k::LINE_BREAK, V::String(o.line_break.word().into())),
            (k::TRUNCATE_AT, V::String(o.truncate_at.word().into())),
            (k::MAX_LINES, V::Integer(i64::from(o.max_lines))),
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
