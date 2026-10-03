//! Normal-mapped 2D lighting: a `sprite` or `shape2d` naming a `normal_map`
//! draws with kiss3d's `LitMaterial2d`, lit per pixel by every `light2d`.
//!
//! Such a node lights itself, so the 2D order puts it after the light map's
//! composite, as it does particles: it draws over every node the light map
//! lit, whatever its `z_index`. kiss3d's light set is one per thread, so every
//! camera sees the same lights, and none of them casts a shadow on it.

use crate::vocabulary::keys as k;
use balaur_core::components::{prop_f32, prop_str};

/// What a normal-mapped node draws with.
#[derive(Clone, Debug, PartialEq)]
pub struct Lit2d {
    pub normal_map: String,
    pub specular_strength: f32,
    pub shininess: f32,
    pub normal_strength: f32,
}

/// The rows a drawable that `LitMaterial2d` can draw appends to its schema.
pub(crate) fn rows() -> Vec<(&'static str, String)> {
    vec![
        (
            k::NORMAL_MAP,
            format!(
                r#"{{ type = "asset", asset = "{}", default = "", description = "A tangent-space normal map: the node then draws lit per pixel by every `light2d`, after the light map and over what it lit, with no shadow. Empty leaves it to the light map. Give the image `srgb = false`" }}"#,
                balaur_core::texture_asset::TEXTURE_ASSET_TYPE
            ),
        ),
        (
            k::SPECULAR_STRENGTH,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "How bright a light's highlight is on a normal-mapped node; zero draws none" }"#.to_string(),
        ),
        (
            k::SHININESS,
            r#"{ type = "float", default = 16.0, min = 1.0, description = "How tight the highlight is on a normal-mapped node; higher is smaller and sharper" }"#.to_string(),
        ),
        (
            k::NORMAL_STRENGTH,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "How far the normal map bends the surface; zero lights it flat" }"#.to_string(),
        ),
    ]
}

/// The lit settings `params` asks for; `None` when it names no normal map.
pub(crate) fn from_params(params: &toml::Value) -> Option<Lit2d> {
    let normal_map = prop_str(params, k::NORMAL_MAP);
    (!normal_map.is_empty()).then(|| Lit2d {
        normal_map: normal_map.to_string(),
        specular_strength: prop_f32(params, k::SPECULAR_STRENGTH).max(0.0),
        shininess: prop_f32(params, k::SHININESS).max(1.0),
        normal_strength: prop_f32(params, k::NORMAL_STRENGTH).max(0.0),
    })
}

/// Put the lit settings on the node's renderable; a change rebuilds its node,
/// because it moves the node onto another material.
pub(crate) fn set_lit(
    eng: &balaur_core::Engine,
    entity: balaur_core::hecs::Entity,
    params: &toml::Value,
) {
    let next = from_params(params);
    let world = eng.world_mut();
    if let Ok(mut r) = world.get::<&mut crate::Renderable2d>(entity)
        && r.lit != next
    {
        r.lit = next;
        r.version += 1;
    }
}

/// The lit rows read back.
pub(crate) fn to_map(lit: Option<&Lit2d>, map: &mut toml::map::Map<String, toml::Value>) {
    let fallback = Lit2d {
        normal_map: String::new(),
        specular_strength: 0.0,
        shininess: 16.0,
        normal_strength: 1.0,
    };
    let lit = lit.unwrap_or(&fallback);
    map.insert(
        k::NORMAL_MAP.into(),
        toml::Value::String(lit.normal_map.clone()),
    );
    for (key, value) in [
        (k::SPECULAR_STRENGTH, lit.specular_strength),
        (k::SHININESS, lit.shininess),
        (k::NORMAL_STRENGTH, lit.normal_strength),
    ] {
        map.insert(key.into(), toml::Value::Float(f64::from(value)));
    }
}

/// Whether a node lights itself and so draws after the light map.
#[cfg(feature = "window")]
pub(crate) fn lights_itself(
    world: &balaur_core::hecs::World,
    entity: balaur_core::hecs::Entity,
) -> bool {
    world
        .get::<&crate::Renderable2d>(entity)
        .is_ok_and(|r| r.lit.is_some() && r.shape != crate::Shape2d::Polygon)
}

/// Move a freshly built node onto `LitMaterial2d`, with its normal map.
#[cfg(feature = "window")]
pub(crate) fn dress(app: &balaur_core::App, node: &mut kiss3d::scene::SceneNode2d, lit: &Lit2d) {
    node.set_material(kiss3d::builtin::LitMaterial2d::shared());
    let normal = crate::texture::upload(
        &app.engine,
        &lit.normal_map,
        crate::texture::PREMULTIPLY_DROPPED,
    );
    let params = kiss3d::builtin::LitParams::default()
        .with_specular(lit.specular_strength, lit.shininess)
        .with_normal_strength(lit.normal_strength);
    node.apply_to_objects_mut_recursive(&mut |object| {
        object.set_normal_map(normal.clone());
        object.set_lit_params(Some(params));
    });
}

/// Hand kiss3d's light set this frame's `light2d`s. With none, the ambient
/// is white, so a normal-mapped node draws as an unlit scene does.
#[cfg(feature = "window")]
pub(crate) fn feed_lights(app: &balaur_core::App) {
    use kiss3d::color::Color;
    use kiss3d::light2d::{Light2d, Light2dManager};

    let world = app.engine.world();
    let root = app.engine.root();
    let mut lights: Vec<Light2d> = Vec::new();
    for entity in balaur_core::scene::collect_subtree(&world, root) {
        let (Ok(light), Ok(global)) = (
            world.get::<&crate::light::Light2d>(entity),
            world.get::<&balaur_core::GlobalTransform>(entity),
        ) else {
            continue;
        };
        let [r, g, b, _] = light.color;
        let color = Color::new(r, g, b, 1.0);
        let at = glamx::Vec2::new(global.position.x, global.position.y);
        let aimed = global.rotation * glamx::Vec3::NEG_Y;
        let aim = glamx::Vec2::new(aimed.x, aimed.y);
        let made = match light.kind {
            crate::light::LightKind2d::Point => {
                Light2d::point(at, color, light.intensity, light.radius)
            }
            crate::light::LightKind2d::Spot => Light2d::spot(
                at,
                aim,
                color,
                light.intensity,
                light.radius,
                light.inner_angle_degrees.to_radians(),
                light.outer_angle_degrees.to_radians(),
            ),
            crate::light::LightKind2d::Directional => {
                Light2d::directional(aim, color, light.intensity)
            }
        };
        lights.push(made.with_height(light.height));
    }
    let ambient = if lights.is_empty() {
        [1.0; 3]
    } else {
        app.engine
            .try_resource::<crate::CameraConfig2d>()
            .map_or([0.0; 3], |config| config.borrow().ambient)
    };
    Light2dManager::get_global_manager(|manager| {
        manager.set_lights(&lights);
        manager.set_ambient(Color::new(ambient[0], ambient[1], ambient[2], 1.0));
    });
}
