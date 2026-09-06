//! Import settings: how a file is read, stated beside the file.
//!
//! Godot writes a `.import` next to every asset; here it is a TOML sidecar
//! named after the whole file, so `art/hero.png.toml` sorts beside its image
//! and cannot collide with a scene or a clip of the same stem.
//! `[import.<kind>]` in `project.toml` sets the default for every file of a
//! kind, and the sidecar overrides it key by key.
//!
//! Settings change pixels and samples, never sizes. A headless run resolves
//! them for nothing and computes the same world, which is why nothing here
//! is allowed to move an extent.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::engine::Engine;
use crate::project::{ProjectFiles, ProjectManifest};

/// The kinds a file sorts into. A kind names its defaults table in
/// `project.toml` and the settings its reader looks for.
pub mod kinds {
    pub const TEXTURE: &str = "texture";
    pub const AUDIO: &str = "audio";
    pub const FONT: &str = "font";
    pub const MODEL: &str = "model";
}

/// Setting keys, so a reader, the editor and the exporter spell them alike.
pub mod keys {
    pub const FILTER: &str = "filter";
    pub const SRGB: &str = "srgb";
    pub const REPEAT: &str = "repeat";
    pub const MIPMAPS: &str = "mipmaps";
    pub const PREMULTIPLY: &str = "premultiply";
    pub const RECODE: &str = "recode";
}

/// The values a key takes.
pub mod words {
    pub const NEAREST: &str = "nearest";
    pub const LINEAR: &str = "linear";
    pub const REPEAT: &str = "repeat";
    pub const MIRROR: &str = "mirror";
    pub const CLAMP: &str = "clamp";
}

/// Which kind a file belongs to, by extension, or `None` for a file no
/// importer claims.
#[must_use]
pub fn kind_of(path: &str) -> Option<&'static str> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tga" => Some(kinds::TEXTURE),
        "ogg" | "wav" | "mp3" | "flac" => Some(kinds::AUDIO),
        "ttf" | "otf" | "ttc" | "fnt" => Some(kinds::FONT),
        "glb" | "gltf" | "obj" => Some(kinds::MODEL),
        _ => None,
    }
}

/// The sidecar that states one file's settings.
#[must_use]
pub fn sidecar_of(path: &str) -> String {
    format!("{path}.toml")
}

/// Whether a project file is a sidecar rather than content of its own.
///
/// The exporter and the asset walk both need this: a sidecar is settings,
/// not an asset a scene may name.
#[must_use]
pub fn is_sidecar(path: &str) -> bool {
    let Some(rest) = path.strip_suffix(".toml") else {
        return false;
    };
    kind_of(rest).is_some()
}

/// One file's settings: the project's defaults for its kind, with the
/// sidecar's keys written over them.
///
/// Answers an empty table for a file of no kind, and never fails: a sidecar
/// that does not parse is reported and ignored, because a game that refused
/// to start over one is worse than a game that starts with plain defaults.
#[must_use]
pub fn settings(eng: &Engine, path: &str) -> toml::Table {
    let mut table = defaults(eng, path);
    let Some(files) = eng.try_resource::<ProjectFiles>() else {
        return table;
    };
    let sidecar = sidecar_of(path);
    let Ok(bytes) = files.borrow().read(&sidecar) else {
        return table;
    };
    if let Ok(Ok(overrides)) = std::str::from_utf8(&bytes).map(toml::from_str::<toml::Table>) {
        for (key, value) in overrides {
            table.insert(key, value);
        }
    } else {
        tracing::warn!("{sidecar} is not a settings file; using the defaults");
    }
    table
}

/// The project's defaults for whatever kind `path` is.
fn defaults(eng: &Engine, path: &str) -> toml::Table {
    let Some(kind) = kind_of(path) else {
        return toml::Table::new();
    };
    eng.try_resource::<ProjectManifest>()
        .and_then(|manifest| manifest.borrow().import.get(kind).cloned())
        .unwrap_or_default()
}

/// A short stable name for a resolved set of settings.
///
/// Folded into the name a texture is uploaded under, so changing one
/// image's filter re-uploads that image and leaves the rest alone.
#[must_use]
pub fn stamp(settings: &toml::Table) -> String {
    if settings.is_empty() {
        return String::new();
    }
    let mut hasher = DefaultHasher::new();
    settings.to_string().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// A string setting, or `fallback` when it is missing or of another type.
#[must_use]
pub fn word<'a>(settings: &'a toml::Table, key: &str, fallback: &'a str) -> &'a str {
    settings
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or(fallback)
}

/// A boolean setting, or `fallback` when it is missing or of another type.
#[must_use]
pub fn flag(settings: &toml::Table, key: &str, fallback: bool) -> bool {
    settings
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::{flag, is_sidecar, kind_of, kinds, sidecar_of, stamp, word};

    #[test]
    fn a_file_sorts_into_the_kind_its_extension_names() {
        assert_eq!(kind_of("art/hero.PNG"), Some(kinds::TEXTURE));
        assert_eq!(kind_of("sfx/hit.wav"), Some(kinds::AUDIO));
        assert_eq!(kind_of("fonts/pixel.fnt"), Some(kinds::FONT));
        assert_eq!(kind_of("scenes/main.toml"), None);
    }

    #[test]
    fn a_sidecar_sorts_beside_the_file_it_settles() {
        assert_eq!(sidecar_of("art/hero.png"), "art/hero.png.toml");
        assert!(is_sidecar("art/hero.png.toml"));
    }

    /// A scene and a clip are content, however they are named.
    #[test]
    fn a_scene_is_not_mistaken_for_a_sidecar() {
        assert!(!is_sidecar("scenes/main.toml"));
        assert!(!is_sidecar("animations/walk.toml"));
    }

    #[test]
    fn a_changed_setting_changes_the_stamp() {
        let nearest: toml::Table = toml::from_str(r#"filter = "nearest""#).unwrap();
        let linear: toml::Table = toml::from_str(r#"filter = "linear""#).unwrap();
        assert_ne!(stamp(&nearest), stamp(&linear));
        assert_eq!(stamp(&toml::Table::new()), "");
    }

    #[test]
    fn a_missing_setting_reads_as_its_fallback() {
        let table: toml::Table = toml::from_str(r#"filter = "nearest""#).unwrap();
        assert_eq!(word(&table, "filter", "linear"), "nearest");
        assert_eq!(word(&table, "repeat", "clamp"), "clamp");
        assert!(flag(&table, "srgb", true));
    }
}
