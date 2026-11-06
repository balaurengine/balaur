//! What a widget's text is shaped and dressed with past its font and size:
//! how its lines break and are cut, the wash behind it, and what its
//! `text_style` says, read into the shaper's own types.

use balaur_text::text_style::TextStyle;
use egui::Color32;
use smol_str::SmolStr;

use crate::widget::properties::WidgetProperties;

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
pub(crate) fn color_of(quad: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(quad[0]), byte(quad[1]), byte(quad[2]), byte(quad[3]))
}

impl TextLook {
    /// What a widget's own properties say, over the defaults. The style it
    /// names is read in by [`Self::dress_with`], which needs the asset layer,
    /// and how its lines break is `widget_text`'s or `widget_field`'s.
    pub(crate) fn of(asked: &WidgetProperties) -> Self {
        Self {
            text_style: asked.text_style.clone(),
            ..Self::default()
        }
    }

    /// Take what the widget's `text_style` says.
    pub(crate) fn dress_with(&mut self, style: TextStyle) {
        self.options.decoration = style.decoration;
        self.line_height = style.line_height;
        self.letter_spacing = style.letter_spacing;
        self.effects = style.effects;
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
