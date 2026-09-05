//! The `sprite_sheet` asset: one texture cut into frames of any size, with
//! the tags and slices an editor drew on it. A `sprite` naming one draws
//! `frames[frame]`, so a clip keying `sprite/frame` steps through a packed
//! atlas as it does through a uniform grid.

use anyhow::{Result, anyhow, bail};
use balaur_plugin::Registry;

use crate::shape::keys as k;

pub const SPRITE_SHEET_ASSET_TYPE: &str = "sprite_sheet";

/// One frame of the sheet: where it sits on the texture, and how long a
/// clip cut from a tag shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetFrame {
    /// `[x, y, w, h]` in texture pixels.
    pub rect: [u32; 4],
    /// Seconds; zero for a frame no tag times.
    pub duration: f32,
}

/// A named run of frames, as the sprite editor tagged it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetTag {
    pub name: String,
    pub from: u32,
    pub to: u32,
    /// `forward`, `reverse`, `pingpong` or `pingpong_reverse`.
    pub direction: String,
    /// Plays before stopping; zero plays forever.
    pub repeat: u32,
}

/// A named rectangle on a frame — a hitbox, a pivot, a nine-patch centre —
/// in the frame's own pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetSlice {
    pub name: String,
    pub rect: [i32; 4],
    /// The nine-patch centre, when the slice has one.
    pub center: Option<[i32; 4]>,
    pub pivot: Option<[i32; 2]>,
}

/// A parsed sheet. Immutable and shared by every sprite that names it.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteSheet {
    /// Project-relative path to the atlas image.
    pub texture: String,
    pub frames: Vec<SheetFrame>,
    pub tags: Vec<SheetTag>,
    pub slices: Vec<SheetSlice>,
}

impl SpriteSheet {
    /// The frame a `sprite.frame` picks: past the end draws the last one,
    /// so an overrun stays visible as the sheet's own art.
    #[must_use]
    pub fn frame(&self, index: u32) -> SheetFrame {
        let last = self.frames.len().saturating_sub(1);
        self.frames[(index as usize).min(last)]
    }

    /// Parse a definition table.
    pub fn parse(value: &toml::Value) -> Result<Self> {
        let texture = value
            .get(k::TEXTURE)
            .and_then(toml::Value::as_str)
            .ok_or_else(|| anyhow!("a sprite_sheet needs a `texture` string naming its image"))?
            .to_string();
        if texture.is_empty() {
            bail!("a sprite_sheet's `texture` names no image");
        }
        let frames = parse_frames(value)?;
        let tags = parse_tags(value, frames.len())?;
        let slices = parse_slices(value)?;
        Ok(Self {
            texture,
            frames,
            tags,
            slices,
        })
    }
}

fn numbers<const N: usize>(value: &toml::Value, what: &str) -> Result<[i64; N]> {
    let items = value
        .as_array()
        .ok_or_else(|| anyhow!("{what} is {}, not a list of {N} numbers", value.type_str()))?;
    if items.len() != N {
        bail!("{what} holds {} numbers; it takes {N}", items.len());
    }
    let mut out = [0; N];
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = balaur_core::components::as_f64(item)
            .ok_or_else(|| anyhow!("{what} holds {}, not a number", item.type_str()))?
            .round() as i64;
    }
    Ok(out)
}

fn parse_frames(value: &toml::Value) -> Result<Vec<SheetFrame>> {
    let items = value
        .get("frames")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow!("a sprite_sheet needs a `frames` list"))?;
    if items.is_empty() {
        bail!("a sprite_sheet needs at least one frame");
    }
    let mut frames = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let rect = item
            .get("rect")
            .ok_or_else(|| anyhow!("frame {index} needs a `rect` of [x, y, w, h]"))?;
        let [x, y, w, h] = numbers::<4>(rect, &format!("frame {index}'s `rect`"))?;
        if x < 0 || y < 0 || w <= 0 || h <= 0 {
            bail!("frame {index}'s `rect` [{x}, {y}, {w}, {h}] is not a rectangle on the image");
        }
        let duration = item
            .get("duration")
            .map(|d| {
                balaur_core::components::as_f64(d)
                    .ok_or_else(|| anyhow!("frame {index}'s `duration` is not a number"))
            })
            .transpose()?
            .unwrap_or(0.0);
        if duration < 0.0 {
            bail!("frame {index}'s `duration` is negative");
        }
        frames.push(SheetFrame {
            rect: [x as u32, y as u32, w as u32, h as u32],
            duration: duration as f32,
        });
    }
    Ok(frames)
}

const DIRECTIONS: &[&str] = &["forward", "reverse", "pingpong", "pingpong_reverse"];

fn parse_tags(value: &toml::Value, frame_count: usize) -> Result<Vec<SheetTag>> {
    let Some(table) = value.get("tags") else {
        return Ok(Vec::new());
    };
    let table = table
        .as_table()
        .ok_or_else(|| anyhow!("`tags` is {}, not a table of tags", table.type_str()))?;
    let mut tags = Vec::with_capacity(table.len());
    for (name, tag) in table {
        let bound = |key: &str| -> Result<u32> {
            let n = tag
                .get(key)
                .and_then(toml::Value::as_integer)
                .ok_or_else(|| anyhow!("tag '{name}' needs an integer `{key}` frame"))?;
            if n < 0 || n as usize >= frame_count {
                bail!("tag '{name}': `{key}` is {n}, but the sheet has {frame_count} frames");
            }
            Ok(n as u32)
        };
        let (from, to) = (bound("from")?, bound("to")?);
        if from > to {
            bail!("tag '{name}' runs from {from} to {to}, backwards");
        }
        let direction = tag
            .get("direction")
            .and_then(toml::Value::as_str)
            .unwrap_or(DIRECTIONS[0]);
        if !DIRECTIONS.contains(&direction) {
            bail!(
                "tag '{name}': `direction` is '{direction}'; one of {}",
                DIRECTIONS.join(", ")
            );
        }
        let repeat = tag.get("repeat").and_then(toml::Value::as_integer).unwrap_or(0);
        tags.push(SheetTag {
            name: name.clone(),
            from,
            to,
            direction: direction.to_string(),
            repeat: repeat.max(0) as u32,
        });
    }
    Ok(tags)
}

