//! A material with no `shader`: its values set on kiss3d's own material,
//! which reads every one of them.

use kiss3d::color::Color;
use kiss3d::scene::{ParallaxMethod, SceneNode3d};

use crate::material::{Builtin, Parallax, View};

/// Put a shader-less material on a node, or take one off: `None` puts back
/// what a node with no material draws with. `was` is what the node last
/// took, so a debug view it showed is taken away again.
pub(crate) fn apply(
    app: &balaur_core::App,
    node: &mut SceneNode3d,
    builtin: Option<&Builtin>,
    was: Option<&Builtin>,
) {
    let plain = Builtin::default();
    let b = builtin.unwrap_or(&plain);
    let color = |[r, g, b, a]: [f32; 4]| Color::new(r, g, b, a);
    node.set_metallic(b.metallic);
    node.set_roughness(b.roughness);
    node.set_emissive(color(b.emission_color));
    node.set_specular_tint(color(b.specular_tint));
    node.set_reflectance(b.reflectance);
    node.set_clearcoat(b.clearcoat, b.clearcoat_roughness);
    node.set_anisotropy(b.anisotropy, b.anisotropy_rotation_degrees.to_radians());
    node.set_subsurface(b.subsurface, b.subsurface_radius);
    node.set_parallax_scale(b.parallax_scale);
    node.set_parallax_layers(b.parallax_layers);
    node.set_parallax_method(match b.parallax_method {
        Parallax::Occlusion => ParallaxMethod::Occlusion,
        Parallax::Relief => ParallaxMethod::Relief {
            max_steps: b.parallax_relief_steps,
        },
    });
    maps(app, node, &b.maps);
    match b.view {
        Some(View::Normals) => {
            node.set_material_with_name(crate::vocabulary::words::AOV_NORMALS);
        }
        Some(View::Uvs) => {
            node.set_material_with_name(crate::vocabulary::words::UVS);
        }
        None if was.is_some_and(|old| old.view.is_some()) => {
            node.set_material_with_name(OBJECT_MATERIAL);
        }
        None => {}
    }
}

/// What kiss3d's material manager calls its own surface material.
const OBJECT_MATERIAL: &str = "object";

/// Bind each map slot's image, or clear the slot. The albedo slot is the
/// node's texture, which a material's own image replaces.
fn maps(app: &balaur_core::App, node: &mut SceneNode3d, maps: &[Option<String>]) {
    let upload = |slot: usize| {
        maps.get(slot).and_then(Option::as_deref).and_then(|path| {
            crate::texture::upload(&app.engine, path, crate::texture::PREMULTIPLY_DROPPED)
        })
    };
    if let Some(albedo) = upload(0) {
        node.set_texture(albedo);
    }
    match upload(1) {
        Some(map) => node.set_normal_map(map),
        None => node.clear_normal_map(),
    };
    match upload(2) {
        Some(map) => node.set_metallic_roughness_map(map),
        None => node.clear_metallic_roughness_map(),
    };
    match upload(3) {
        Some(map) => node.set_ao_map(map),
        None => node.clear_ao_map(),
    };
    match upload(4) {
        Some(map) => node.set_emissive_map(map),
        None => node.clear_emissive_map(),
    };
    match upload(5) {
        Some(map) => node.set_height_map(map),
        None => node.clear_height_map(),
    };
}
