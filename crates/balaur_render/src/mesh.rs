//! The `mesh` render component: which geometry a node draws.
//!
//! The geometry, its parsers and the `mesh` asset type live in
//! `balaur_core::mesh`: physics needs them too, and does not depend on this
//! crate. What lives here is the component that points a node at one.

use crate::shape::{keys as k, words};
use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_core::mesh::{MESH_ASSET_TYPE, MeshData};
use balaur_plugin::Registry;

/// The `mesh` component: authored geometry on a node.
///
/// Writes `Shape::Mesh` plus the asset reference, the same split `sprite` and
/// `shape2d`'s polyline use — the shape enum carries parameters, the
/// renderable carries the reference. A mesh whose asset carries a skin
/// deforms with the rig `skeleton` names.
/// What a morph weight's property key starts with: `morph.smile` is the
/// weight of the target a glTF called `smile`.
pub(crate) const MORPH_PREFIX: &str = "morph.";

/// How far a mesh is blended towards each of its shapes.
///
/// The names come from the asset and the weights from the component's
/// params, so a clip track driving `mesh/morph.smile` writes one number and
/// leaves the others where they were.
pub struct MorphWeights {
    pub names: Vec<String>,
    pub weights: Vec<f32>,
    /// Bumped when a weight changes, so a backend knows to push them.
    pub version: u64,
}

/// Read the weights a params table names and put them on the node, in the
/// order the asset carries its targets.
///
/// A mesh with no shapes to blend gets no component at all, and a name the
/// asset does not carry is ignored rather than refused: a clip written for
/// one model should not stop another from loading.
fn set_morph_weights(eng: &Engine, entity: Entity, source: &str, params: &toml::Value) {
    let names: Vec<String> = balaur_core::assets::load_typed::<MeshData>(eng, source)
        .and_then(|definition| balaur_core::mesh::load_from(eng, &definition))
        .map(|mesh| mesh.morphs.into_iter().map(|target| target.name).collect())
        .unwrap_or_default();
    let mut world = eng.world_mut();
    if names.is_empty() {
        let _ = world.remove_one::<MorphWeights>(entity);
        return;
    }
    let weights: Vec<f32> = names
        .iter()
        .map(|name| {
            params
                .get(format!("{MORPH_PREFIX}{name}").as_str())
                .and_then(balaur_core::components::as_f64)
                .unwrap_or(0.0) as f32
        })
        .collect();
    if let Ok(mut existing) = world.get::<&mut MorphWeights>(entity) {
        if existing.names != names || existing.weights != weights {
            existing.names = names;
            existing.weights = weights;
            existing.version += 1;
        }
        return;
    }
    let _ = world.insert_one(
        entity,
        MorphWeights {
            names,
            weights,
            version: 0,
        },
    );
}

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
                crate::set_mesh(eng, entity, source.clone(), text(k::SKELETON), text(k::TEXTURE))?;
                set_morph_weights(eng, entity, &source, params);
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
                // One key per shape the mesh can blend towards, so a clip
                // track spells `mesh/morph.smile` and a patch keeps the rest.
                if let Ok(morphs) = world.get::<&MorphWeights>(entity) {
                    for (name, weight) in morphs.names.iter().zip(&morphs.weights) {
                        map.insert(
                            format!("{MORPH_PREFIX}{name}"),
                            toml::Value::Float(f64::from(*weight)),
                        );
                    }
                }
                Some(toml::Value::Table(map))
            }),
        },
    );
}
