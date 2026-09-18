//! `balaur shrink`: a smaller copy of every image in a project, written beside
//! the original as the variant one target answers to.
//!
//! `wall.png` gains `wall.web.png`, and `balaur export --target web` folds the
//! second onto the first: one asset, two bytes, and a phone or a browser never
//! carries the desktop's. Nothing else in the project changes, because a
//! variant keeps the canonical name, and the export records the size the
//! original was drawn at beside the copy, so a sprite, a sheet's frames and a
//! tile still measure in the pixels the artist counted.
//!
//! Pixel art is left alone. Sampled nearest, every texel is a deliberate
//! square, and a smaller copy has to drop some of them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Images this reads and writes. A format the engine samples but this cannot
/// re-encode would be a file it silently skipped.
const IMAGES: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// What one run did.
#[derive(Debug, Default)]
pub struct Shrunk {
    /// Project-relative paths written, in the order they were walked.
    pub written: Vec<String>,
    /// What was left alone, each with why.
    pub skipped: Vec<(String, String)>,
    pub before: u64,
    pub after: u64,
}

/// Write a `tag` variant of every image under `project`, scaled by `scale`.
///
/// # Errors
/// If the project cannot be read, or an image it names does not decode.
pub fn shrink(project: &Path, tag: &str, scale: f32) -> Result<Shrunk> {
    if !(0.0..1.0).contains(&scale) || scale <= 0.0 {
        bail!("scale is a fraction of the original between 0 and 1, not {scale}");
    }
    if !balaur_core::tags::ALL.contains(&tag) {
        bail!(
            "'{tag}' is not a tag an export answers to; one of: {}",
            balaur_core::tags::ALL.join(", ")
        );
    }
    // What the project says is not the game's is not the game's here either.
    let source = std::fs::read_to_string(project.join("project.toml")).unwrap_or_default();
    let ignored = balaur_core::ignore::from_manifest(&source);
    // `[import.texture]` as the tag's own build reads it, overrides and all.
    let tags = balaur_core::tags::Tags(vec![tag.to_string()]);
    let manifest = balaur_core::settings::resolve(&source, &tags).unwrap_or_default();
    let (images, pages) = images_under(project);
    let mut out = Shrunk::default();
    for path in images {
        let rel = relative(project, &path);
        if is_variant(&rel) || balaur_core::ignore::ignored(&ignored, &rel) {
            continue;
        }
        if pages.contains(&rel) {
            out.skipped.push((
                rel,
                "a bitmap font's page: its glyph boxes count its pixels".into(),
            ));
            continue;
        }
        if is_pixel_art(project, &manifest, &rel) {
            out.skipped.push((
                rel,
                "pixel art, sampled nearest: a smaller copy drops texels".into(),
            ));
            continue;
        }
        let Some(target) = variant_name(&rel, tag) else {
            continue;
        };
        let before = std::fs::metadata(&path).map_or(0, |m| m.len());
        let bytes = write_scaled(&path, &project.join(&target), scale)
            .with_context(|| format!("shrinking {rel}"))?;
        out.before += before;
        out.after += bytes;
        out.written.push(target);
    }
    Ok(out)
}

/// Decode, scale and re-encode one image in its own format, answering the
/// bytes written. The format is kept so the variant matches the name the
/// canonical file is folded onto.
fn write_scaled(from: &Path, to: &Path, scale: f32) -> Result<u64> {
    let image = image::open(from)?;
    let width = ((image.width() as f32 * scale).round() as u32).max(1);
    let height = ((image.height() as f32 * scale).round() as u32).max(1);
    let smaller = image.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    smaller.save(to)?;
    Ok(std::fs::metadata(to).map_or(0, |m| m.len()))
}

/// `art/hero.png` with `web` becomes `art/hero.web.png`.
fn variant_name(rel: &str, tag: &str) -> Option<String> {
    let (stem, extension) = rel.rsplit_once('.')?;
    Some(format!("{stem}.{tag}.{extension}"))
}

/// Whether this path is already some target's copy, so a run never shrinks
/// what a run before it wrote.
fn is_variant(rel: &str) -> bool {
    rel.rsplit_once('.')
        .and_then(|(stem, _)| stem.rsplit_once('.'))
        .is_some_and(|(_, tag)| balaur_core::tags::ALL.contains(&tag))
}

/// Every image in the project in a stable order, and the pages its bitmap
/// fonts draw from.
fn images_under(project: &Path) -> (Vec<PathBuf>, BTreeSet<String>) {
    let (mut found, mut fonts) = (Vec::new(), Vec::new());
    walk(project, &mut found, &mut fonts);
    found.sort();
    let pages = fonts
        .iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            balaur_text::bitmap::page_of(&relative(project, path), &text)
        })
        .collect();
    (found, pages)
}

