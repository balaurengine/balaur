//! The `text_style` asset: how a text is dressed past its font — the space
//! between its lines and its glyphs, the lines drawn with them, an outline
//! and a shadow. Godot's `LabelSettings`: one file says it for every label,
//! world text and widget naming it.

use std::rc::Rc;

use anyhow::Result;
use balaur_core::Engine;
use balaur_core::components::{ComponentDef, Schema, rgba};
use egui::{Color32, vec2};

use crate::options::{Decoration, Underline};
use crate::vocabulary::{keys as k, options, words as w};

pub use balaur_core::assets::TEXT_STYLE_ASSET_TYPE;

const DOC: &str = "How a text is dressed past its font: the space between its lines and its glyphs, the lines drawn with them, an outline and a shadow. `text2d`, `text3d` and a widget name one in their `text_style` row, or write one inline:\n\n```toml\ntype = \"text_style\"\noutline_size = 2.0\nshadow_offset_y = 3.0\n```";

/// A `text_style` definition as the shaper and the painters take it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Baseline to baseline as a multiple of the size; zero takes the
    /// default.
    pub line_height: f32,
    /// Extra space between glyphs, in the text's pixels.
    pub letter_spacing: f32,
    pub decoration: Decoration,
    pub effects: crate::Effects,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            line_height: 0.0,
            letter_spacing: 0.0,
            decoration: Decoration::default(),
            effects: crate::Effects {
                outline_size: 0.0,
                outline_color: Color32::BLACK,
                shadow_offset: egui::Vec2::ZERO,
                shadow_color: Color32::from_black_alpha(128),
            },
        }
    }
}

/// The colour a definition writes, as the channels a scene spells it with.
fn colour(quad: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(quad[0]), byte(quad[1]), byte(quad[2]), byte(quad[3]))
}

fn schema() -> Rc<Schema> {
    let line_colour = |what: &str| {
        format!(
            r#"{{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The {what}'s colour; alpha 0 takes the glyphs' own" }}"#
        )
    };
    ComponentDef::parse_schema(
        TEXT_STYLE_ASSET_TYPE,
        &ComponentDef::schema(&[
            (
                k::LINE_HEIGHT,
                r#"{ type = "float", default = 0.0, min = 0.0, description = "Baseline to baseline as a multiple of the size; zero takes the default" }"#,
            ),
            (
                k::LETTER_SPACING,
                r#"{ type = "float", default = 0.0, description = "Extra space between glyphs, in the text's pixels" }"#,
            ),
            (
                k::OUTLINE_SIZE,
                r#"{ type = "float", default = 0.0, min = 0.0, description = "Pixels the outline reaches round the glyphs; zero draws none" }"#,
            ),
            (
                k::OUTLINE_COLOR,
                r#"{ type = "color", default = [0.0, 0.0, 0.0, 1.0], description = "The outline's colour" }"#,
            ),
            (
                k::SHADOW_COLOR,
                r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.5], description = "The shadow's colour" }"#,
            ),
            (
                k::SHADOW_OFFSET_X,
                r#"{ type = "float", default = 0.0, description = "Pixels the shadow is moved along x; zero with y draws none" }"#,
            ),
            (
                k::SHADOW_OFFSET_Y,
                r#"{ type = "float", default = 0.0, description = "Pixels the shadow is moved along y" }"#,
            ),
            (
                k::UNDERLINE,
                &format!(
                    r#"{{ type = "enum", default = "{}", options = [{}], description = "A line under the glyphs, once or twice" }}"#,
                    w::NONE,
                    options(w::UNDERLINES)
                ),
            ),
            (k::UNDERLINE_COLOR, &line_colour("underline")),
            (
                k::OVERLINE,
                r#"{ type = "bool", default = false, description = "A line over the glyphs" }"#,
            ),
            (k::OVERLINE_COLOR, &line_colour("overline")),
            (
                k::STRIKETHROUGH,
                r#"{ type = "bool", default = false, description = "A line through the glyphs" }"#,
            ),
            (k::STRIKETHROUGH_COLOR, &line_colour("strikethrough")),
        ]),
    )
}

