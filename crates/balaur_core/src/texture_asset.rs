//! The `texture` asset: an image and the settings it is read with, so one
//! scene can read a picture differently from the sidecar beside it.
//!
//! Every texture property takes the four forms any asset does. A plain image
//! path is the common one and resolves as `{ source = <path> }`, so nothing a
//! scene already writes changes. A `textures/*.toml` file, an `[[assets]]`
//! block and an inline table each carry a `source` and any import setting,
//! which wins over the image's sidecar and the project's `[import.texture]`.

use std::collections::HashMap;
use std::rc::Rc;

use anyhow::{Result, anyhow, bail};

use crate::app::App;
use crate::engine::Engine;
use crate::import::{self, Resolved, kinds};

pub const TEXTURE_ASSET_TYPE: &str = "texture";

/// The key naming the image a texture reads.
pub const SOURCE: &str = "source";

const TEXTURE_ASSET_DOC: &str = "An image and the import settings it is read with. A texture property takes a plain image path, which reads the image with its sidecar; this is for one use of a picture that reads it differently. Any key the image's sidecar takes may be written here, and wins over it.\n\n```toml\ntype = \"texture\"\nsource = \"art/hero.png\"\nfilter = \"nearest\"                # this use crisp, the sidecar's smooth\npixels_per_unit = 32\n```";

/// A `texture` definition: the image, and the settings written beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureAsset {
    pub source: String,
    pub settings: toml::Table,
}

pub(crate) fn register_texture_asset(app: &mut App) {
    app.register_asset_type(TEXTURE_ASSET_TYPE, "textures", TEXTURE_ASSET_DOC, |value| {
        Ok(Rc::new(parse(value)?) as Rc<dyn std::any::Any>)
    });
}

/// A definition table as a texture. The keys that name the asset rather
/// than set how the image is read are dropped.
///
/// # Errors
/// If `source` is missing or names no image a texture reads.
pub fn parse(value: &toml::Value) -> Result<TextureAsset> {
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("a texture is a table with a `source`"))?;
    let source = table
        .get(SOURCE)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a texture names its image in `source`"))?;
    if import::kind_of(source) != Some(kinds::TEXTURE) {
        bail!("`{source}` is not an image a texture reads");
    }
    let mut settings = table.clone();
    for key in ["type", "id", SOURCE] {
        settings.remove(key);
    }
    Ok(TextureAsset {
        source: source.to_string(),
        settings,
    })
}

/// What a reference to an image file is, when the asset layer is asked for
/// its definition: `{ type = "texture", source = <path> }`. Answers `None`
/// for a path that is not an image.
#[must_use]
pub fn image_definition(path: &str) -> Option<toml::Value> {
    if import::kind_of(path) != Some(kinds::TEXTURE) {
        return None;
    }
    let mut table = toml::Table::new();
    table.insert(
        "type".into(),
        toml::Value::String(TEXTURE_ASSET_TYPE.into()),
    );
    table.insert(SOURCE.into(), toml::Value::String(path.to_string()));
    Some(toml::Value::Table(table))
}

/// A texture property resolved: the image to read, and the settings it is
/// read with.
#[derive(Clone)]
pub struct TextureSource {
    pub path: String,
    pub settings: Rc<Resolved>,
}

/// Resolutions already made, dropped when an asset is saved.
#[derive(Default)]
struct Sources {
    generation: u64,
    by_reference: HashMap<String, TextureSource>,
}

/// Whether a reference is an image path itself, which needs no asset load.
fn is_plain_image(reference: &str) -> bool {
    !reference.contains('#')
        && !reference.starts_with(crate::assets::ID_PREFIX)
        && import::kind_of(reference) == Some(kinds::TEXTURE)
}

/// The image a texture property names and the settings it is read with: the
/// project's `[import.texture]`, then the image's sidecar, then whatever the
/// `texture` asset itself says. Resolved once per asset generation.
///
/// # Errors
/// If the reference names no texture.
pub fn source(eng: &Engine, reference: &str) -> Result<TextureSource> {
    if is_plain_image(reference) {
        return Ok(TextureSource {
            path: reference.to_string(),
            settings: import::resolved(eng, reference),
        });
    }
    let generation = crate::assets::generation(eng);
    if eng.try_resource::<Sources>().is_none() {
        eng.insert_resource(Sources::default());
    }
    let cache = eng.resource::<Sources>();
    {
        let held = cache.borrow();
        if held.generation == generation
            && let Some(found) = held.by_reference.get(reference)
        {
            return Ok(found.clone());
        }
    }
    let asset = crate::assets::load_typed::<TextureAsset>(eng, reference)?;
    let mut settings = import::resolved(eng, &asset.source).settings.clone();
    settings.extend(asset.settings.clone());
    let found = TextureSource {
        path: asset.source.clone(),
        settings: Rc::new(Resolved {
            stamp: import::stamp(&settings),
            settings,
        }),
    };
    let mut held = cache.borrow_mut();
    if held.generation != generation {
        held.by_reference.clear();
        held.generation = generation;
    }
    held.by_reference
        .insert(reference.to_string(), found.clone());
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::{image_definition, is_plain_image, parse};

    #[test]
    fn a_texture_keeps_its_settings_and_drops_its_names() {
        let value: toml::Value = toml::from_str(
            "type = \"texture\"\nid = \"crisp\"\nsource = \"art/hero.png\"\nfilter = \"nearest\"",
        )
        .unwrap();
        let texture = parse(&value).unwrap();
        assert_eq!(texture.source, "art/hero.png");
        assert_eq!(texture.settings.len(), 1);
        assert_eq!(texture.settings["filter"].as_str(), Some("nearest"));
    }

    #[test]
    fn a_texture_needs_an_image_to_read() {
        let none: toml::Value = toml::from_str("filter = \"nearest\"").unwrap();
        assert!(parse(&none).is_err());
        let scene: toml::Value = toml::from_str("source = \"scenes/main.toml\"").unwrap();
        assert!(parse(&scene).is_err());
    }

    #[test]
    fn an_image_path_is_a_texture_of_itself() {
        let value = image_definition("art/hero.png").unwrap();
        assert_eq!(parse(&value).unwrap().source, "art/hero.png");
        assert!(image_definition("scenes/main.toml").is_none());
        assert!(is_plain_image("art/hero.png"));
        assert!(!is_plain_image("#crisp") && !is_plain_image("textures/hero.toml"));
    }
}
