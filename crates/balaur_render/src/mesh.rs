//! The `mesh` render component: which geometry a node draws.
//!
//! The geometry, its parsers and the `mesh` asset type live in
//! `balaur_core::mesh`: physics needs them too, and does not depend on this
//! crate. What lives here is the component that points a node at one.

use balaur_core::mesh::{MeshData, MESH_ASSET_TYPE};
use balaur_core::App;

/// The `mesh` component: authored geometry on a node.
///
/// Writes `Shape::Mesh` plus the asset reference, the same split `sprite` and
/// `shape2d`'s polyline use — the shape enum carries parameters, the
/// renderable carries the reference.
pub(crate) fn register_mesh_component(app: &mut App) {
    app.register_component(
        MESH_ASSET_TYPE,
        balaur_core::components::ComponentDef {
            schema: balaur_core::components::ComponentDef::parse_schema(
                MESH_ASSET_TYPE,
                r#"source = { type = "asset", asset = "mesh", default = "", description = "The mesh asset this node draws" }"#,
            ),
            tags: &["3d", "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let source = params
                    .get("source")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                // Warned, not refused: one unreadable model must not take the
                // whole scene down, and the node still has a place in the tree.
                if !source.is_empty() {
                    if let Err(why) = balaur_core::assets::load_typed::<MeshData>(eng, &source) {
                        tracing::warn!("mesh '{source}': {why:#}");
                    }
                }
                crate::set_mesh(eng, entity, source)
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
                map.insert("source".into(), toml::Value::String(source));
                Some(toml::Value::Table(map))
            }),
        },
    );
}
