//! The glyph atlas the shaper owns: one image that shaped glyphs are drawn
//! into once and quoted by UV from then on.
//!
//! Separate from egui's own font texture, which is filled by `char` and has
//! no way in for a glyph a shaper chose. When this one fills up it doubles,
//! the way egui's own atlas does, and only starts over once it has reached
//! `SIDE_MAX`; either way a layout that quoted the old one is rebuilt by its
//! generation, since a UV is a fraction of a side that just changed.
//!
//! The pixels live here rather than in a texture, because two consumers draw
//! from them: the widget layer through egui, and the world through the
//! renderer. Each mirrors this image on its own terms — egui takes the region
//! that changed, the renderer re-uploads when `revision` moves.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::{CacheKey, FontSystem, SwashCache, SwashContent};
use egui::epaint::ImageDelta;
use egui::{Color32, ColorImage, ImageData, Rect, TextureId, TextureOptions, Vec2};

/// The side it opens at, and the one past which it starts over instead of
/// doubling again. Small to begin with, because an atlas is uploaded whole
/// the first time and most projects never outgrow one page of glyphs.
const SIDE_START: usize = 256;
const SIDE_MAX: usize = 4096;
/// A pixel of clearance so linear filtering never bleeds a neighbour in.
const PAD: usize = 1;

/// Where one rasterised glyph sits, and how to place its quad.
#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) uv: Rect,
    pub(crate) size: Vec2,
    /// Left and top bearing: where the bitmap's corner sits relative to the
    /// glyph origin on the baseline.
    pub(crate) offset: Vec2,
    /// Whether the bitmap carries its own colour, as an emoji does.
    pub(crate) colored: bool,
}

/// The atlas's own pixels, in the RGBA egui and wgpu both take.
struct Pixels {
    rgba: Vec<u8>,
}

impl Default for Pixels {
    fn default() -> Self {
        Self {
            rgba: vec![0; SIDE_START * SIDE_START * 4],
        }
    }
}

pub struct GlyphAtlas {
    pixels: Pixels,
    /// The square's side now. Doubles when a glyph no longer fits.
    side: usize,
    texture: Option<TextureId>,
    /// The side the egui texture was allocated at, so a grown atlas knows to
    /// hand back the old one and take a bigger.
    texture_side: usize,
    cursor: (usize, usize),
    row_height: usize,
    slots: HashMap<CacheKey, Option<Slot>>,
    /// Bumped whenever the texture is replaced, so a layout holding UVs into
    /// the old one knows to rebuild.
    pub(crate) generation: u64,
    /// Bumped by every write, so a consumer can tell whether it is behind.
    revision: u64,
    /// The box written since egui last took one, as `[min_x, min_y, max_x,
    /// max_y]`; egui is uploaded from this and the renderer from `revision`.
    dirty: Option<[usize; 4]>,
}

impl Default for GlyphAtlas {
    fn default() -> Self {
        Self {
            pixels: Pixels::default(),
            side: SIDE_START,
            texture: None,
            texture_side: 0,
            cursor: (0, 0),
            row_height: 0,
            slots: HashMap::new(),
            generation: 0,
            revision: 0,
            dirty: None,
        }
    }
}

impl GlyphAtlas {
    pub(crate) fn texture(&self) -> Option<TextureId> {
        self.texture
    }

    fn open(&mut self, ctx: &egui::Context) -> TextureId {
        if let Some(id) = self.texture {
            if self.texture_side == self.side {
                return id;
            }
            ctx.tex_manager().write().free(id);
            self.texture = None;
        }
        let blank = ColorImage::filled([self.side, self.side], Color32::TRANSPARENT);
        let id = ctx.tex_manager().write().alloc(
            "balaur text atlas".into(),
            ImageData::Color(Arc::new(blank)),
            TextureOptions::LINEAR,
        );
        self.texture = Some(id);
        self.texture_side = self.side;
        id
    }

    /// Start over on a blank image; every slot handed out so far is void.
    fn reset(&mut self) {
        self.pixels.rgba.fill(0);
        self.cursor = (0, 0);
        self.row_height = 0;
        self.slots.clear();
        self.generation += 1;
        self.revision += 1;
        self.dirty = Some([0, 0, self.side, self.side]);
    }

