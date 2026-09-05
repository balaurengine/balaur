//! The `mesh` render component: which geometry a node draws.
//!
//! The geometry, its parsers and the `mesh` asset type live in
//! `balaur_core::mesh`: physics needs them too, and does not depend on this
//! crate. What lives here is the component that points a node at one.

use crate::shape::{keys as k, words};
use balaur_core::mesh::{MESH_ASSET_TYPE, MeshData};
use balaur_plugin::Registry;

/// The `mesh` component: authored geometry on a node.
///
/// Writes `Shape::Mesh` plus the asset reference, the same split `sprite` and
/// `shape2d`'s polyline use — the shape enum carries parameters, the
/// renderable carries the reference. A mesh whose asset carries a skin
/// deforms with the rig `skeleton` names.
pub(crate) fn register_mesh_component(reg: &mut Registry<'_>) {
    reg.register_component(
        MESH_ASSET_TYPE,
        balaur_core::components::ComponentDef {
            doc: "Authored 3D geometry from a `mesh` asset, drawn at the node and deformed by the rig `skeleton` names when the asset carries a skin.",
            schema: balaur_core::components::ComponentDef::parse_schema(
                MESH_ASSET_TYPE,
                &balaur_core::components::ComponentDef::schema(&[
                    (k::SOURCE, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The mesh asset this node draws" }}"#, balaur_core::mesh::MESH_ASSET_TYPE)),
                    (k::SKELETON, r#"{ type = "string", default = "", description = "Node path to the rig a skinned mesh deforms with, relative to this node; empty means this node" }"#),
                    (k::TEXTURE, r#"{ type = "string", default = "", description = "Image file, project-relative; empty draws the colour alone" }"#),
                    (k::MATERIAL, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#, crate::material::MATERIAL_ASSET_TYPE)),
                ]),
            ),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let text = |key: &str| {
                    params
                        .get(key)
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                let source = text(k::SOURCE);
                // Warned, not refused: one unreadable model must not take the
                // whole scene down, and the node still has a place in the tree.
                if !source.is_empty()
                    && let Err(why) = balaur_core::assets::load_typed::<MeshData>(eng, &source) {
                        tracing::warn!("mesh '{source}': {why:#}");
                    }
                crate::set_mesh(eng, entity, source, text(k::SKELETON), text(k::TEXTURE))?;
                crate::material::set_material_3d(eng, entity, &text("material"))
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<crate::Renderable>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let renderable = world.get::<&crate::Renderable>(entity).ok()?;
                let source = renderable.mesh.clone()?;
                let mut map = toml::map::Map::new();
                map.insert(k::SOURCE.into(), toml::Value::String(source));
                map.insert(
                    k::SKELETON.into(),
                    toml::Value::String(renderable.skeleton.clone()),
                );
                map.insert(
                    k::TEXTURE.into(),
                    toml::Value::String(renderable.texture.clone()),
                );
                map.insert(
                    "material".into(),
                    toml::Value::String(renderable.material.clone()),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
}
