//! `render.stats`: what one frame costs, per node and in total.
//!
//! Measured from the scene the renderer actually walks, so a node that draws
//! nothing costs nothing here. Presentation, never simulation: nothing in the
//! digest reads it, and a headless run reports what it can count without a
//! GPU — the triangles and the images, which are the numbers that decide a
//! pack's size anyway.

use std::collections::BTreeMap;

use balaur_core::collections::DetHashMap;
use balaur_core::primitive::Solid;
use balaur_script::{Bindings, BindingsExt as _, Value};

use crate::Engine;

/// One node's cost this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeCost {
    /// Draw calls: one per node, plus one per pass a material asks for.
    pub draws: u32,
    pub triangles: u32,
    /// Bytes of image the node's textures occupy, uncompressed on the GPU.
    pub texture_bytes: u64,
    /// How many copies a `cloner` above it draws, or one.
    pub copies: u32,
}

/// The frame's cost, by node path.
///
/// A map rather than a list, so the editor's dock can sort by whichever
/// column it likes and a name is what a row is addressed by.
#[derive(Default)]
pub struct Stats {
    pub by_node: BTreeMap<String, NodeCost>,
    /// Bytes of image uploaded, counted once per distinct texture rather than
    /// once per node that names it.
    pub texture_bytes: u64,
    /// Distinct images uploaded.
    pub textures: u32,
}

impl Stats {
    #[must_use]
    pub fn total(&self) -> NodeCost {
        let mut out = NodeCost::default();
        for cost in self.by_node.values() {
            out.draws += cost.draws;
            out.triangles += cost.triangles;
            out.copies += cost.copies;
        }
        out.texture_bytes = self.texture_bytes;
        out
    }
}

/// What counting a node cost last time, so a still frame counts nothing
/// twice.
///
/// Every answer here is the whole asset: an image is read to be measured and a
/// primitive is spun into triangles to be counted. Both are the same until an
/// asset reloads, and this pass runs every frame over every node.
#[derive(Default)]
pub(crate) struct Measured {
    /// The asset generation these answers were read at.
    generation: u64,
    /// Bytes an image occupies on the GPU, by project-relative path.
    images: DetHashMap<String, u64>,
    /// Triangles a `mesh` asset holds, by reference.
    meshes: DetHashMap<String, u32>,
    /// Triangles a primitive spins into. A list because a `Solid` holds
    /// floats and so is compared rather than hashed; a scene has a handful.
    solids: Vec<(Solid, u32)>,
    /// The subtree walked this frame, kept so the walk allocates nothing.
    subtree: Vec<balaur_core::hecs::Entity>,
}

/// Count what the scene would draw, once per frame.
///
/// Walks the same components the backend syncs from, so the numbers are the
/// scene's rather than the backend's bookkeeping; a build with no renderer
/// still reports them, which is what lets an export budget be a test.
pub fn measure_system(eng: &Engine, _dt: f32) {
    let mut fresh = Stats::default();
    let mut seen: BTreeMap<String, u64> = BTreeMap::new();
    let cache = eng.resource::<Measured>();
    let mut cache = cache.borrow_mut();
    let generation = balaur_core::assets::generation(eng);
    if cache.generation != generation {
        cache.generation = generation;
        cache.images.clear();
        cache.meshes.clear();
    }
    {
        let world = eng.world();
        let mut subtree = std::mem::take(&mut cache.subtree);
        subtree.clear();
        balaur_core::scene::collect_subtree_into(&world, eng.root(), &mut subtree);
        for &entity in &subtree {
            let visible = world
                .get::<&balaur_core::GlobalAppearance>(entity)
                .is_ok_and(|a| a.visible);
            if !visible {
                continue;
            }
            let copies = world
                .get::<&crate::Clones>(entity)
                .map_or(1, |clones| clones.0.len().max(1) as u32);
            let mut cost = NodeCost {
                copies,
                ..NodeCost::default()
            };
            if let Ok(renderable) = world.get::<&crate::Renderable>(entity) {
                cost.draws += 1;
                cost.triangles += triangles_3d(eng, &renderable, &mut cache) * copies;
                if !renderable.texture.is_empty() {
                    count_image(eng, &mut cache, &mut seen, &mut cost, &renderable.texture);
                }
            }
            if let Ok(renderable) = world.get::<&crate::Renderable2d>(entity) {
                cost.draws += 1;
                // Two triangles a quad, which is what every 2D shape is drawn
                // as once it is triangulated.
                cost.triangles += 2 * copies;
                if let Some(sprite) = &renderable.sprite {
                    count_image(eng, &mut cache, &mut seen, &mut cost, &sprite.path);
                }
            }
            if cost.draws == 0 {
                continue;
            }
            let name = balaur_core::scene::node_path(&world, entity);
            fresh.by_node.insert(name, cost);
        }
        cache.subtree = subtree;
    }
    fresh.textures = u32::try_from(seen.len()).unwrap_or(u32::MAX);
    fresh.texture_bytes = seen.values().sum();
    *eng.resource::<Stats>().borrow_mut() = fresh;
}