fn parse_slices(value: &toml::Value) -> Result<Vec<SheetSlice>> {
    let Some(table) = value.get("slices") else {
        return Ok(Vec::new());
    };
    let table = table
        .as_table()
        .ok_or_else(|| anyhow!("`slices` is {}, not a table of slices", table.type_str()))?;
    let mut slices = Vec::with_capacity(table.len());
    for (name, slice) in table {
        let rect = slice
            .get("rect")
            .ok_or_else(|| anyhow!("slice '{name}' needs a `rect` of [x, y, w, h]"))?;
        let [x, y, w, h] = numbers::<4>(rect, &format!("slice '{name}'s `rect`"))?;
        let center = slice
            .get("center")
            .map(|c| numbers::<4>(c, &format!("slice '{name}'s `center`")))
            .transpose()?
            .map(|[cx, cy, cw, ch]| [cx as i32, cy as i32, cw as i32, ch as i32]);
        let pivot = slice
            .get("pivot")
            .map(|p| numbers::<2>(p, &format!("slice '{name}'s `pivot`")))
            .transpose()?
            .map(|[px, py]| [px as i32, py as i32]);
        slices.push(SheetSlice {
            name: name.clone(),
            rect: [x as i32, y as i32, w as i32, h as i32],
            center,
            pivot,
        });
    }
    Ok(slices)
}

const SHEET_ASSET_DOC: &str = r#"An image cut into frames of any size, for `sprite.sheet`: `texture` names
the image and each of `frames` is a `rect` of `[x, y, w, h]` texture pixels
with the `duration` in seconds a clip shows it for. `sprite.frame` indexes
the list, past the end drawing the last frame. `[tags.<name>]` is a run of
frames `from` one index `to` another with a `direction` (`forward`,
`reverse`, `pingpong`, `pingpong_reverse`) and a `repeat` count, zero for
ever; `[slices.<name>]` is a `rect` on a frame, in the frame's own pixels,
with an optional nine-patch `center` and `pivot`, and `keys` when the slice
moves between frames. `balaur import file.aseprite` writes one of these
beside the atlas it packs and a clip per tag.

```toml
type = "sprite_sheet"
texture = "art/walk.png"
frames = [
  { rect = [0, 0, 32, 32], duration = 0.1 },
  { rect = [32, 0, 32, 32], duration = 0.1 },
]

[tags.walk]
from = 0
to = 1
direction = "forward"

[slices.hitbox]
rect = [8, 4, 16, 28]
```"#;

/// The `sprite_sheet` asset type: files live in `sheets/`.
pub(crate) fn register_sheet_asset(reg: &mut Registry<'_>) {
    reg.register_asset_type(SPRITE_SHEET_ASSET_TYPE, "sheets", SHEET_ASSET_DOC, |value| {
        Ok(std::rc::Rc::new(SpriteSheet::parse(value)?) as std::rc::Rc<dyn std::any::Any>)
    });
}

#[cfg(test)]
mod tests {
    use super::SpriteSheet;

    fn sheet(text: &str) -> anyhow::Result<SpriteSheet> {
        SpriteSheet::parse(&toml::from_str(text).unwrap())
    }

    #[test]
    fn a_sheet_parses_frames_tags_and_slices() {
        let sheet = sheet(
            r#"texture = "art/walk.png"
frames = [{ rect = [0, 0, 32, 32], duration = 0.1 }, { rect = [32, 0, 32, 32] }]
[tags.walk]
from = 0
to = 1
direction = "pingpong"
[slices.hitbox]
rect = [8, 4, 16, 28]
pivot = [16, 32]
"#,
        )
        .unwrap();
        assert_eq!(sheet.frames.len(), 2);
        assert_eq!(sheet.frames[1].rect, [32, 0, 32, 32]);
        assert!(sheet.frames[1].duration.abs() < f32::EPSILON);
        assert_eq!(sheet.tags[0].direction, "pingpong");
        assert_eq!(sheet.slices[0].pivot, Some([16, 32]));
        assert_eq!(sheet.frame(7).rect, [32, 0, 32, 32], "past the end is the last frame");
    }

    #[test]
    fn a_tag_past_the_frames_or_a_bad_direction_is_refused_by_name() {
        let error = sheet(
            "texture = \"a.png\"\nframes = [{ rect = [0, 0, 1, 1] }]\n[tags.run]\nfrom = 0\nto = 3\n",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("run") && error.contains("3"), "unhelpful: {error}");
        let error = sheet(
            "texture = \"a.png\"\nframes = [{ rect = [0, 0, 1, 1] }]\n[tags.run]\nfrom = 0\nto = 0\ndirection = \"sideways\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("sideways"), "unhelpful: {error}");
    }

    #[test]
    fn a_sheet_with_no_frames_or_a_flat_rect_is_refused() {
        assert!(sheet("texture = \"a.png\"\nframes = []\n").is_err());
        assert!(sheet("texture = \"a.png\"\nframes = [{ rect = [0, 0, 0, 4] }]\n").is_err());
    }
}
