//! `balaur import file.aseprite`: the sprite editor's own file as an atlas
//! page, a `sprite_sheet` naming every frame, tag and slice on it, and one
//! `animation_clip` per tag keying `sprite/frame`.
//!
//! Frames are packed at the canvas size, row by row, so a rectangle never
//! crosses a page and a slice drawn on the canvas is a slice on the frame.
//! The three files are plain TOML and PNG the editor edits like any other.

use std::fmt::Write as _;
use std::io::Cursor;

use anyhow::{Context as _, Result, anyhow, bail};
use aseprite_loader::binary::chunks::slice::SliceChunk;
use aseprite_loader::binary::chunks::tags::AnimationDirection;
use aseprite_loader::loader::{AsepriteFile, LayerSelection};

/// What one `.aseprite` becomes.
#[derive(Debug)]
pub struct AsepriteImport {
    /// The atlas page, PNG-encoded.
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub frames: usize,
    /// The `sprite_sheet` document, `sheets/<stem>.toml`.
    pub sheet: String,
    /// The clip library, `animations/<stem>.toml`: one clip per tag, or one
    /// over every frame when the file has no tags and more than one frame.
    pub clips: Option<String>,
}

/// One frame's place on the page and how long a tag shows it.
struct Packed {
    rect: [u32; 4],
    milliseconds: u32,
}

/// Import `bytes` as the sheet `stem`, drawing from `texture` — the
/// project-relative path the PNG will be written to. `layers` names the
/// layers to composite; empty means the ones visible in the editor.
pub fn import(
    bytes: &[u8],
    stem: &str,
    texture: &str,
    layers: &[String],
) -> Result<AsepriteImport> {
    let file = AsepriteFile::load(bytes).map_err(|e| anyhow!("reading the sprite: {e}"))?;
    let (width, height) = file.size();
    let (width, height) = (u32::from(width), u32::from(height));
    let count = file.frames().len();
    if width == 0 || height == 0 || count == 0 {
        bail!("the sprite has nothing to draw: {width}x{height} pixels, {count} frames");
    }
    let selection = select_layers(&file, layers)?;
    let columns = columns_for(count);
    let rows = count.div_ceil(columns);
    let (page_w, page_h) = (width * columns as u32, height * rows as u32);
    let mut page = vec![0u8; (page_w * page_h * 4) as usize];
    let mut frame_pixels = vec![0u8; (width * height * 4) as usize];
    let mut packed = Vec::with_capacity(count);
    for (index, frame) in file.frames().iter().enumerate() {
        frame_pixels.fill(0);
        file.render_frame(index, &mut frame_pixels, &selection)
            .map_err(|e| anyhow!("frame {index}: {e}"))?;
        let cell_x = (index % columns) as u32 * width;
        let cell_y = (index / columns) as u32 * height;
        for row in 0..height {
            let from = (row * width * 4) as usize;
            let to = (((cell_y + row) * page_w + cell_x) * 4) as usize;
            page[to..to + (width * 4) as usize]
                .copy_from_slice(&frame_pixels[from..from + (width * 4) as usize]);
        }
        packed.push(Packed {
            rect: [cell_x, cell_y, width, height],
            milliseconds: u32::from(frame.duration),
        });
    }
    let image = image::RgbaImage::from_raw(page_w, page_h, page)
        .ok_or_else(|| anyhow!("the atlas page does not fit its pixels"))?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .context("encoding the atlas")?;
    let sheet = sheet_toml(&file, stem, texture, &packed);
    let clips = clips_toml(&file, stem, &packed);
    Ok(AsepriteImport {
        png,
        width: page_w,
        height: page_h,
        frames: count,
        sheet,
        clips,
    })
}

fn select_layers(file: &AsepriteFile<'_>, layers: &[String]) -> Result<LayerSelection> {
    if layers.is_empty() {
        return Ok(LayerSelection::Visible);
    }
    let known: Vec<&str> = file.layers().iter().map(|l| l.name.as_str()).collect();
    for name in layers {
        if !known.contains(&name.as_str()) {
            bail!("no layer named '{name}'; the file has {}", known.join(", "));
        }
    }
    let names: Vec<&str> = layers.iter().map(String::as_str).collect();
    Ok(file.select_layers_by_name(&names))
}

