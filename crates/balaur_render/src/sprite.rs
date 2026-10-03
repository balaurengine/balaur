//! The `sprite` component: its schema and scene-file round trip. The data it
//! writes (`SpriteTexture` inside `Renderable2d`) lives in the crate root.

use balaur_core::components::ComponentDef;
use balaur_plugin::Registry;

use crate::vocabulary::{keys as k, words};
use crate::{Renderable2d, Shape2d, SpriteSheet2d, SpriteTexture, set_sprite};

/// The `sprite` component's property schema, lifted out so the
/// registration below stays readable.
fn sprite_schema() -> std::rc::Rc<toml::Value> {
    let mut overlay = crate::overlay::schema_2d(crate::overlay::Drawn::Builtin);
    overlay.extend(crate::lit_2d::rows());
    ComponentDef::parse_schema(
        "sprite",
        &balaur_core::components::ComponentDef::schema(&crate::overlay::with_rows(
            &[
                (
                    k::TEXTURE,
                    &format!(
                        r#"{{ type = "asset", asset = "{}", default = "", description = "Image file, project-relative, or a `texture` asset that reads it with settings of its own; required" }}"#,
                        balaur_core::texture_asset::TEXTURE_ASSET_TYPE
                    ),
                ),
                (
                    k::FRAME,
                    r#"{ type = "int", default = 0, min = 0, description = "Current sheet cell, counted left-to-right then top-to-bottom" }"#,
                ),
                (
                    k::FLIP_X,
                    r#"{ type = "bool", default = false, description = "Mirror horizontally" }"#,
                ),
                (
                    k::FLIP_Y,
                    r#"{ type = "bool", default = false, description = "Mirror vertically" }"#,
                ),
                (
                    k::PIXELS_PER_UNIT,
                    r#"{ type = "float", default = 0.0, min = 0.0, description = "Texture pixels per world unit; 0 takes the texture's own `pixels_per_unit` import setting, which is 100 unless it says" }"#,
                ),
                (
                    k::OFFSET,
                    r#"{ type = "vec2", default = [0.0, 0.0], description = "Where the image sits against the node, in texture pixels with y down; turns and scales with the node" }"#,
                ),
                (
                    k::CENTERED,
                    r#"{ type = "bool", default = true, description = "Centre the image on the node; off puts its top-left corner there" }"#,
                ),
                (
                    k::SIZE,
                    r#"{ type = "vec2", default = [0.0, 0.0], description = "Whole size in world units; [0, 0] sizes from the texture" }"#,
                ),
                (
                    k::SHEET,
                    &format!(
                        r#"{{ type = "asset", asset = "{}", default = "", description = "A sprite_sheet whose frames `frame` indexes; its texture is drawn unless `texture` names another, and it wins over `columns`, `rows` and the region" }}"#,
                        crate::sheet::SPRITE_SHEET_ASSET_TYPE
                    ),
                ),
                (
                    k::REGION_ORIGIN,
                    r#"{ type = "vec2", default = [0.0, 0.0], description = "Top-left corner of the atlas cell to draw, in texture pixels; used with `region_size`" }"#,
                ),
                (
                    k::REGION_SIZE,
                    r#"{ type = "vec2", default = [0.0, 0.0], description = "Size of the atlas cell to draw, in texture pixels; [0, 0] draws the whole image and sizes the quad from the cell" }"#,
                ),
                (
                    k::COLOR,
                    r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#,
                ),
                (
                    k::MATERIAL,
                    &format!(
                        r#"{{ type = "asset", asset = "{}", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#,
                        crate::material::MATERIAL_ASSET_TYPE
                    ),
                ),
                (
                    k::NINE_SLICE_MARGINS_PIXELS,
                    r#"{ type = "vec4", default = [0.0, 0.0, 0.0, 0.0], description = "Left, right, top and bottom margins in texture pixels: the corners keep their size at `pixels_per_unit` while the edges and the middle stretch to `size`. All zero takes the margins from the `sheet`'s first slice with a `center`, drawing that slice's part of the frame, and draws one plain quad without one" }"#,
                ),
            ],
            &overlay,
        )),
    )
}