    /// Double the square, keeping every glyph where it already sits.
    ///
    /// The rows move, because a wider atlas has a longer stride, but no glyph
    /// changes pixel coordinates, so the slots stay good. A UV does not: it
    /// is a fraction of a side that just changed, which is what `generation`
    /// makes a cached layout rebuild for.
    fn grow(&mut self) -> bool {
        let side = self.side * 2;
        if side > SIDE_MAX {
            return false;
        }
        let mut wider = vec![0u8; side * side * 4];
        for row in 0..self.side {
            let from = row * self.side * 4;
            let to = row * side * 4;
            wider[to..to + self.side * 4]
                .copy_from_slice(&self.pixels.rgba[from..from + self.side * 4]);
        }
        self.pixels.rgba = wider;
        self.side = side;
        // A slot's `uv` is a fraction of the side, and the side just doubled;
        // the pixels did not move, so halving each one keeps it on its glyph.
        for held in self.slots.values_mut().flatten() {
            held.uv = Rect::from_min_max(
                (held.uv.min.to_vec2() * 0.5).to_pos2(),
                (held.uv.max.to_vec2() * 0.5).to_pos2(),
            );
        }
        self.generation += 1;
        self.revision += 1;
        self.dirty = Some([0, 0, side, side]);
        true
    }

    /// Copy one rasterised glyph in, and note the box it landed in.
    fn write(&mut self, x: usize, y: usize, width: usize, height: usize, pixels: &[Color32]) {
        for row in 0..height {
            let from = row * width;
            let to = ((y + row) * self.side + x) * 4;
            for column in 0..width {
                let [r, g, b, a] = pixels[from + column].to_array();
                let at = to + column * 4;
                self.pixels.rgba[at] = r;
                self.pixels.rgba[at + 1] = g;
                self.pixels.rgba[at + 2] = b;
                self.pixels.rgba[at + 3] = a;
            }
        }
        self.revision += 1;
        self.dirty = Some(match self.dirty {
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x + width), y1.max(y + height)],
            None => [x, y, x + width, y + height],
        });
    }

    /// Copy a whole image in — a bitmap font's page — and answer the box it
    /// landed in, as atlas coordinates. `None` when it does not fit.
    ///
    /// A page is one allocation rather than one per glyph: its glyphs are
    /// already packed, and re-packing them would only move them about.
    pub(crate) fn place_image(&mut self, rgba: &[u8], width: usize, height: usize) -> Option<Rect> {
        if width == 0 || height == 0 || width + 2 * PAD > SIDE_MAX || height + 2 * PAD > SIDE_MAX {
            return None;
        }
        let (x, y) = if let Some(at) = self.allocate(width + 2 * PAD, height + 2 * PAD) {
            at
        } else {
            self.reset();
            self.allocate(width + 2 * PAD, height + 2 * PAD)?
        };
        let pixels: Vec<Color32> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        if pixels.len() < width * height {
            return None;
        }
        self.write(x + PAD, y + PAD, width, height, &pixels);
        let side = self.side as f32;
        Some(Rect::from_min_max(
            egui::pos2((x + PAD) as f32 / side, (y + PAD) as f32 / side),
            egui::pos2(
                (x + PAD + width) as f32 / side,
                (y + PAD + height) as f32 / side,
            ),
        ))
    }

    /// The whole image, for a consumer that uploads it entire.
    pub fn rgba(&self) -> &[u8] {
        &self.pixels.rgba
    }

    /// The atlas is square; this is its side in pixels now. It doubles as the
    /// atlas fills, so a consumer holding a texture re-makes it when
    /// `revision` moves and this does not match.
    pub fn side(&self) -> usize {
        self.side
    }

    /// Bumped by every glyph written and by every reset: a consumer holding
    /// an older number is looking at a stale copy.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Hand egui whatever has been written since it last asked.
    pub(crate) fn flush_egui(&mut self, ctx: &egui::Context) {
        let Some([x0, y0, x1, y1]) = self.dirty.take() else {
            return;
        };
        let id = self.open(ctx);
        let (width, height) = (x1 - x0, y1 - y0);
        let mut patch = Vec::with_capacity(width * height);
        for row in y0..y1 {
            for column in x0..x1 {
                let at = (row * self.side + column) * 4;
                let p = &self.pixels.rgba[at..at + 4];
                patch.push(Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]));
            }
        }
        ctx.tex_manager().write().set(
            id,
            ImageDelta::partial(
                [x0, y0],
                ColorImage::new([width, height], patch),
                TextureOptions::LINEAR,
            ),
        );
    }

    /// The slot for one glyph, rasterising it on first sight. `None` for a
    /// glyph with no outline, a space or a control character. `hard` draws
    /// its coverage fully on or off, for a face whose `antialias` is off.
    pub(crate) fn slot(
        &mut self,
        fonts: &mut FontSystem,
        swash: &mut SwashCache,
        key: CacheKey,
        hard: bool,
    ) -> Option<Slot> {
        if let Some(slot) = self.slots.get(&key) {
            return *slot;
        }
        let image = swash.get_image(fonts, key).clone()?;
        let (width, height) = (
            image.placement.width as usize,
            image.placement.height as usize,
        );
        if width == 0 || height == 0 {
            self.slots.insert(key, None);
            return None;
        }
        if width + 2 * PAD > SIDE_MAX || height + 2 * PAD > SIDE_MAX {
            tracing::warn!("a glyph larger than the atlas was skipped");
            self.slots.insert(key, None);
            return None;
        }
        let pixels: Vec<Color32> = match image.content {
            SwashContent::Mask => image
                .data
                .iter()
                .map(|&a| if hard { hard_edge(a) } else { a })
                .map(|a| Color32::from_rgba_premultiplied(a, a, a, a))
                .collect(),
            SwashContent::Color | SwashContent::SubpixelMask => image
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                .collect(),
        };
        let (x, y) = if let Some(at) = self.allocate(width + 2 * PAD, height + 2 * PAD) {
            at
        } else {
            self.reset();
            self.allocate(width + 2 * PAD, height + 2 * PAD)?
        };
        self.write(x + PAD, y + PAD, width, height, &pixels);
        let side = self.side as f32;
        let min = egui::pos2((x + PAD) as f32 / side, (y + PAD) as f32 / side);
        let max = egui::pos2(
            (x + PAD + width) as f32 / side,
            (y + PAD + height) as f32 / side,
        );
        let slot = Slot {
            uv: Rect::from_min_max(min, max),
            size: Vec2::new(width as f32, height as f32),
            offset: Vec2::new(image.placement.left as f32, image.placement.top as f32),
            colored: image.content != SwashContent::Mask,
        };
        self.slots.insert(key, Some(slot));
        Some(slot)
    }

    /// A shelf packer: rows left to right, rows top to bottom.
    fn allocate(&mut self, width: usize, height: usize) -> Option<(usize, usize)> {
        if self.cursor.0 + width > self.side {
            self.cursor = (0, self.cursor.1 + self.row_height);
            self.row_height = 0;
        }
        // Doubling is cheaper than starting over: every glyph already
        // rasterised keeps its place, where a reset re-rasterises the lot.
        while self.cursor.0 + width > self.side || self.cursor.1 + height > self.side {
            if !self.grow() {
                return None;
            }
        }
        let at = self.cursor;
        self.cursor.0 += width;
        self.row_height = self.row_height.max(height);
        Some(at)
    }
}

