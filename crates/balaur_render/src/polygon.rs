//! The `polygon` component: a filled, textured 2D polygon, deformed by a
//! rig when its mesh carries skin weights. Writes `Shape2d::Polygon` plus a
//! [`PolygonMesh`] into `Renderable2d`, the split `sprite` uses.
//!
//! The geometry is resolved here, headless: the mesh asset is loaded, the
//! default UVs are derived from the texture header, and the result is what a
//! script reads back and what the backend uploads. Nothing GPU-shaped is
//! decided at draw time.

use balaur_plugin::Registry;
use std::sync::Arc;

use anyhow::Result;
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Written};
use balaur_core::hecs::Entity;
use balaur_geometry::mesh::MeshSkin;
use glamx::Vec2;

use crate::vocabulary::words;
use crate::{Renderable2d, set_color, set_polygon};

/// A filled, textured 2D polygon from the `mesh` asset's points and triangles. With skin weights, the rig `skeleton` names deforms it.
// The node draws the [`PolygonMesh`] each write resolves into its
// [`Renderable2d`].
#[derive(Clone, Component)]
#[component(name = "polygon", on_remove = polygon_removed)]
pub struct Polygon {
    /// Vertices, triangulation, UVs and skin weights; positions are [x, y] in the node's space
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Image file, project-relative, or a `texture` asset; empty draws the tint alone
    #[property(asset = "texture")]
    pub texture: String,
    /// Tint, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub color: [f32; 4],
    /// The material this draws with; empty takes an inherited `material` component, else the built-in one
    #[property(asset = "material")]
    pub material: String,
    /// The rig root; empty means this node
    #[property(node)]
    pub skeleton: String,
    /// Texture pixels per world unit, for the default UV mapping
    #[property(default = 100.0, min = 0.01, group = "placement")]
    pub pixels_per_unit: f32,
}

/// What a `Shape2d::Polygon` draws: resolved geometry, its texture, and the
/// rig it deforms with.
#[derive(Clone, Debug, PartialEq)]
pub struct PolygonMesh {
    /// The `mesh` asset reference the component was given.
    pub mesh: String,
    /// Project-relative image path; empty draws the tint alone.
    pub texture: String,
    /// Node path to the rig root, relative to the polygon's node; empty
    /// means the polygon's own node.
    pub skeleton: String,
    pub pixels_per_unit: f32,
    /// Authored positions, in the node's local space.
    pub positions: Vec<Vec2>,
    pub indices: Vec<[u32; 3]>,
    /// One per position: the mesh's own, or derived from the texture.
    pub uvs: Vec<Vec2>,
    /// Bone influences, when the mesh has them.
    pub skin: Option<MeshSkin>,
}

impl PolygonMesh {
    /// The default UV of a point: the texture centred on the node's origin
    /// at `pixels_per_unit`, v downward — so a polygon traced over a sprite
    /// shows that sprite undistorted.
    #[must_use]
    pub fn default_uv(p: Vec2, ppu: f32, texture_size: (u32, u32)) -> Vec2 {
        let (w, h) = (texture_size.0.max(1) as f32, texture_size.1.max(1) as f32);
        Vec2::new(0.5 + p.x * ppu / w, 0.5 - p.y * ppu / h)
    }
}

/// What a removed `Polygon` leaves behind, taken with it.
fn polygon_removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<Renderable2d>(entity);
}

/// The `polygon` component: writes `Shape2d::Polygon` and a [`PolygonMesh`]
/// into `Renderable2d`.
pub(crate) fn register_polygon_component(reg: &mut Registry<'_>) {
    reg.add_system(balaur_core::Stage::SceneSync, resolve_solved_2d_system);
    reg.register_component(
        Polygon::NAME,
        ComponentDef {
            tags: &[words::ORTHOGRAPHIC, "render", "animation"],
            ..ComponentDef::of::<Polygon>(Some(polygon_written))
        },
    );
}

/// What a write of `polygon` leaves to be worked out again.
fn polygon_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    polygon_laid_out(eng, entity)
}

/// The polygon's geometry, tint and material, resolved again from what was
/// written.
fn polygon_laid_out(eng: &Engine, entity: Entity) -> Result<()> {
    let Some(asked) = eng
        .world()
        .get::<&Polygon>(entity)
        .ok()
        .map(|asked| Polygon::clone(&asked))
    else {
        return Ok(());
    };
    let ppu = asked.pixels_per_unit.max(0.01);
    let polygon = resolve(eng, asked.mesh, asked.texture, asked.skeleton, ppu)?;
    set_polygon(eng, entity, Arc::new(polygon))?;
    set_color(eng, entity, asked.color)?;
    crate::material::set_material_2d(eng, entity, &asked.material)
}