/// Add one image to a node's cost and to the frame's distinct set.
fn count_image(
    eng: &Engine,
    cache: &mut Measured,
    seen: &mut BTreeMap<String, u64>,
    cost: &mut NodeCost,
    path: &str,
) {
    let bytes = if let Some(bytes) = cache.images.get(path) {
        *bytes
    } else {
        // A zero is not remembered: it means the file did not read, and the
        // next frame is when it may.
        let bytes = image_bytes(eng, path);
        if bytes > 0 {
            cache.images.insert(path.to_string(), bytes);
        }
        bytes
    };
    cost.texture_bytes += bytes;
    if !seen.contains_key(path) {
        seen.insert(path.to_string(), bytes);
    }
}

/// A 3D renderable's triangle count, from the geometry it names.
fn triangles_3d(eng: &Engine, renderable: &crate::Renderable, cache: &mut Measured) -> u32 {
    if let Some(built) = &renderable.built {
        return u32::try_from(built.indices.len() / 3).unwrap_or(u32::MAX);
    }
    if let Some(name) = &renderable.mesh
        && !name.is_empty()
    {
        if let Some(count) = cache.meshes.get(name.as_str()) {
            return *count;
        }
        if let Ok(definition) =
            balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, name)
            && let Ok(data) = balaur_core::mesh::load_from(eng, &definition)
        {
            let count = u32::try_from(data.indices.len() / 3).unwrap_or(u32::MAX);
            cache.meshes.insert(name.clone(), count);
            return count;
        }
    }
    match renderable.shape.solid() {
        Some(solid) => {
            if let Some((_, count)) = cache.solids.iter().find(|(s, _)| *s == solid) {
                return *count;
            }
            let count = u32::try_from(solid.build().indices.len() / 3).unwrap_or(u32::MAX);
            cache.solids.push((solid, count));
            count
        }
        None => 0,
    }
}

/// Four bytes a pixel, which is what every format the engine uploads becomes.
fn image_bytes(eng: &Engine, path: &str) -> u64 {
    let files = eng.resource::<balaur_core::project::ProjectFiles>();
    let read = files.borrow().read(path);
    let Ok(bytes) = read else {
        return 0;
    };
    match crate::texture::image_size(&bytes, path) {
        Ok((width, height)) => u64::from(width) * u64::from(height) * 4,
        Err(_) => 0,
    }
}

fn cost_value(name: &str, cost: &NodeCost) -> Value {
    Value::Map(vec![
        ("node".into(), Value::Str(name.to_string())),
        ("draws".into(), Value::Num(f64::from(cost.draws))),
        ("triangles".into(), Value::Num(f64::from(cost.triangles))),
        (
            "texture_bytes".into(),
            Value::Num(cost.texture_bytes as f64),
        ),
        ("copies".into(), Value::Num(f64::from(cost.copies))),
    ])
}

/// `render.stats`: the frame's totals and a row per node that drew.
pub(crate) fn install_stats_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "stats",
        &[],
        "()",
        "What this frame draws: `{ draws, triangles, texture_bytes, textures, nodes }`, where `nodes` is a row per node that drew, each `{ node, draws, triangles, texture_bytes, copies }`. Presentation, never simulation: nothing in the digest reads it.",
    )]);
    m.function("stats", |eng: &Engine, ()| {
        let stats = eng.resource::<Stats>();
        let stats = stats.borrow();
        let total = stats.total();
        Ok(Value::Map(vec![
            ("draws".into(), Value::Num(f64::from(total.draws))),
            ("triangles".into(), Value::Num(f64::from(total.triangles))),
            (
                "texture_bytes".into(),
                Value::Num(stats.texture_bytes as f64),
            ),
            ("textures".into(), Value::Num(f64::from(stats.textures))),
            (
                "nodes".into(),
                Value::List(
                    stats
                        .by_node
                        .iter()
                        .map(|(name, cost)| cost_value(name, cost))
                        .collect(),
                ),
            ),
        ]))
    });
}
