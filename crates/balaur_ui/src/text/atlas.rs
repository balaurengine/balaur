//! The glyph atlas the widget layer owns: one egui-managed texture that
//! shaped glyphs are drawn into once and quoted by UV from then on.
//!
//! Separate from egui's own font texture, which is filled by `char` and has
//! no way in for a glyph a shaper chose. When this one fills up it starts
//! over on a fresh texture; a layout that quoted the old one is rebuilt by
//! its generation number.

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

#[derive(Default)]
pub(crate) struct GlyphAtlas {
    texture: Option<TextureId>,
    cursor: (usize, usize),
    row_height: usize,
    slots: HashMap<CacheKey, Option<Slot>>,
    /// Bumped whenever the texture is replaced, so a layout holding UVs into
    /// the old one knows to rebuild.
    pub(crate) generation: u64,
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

    /// Start over on a fresh texture; every slot handed out so far is void.
    fn reset(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.texture.take() {
            ctx.tex_manager().write().free(id);
        }
        self.cursor = (0, 0);
        self.row_height = 0;
        self.slots.clear();
        self.generation += 1;
    }

    /// The slot for one glyph, rasterising it on first sight. `None` for a
    /// glyph with no outline, a space or a control character.
    pub(crate) fn slot(
        &mut self,
        ctx: &egui::Context,
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
            self.reset(ctx);
            self.allocate(width + 2 * PAD, height + 2 * PAD)?
        };
        let id = self.open(ctx);
        let patch = ColorImage::new([width, height], pixels);
        ctx.tex_manager().write().set(
            id,
            ImageDelta::partial([x + PAD, y + PAD], patch, TextureOptions::LINEAR),
        );
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
