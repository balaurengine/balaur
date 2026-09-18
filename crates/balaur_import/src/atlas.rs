//! `balaur atlas` and `balaur import file.gif`: frames packed onto one page,
//! with a `sprite_sheet` listing each and one clip per run of frames.
//!
//! Loose frames are grouped into runs by name: `walk_01.png`, `walk_02.png`
//! are the run `walk`, and `coin.png` alone is the run `coin`. Every run is a
//! tag on the sheet, so a single picture can be found by its name too, and a
//! run of more than one frame is also a clip keying `sprite/frame`.

use std::fmt::Write as _;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use image::RgbaImage;
use texture_packer::{TexturePacker, TexturePackerConfig, exporter::ImageExporter};

/// The largest page the packer is allowed, which every GPU samples.
pub const MAX_PAGE: u32 = 4096;
/// Transparent pixels between two frames, and the edge pixels repeated into
/// them, so a linear filter at a frame's edge never reads its neighbour.
const PADDING: u32 = 2;
const EXTRUSION: u32 = 1;

/// Images `balaur atlas` reads from a folder.
const IMAGES: [&str; 4] = ["png", "webp", "jpg", "jpeg"];

/// One frame on its way onto a page.
pub struct Frame {
    pub name: String,
    pub image: RgbaImage,
    pub milliseconds: u32,
}

/// What a set of frames becomes.
#[derive(Debug)]
pub struct Packed {
    /// The page, lossless WebP.
    pub page: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub frames: usize,
    /// The `sprite_sheet` document.
    pub sheet: String,
    /// The clip library: one clip per run of more than one frame.
    pub clips: Option<String>,
}

/// Pack `frames`, in their order, as the sheet `stem` drawing from `texture`.
pub fn pack(frames: &[Frame], stem: &str, texture: &str) -> Result<Packed> {
    if frames.is_empty() {
        bail!("an atlas needs at least one frame");
    }
    let config = TexturePackerConfig {
        max_width: MAX_PAGE,
        max_height: MAX_PAGE,
        allow_rotation: false,
        texture_padding: PADDING,
        texture_extrusion: EXTRUSION,
        trim: false,
        ..TexturePackerConfig::default()
    };
    let mut packer = TexturePacker::new_skyline(config);
    // Tallest first packs a skyline tighter; the sheet keeps the given order.
    let mut order: Vec<usize> = (0..frames.len()).collect();
    order.sort_by_key(|&at| {
        std::cmp::Reverse((frames[at].image.height(), frames[at].image.width()))
    });
    for at in order {
        packer.pack_ref(at, &frames[at].image).map_err(|_| {
            anyhow!(
                "the frames do not fit on a {MAX_PAGE}x{MAX_PAGE} page; `{}` did not",
                frames[at].name
            )
        })?;
    }
    let page = ImageExporter::export(&packer, None).map_err(|why| anyhow!("{why}"))?;
    let (width, height) = (page.width(), page.height());
    let mut rects = Vec::with_capacity(frames.len());
    for (at, frame) in frames.iter().enumerate() {
        let placed = packer
            .get_frame(&at)
            .with_context(|| format!("`{}` was packed nowhere", frame.name))?;
        let r = placed.frame;
        rects.push([r.x, r.y, r.w, r.h]);
    }
    let mut bytes = Vec::new();
    page.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::WebP)
        .context("encoding the page")?;
    let runs = runs(frames);
    Ok(Packed {
        page: bytes,
        width,
        height,
        frames: frames.len(),
        sheet: sheet_toml(stem, texture, frames, &rects, &runs),
        clips: clips_toml(stem, frames, &runs),
    })
}

/// The run a frame belongs to: its name without a trailing number and the
/// separator before it. `walk_01` is `walk`; `coin` is `coin`.
#[must_use]
pub fn run_name(name: &str) -> String {
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let stem = stem.trim_end_matches(['_', '-', ' ', '.']);
    if stem.is_empty() { name } else { stem }.to_string()
}