/// The `sprite` component: a textured 2D quad.
/// Every `sprite` property onto the node.
fn apply_sprite(
    eng: &balaur_core::Engine,
    entity: balaur_core::hecs::Entity,
    params: &toml::Value,
) -> anyhow::Result<()> {
    let num = |key: &str| balaur_core::components::prop_f64(params, key);
    let mut texture = params
        .get(k::TEXTURE)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let frame = num(k::FRAME) as u32;
    let sheet_asset = params
        .get(k::SHEET)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let (atlas, grid) = atlas_frame(eng, &sheet_asset, frame, &mut texture)?;
    let margins = margins_of(params);
    let patch = if margins.iter().any(|m| *m > 0.0) {
        None
    } else {
        sheet_patch(eng, &sheet_asset, frame, &texture, atlas, grid)
    };
    let sheet_texture = !sheet_asset.is_empty() && texture_was_empty(params);
    // A `columns` x `rows` sheet draws as that grid, unless a nine-patch
    // slice crops the frame to a region of its own.
    let sheet = grid
        .filter(|_| patch.is_none())
        .map(|[columns, rows]| SpriteSheet2d { columns, rows });
    let he = |i: usize| {
        params
            .get(k::SIZE)
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(0.0) as f32
            / 2.0
    };
    // An absent (or zero) size means "size it from the image".
    let explicit = (he(0) > 0.0 && he(1) > 0.0).then(|| (he(0), he(1)));
    let pair = |key: &str, i: usize| {
        params
            .get(key)
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(0.0) as f32
    };
    let (rw, rh) = (pair(k::REGION_SIZE, 0), pair(k::REGION_SIZE, 1));
    let region = patch.map(|(rect, _)| rect).or(atlas).or_else(|| {
        (rw > 0.0 && rh > 0.0).then(|| {
            [
                pair(k::REGION_ORIGIN, 0).max(0.0).round() as u32,
                pair(k::REGION_ORIGIN, 1).max(0.0).round() as u32,
                rw.round() as u32,
                rh.round() as u32,
            ]
        })
    });
    let own_ppu = num(k::PIXELS_PER_UNIT) as f32;
    let per = if own_ppu > 0.0 {
        own_ppu
    } else {
        crate::texture::pixels_per_unit(eng, &texture)
    };
    let offset = [pair(k::OFFSET, 0), pair(k::OFFSET, 1)];
    set_sprite(
        eng,
        entity,
        SpriteTexture {
            path: texture,
            sheet,
            frame,
            flip_x: params.get(k::FLIP_X).and_then(toml::Value::as_bool) == Some(true),
            flip_y: params.get(k::FLIP_Y).and_then(toml::Value::as_bool) == Some(true),
            region,
            sheet_asset,
            sheet_texture,
            offset,
            centered: params.get(k::CENTERED).and_then(toml::Value::as_bool) != Some(false),
            shift: [offset[0] / per, -offset[1] / per],
            own_pixels_per_unit: own_ppu,
            nine_slice_margins: margins,
            nine: patch
                .map(|(_, margins)| margins)
                .or_else(|| margins.iter().any(|m| *m > 0.0).then_some(margins)),
        },
        explicit,
        per,
    )?;
    crate::set_color(eng, entity, crate::color_from_params(params))?;
    crate::overlay_from_params(eng, entity, params)?;
    crate::lit_2d::set_lit(eng, entity, params);
    crate::material::set_material_2d(
        eng,
        entity,
        params
            .get("material")
            .and_then(toml::Value::as_str)
            .unwrap_or_default(),
    )
}

pub(crate) fn register_sprite_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "sprite",
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "A textured 2D quad at the node, sized by `pixels_per_unit`. `columns` and `rows`, or a `sprite_sheet` in `sheet`, cut it into frames `frame` picks.",
            schema: sprite_schema(),
            tags: &[words::ORTHOGRAPHIC, "render"],
            expects: &[],
            apply: Box::new(apply_sprite),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Renderable2d>(entity);
                Ok(())
            }),
            get: Box::new(read_sprite),
        },
    );
}

