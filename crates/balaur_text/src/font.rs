//! The `font` asset: the face a text draws in and how its glyphs are picked
//! and drawn. A face file is a font of itself, so a text's `font` row takes
//! `fonts/Inter.ttf` as it is, or a definition that shapes it differently.

use std::rc::Rc;

use anyhow::Result;
use balaur_core::assets::FONT_ASSET_TYPE;
use balaur_core::components::{ComponentDef, Schema};

use crate::options::{Feature, Hinting, Shaping, Stretch};
use crate::vocabulary::{keys as k, words as w};

const DOC: &str = "The face a text draws in and how its glyphs are picked and drawn. A face file — `.ttf`, `.otf`, `.ttc`, or a `.fnt` bitmap face — is a font of itself, so a `font` row takes its path as it is; a definition shapes one differently. `font_size`, `font_weight` and `font_style` stay on the text: weight and style pick a face of the font's family among the project's `fonts/`.\n\n```toml\ntype = \"font\"\nsource = \"fonts/Inter-Regular.ttf\"\nfeatures = [\"smcp\", \"liga=0\"]\n```";

/// How a font's glyphs are picked and drawn, past its face: what a text
/// shaped in it takes, wherever it draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontSettings {
    pub stretch: Stretch,
    pub features: Vec<Feature>,
    pub shaping: Shaping,
    /// Round each advance to a whole pixel while laying out.
    pub snap_advances: bool,
    pub hinting: Hinting,
    /// Rasterise on the pixel grid with no subpixel offset, for a pixel face.
    pub pixel_snap: bool,
    /// Resize a monospace face's glyphs to this advance, in pixels.
    pub monospace_width: Option<f32>,
}

impl FontSettings {
    /// The features as cosmic-text takes them.
    pub(crate) fn font_features(&self) -> cosmic_text::FontFeatures {
        let mut features = cosmic_text::FontFeatures::new();
        for feature in &self.features {
            features.set(cosmic_text::FeatureTag::new(&feature.tag), feature.value);
        }
        features
    }
}

/// A `font` definition: its face, and how the face is shaped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Font {
    /// The face file; empty takes the default font's face with these
    /// settings.
    pub source: String,
    pub settings: FontSettings,
}

fn schema() -> Rc<Schema> {
    ComponentDef::parse_schema(
        FONT_ASSET_TYPE,
        &ComponentDef::schema(&[
            (
                k::SOURCE,
                r#"{ type = "string", default = "", description = "The face: a `.ttf`, `.otf` or `.ttc` file, or a `.fnt` bitmap face. Empty takes the default font's face" }"#,
            ),
            (
                k::FEATURES,
                r#"{ type = "list", of = { type = "string" }, default = [], description = "OpenType features: `smcp` turns one on, `liga=0` turns one off, `salt=2` picks an alternate" }"#,
            ),
            (
                k::STRETCH,
                &format!(
                    r#"{{ type = "enum", default = "{}", options = [{}], description = "How wide a face is picked among the faces its family ships; nothing is stretched that no face draws" }}"#,
                    w::NORMAL,
                    crate::vocabulary::options(w::FONT_STRETCHES)
                ),
            ),
            (
                k::SHAPING,
                &format!(
                    r#"{{ type = "enum", default = "{}", options = [{}], description = "Complex shaping joins the scripts that need it and falls back to another face; simple does neither" }}"#,
                    w::COMPLEX,
                    crate::vocabulary::options(w::SHAPINGS)
                ),
            ),
            (
                k::HINTING,
                &format!(
                    r#"{{ type = "enum", default = "{}", options = [{}], description = "Snap outlines to the pixel grid; auto takes the face's import setting" }}"#,
                    w::AUTO,
                    crate::vocabulary::options(w::HINTINGS)
                ),
            ),
            (
                k::PIXEL_SNAP,
                r#"{ type = "bool", default = false, description = "Rasterise on whole pixels, for a pixel face" }"#,
            ),
            (
                k::SNAP_ADVANCES,
                r#"{ type = "bool", default = false, description = "Round each glyph's advance to a whole pixel" }"#,
            ),
            (
                k::MONOSPACE_WIDTH,
                r#"{ type = "float", default = 0.0, min = 0.0, description = "Pixels a monospace face's advance is set to; zero keeps its own" }"#,
            ),
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

/// A definition's rows as the shaper takes them.
#[must_use]
pub fn read(table: &toml::Value) -> Font {
    let text = |key: &str| table.get(key).and_then(toml::Value::as_str).unwrap_or("");
    let flag = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
    };
    let monospace = table
        .get(k::MONOSPACE_WIDTH)
        .and_then(balaur_core::components::as_f64)
        .map_or(0.0, |width| width as f32);
    Font {
        source: text(k::SOURCE).to_string(),
        settings: FontSettings {
            stretch: Stretch::of(text(k::STRETCH)),
            features: table
                .get(k::FEATURES)
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .filter_map(Feature::parse)
                .collect(),
            shaping: Shaping::of(text(k::SHAPING)),
            snap_advances: flag(k::SNAP_ADVANCES),
            hinting: Hinting::of(text(k::HINTING)),
            pixel_snap: flag(k::PIXEL_SNAP),
            monospace_width: (monospace > 0.0).then_some(monospace),
        },
    }
}

/// Register the `font` asset, whose definitions sit in `fonts/` beside the
/// faces they name.
pub fn register_font_asset(reg: &mut balaur_plugin::Registry<'_>) {
    reg.register_record_asset(FONT_ASSET_TYPE, "fonts", DOC, schema(), parse);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_definition_reads_its_face_and_its_settings() {
        let table: toml::Value = toml::from_str(
            "source = \"fonts/Inter.ttf\"\nfeatures = [\"smcp\", \"liga=0\", \"toolong\"]\nstretch = \"condensed\"\nmonospace_width = 9.0",
        )
        .unwrap();
        let font = read(&table);
        assert_eq!(font.source, "fonts/Inter.ttf");
        assert_eq!(font.settings.features.len(), 2);
        assert_eq!(font.settings.stretch, Stretch::Condensed);
        assert_eq!(font.settings.monospace_width, Some(9.0));
        assert_eq!(
            read(&toml::Value::Table(toml::Table::new())),
            Font::default()
        );
    }
}
