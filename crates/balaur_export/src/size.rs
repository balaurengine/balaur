//! What an export weighs: the policy over `recode`'s pure functions.
//!
//! `[export]` says what may be dropped and what may be re-encoded; this
//! applies it to a built pack and answers what it cost, so the CLI and the
//! editor print the same summary.
//!
//! Nothing here changes a pack key. A texture named `art/hero.png` keeps that
//! name whatever bytes it ends up holding, because every reader identifies an
//! image by its content and no scene has to be rewritten.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use balaur::Pack;
use balaur::import::words;

use crate::config::ExportConfig;
use crate::recode::{self, AudioMode, FontMode, ImageMode, Saving};

/// Code points every subset face keeps, whatever the project shows: a game
/// that prints a number or a slash should not lose it to a scan.
const ALWAYS: std::ops::RangeInclusive<char> = ' '..='~';

/// What preparing a pack for shipping did to it.
#[derive(Debug, Default, Clone)]
pub struct Summary {
    /// Assets dropped because nothing named them.
    pub dropped: Vec<String>,
    /// Bytes those assets weighed.
    pub dropped_bytes: usize,
    /// Every entry a re-encode made smaller, heaviest saving first.
    pub savings: Vec<Saving>,
}

impl Summary {
    /// Bytes the re-encodes saved.
    #[must_use]
    pub fn recoded_bytes(&self) -> usize {
        self.savings.iter().map(Saving::saved).sum()
    }

    /// Bytes this pack no longer carries at all.
    #[must_use]
    pub fn total_saved(&self) -> usize {
        self.dropped_bytes + self.recoded_bytes()
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.dropped.is_empty() {
            writeln!(
                f,
                "dropped {} unreferenced {} ({})",
                self.dropped.len(),
                if self.dropped.len() == 1 {
                    "asset"
                } else {
                    "assets"
                },
                kb(self.dropped_bytes)
            )?;
        }
        for saving in &self.savings {
            writeln!(
                f,
                "  {} {} -> {}",
                saving.path,
                kb(saving.before),
                kb(saving.after)
            )?;
        }
        if self.total_saved() > 0 {
            write!(f, "saved {}", kb(self.total_saved()))?;
        }
        Ok(())
    }
}

fn kb(bytes: usize) -> String {
    #[allow(
        clippy::cast_precision_loss,
        reason = "a pack a float cannot count would not fit in memory"
    )]
    let kilobytes = bytes as f64 / 1024.0;
    format!("{kilobytes:.0} KB")
}

/// Drop what nothing names and re-encode what `[export]` allows.
///
/// Errors only when an entry the config asked to re-encode does not decode:
/// a broken asset is worth failing an export over, since the game would ship
/// with it.
pub fn prepare(pack: &mut Pack, config: &ExportConfig) -> Result<Summary> {
    let mut summary = Summary::default();
    if config.strip {
        for key in pack.unreferenced(&config.keep) {
            let bytes = pack.assets.get(&key).map_or(0, Vec::len);
            summary.dropped_bytes += bytes;
            summary.dropped.push(key);
        }
        pack.strip(&config.keep);
    }
    let keep = code_points(pack, config);
    // Disjoint fields: the settings beside a file are read while its bytes
    // are being replaced.
    let settings = &pack.scenes;
    for (path, bytes) in &mut pack.assets {
        let before = bytes.len();
        let Some(smaller) = smaller(path, bytes, config, &keep, settings)? else {
            continue;
        };
        let after = smaller.len();
        *bytes = smaller;
        summary.savings.push(Saving {
            path: path.clone(),
            before,
            after,
        });
    }
    summary
        .savings
        .sort_by_key(|saving| std::cmp::Reverse(saving.saved()));
    Ok(summary)
}