fn relative(project: &Path, path: &Path) -> String {
    path.strip_prefix(project)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, fonts: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if path.is_dir() {
            walk(&path, out, fonts);
        } else if IMAGES.contains(&extension.as_str()) {
            out.push(path);
        } else if extension == "fnt" {
            fonts.push(path);
        }
    }
}

/// Whether the image is sampled nearest, by its sidecar or the project's
/// `[import.texture]`, which is how a project marks pixel art.
fn is_pixel_art(project: &Path, manifest: &toml::Table, rel: &str) -> bool {
    let sidecar = std::fs::read_to_string(project.join(balaur_core::import::sidecar_of(rel))).ok();
    let settings = balaur_core::import::merged(manifest, rel, sidecar.as_deref());
    balaur_core::import::texture::is_pixel_art(&settings)
}

#[cfg(test)]
mod tests {
    use super::shrink;

    /// Two images, one of them marked as pixel art by its sidecar.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let art = dir.path().join("art");
        std::fs::create_dir_all(&art).unwrap();
        for name in ["hero.png", "rock.png"] {
            image::RgbaImage::from_pixel(8, 4, image::Rgba([1, 2, 3, 255]))
                .save(art.join(name))
                .unwrap();
        }
        std::fs::write(
            dir.path().join("project.toml"),
            "[application]\nname = \"t\"\n",
        )
        .unwrap();
        std::fs::write(art.join("hero.png.toml"), "filter = \"nearest\"\n").unwrap();
        dir
    }

    /// The copy is the variant name the exporter folds, at the scale asked for.
    #[test]
    fn an_image_gains_the_variant_its_target_answers_to() {
        let dir = project();
        let done = shrink(dir.path(), "web", 0.5).unwrap();
        assert_eq!(done.written, vec!["art/rock.web.png".to_string()]);
        let made = image::open(dir.path().join("art/rock.web.png")).unwrap();
        assert_eq!((made.width(), made.height()), (4, 2));
    }

    /// Every texel of pixel art is a deliberate square; a smaller copy cannot
    /// keep them all.
    #[test]
    fn pixel_art_is_left_alone() {
        let dir = project();
        let done = shrink(dir.path(), "web", 0.5).unwrap();
        assert_eq!(done.skipped.len(), 1, "{:?}", done.skipped);
        assert_eq!(done.skipped[0].0, "art/hero.png");
        assert!(!dir.path().join("art/hero.web.png").exists());
    }

    /// Running twice writes the same one file: a variant is not itself shrunk.
    #[test]
    fn a_second_run_does_not_shrink_what_the_first_one_wrote() {
        let dir = project();
        shrink(dir.path(), "web", 0.5).unwrap();
        let again = shrink(dir.path(), "web", 0.5).unwrap();
        assert_eq!(again.written, vec!["art/rock.web.png".to_string()]);
        assert!(!dir.path().join("art/rock.web.web.png").exists());
    }

    /// A project that says `nearest` once has marked every image as pixel art.
    #[test]
    fn a_project_wide_nearest_filter_marks_every_image() {
        let dir = project();
        std::fs::write(
            dir.path().join("project.toml"),
            "[application]\nname = \"t\"\n\n[import.texture]\nfilter = \"nearest\"\n",
        )
        .unwrap();
        let done = shrink(dir.path(), "web", 0.5).unwrap();
        assert!(done.written.is_empty(), "{:?}", done.written);
        assert_eq!(done.skipped.len(), 2);
    }

    /// A bitmap font's glyph boxes are in its page's pixels.
    #[test]
    fn a_bitmap_font_page_is_left_alone() {
        let dir = project();
        std::fs::write(
            dir.path().join("art/pixel.fnt"),
            "info size=8\npage id=0 file=\"rock.png\"\nchar id=65 x=0 y=0 width=4 height=4 xadvance=4\n",
        )
        .unwrap();
        let done = shrink(dir.path(), "web", 0.5).unwrap();
        assert!(done.written.is_empty(), "{:?}", done.written);
        assert!(
            done.skipped
                .iter()
                .any(|(rel, why)| rel == "art/rock.png" && why.contains("font"))
        );
    }

    /// A tag no export answers to would write a file nothing ever picks.
    #[test]
    fn a_tag_no_target_answers_to_is_refused() {
        let dir = project();
        let error = shrink(dir.path(), "titchy", 0.5).unwrap_err().to_string();
        assert!(error.contains("titchy") && error.contains("web"), "{error}");
    }
}
