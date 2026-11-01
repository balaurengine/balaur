//! The `sprite` component. The data it writes (`SpriteTexture` inside
//! `Renderable2d`) lives in the crate root.

use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Written};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

use crate::vocabulary::words;
use crate::{Renderable2d, SpriteSheet2d, SpriteTexture, set_sprite};

/// A textured 2D quad at the node, sized by `pixels_per_unit`. A `sprite_sheet` in `sheet` cuts it into frames `frame` picks.
// The node draws the [`SpriteTexture`] each write leaves in its
// [`Renderable2d`].
#[derive(Clone, Component)]
#[component(name = "sprite", on_remove = sprite_removed)]
pub struct Sprite {
    /// Image file, project-relative, or a `texture` asset that reads it with settings of its own. Empty draws nothing
    #[property(asset = "texture")]
    pub texture: String,
    /// Tint, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub color: [f32; 4],
    /// A sprite_sheet whose frames `frame` indexes; its texture is drawn unless `texture` names another, and it wins over the region
    #[property(asset = "sprite_sheet")]
    pub sheet: String,
    /// Current sheet cell, counted left-to-right then top-to-bottom
    #[property(min = 0)]
    pub frame: i64,
    /// Mirror horizontally
    pub flip_x: bool,
    /// Mirror vertically
    pub flip_y: bool,
    /// The material this draws with; empty draws with the built-in one
    #[property(asset = "material")]
    pub material: String,
    /// Centre the image on the node; off puts its top-left corner there
    #[property(default = true, group = "placement")]
    pub centered: bool,
    /// Where the image sits against the node, in texture pixels with y down; turns and scales with the node
    #[property(group = "placement")]
    pub offset: [f32; 2],
    /// Whole size in world units; [0, 0] sizes from the texture
    #[property(group = "placement")]
    pub size: [f32; 2],
    /// Texture pixels per world unit; 0 takes the texture's own `pixels_per_unit` import setting, which is 100 unless it says
    #[property(min = 0.0, group = "placement")]
    pub pixels_per_unit: f32,
    /// Top-left corner of the atlas cell to draw, in texture pixels; used with `region_size`
    #[property(group = "region")]
    pub region_origin: [f32; 2],
    /// Size of the atlas cell to draw, in texture pixels; [0, 0] draws the whole image and sizes the quad from the cell
    #[property(group = "region")]
    pub region_size: [f32; 2],
    /// Left, right, top and bottom margins in texture pixels: the corners keep their size at `pixels_per_unit` while the edges and the middle stretch to `size`. All zero takes the margins from the `sheet`'s first slice with a `center`, drawing that slice's part of the frame, and draws one plain quad without one
    #[property(group = "region")]
    pub nine_slice_margins_pixels: [f32; 4],
}

/// What a write of `sprite` leaves to be worked out again.
fn sprite_written(
    eng: &balaur_core::Engine,
    entity: Entity,
    _written: Written,
) -> anyhow::Result<()> {
    sprite_laid_out(eng, entity)
}