/// The squarest page: as many columns as the square root of the count,
/// rounded up.
fn columns_for(count: usize) -> usize {
    let mut columns = 1;
    while columns * columns < count {
        columns += 1;
    }
    columns
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

/// Milliseconds as a TOML float: `0.0`, never the integer `0`.
fn seconds(milliseconds: u32) -> String {
    let text = format!("{}", f64::from(milliseconds) / 1000.0);
    if text.contains('.') {
        text
    } else {
        format!("{text}.0")
    }
}

fn rect(r: [u32; 4]) -> String {
    format!("[{}, {}, {}, {}]", r[0], r[1], r[2], r[3])
}

const fn direction_name(direction: AnimationDirection) -> &'static str {
    match direction {
        AnimationDirection::Reverse => "reverse",
        AnimationDirection::PingPong => "pingpong",
        AnimationDirection::PingPongReverse => "pingpong_reverse",
        AnimationDirection::Forward | AnimationDirection::Unknown(_) => "forward",
    }
}

fn sheet_toml(file: &AsepriteFile<'_>, stem: &str, texture: &str, packed: &[Packed]) -> String {
    let mut out = format!(
        "# Imported from {stem}.aseprite by `balaur import`; edit the source and import again.\n\
         type = \"sprite_sheet\"\n\
         texture = {}\nframes = [\n",
        toml::Value::String(texture.to_string())
    );
    for frame in packed {
        let _ = writeln!(
            out,
            "  {{ rect = {}, duration = {} }},",
            rect(frame.rect),
            seconds(frame.milliseconds)
        );
    }
    out.push_str("]\n");
    for tag in file.tags() {
        let _ = write!(
            out,
            "\n[tags.{}]\nfrom = {}\nto = {}\ndirection = \"{}\"\nrepeat = {}\n",
            key(&tag.name),
            tag.range.start(),
            tag.range.end(),
            direction_name(tag.direction),
            tag.repeat.unwrap_or(0)
        );
    }
    for slice in file.slices() {
        out.push_str(&slice_toml(slice));
    }
    out
}

/// A slice: its first key flattened onto the table, and every key listed
/// when it has more than one — a hitbox that moves with the frames.
fn slice_toml(slice: &SliceChunk<'_>) -> String {
    let Some(first) = slice.slice_keys.first() else {
        return String::new();
    };
    let mut out = format!(
        "\n[slices.{}]\n{}",
        key(slice.name),
        slice_key_lines(first, "")
    );
    if slice.slice_keys.len() > 1 {
        out.push_str("keys = [\n");
        for k in &slice.slice_keys {
            let _ = writeln!(
                out,
                "  {{ frame = {}, {} }},",
                k.frame_number,
                slice_key_lines(k, ", ").trim_end_matches(", ")
            );
        }
        out.push_str("]\n");
    }
    out
}

/// `rect`, `center` and `pivot` of one key, each followed by `sep` — a
/// newline for a table, a comma for an inline one.
fn slice_key_lines(k: &aseprite_loader::binary::chunks::slice::SliceKey, sep: &str) -> String {
    let sep = if sep.is_empty() { "\n" } else { sep };
    let mut out = format!("rect = [{}, {}, {}, {}]{sep}", k.x, k.y, k.width, k.height);
    if let Some(c) = k.nine_patch {
        let _ = write!(
            out,
            "center = [{}, {}, {}, {}]{sep}",
            c.x, c.y, c.width, c.height
        );
    }
    if let Some(p) = k.pivot {
        let _ = write!(out, "pivot = [{}, {}]{sep}", p.x, p.y);
    }
    out
}