/// Consecutive frames sharing a run name, as `(name, first, last)`. A name
/// that comes back after another run is numbered, so every tag is distinct.
fn runs(frames: &[Frame]) -> Vec<(String, usize, usize)> {
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    for (at, frame) in frames.iter().enumerate() {
        let name = run_name(&frame.name);
        match out.last_mut() {
            Some((last, _, end)) if *last == name && *end + 1 == at => *end = at,
            _ => {
                let again = format!("{name}_");
                let taken = out
                    .iter()
                    .filter(|(held, ..)| *held == name || held.starts_with(&again))
                    .count();
                let name = if taken == 0 {
                    name
                } else {
                    format!("{name}_{taken}")
                };
                out.push((name, at, at));
            }
        }
    }
    out
}

/// Every image in `inputs`, a folder walked or a file taken as is, in the
/// order a person numbering frames meant: `walk_2` before `walk_10`.
pub fn files_from(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(input)
                .with_context(|| format!("reading {}", input.display()))?
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| is_image(path) && !is_variant_or_hidden(path))
                .collect();
            found.sort_by(|a, b| natural(&stem_of(a), &stem_of(b)));
            files.extend(found);
        } else {
            files.push(input.clone());
        }
    }
    Ok(files)
}

/// Each file read as one frame named by its stem, shown for `milliseconds`.
pub fn frames_from(files: &[PathBuf], milliseconds: u32) -> Result<Vec<Frame>> {
    files
        .iter()
        .map(|path| {
            let image = image::open(path)
                .with_context(|| format!("reading {}", path.display()))?
                .to_rgba8();
            Ok(Frame {
                name: stem_of(path),
                image,
                milliseconds,
            })
        })
        .collect()
}

/// Every frame of a GIF, composited to its canvas, with its own delay.
pub fn frames_of_gif(bytes: &[u8], stem: &str) -> Result<Vec<Frame>> {
    use image::AnimationDecoder;
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|why| anyhow!("reading the GIF: {why}"))?;
    let mut frames = Vec::new();
    for (at, frame) in decoder.into_frames().enumerate() {
        let frame = frame.map_err(|why| anyhow!("frame {at}: {why}"))?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        // A browser plays a zero delay at a tenth of a second, and so does this.
        let milliseconds = match numerator / denominator.max(1) {
            0 => 100,
            ms => ms,
        };
        frames.push(Frame {
            name: format!("{stem}_{at}"),
            image: frame.into_buffer(),
            milliseconds,
        });
    }
    if frames.is_empty() {
        bail!("the GIF has no frames");
    }
    Ok(frames)
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGES.contains(&e.to_ascii_lowercase().as_str()))
}

/// A target's copy (`hero.web.png`) or a dotfile, neither of them a frame.
fn is_variant_or_hidden(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.starts_with('.') {
        return true;
    }
    let stem = stem_of(path);
    stem.rsplit_once('.')
        .is_some_and(|(_, tag)| balaur_core::tags::ALL.contains(&tag))
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string()
}

/// Names compared with their digit runs as numbers.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn chunks(text: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in text.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((was, chunk)) if *was == digit => chunk.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let key = |text: &str| {
        chunks(text)
            .into_iter()
            .map(|(digit, chunk)| {
                if digit {
                    (chunk.trim_start_matches('0').len(), chunk)
                } else {
                    (0, chunk.to_lowercase())
                }
            })
            .collect::<Vec<_>>()
    };
    key(a).cmp(&key(b))
}

/// A table key as TOML spells it: bare when it can be, quoted otherwise.
fn key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        name.to_string()
    } else {
        toml::Value::String(name.to_string()).to_string()
    }
}

/// Milliseconds as a TOML float: `0.1`, never the integer `0`.
fn seconds(milliseconds: u32) -> String {
    let text = format!("{}", f64::from(milliseconds) / 1000.0);
    if text.contains('.') {
        text
    } else {
        format!("{text}.0")
    }
}

fn sheet_toml(
    stem: &str,
    texture: &str,
    frames: &[Frame],
    rects: &[[u32; 4]],
    runs: &[(String, usize, usize)],
) -> String {
    let mut out = format!(
        "# Packed by `balaur atlas` as {stem}; pack the frames again rather than editing.\n\
         type = \"sprite_sheet\"\n\
         texture = {}\nframes = [\n",
        toml::Value::String(texture.to_string())
    );
    for (frame, r) in frames.iter().zip(rects) {
        let _ = writeln!(
            out,
            "  {{ rect = [{}, {}, {}, {}], duration = {} }},",
            r[0],
            r[1],
            r[2],
            r[3],
            seconds(frame.milliseconds)
        );
    }
    out.push_str("]\n");
    for (name, first, last) in runs {
        let _ = write!(
            out,
            "\n[tags.{}]\nfrom = {first}\nto = {last}\ndirection = \"forward\"\nrepeat = 0\n",
            key(name)
        );
    }
    out
}

