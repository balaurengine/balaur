//! The `sprite` component: its schema and scene-file round trip. The data it
//! writes (`SpriteTexture` inside `Renderable2d`) lives in the crate root.

use balaur_core::components::ComponentDef;
use balaur_plugin::Registry;

use crate::shape::{keys as k, words};
use crate::{Renderable2d, Shape2d, SpriteSheet2d, SpriteTexture, set_sprite};

/// The `sprite` component's property schema, lifted out so the
/// registration below stays readable.
fn sprite_schema() -> toml::Value {
    ComponentDef::parse_schema(
        "sprite",
        &balaur_core::components::ComponentDef::schema(&[
            (
                k::TEXTURE,
                r#"{ type = "string", default = "", description = "Image file, project-relative; required" }"#,
            ),
            (
                k::COLUMNS,
                r#"{ type = "float", default = 0.0, min = 0.0, description = "Sheet grid columns for flipbook sprites; 0 means a single image" }"#,
            ),
            (
                k::ROWS,
                r#"{ type = "float", default = 0.0, min = 0.0, description = "Sheet grid rows for flipbook sprites; 0 means a single image" }"#,
            ),
            (
                k::FRAME,
                r#"{ type = "float", default = 0.0, min = 0.0, description = "Current sheet cell, counted left-to-right then top-to-bottom" }"#,
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
                r#"{ type = "float", default = 100.0, min = 0.01, description = "Texture pixels per world unit" }"#,
            ),
            (
                k::HALF_EXTENTS,
                r#"{ type = "vec2", default = [0.0, 0.0], description = "Size override in world units; [0, 0] sizes from the texture" }"#,
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
                r#"{ type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#,
            ),
            (
                k::MATERIAL,
                &format!(
                    r#"{{ type = "asset", asset = "{}", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#,
                    crate::material::MATERIAL_ASSET_TYPE
                ),
            ),
        ]),
    )
}

/// The `sprite` component: a textured 2D quad.
pub(crate) fn register_sprite_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "sprite",
        ComponentDef {
            doc: "A textured 2D quad at the node, sized from its image at `pixels_per_unit` texture pixels per world unit. A `columns` x `rows` sheet makes it a flipbook `frame` steps through.",
            schema: sprite_schema(),
            tags: &[words::ORTHOGRAPHIC, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let num = |key: &str| {
                    params
                        .get(key)
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.0)
                };
                let texture = params
                    .get(k::TEXTURE)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let columns = num(k::COLUMNS) as u32;
                let rows = num(k::ROWS) as u32;
                // A sheet needs both counts; one alone is a typo, not a grid.
                let sheet = (columns > 0 && rows > 0).then_some(SpriteSheet2d { columns, rows });
                let he = |i: usize| {
                    params
                        .get(k::HALF_EXTENTS)
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.0) as f32
                };
                // Absent (or zero) half-extents mean "size it from the image".
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
                let region = (rw > 0.0 && rh > 0.0).then(|| {
                    [
                        pair(k::REGION_ORIGIN, 0).max(0.0).round() as u32,
                        pair(k::REGION_ORIGIN, 1).max(0.0).round() as u32,
                        rw.round() as u32,
                        rh.round() as u32,
                    ]
                });
                let ppu = num(k::PIXELS_PER_UNIT) as f32;
                set_sprite(
                    eng,
                    entity,
                    SpriteTexture {
                        path: texture,
                        sheet,
                        frame: num(k::FRAME) as u32,
                        flip_x: params.get(k::FLIP_X).and_then(toml::Value::as_bool) == Some(true),
                        flip_y: params.get(k::FLIP_Y).and_then(toml::Value::as_bool) == Some(true),
                        region,
                    },
                    explicit,
                    if ppu > 0.0 {
                        ppu
                    } else {
                        crate::DEFAULT_PIXELS_PER_UNIT
                    },
                )?;
                crate::set_color(eng, entity, crate::color_from_params(params))?;
                crate::material::set_material_2d(
                    eng,
                    entity,
                    params
                        .get("material")
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default(),
                )
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Renderable2d>(entity);
                Ok(())
            }),
            get: Box::new(read_sprite),
        },
    );
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
    map.insert(k::TEXTURE.into(), toml::Value::String(sprite.path.clone()));
    if let Some(sheet) = sprite.sheet {
        map.insert(
            k::COLUMNS.into(),
            toml::Value::Float(f64::from(sheet.columns)),
        );
        map.insert(k::ROWS.into(), toml::Value::Float(f64::from(sheet.rows)));
    }
    map.insert(k::FRAME.into(), toml::Value::Float(f64::from(sprite.frame)));
    map.insert(k::FLIP_X.into(), toml::Value::Boolean(sprite.flip_x));
    map.insert(k::FLIP_Y.into(), toml::Value::Boolean(sprite.flip_y));
    if let Some([x, y, w, h]) = sprite.region {
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
            k::HALF_EXTENTS.into(),
            toml::Value::Array(vec![
                toml::Value::Float(f64::from(hx)),
                toml::Value::Float(f64::from(hy)),
            ]),
        );
    }
    map.insert(k::COLOR.into(), crate::color_to_toml(renderable.color));
    map.insert(
        "material".into(),
        toml::Value::String(renderable.material.clone()),
    );
    Some(toml::Value::Table(map))
}