/// The quad a sprite draws, worked out again from what was written: its
/// texture and frame, its sheet's cut, its size and where it sits.
fn sprite_laid_out(eng: &balaur_core::Engine, entity: Entity) -> anyhow::Result<()> {
    let Some(asked) = eng
        .world()
        .get::<&Sprite>(entity)
        .ok()
        .map(|asked| Sprite::clone(&asked))
    else {
        return Ok(());
    };
    let mut texture = asked.texture.clone();
    let frame = u32::try_from(asked.frame.max(0)).unwrap_or(u32::MAX);
    let sheet_asset = asked.sheet.trim().to_string();
    let (atlas, grid) = atlas_frame(eng, &sheet_asset, frame, &mut texture)?;
    let margins = asked
        .nine_slice_margins_pixels
        .map(|margin| margin.max(0.0));
    let patch = if margins.iter().any(|m| *m > 0.0) {
        None
    } else {
        sheet_patch(eng, &sheet_asset, frame, &texture, atlas, grid)
    };
    // A `columns` x `rows` sheet draws as that grid, unless a nine-patch
    // slice crops the frame to a region of its own.
    let sheet = grid
        .filter(|_| patch.is_none())
        .map(|[columns, rows]| SpriteSheet2d { columns, rows });
    // An absent (or zero) size means "size it from the image".
    let [hx, hy] = asked.size.map(|whole| whole / 2.0);
    let explicit = (hx > 0.0 && hy > 0.0).then_some((hx, hy));
    let [rw, rh] = asked.region_size;
    let region = patch.map(|(rect, _)| rect).or(atlas).or_else(|| {
        (rw > 0.0 && rh > 0.0).then(|| {
            let [x, y] = asked.region_origin.map(|at| at.max(0.0).round() as u32);
            [x, y, rw.round() as u32, rh.round() as u32]
        })
    });
    let own_ppu = asked.pixels_per_unit;
    let per = if own_ppu > 0.0 {
        own_ppu
    } else {
        crate::texture::pixels_per_unit(eng, &texture)
    };
    let offset = asked.offset;
    set_sprite(
        eng,
        entity,
        SpriteTexture {
            path: texture,
            sheet,
            frame,
            flip_x: asked.flip_x,
            flip_y: asked.flip_y,
            region,
            centered: asked.centered,
            shift: [offset[0] / per, -offset[1] / per],
            nine: patch
                .map(|(_, margins)| margins)
                .or_else(|| margins.iter().any(|m| *m > 0.0).then_some(margins)),
        },
        explicit,
        per,
    )?;
    crate::set_color(eng, entity, asked.color)?;
    crate::material::set_material_2d(eng, entity, &asked.material)
}

/// What a removed `Sprite` leaves behind, taken with it.
fn sprite_removed(eng: &balaur_core::Engine, entity: Entity) {
    let _ = eng.remove_one::<Renderable2d>(entity);
}

pub(crate) fn register_sprite_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Sprite::NAME,
        ComponentDef {
            tags: &[words::ORTHOGRAPHIC, "render"],
            ..ComponentDef::of::<Sprite>(Some(sprite_written))
        },
    );
}

/// The first slice of a sheet that carries a nine-patch `center`, as the
/// region of the texture it covers on `frame` and the margins its centre
/// leaves: left, right, top, bottom, in texture pixels. `None` without one.
fn sheet_patch(
    eng: &balaur_core::Engine,
    sheet_asset: &str,
    frame: u32,
    texture: &str,
    atlas: Option<[u32; 4]>,
    grid: Option<[u32; 2]>,
) -> Option<([u32; 4], [f32; 4])> {
    if sheet_asset.is_empty() {
        return None;
    }
    let sheet =
        balaur_core::assets::load_typed::<crate::sheet::SpriteSheet>(eng, sheet_asset).ok()?;
    let slice = sheet.slices.iter().find(|slice| slice.center.is_some())?;
    let [cx, cy, cw, ch] = slice.center?;
    let [sx, sy, sw, sh] = slice.rect;
    // Where the frame sits on the texture: listed, or one cell of the grid.
    let [fx, fy] = match (atlas, grid) {
        (Some([x, y, _, _]), _) => [x, y],
        (None, Some([columns, rows])) => {
            let (w, h) = crate::texture::size_of(eng, texture).ok()?;
            let (cell_w, cell_h) = (w / columns.max(1), h / rows.max(1));
            [
                (frame % columns.max(1)) * cell_w,
                (frame / columns.max(1)) * cell_h,
            ]
        }
        (None, None) => [0, 0],
    };
    let at = |base: u32, offset: i32| u32::try_from(i64::from(base) + i64::from(offset)).ok();
    let rect = [
        at(fx, sx)?,
        at(fy, sy)?,
        u32::try_from(sw).ok()?,
        u32::try_from(sh).ok()?,
    ];
    let margins = [cx, sw - cx - cw, cy, sh - cy - ch].map(|m| m.max(0) as f32);
    Some((rect, margins))
}

