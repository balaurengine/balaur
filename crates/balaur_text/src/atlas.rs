//! The glyph atlas the shaper owns: one image that shaped glyphs are drawn
//! into once and quoted by UV from then on.
//!
//! Separate from egui's own font texture, which is filled by `char` and has
//! no way in for a glyph a shaper chose. When this one fills up it starts
//! over; a layout that quoted the old one is rebuilt by its generation.
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

const SIDE: usize = 1024;
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
            rgba: vec![0; SIDE * SIDE * 4],
        }
    }
}

#[derive(Default)]
pub struct GlyphAtlas {
    pixels: Pixels,
    texture: Option<TextureId>,
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

impl GlyphAtlas {
    pub(crate) fn texture(&self) -> Option<TextureId> {
        self.texture
    }

    fn open(&mut self, ctx: &egui::Context) -> TextureId {
        if let Some(id) = self.texture {
            return id;
        }
        let blank = ColorImage::filled([SIDE, SIDE], Color32::TRANSPARENT);
        let id = ctx.tex_manager().write().alloc(
            "balaur text atlas".into(),
            ImageData::Color(Arc::new(blank)),
            TextureOptions::LINEAR,
        );
        self.texture = Some(id);
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
        self.dirty = Some([0, 0, SIDE, SIDE]);
    }

    /// Copy one rasterised glyph in, and note the box it landed in.
    fn write(&mut self, x: usize, y: usize, width: usize, height: usize, pixels: &[Color32]) {
        for row in 0..height {
            let from = row * width;
            let to = ((y + row) * SIDE + x) * 4;
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
        if width == 0 || height == 0 || width + 2 * PAD > SIDE || height + 2 * PAD > SIDE {
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
        let side = SIDE as f32;
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

    /// The atlas is square; this is its side in pixels.
    pub fn side(&self) -> usize {
        SIDE
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
                let at = (row * SIDE + column) * 4;
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
    /// glyph with no outline, a space or a control character.
    pub(crate) fn slot(
        &mut self,
        fonts: &mut FontSystem,
        swash: &mut SwashCache,
        key: CacheKey,
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
        if width + 2 * PAD > SIDE || height + 2 * PAD > SIDE {
            tracing::warn!("a glyph larger than the atlas was skipped");
            self.slots.insert(key, None);
            return None;
        }
        let pixels: Vec<Color32> = match image.content {
            SwashContent::Mask => image
                .data
                .iter()
                .map(|&a| Color32::from_rgba_premultiplied(a, a, a, a))
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
        let side = SIDE as f32;
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
        if self.cursor.0 + width > SIDE {
            self.cursor = (0, self.cursor.1 + self.row_height);
            self.row_height = 0;
        }
        if self.cursor.1 + height > SIDE {
            return None;
        }
        let at = self.cursor;
        self.cursor.0 += width;
        self.row_height = self.row_height.max(height);
        Some(at)
    }
}