/// A glyph pixel's coverage with no edge between in and out: at least half
/// covered is ink, anything less is paper.
fn hard_edge(coverage: u8) -> u8 {
    if coverage >= 128 { 255 } else { 0 }
}

#[cfg(test)]
mod hard_edge_tests {
    use super::hard_edge;

    #[test]
    fn a_hard_edge_is_ink_from_half_covered() {
        assert_eq!(hard_edge(0), 0);
        assert_eq!(hard_edge(127), 0);
        assert_eq!(hard_edge(128), 255);
        assert_eq!(hard_edge(255), 255);
    }
}

#[cfg(test)]
mod grow_tests {
    use super::{GlyphAtlas, SIDE_START, Slot};
    use cosmic_text::CacheKey;
    use egui::{Rect, Vec2, pos2};

    /// A glyph keeps its pixels where they are, so its UV has to shrink by
    /// as much as the side grew or it reads another glyph's cell.
    #[test]
    fn growing_the_atlas_keeps_every_slot_on_its_own_glyph() {
        let mut atlas = GlyphAtlas::default();
        let side = SIDE_START as f32;
        let key = CacheKey::new(
            cosmic_text::fontdb::ID::dummy(),
            1,
            16.0,
            (0.0, 0.0),
            cosmic_text::Weight::NORMAL,
            cosmic_text::CacheKeyFlags::empty(),
        )
        .0;
        atlas.slots.insert(
            key,
            Some(Slot {
                uv: Rect::from_min_max(
                    pos2(64.0 / side, 32.0 / side),
                    pos2(80.0 / side, 48.0 / side),
                ),
                size: Vec2::new(16.0, 16.0),
                offset: Vec2::ZERO,
                colored: false,
            }),
        );
        assert!(atlas.grow(), "the atlas doubles from its starting side");
        let grown = atlas.side as f32;
        let held = atlas.slots[&key].expect("the slot is still there");
        assert!((held.uv.min.x - 64.0 / grown).abs() < 1e-6, "{:?}", held.uv);
        assert!((held.uv.max.y - 48.0 / grown).abs() < 1e-6, "{:?}", held.uv);
    }
}
