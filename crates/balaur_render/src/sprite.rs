//! The `sprite` component: its schema and scene-file round trip. The data it
//! writes (`SpriteTexture` inside `Renderable2d`) lives in the crate root.

use balaur_core::components::ComponentDef;
use balaur_core::App;

use crate::{set_sprite, Renderable2d, Shape2d, SpriteSheet2d, SpriteTexture};

/// The `sprite` component: a textured 2D quad.
pub(crate) fn register_sprite_component(app: &mut App) {
    app.register_component(
        "sprite",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "sprite",
                r#"texture = { type = "string", default = "", description = "Image file, project-relative; required" }
columns = { type = "float", default = 0.0, min = 0.0, description = "Sheet grid columns for flipbook sprites; 0 means a single image" }
rows = { type = "float", default = 0.0, min = 0.0, description = "Sheet grid rows for flipbook sprites; 0 means a single image" }
frame = { type = "float", default = 0.0, min = 0.0, description = "Current sheet cell, counted left-to-right then top-to-bottom" }
flip_x = { type = "bool", default = false, description = "Mirror horizontally" }
flip_y = { type = "bool", default = false, description = "Mirror vertically" }
pixels_per_unit = { type = "float", default = 100.0, min = 0.01, description = "Texture pixels per world unit" }
half_extents = { type = "vec2", default = [0.0, 0.0], description = "Size override in world units; [0, 0] sizes from the texture" }
color = { type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }
material = { type = "asset", asset = "material", default = "", description = "The material this draws with; empty draws with the built-in one" }"#,
            ),
            tags: &["2d", "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let num = |key: &str| {
                    params
                        .get(key)
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.0)
                };
                let texture = params
                    .get("texture")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let columns = num("columns") as u32;
                let rows = num("rows") as u32;
                // A sheet needs both counts; one alone is a typo, not a grid.
                let sheet = (columns > 0 && rows > 0).then_some(SpriteSheet2d { columns, rows });
                let he = |i: usize| {
                    params
                        .get("half_extents")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(i))
                        .and_then(balaur_core::components::as_f64)
                        .unwrap_or(0.0) as f32
                };
                // Absent (or zero) half-extents mean "size it from the image".
                let explicit = (he(0) > 0.0 && he(1) > 0.0).then(|| (he(0), he(1)));
                let ppu = num("pixels_per_unit") as f32;
                set_sprite(
                    eng,
                    entity,
                    SpriteTexture {
                        path: texture,
                        sheet,
                        frame: num("frame") as u32,
                        flip_x: params.get("flip_x").and_then(toml::Value::as_bool) == Some(true),
                        flip_y: params.get("flip_y").and_then(toml::Value::as_bool) == Some(true),
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
    map.insert("texture".into(), toml::Value::String(sprite.path.clone()));
    if let Some(sheet) = sprite.sheet {
        map.insert(
            "columns".into(),
            toml::Value::Float(f64::from(sheet.columns)),
        );
        map.insert("rows".into(), toml::Value::Float(f64::from(sheet.rows)));
    }
    map.insert("frame".into(), toml::Value::Float(f64::from(sprite.frame)));
    map.insert("flip_x".into(), toml::Value::Boolean(sprite.flip_x));
    map.insert("flip_y".into(), toml::Value::Boolean(sprite.flip_y));
    // Written out resolved: a saved scene reloads to the same size even if
    // the image on disk is replaced with a different one.
    map.insert(
        "half_extents".into(),
        toml::Value::Array(vec![
            toml::Value::Float(f64::from(hx)),
            toml::Value::Float(f64::from(hy)),
        ]),
    );
    map.insert("color".into(), crate::color_to_toml(renderable.color));
    map.insert(
        "material".into(),
        toml::Value::String(renderable.material.clone()),
    );
    Some(toml::Value::Table(map))
}