/// The rectangle a `sprite_sheet` gives `frame`, filling in the sheet's
/// texture when the component names none. `None` without a sheet.
/// What a sheet says about the frame a sprite draws: the rect it sits on, or
/// the `columns` x `rows` cut the whole sheet is.
type SheetCut = (Option<[u32; 4]>, Option<[u32; 2]>);

fn atlas_frame(
    eng: &balaur_core::Engine,
    sheet_asset: &str,
    frame: u32,
    texture: &mut String,
) -> anyhow::Result<SheetCut> {
    if sheet_asset.is_empty() {
        return Ok((None, None));
    }
    let sheet = balaur_core::assets::load_typed::<crate::sheet::SpriteSheet>(eng, sheet_asset)?;
    if texture.is_empty() {
        texture.clone_from(&sheet.texture);
    }
    if let Some(grid) = sheet.grid {
        return Ok((None, Some(grid)));
    }
    Ok((Some(sheet.frame(frame).rect), None))
}

/// What a nine-slice sprite's mesh is built from: its whole size and its
/// margins in world units, the same margins as fractions of the rectangle it
/// draws, and that rectangle in the texture's UVs, `[min_x, min_y, max_x,
/// max_y]`, flipped as the sprite is.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    not(feature = "window"),
    allow(dead_code, reason = "the mesh is the backend's, and the tests'")
)]
pub(crate) struct NineKey {
    pub(crate) size: [f32; 2],
    pub(crate) world: [f32; 4],
    pub(crate) uv: [f32; 4],
    pub(crate) rect: [f32; 4],
}

/// The mesh a sprite cut by `margins` (left, right, top, bottom, texture
/// pixels) draws, for a quad of half extents `hx` by `hy` drawing a `cell`
/// of that many pixels at `rect`.
///
/// A flip mirrors the rectangle, so the margins swap sides with it: the
/// corner on the left is then the image's right one, at its own size.
#[cfg_attr(
    not(feature = "window"),
    allow(dead_code, reason = "the mesh is the backend's, and the tests'")
)]
pub(crate) fn nine_key(
    margins: [f32; 4],
    (hx, hy): (f32, f32),
    pixels_per_unit: f32,
    cell: (f32, f32),
    rect: [f32; 4],
) -> NineKey {
    let [mut left, mut right, mut top, mut bottom] = margins;
    if rect[0] > rect[2] {
        std::mem::swap(&mut left, &mut right);
    }
    if rect[1] > rect[3] {
        std::mem::swap(&mut top, &mut bottom);
    }
    let ppu = pixels_per_unit.max(0.01);
    let (cw, ch) = (cell.0.max(1.0), cell.1.max(1.0));
    NineKey {
        size: [2.0 * hx, 2.0 * hy],
        world: [left / ppu, right / ppu, top / ppu, bottom / ppu],
        uv: [left / cw, right / cw, top / ch, bottom / ch],
        rect,
    }
}

#[cfg(test)]
mod tests {
    use super::nine_key;

    fn same<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn a_flipped_nine_slice_keeps_each_corner_at_its_own_size() {
        let margins = [32.0, 8.0, 16.0, 4.0];
        let plain = nine_key(
            margins,
            (2.0, 1.0),
            16.0,
            (64.0, 32.0),
            [0.0, 0.0, 1.0, 1.0],
        );
        assert!(same(plain.size, [4.0, 2.0]));
        assert!(same(plain.world, [2.0, 0.5, 1.0, 0.25]));
        assert!(same(plain.uv, [0.5, 0.125, 0.5, 0.125]));
        let mirrored = nine_key(
            margins,
            (2.0, 1.0),
            16.0,
            (64.0, 32.0),
            [1.0, 0.0, 0.0, 1.0],
        );
        assert!(
            same(mirrored.world, [0.5, 2.0, 1.0, 0.25]),
            "the image's right corner is now on the left"
        );
    }
}