fn clips_toml(stem: &str, frames: &[Frame], runs: &[(String, usize, usize)]) -> Option<String> {
    let moving: Vec<&(String, usize, usize)> = runs.iter().filter(|(_, a, b)| b > a).collect();
    if moving.is_empty() {
        return None;
    }
    let mut out = format!(
        "# Packed by `balaur atlas` as {stem}: one clip per run of frames, keying `sprite/frame`.\n\
         type = \"animation_clip\"\n"
    );
    for (name, first, last) in moving {
        let (mut at, mut keys) = (0, String::new());
        for (frame, shown) in frames.iter().enumerate().take(*last + 1).skip(*first) {
            let _ = writeln!(keys, "  {{ t = {}, value = {frame}.0 }},", seconds(at));
            at += shown.milliseconds;
        }
        let _ = write!(
            out,
            "\n[clips.{name}]\nlength = {}\nloop = \"loop\"\n\n[[clips.{name}.tracks]]\nproperty = \"sprite/frame\"\ninterp = \"step\"\nkeys = [\n{keys}]\n",
            seconds(at.max(1)),
            name = key(name)
        );
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{Frame, natural, pack, run_name};
    use image::{Rgba, RgbaImage};

    fn frame(name: &str, width: u32, height: u32) -> Frame {
        Frame {
            name: name.to_string(),
            image: RgbaImage::from_pixel(width, height, Rgba([200, 10, 10, 255])),
            milliseconds: 100,
        }
    }

    #[test]
    fn a_trailing_number_names_a_run() {
        assert_eq!(run_name("walk_01"), "walk");
        assert_eq!(run_name("walk-2"), "walk");
        assert_eq!(run_name("coin"), "coin");
        assert_eq!(run_name("007"), "007");
    }

    #[test]
    fn numbers_sort_as_numbers() {
        let mut names = vec!["walk_10", "walk_2", "walk_1"];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(names, ["walk_1", "walk_2", "walk_10"]);
    }

    /// Frames of several sizes land on one page apart, each a tag, and a run
    /// of more than one is a clip.
    #[test]
    fn frames_of_any_size_pack_into_a_sheet_and_a_clip() {
        let frames = [
            frame("walk_1", 16, 24),
            frame("walk_2", 16, 24),
            frame("coin", 8, 8),
        ];
        let packed = pack(&frames, "hero", "art/hero.webp").unwrap();
        let sheet: toml::Table = toml::from_str(&packed.sheet).unwrap();
        let rects: Vec<Vec<i64>> = sheet["frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                f["rect"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_integer().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(rects.len(), 3);
        assert_eq!((rects[0][2], rects[0][3]), (16, 24));
        assert_eq!((rects[2][2], rects[2][3]), (8, 8));
        let overlaps = |a: &[i64], b: &[i64]| {
            a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
        };
        assert!(!overlaps(&rects[0], &rects[1]) && !overlaps(&rects[1], &rects[2]));
        assert_eq!(sheet["tags"]["walk"]["to"].as_integer(), Some(1));
        assert_eq!(sheet["tags"]["coin"]["from"].as_integer(), Some(2));
        let clips: toml::Table = toml::from_str(packed.clips.as_deref().unwrap()).unwrap();
        assert!(clips["clips"].get("walk").is_some());
        assert!(clips["clips"].get("coin").is_none(), "one frame is no clip");
    }

    #[test]
    fn a_run_that_comes_back_is_a_second_tag() {
        let frames = [frame("a_1", 4, 4), frame("b", 4, 4), frame("a_2", 4, 4)];
        let packed = pack(&frames, "x", "art/x.webp").unwrap();
        let sheet: toml::Table = toml::from_str(&packed.sheet).unwrap();
        assert!(sheet["tags"].get("a").is_some() && sheet["tags"].get("a_1").is_some());
    }
}