/// The smaller form of one asset, or `None` to keep the author's bytes.
fn smaller(
    path: &str,
    bytes: &[u8],
    config: &ExportConfig,
    keep: &BTreeSet<char>,
    settings: &BTreeMap<String, String>,
) -> Result<Option<Vec<u8>>> {
    use balaur::import::{kind_of, kinds};
    let own = recode_word(settings, path);
    let own = own.as_deref();
    let images = image_mode(own, config.images);
    let audio = audio_mode(own, config.audio);
    match kind_of(path) {
        // The mode is read before the bytes are: an export that asked for no
        // re-encoding must not fail over a file that does not decode.
        Some(kinds::TEXTURE) if images != ImageMode::Keep => {
            recode::image_at(bytes, images, config.images_quality)
        }
        Some(kinds::AUDIO) if audio != AudioMode::Keep => {
            recode::audio_at(bytes, audio, config.audio_quality)
        }
        // A `.fnt` is a text descriptor and a page image, neither of them a
        // face a subsetter can read.
        Some(kinds::FONT)
            if config.fonts == FontMode::Subset
                && own != Some(words::KEEP)
                && !path.to_ascii_lowercase().ends_with(".fnt")
                && !kept_whole(path, &config.font_keep) =>
        {
            recode::font(bytes, keep)
        }
        _ => Ok(None),
    }
}

/// What a file's own import sidecar says about re-encoding, if it says
/// anything: `recode` beside the file beats the `[export]` mode for it alone,
/// which is how one picture opts out of a pass the rest of them take.
fn recode_word(settings: &BTreeMap<String, String>, path: &str) -> Option<String> {
    let text = settings.get(&balaur::import::sidecar_of(path))?;
    let table: toml::Table = toml::from_str(text).ok()?;
    Some(
        table
            .get(balaur::import::keys::RECODE)?
            .as_str()?
            .to_string(),
    )
}

/// The image mode one file is re-encoded under: its own word, or the export's.
fn image_mode(own: Option<&str>, fallback: ImageMode) -> ImageMode {
    match own {
        None => fallback,
        Some(words::KEEP) => ImageMode::Keep,
        Some("webp") => ImageMode::Webp,
        Some(other) => {
            tracing::warn!("recode: '{other}' is not a way to re-encode a picture");
            fallback
        }
    }
}

/// The audio mode one file is re-encoded under: its own word, or the export's.
fn audio_mode(own: Option<&str>, fallback: AudioMode) -> AudioMode {
    match own {
        Some(words::KEEP) => AudioMode::Keep,
        Some("flac") => AudioMode::Flac,
        // A picture's word on a sound is not a mistake worth a warning: one
        // `[import.texture]` default reaches every file of its own kind only.
        _ => fallback,
    }
}

/// Whether a face is one `font_keep` protects from subsetting.
fn kept_whole(path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pattern| balaur::pack::glob_matches(pattern, path))
}

/// Every code point a subset face has to keep.
///
/// Read from the pack's own text rather than from the strings a game will
/// draw, which nothing can know before it runs: a scene, an asset document
/// and a script source are all scanned whole, so a label's characters are in
/// here along with a good deal that is not. `font_ranges` adds what a server
/// or a player will supply later.
fn code_points(pack: &Pack, config: &ExportConfig) -> BTreeSet<char> {
    let mut keep: BTreeSet<char> = ALWAYS.collect();
    for text in pack.scenes.values() {
        keep.extend(text.chars());
    }
    for bytes in pack.scripts.values() {
        if let Ok(text) = std::str::from_utf8(bytes) {
            keep.extend(text.chars());
        }
    }
    for range in &config.font_ranges {
        keep.extend(parse_range(range));
    }
    keep
}

