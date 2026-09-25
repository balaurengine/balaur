//! The `mesh` render component: which geometry a node draws.
//!
//! The geometry, its parsers and the `mesh` asset type live in
//! `balaur_core::mesh`: physics needs them too, and does not depend on this
//! crate. What lives here is the component that points a node at one.

use crate::vocabulary::{keys as k, words};
use crate::{Bounds3d, Renderable3d, Shape3d};
use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_core::mesh::{MESH_ASSET_TYPE, MeshData, SolvedMesh};
use balaur_plugin::Registry;

/// The `mesh` component: authored geometry on a node.
///
/// Writes `Shape3d::Mesh` plus the asset reference, the same split `sprite` and
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
    // Same stage as the other renderable builders: a solver's own geometry
    // becomes its node's renderable once the scene has settled.
    reg.add_system(balaur_core::Stage::SceneSync, resolve_solved_system);
    reg.register_component(
        MESH_ASSET_TYPE,
        balaur_core::components::ComponentDef {
            doc: "3D geometry from the `mesh` asset in `source`, drawn at the node. With a skin, the rig `skeleton` names deforms it.",
            schema: balaur_core::components::ComponentDef::parse_schema(
                MESH_ASSET_TYPE,
                &balaur_core::components::ComponentDef::schema(&[
                    (k::SOURCE, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The mesh asset this node draws" }}"#, balaur_core::mesh::MESH_ASSET_TYPE)),
                    (k::SKELETON, r#"{ type = "string", default = "", description = "Node path to the rig a skinned mesh deforms with, relative to this node; empty means this node" }"#),
                    (k::TEXTURE, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "Image file, project-relative, or a `texture` asset; empty draws the colour alone" }}"#, balaur_core::texture_asset::TEXTURE_ASSET_TYPE)),
                    (k::MATERIAL, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#, crate::material::MATERIAL_ASSET_TYPE)),
                    (k::SHADOWS, r#"{ type = "bool", default = true, description = "Whether this casts a shadow from the lights that cast" }"#),
                    (k::LAYERS, r#"{ type = "int", default = -1, description = "Light-layer bitmask; a `light3d` lights this when their masks share a bit. -1 is every layer" }"#),
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
                // Warned, not refused, and resolved the whole way: one bad model
                // must not take the scene down, and a headless run should hear of it.
                if !source.is_empty()
                    && let Err(why) = balaur_core::assets::load_typed::<MeshData>(eng, &source)
                        .and_then(|definition| balaur_core::mesh::load_from(eng, &definition))
                {
                    tracing::warn!("mesh '{source}': {why:#}");
                }
                crate::set_mesh(eng, entity, source.clone(), text(k::SKELETON), text(k::TEXTURE))?;
                set_morph_weights(eng, entity, &source, params);
                crate::lighting_from_params(eng, entity, params);
                crate::material::set_material_3d(eng, entity, &text("material"))
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<crate::Renderable3d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let renderable = world.get::<&crate::Renderable3d>(entity).ok()?;
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
                map.insert(k::SHADOWS.into(), toml::Value::Boolean(renderable.shadows));
                map.insert(
                    k::LAYERS.into(),
                    toml::Value::Integer(i64::from(renderable.layers.cast_signed())),
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

// A soft body built from a mesh is drawn as that mesh, deformed. One laid out
// by a generator — a cloth, a cuboid, a rope — has no asset to be drawn as at
// all, so the geometry it reports each step is its only geometry.

/// Give every solver-owned node without a renderable one built from what the
/// solver last reported, and rebuild it when the topology changes.
///
/// Only the topology: the positions move every step and the backend uploads
/// those itself, so a version bump per step would rebuild the node every
/// frame instead of rewriting its buffers.
pub(crate) fn resolve_solved_system(eng: &Engine, _dt: f32) {
    let mut wanted: Vec<(Entity, MeshData, u32)> = Vec::new();
    {
        let world = eng.world();
        for (entity, solved) in &mut world.query::<(Entity, &SolvedMesh)>() {
            if solved.positions.is_empty() || solved.indices.is_empty() {
                continue;
            }
            // A node that draws something of its own keeps drawing it; the
            // solver deforms that instead of replacing it.
            if let Ok(renderable) = world.get::<&Renderable3d>(entity)
                && !is_ours(&renderable)
            {
                continue;
            }
            if world
                .get::<&Renderable3d>(entity)
                .is_ok_and(|r| r.version == u64::from(solved.topology))
            {
                continue;
            }
            wanted.push((entity, mesh_of(solved), solved.topology));
        }
    }
    for (entity, mesh, topology) in wanted {
        install(eng, entity, mesh, topology);
    }
}

/// Whether a renderable is one of ours to replace: `Built` geometry with no
/// asset behind it is what this module makes, and nothing else does on a node
/// the solver owns.
fn is_ours(renderable: &Renderable3d) -> bool {
    renderable.shape == Shape3d::Built && renderable.mesh.is_none()
}

fn mesh_of(solved: &SolvedMesh) -> MeshData {
    MeshData {
        positions: solved.positions.clone(),
        indices: solved.indices.clone(),
        ..MeshData::default()
    }
}

fn install(eng: &Engine, entity: Entity, mesh: MeshData, topology: u32) {
    let bounds = mesh.bounds().map(|(min, max)| {
        let (min, max) = (glamx::Vec3::from_array(min), glamx::Vec3::from_array(max));
        Bounds3d {
            centre: (min + max) / 2.0,
            half: (max - min) / 2.0,
        }
    });
    let built = Some(std::sync::Arc::new(mesh));
    let mut world = eng.world_mut();
    // The version is the topology, so the backend rebuilds on a tear and on
    // nothing else (see the module docs).
    let version = u64::from(topology);
    if let Ok(mut renderable) = world.get::<&mut Renderable3d>(entity) {
        renderable.shape = Shape3d::Built;
        renderable.built = built;
        renderable.bounds = bounds;
        renderable.version = version;
        return;
    }
    let _ = world.insert_one(
        entity,
        Renderable3d {
            shape: Shape3d::Built,
            bounds,
            color: [0.8, 0.8, 0.8, 1.0],
            mesh: None,
            built,
            skeleton: String::new(),
            texture: String::new(),
            material: String::new(),
            shadows: true,
            layers: u32::MAX,
            version,
        },
    );
}