/// A definition with every row filled in, as the asset layer hands it over.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the asset layer's parser type, which other assets fail through"
)]
fn parse(table: &toml::Value) -> Result<Rc<dyn std::any::Any>> {
    Ok(Rc::new(read(table)))
}

/// A definition's rows as the shaper and the painters take them.
#[must_use]
pub fn read(table: &toml::Value) -> TextStyle {
    let base = TextStyle::default();
    let number = |key: &str| {
        table
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .map_or(0.0, |v| v as f32)
    };
    let flag = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
    };
    let quad = |key: &str| table.get(key).and_then(rgba);
    // A line's colour with no alpha follows the glyphs under it.
    let line = |key: &str| quad(key).filter(|quad| quad[3] > 0.0).map(colour);
    TextStyle {
        line_height: number(k::LINE_HEIGHT).max(0.0),
        letter_spacing: number(k::LETTER_SPACING),
        decoration: Decoration {
            underline: Underline::of(
                table
                    .get(k::UNDERLINE)
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default(),
            ),
            underline_color: line(k::UNDERLINE_COLOR),
            strikethrough: flag(k::STRIKETHROUGH),
            strikethrough_color: line(k::STRIKETHROUGH_COLOR),
            overline: flag(k::OVERLINE),
            overline_color: line(k::OVERLINE_COLOR),
        },
        effects: crate::Effects {
            outline_size: number(k::OUTLINE_SIZE).max(0.0),
            outline_color: quad(k::OUTLINE_COLOR).map_or(base.effects.outline_color, colour),
            shadow_offset: vec2(number(k::SHADOW_OFFSET_X), number(k::SHADOW_OFFSET_Y)),
            shadow_color: quad(k::SHADOW_COLOR).map_or(base.effects.shadow_color, colour),
        },
    }
}

/// Register the `text_style` asset, whose definitions sit in `text_styles/`.
pub fn register_text_style_asset(reg: &mut balaur_plugin::Registry<'_>) {
    reg.register_record_asset(TEXT_STYLE_ASSET_TYPE, "text_styles", DOC, schema(), parse);
}

/// The style `reference` names: the default for an empty one, and for one
/// that does not read, which is said once.
#[must_use]
pub fn of(eng: &Engine, reference: &str) -> TextStyle {
    if reference.is_empty() {
        return TextStyle::default();
    }
    match balaur_core::assets::load_typed::<TextStyle>(eng, reference) {
        Ok(style) => *style,
        Err(err) => {
            if balaur_core::logbuf::first_time("text style", reference) {
                tracing::warn!("text style '{reference}': {err:#}");
            }
            TextStyle::default()
        }
    }
}

/// Whether `reference` names a style kept in a file, which a save can change
/// under the text naming it; an inline one is filed by its own content.
#[must_use]
pub fn names_a_file(reference: &str) -> bool {
    !reference.is_empty() && !reference.starts_with('#')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_definition_reads_its_lines_outline_and_shadow() {
        let table: toml::Value = toml::from_str(
            "underline = \"double\"\nunderline_color = [1.0, 0.0, 0.0, 1.0]\noutline_size = 2.0\nshadow_offset_y = 3.0\nline_height = 1.5",
        )
        .unwrap();
        let style = read(&table);
        assert_eq!(style.decoration.underline, Underline::Double);
        assert_eq!(style.decoration.underline_color, Some(Color32::RED));
        assert!((style.effects.outline_size - 2.0).abs() < f32::EPSILON);
        assert!((style.effects.shadow_offset.y - 3.0).abs() < f32::EPSILON);
        assert!((style.line_height - 1.5).abs() < f32::EPSILON);
        assert_eq!(
            read(&toml::Value::Table(toml::Table::new())),
            TextStyle::default()
        );
    }
}