/// The frames a tag plays, in order, and what the clip does at the end.
fn sequence(
    from: u32,
    to: u32,
    direction: AnimationDirection,
    repeat: Option<u16>,
) -> (Vec<u32>, &'static str) {
    let forward: Vec<u32> = (from..=to).collect();
    let (run, wrap) = match direction {
        AnimationDirection::Reverse => (forward.iter().rev().copied().collect(), "loop"),
        AnimationDirection::PingPong => (forward, "pingpong"),
        AnimationDirection::PingPongReverse => {
            (forward.iter().rev().copied().collect(), "pingpong")
        }
        AnimationDirection::Forward | AnimationDirection::Unknown(_) => (forward, "loop"),
    };
    let Some(times) = repeat.filter(|n| *n > 0) else {
        return (run, wrap);
    };
    // A counted play is unrolled and stops: the clip format loops for ever
    // or not at all.
    let cycle: Vec<u32> = if wrap == "pingpong" && run.len() > 2 {
        run.iter()
            .chain(run[1..run.len() - 1].iter().rev())
            .copied()
            .collect()
    } else {
        run
    };
    let unrolled = std::iter::repeat_n(cycle, usize::from(times))
        .flatten()
        .collect();
    (unrolled, "none")
}

fn clips_toml(file: &AsepriteFile<'_>, stem: &str, packed: &[Packed]) -> Option<String> {
    let last = (packed.len() - 1) as u32;
    let tags: Vec<(String, Vec<u32>, &str)> = if file.tags().is_empty() {
        if packed.len() < 2 {
            return None;
        }
        vec![(stem.to_string(), (0..=last).collect(), "loop")]
    } else {
        file.tags()
            .iter()
            .map(|tag| {
                let (frames, wrap) = sequence(
                    u32::from(*tag.range.start()).min(last),
                    u32::from(*tag.range.end()).min(last),
                    tag.direction,
                    tag.repeat,
                );
                (tag.name.clone(), frames, wrap)
            })
            .collect()
    };
    let mut out = format!(
        "# Imported from {stem}.aseprite by `balaur import`: one clip per tag, keying `sprite/frame`.\n\
         type = \"animation_clip\"\n"
    );
    for (name, frames, wrap) in tags {
        let mut at = 0;
        let mut keys = String::new();
        for frame in &frames {
            let _ = writeln!(keys, "  {{ t = {}, value = {frame}.0 }},", seconds(at));
            at += packed[*frame as usize].milliseconds;
        }
        let _ = write!(
            out,
            "\n[clips.{name}]\nlength = {}\nloop = \"{wrap}\"\n\n[[clips.{name}.tracks]]\nproperty = \"sprite/frame\"\ninterp = \"step\"\nkeys = [\n{keys}]\n",
            seconds(at.max(1)),
            name = key(&name)
        );
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{columns_for, key, sequence};
    use aseprite_loader::binary::chunks::tags::AnimationDirection;

    #[test]
    fn the_page_is_as_square_as_the_count_allows() {
        assert_eq!(columns_for(1), 1);
        assert_eq!(columns_for(3), 2);
        assert_eq!(columns_for(4), 2);
        assert_eq!(columns_for(5), 3);
    }

    #[test]
    fn a_key_is_bare_when_toml_allows_and_quoted_otherwise() {
        assert_eq!(key("walk_left"), "walk_left");
        assert_eq!(key("Walk Left"), "\"Walk Left\"");
    }

    #[test]
    fn a_tag_plays_in_its_direction_and_a_counted_one_unrolls() {
        assert_eq!(
            sequence(1, 3, AnimationDirection::Forward, None),
            (vec![1, 2, 3], "loop")
        );
        assert_eq!(
            sequence(1, 3, AnimationDirection::Reverse, None),
            (vec![3, 2, 1], "loop")
        );
        assert_eq!(
            sequence(1, 3, AnimationDirection::PingPong, None),
            (vec![1, 2, 3], "pingpong")
        );
        assert_eq!(
            sequence(1, 3, AnimationDirection::Forward, Some(2)),
            (vec![1, 2, 3, 1, 2, 3], "none")
        );
        assert_eq!(
            sequence(1, 3, AnimationDirection::PingPong, Some(2)),
            (vec![1, 2, 3, 2, 1, 2, 3, 2], "none")
        );
    }
}