/// The geometry a `polygon` draws, with a missing or unreadable mesh warned
/// about and drawn as nothing: one bad reference must not take the scene
/// down, and the editor adds the component before it has any points.
pub(crate) fn resolve(
    eng: &Engine,
    mesh: String,
    texture: String,
    skeleton: String,
    ppu: f32,
) -> Result<PolygonMesh> {
    let mut polygon = PolygonMesh {
        mesh,
        texture,
        skeleton,
        pixels_per_unit: ppu,
        positions: Vec::new(),
        indices: Vec::new(),
        uvs: Vec::new(),
        skin: None,
    };
    if polygon.mesh.is_empty() {
        return Ok(polygon);
    }
    let data = match balaur_geometry::mesh::resolved(eng, &polygon.mesh) {
        Ok(data) => data,
        Err(why) => {
            tracing::warn!("polygon mesh '{}': {why:#}", polygon.mesh);
            return Ok(polygon);
        }
    };
    polygon.positions = data
        .positions
        .iter()
        .map(|p| Vec2::new(p[0], p[1]))
        .collect();
    polygon.indices.clone_from(&data.indices);
    polygon.uvs = if let Some(uvs) = &data.uvs {
        uvs.iter().map(|uv| Vec2::new(uv[0], uv[1])).collect()
    } else {
        let size = texture_size(eng, &polygon.texture)?;
        polygon
            .positions
            .iter()
            .map(|&p| PolygonMesh::default_uv(p, ppu, size))
            .collect()
    };
    polygon.skin.clone_from(&data.skin);
    Ok(polygon)
}

/// The image's pixel size, read from its header in every build — the
/// resolved UVs are state a script reads, so headless must agree with
/// windowed. No texture is a unit square, which maps every point to (0.5, 0.5).
fn texture_size(eng: &Engine, texture: &str) -> Result<(u32, u32)> {
    if texture.is_empty() {
        return Ok((1, 1));
    }
    crate::texture::size_of(eng, texture)
}

/// On a node drawn from a 2D solver's cells, with the topology it was built
/// at: a soft body a generator laid out has no polygon of its own to deform.
struct SolverDrawn2d(u32);

/// Give a 2D soft body with nothing of its own to draw a polygon of its cells,
/// rebuilt when the topology changes and dropped once the body is.
///
/// Only the topology: `skinned_2d::write_deform` moves the vertices every
/// frame, and a new polygon per step would rebuild the node every frame.
fn resolve_solved_2d_system(eng: &Engine, _dt: f32) {
    let mut wanted: Vec<(Entity, PolygonMesh, u32)> = Vec::new();
    let mut tinted: Vec<(Entity, [f32; 4])> = Vec::new();
    {
        let world = eng.world();
        for (entity, solved) in
            &mut world.query::<(Entity, &balaur_geometry::mesh::SolvedPolygon)>()
        {
            if solved.positions.is_empty() || solved.indices.is_empty() {
                continue;
            }
            let drawn = world.get::<&SolverDrawn2d>(entity).ok().map(|d| d.0);
            // A node that draws something of its own is the solver's to deform.
            if drawn.is_none() && world.get::<&Renderable2d>(entity).is_ok() {
                continue;
            }
            tinted.push((entity, solved.color));
            if drawn == Some(solved.topology) {
                continue;
            }
            wanted.push((entity, cells_of(solved), solved.topology));
        }
    }
    for (entity, mesh, topology) in wanted {
        let _ = set_polygon(eng, entity, Arc::new(mesh));
        let _ = eng.insert_one(entity, SolverDrawn2d(topology));
    }
    for (entity, color) in tinted {
        if let Ok(mut drawn) = eng.world().get::<&mut Renderable2d>(entity) {
            drawn.color = color;
        }
    }
    let gone: Vec<Entity> = eng
        .world()
        .query::<Entity>()
        .with::<&SolverDrawn2d>()
        .without::<&balaur_geometry::mesh::SolvedPolygon>()
        .iter()
        .collect();
    for entity in gone {
        let _ = eng.remove_one::<SolverDrawn2d>(entity);
        let _ = eng.remove_one::<Renderable2d>(entity);
    }
}

fn cells_of(solved: &balaur_geometry::mesh::SolvedPolygon) -> PolygonMesh {
    PolygonMesh {
        mesh: String::new(),
        texture: String::new(),
        skeleton: String::new(),
        pixels_per_unit: crate::DEFAULT_PIXELS_PER_UNIT,
        positions: solved
            .positions
            .iter()
            .map(|p| Vec2::from_array(*p))
            .collect(),
        indices: solved.indices.clone(),
        uvs: vec![Vec2::ZERO; solved.positions.len()],
        skin: None,
    }
}