/// A `first-last` pair of hex code points, or one code point on its own.
/// A range that does not parse is dropped with a warning: an export is not
/// worth failing over a font key.
fn parse_range(range: &str) -> Vec<char> {
    let point = |text: &str| {
        u32::from_str_radix(text.trim(), 16)
            .ok()
            .and_then(char::from_u32)
    };
    let bounds = match range.split_once('-') {
        Some((first, last)) => (point(first), point(last)),
        None => (point(range), point(range)),
    };
    match bounds {
        (Some(first), Some(last)) if first <= last => (first..=last).collect(),
        _ => {
            tracing::warn!("[export] font_ranges: '{range}' is not a code point range");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ExportConfig, Summary, code_points, parse_range, prepare};
    use crate::recode::Saving;
    use balaur::Pack;

    fn pack_with(key: &str, bytes: Vec<u8>) -> Pack {
        let mut pack = Pack {
            manifest: "[application]\nname = \"t\"\nmain_scene = \"s\"\n".into(),
            ..Pack::default()
        };
        pack.assets.insert(key.to_string(), bytes);
        pack
    }

    #[test]
    fn an_asset_nothing_names_is_dropped_only_when_strip_is_on() {
        let mut config = ExportConfig::default();
        let mut pack = pack_with("art/unused.png", vec![1, 2, 3]);

        let kept = prepare(&mut pack, &config).unwrap();
        assert!(kept.dropped.is_empty(), "off by default");
        assert!(pack.assets.contains_key("art/unused.png"));

        config.strip = true;
        let dropped = prepare(&mut pack, &config).unwrap();
        assert_eq!(dropped.dropped, vec!["art/unused.png".to_string()]);
        assert_eq!(dropped.dropped_bytes, 3);
        assert!(pack.assets.is_empty());
    }

    #[test]
    fn a_keep_glob_survives_a_strip() {
        let config = ExportConfig {
            strip: true,
            keep: vec!["art/**".to_string()],
            ..ExportConfig::default()
        };
        let mut pack = pack_with("art/unused.png", vec![1, 2, 3]);
        let summary = prepare(&mut pack, &config).unwrap();
        assert!(summary.dropped.is_empty());
        assert!(pack.assets.contains_key("art/unused.png"));
    }

    /// Every printable ASCII character, whatever the project's own text holds.
    /// One picture opts out of the pass the rest of them take.
    #[test]
    fn a_files_own_recode_setting_beats_the_export_mode() {
        use crate::recode::ImageMode;
        let config = ExportConfig {
            images: ImageMode::Webp,
            ..ExportConfig::default()
        };
        let source = sample_png();
        let mut pack = pack_with("art/kept.png", source.clone());
        pack.assets.insert("art/shrunk.png".into(), source.clone());
        pack.scenes
            .insert("art/kept.png.toml".into(), "recode = \"keep\"\n".into());
        let summary = prepare(&mut pack, &config).unwrap();
        assert_eq!(
            pack.assets["art/kept.png"], source,
            "the file that asked to be kept is the author's bytes"
        );
        assert_eq!(
            summary.savings.len(),
            1,
            "and the other one was still re-encoded"
        );
        assert_eq!(summary.savings[0].path, "art/shrunk.png");
    }

    /// A gradient with enough structure that a re-encode can win.
    fn sample_png() -> Vec<u8> {
        let pixels = image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 4 % 256) as u8, (y * 4 % 256) as u8, 0, 255])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn the_kept_code_points_always_cover_ascii() {
        let pack = pack_with("art/a.png", Vec::new());
        let keep = code_points(&pack, &ExportConfig::default());
        for wanted in [' ', '0', '9', 'A', 'z', '/', '~'] {
            assert!(keep.contains(&wanted), "{wanted} is missing");
        }
    }

    #[test]
    fn a_scenes_own_characters_are_kept() {
        let mut pack = pack_with("art/a.png", Vec::new());
        pack.scenes
            .insert("scenes/main.toml".into(), "text = \"Grüß dich\"".into());
        let keep = code_points(&pack, &ExportConfig::default());
        assert!(keep.contains(&'ü') && keep.contains(&'ß'));
    }

    #[test]
    fn a_font_range_is_read_as_hex_code_points() {
        assert_eq!(parse_range("0041-0043"), vec!['A', 'B', 'C']);
        assert_eq!(parse_range("0041"), vec!['A']);
        assert!(parse_range("nonsense").is_empty());
    }

    #[test]
    fn a_summary_adds_what_was_dropped_to_what_was_recoded() {
        let summary = Summary {
            dropped: vec!["a.png".into()],
            dropped_bytes: 100,
            savings: vec![Saving {
                path: "b.png".into(),
                before: 90,
                after: 40,
            }],
        };
        assert_eq!(summary.recoded_bytes(), 50);
        assert_eq!(summary.total_saved(), 150);
    }
}
