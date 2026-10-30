//! Normal-mapped 2D lighting: a `sprite` or `shape2d` whose material has no
//! shader and a `normal` map draws with kiss3d's `LitMaterial2d`, lit per
//! pixel by every `light2d`.
//!
//! Such a node lights itself, so the 2D order puts it after the light map's
//! composite, as it does particles: it draws over every node the light map
//! lit, whatever its `z_index`. kiss3d's light set is one per thread, so every
//! camera sees the same lights, and none of them casts a shadow on it.

/// What a normal-mapped node draws with.
#[cfg(feature = "window")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lit2d {
    pub(crate) normal_map: String,
    pub(crate) specular_strength: f32,
    pub(crate) shininess: f32,
    pub(crate) normal_strength: f32,
}

/// The lit settings a node's material gives it: a shader-less material
/// whose `normal` slot names a normal map, with its strengths. `None` for
/// any other material, and for none.
#[cfg(feature = "window")]
pub(crate) fn of(eng: &balaur_core::Engine, reference: &str) -> Option<Lit2d> {
    let builtin = crate::material::builtin_of(eng, reference)?;
    let slot = crate::material::TEXTURE_SLOTS
        .iter()
        .position(|name| *name == NORMAL_SLOT)?;
    let normal_map = builtin.maps.get(slot)?.clone()?;
    Some(Lit2d {
        normal_map,
        specular_strength: builtin.specular_strength,
        shininess: builtin.shininess,
        normal_strength: builtin.normal_strength,
    })
}

/// The texture slot a 2D node's normal map is read from: the 3D one's.
#[cfg(feature = "window")]
const NORMAL_SLOT: &str = "normal";

/// Whether a node lights itself and so draws after the light map.
#[cfg(feature = "window")]
pub(crate) fn lights_itself(
    eng: &balaur_core::Engine,
    world: &balaur_core::hecs::World,
    entity: balaur_core::hecs::Entity,
) -> bool {
    let Ok(renderable) = world.get::<&crate::Renderable2d>(entity) else {
        return false;
    };
    if renderable.shape == crate::Shape2d::Polygon {
        return false;
    }
    let reference = if renderable.material.is_empty() {
        world
            .get::<&balaur_core::GlobalAppearance>(entity)
            .map_or_else(
                |_| String::new(),
                |appearance| appearance.material.reference().to_string(),
            )
    } else {
        renderable.material.clone()
    };
    of(eng, &reference).is_some()
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