/// `nine_slice_margins_pixels`: left, right, top, bottom, none below zero.
fn margins_of(params: &toml::Value) -> [f32; 4] {
    let at = |i: usize| {
        params
            .get(k::NINE_SLICE_MARGINS_PIXELS)
            .and_then(toml::Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .map_or(0.0, |v| (v as f32).max(0.0))
    };
    [at(0), at(1), at(2), at(3)]
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

fn texture_was_empty(params: &toml::Value) -> bool {
    params
        .get(k::TEXTURE)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .is_empty()
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

/// The `sprite` component's properties, read back off the node.
fn read_sprite(
    eng: &balaur_core::Engine,
    entity: balaur_core::hecs::Entity,
) -> Option<toml::Value> {
    let world = eng.world();
    let renderable = world.get::<&Renderable2d>(entity).ok()?;
    let sprite = renderable.sprite.as_ref()?;
    let Shape2d::Sprite { hx, hy } = renderable.shape else {
        return None;
    };
    let mut map = toml::map::Map::new();
    let texture = if sprite.sheet_texture {
        String::new()
    } else {
        sprite.path.clone()
    };
    map.insert(k::TEXTURE.into(), toml::Value::String(texture));
    if !sprite.sheet_asset.is_empty() {
        map.insert(
            k::SHEET.into(),
            toml::Value::String(sprite.sheet_asset.clone()),
        );
    }
    map.insert(
        k::FRAME.into(),
        toml::Value::Integer(i64::from(sprite.frame)),
    );
    map.insert(k::FLIP_X.into(), toml::Value::Boolean(sprite.flip_x));
    map.insert(
        k::OFFSET.into(),
        toml::Value::Array(vec![
            toml::Value::Float(f64::from(sprite.offset[0])),
            toml::Value::Float(f64::from(sprite.offset[1])),
        ]),
    );
    map.insert(k::CENTERED.into(), toml::Value::Boolean(sprite.centered));
    map.insert(k::FLIP_Y.into(), toml::Value::Boolean(sprite.flip_y));
    // A region the sheet chose is derived, and reporting it would pin the
    // quad to one frame the first time anything patched the component.
    if let Some([x, y, w, h]) = sprite.region.filter(|_| sprite.sheet_asset.is_empty()) {
        let pair = |a: u32, b: u32| {
            toml::Value::Array(vec![
                toml::Value::Float(f64::from(a)),
                toml::Value::Float(f64::from(b)),
            ])
        };
        map.insert(k::REGION_ORIGIN.into(), pair(x, y));
        map.insert(k::REGION_SIZE.into(), pair(w, h));
    }
    // A derived size is absent, not resolved: `patch` overlays what `get`
    // reports, so reporting it would freeze the quad the first time anything
    // read the component back.
    if renderable.sized {
        map.insert(
            k::SIZE.into(),
            toml::Value::Array(vec![
                toml::Value::Float(f64::from(hx * 2.0)),
                toml::Value::Float(f64::from(hy * 2.0)),
            ]),
        );
    }
    // As authored: 0 follows the texture, and reporting the value it
    // resolved to would pin it the first time anything patched the sprite.
    map.insert(
        k::PIXELS_PER_UNIT.into(),
        toml::Value::Float(f64::from(sprite.own_pixels_per_unit)),
    );
    map.insert(k::COLOR.into(), crate::color_to_toml(renderable.color));
    map.insert(
        "material".into(),
        toml::Value::String(renderable.material.clone()),
    );
    map.insert(
        k::NINE_SLICE_MARGINS_PIXELS.into(),
        toml::Value::Array(
            sprite
                .nine_slice_margins
                .iter()
                .map(|m| toml::Value::Float(f64::from(*m)))
                .collect(),
        ),
    );
    crate::overlay::overlay_2d_to_map(&renderable.overlay, &mut map);
    crate::lit_2d::to_map(renderable.lit.as_ref(), &mut map);
    Some(toml::Value::Table(map))
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
