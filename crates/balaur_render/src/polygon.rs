//! The `polygon` component: a filled, textured 2D polygon, deformed by a
//! rig when its mesh carries skin weights. Writes `Shape2d::Polygon` plus a
//! [`PolygonMesh`] into `Renderable2d`, the split `sprite` uses.
//!
//! The geometry is resolved here, headless: the mesh asset is loaded, the
//! default UVs are derived from the texture header, and the result is what a
//! script reads back and what the backend uploads. Nothing GPU-shaped is
//! decided at draw time.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use balaur_core::components::{as_f64, ComponentDef};
use balaur_core::hecs::Entity;
use balaur_core::mesh::{MeshData, MeshSkin};
use balaur_core::{App, Engine};
use glamx::Vec2;

use crate::{color_from_params, color_to_toml, set_color, set_polygon, Renderable2d, Shape2d};

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

const POLYGON_SCHEMA: &str = r#"mesh = { type = "asset", asset = "mesh", default = "", description = "Vertices, triangulation, UVs and skin weights; positions are [x, y] in the node's space" }
texture = { type = "string", default = "", description = "Image file, project-relative; empty draws the tint alone" }
pixels_per_unit = { type = "float", default = 100.0, min = 0.01, description = "Texture pixels per world unit, for the default UV mapping" }
skeleton = { type = "string", default = "", description = "Node path to the rig root, relative to this node; empty means this node" }
color = { type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#;

/// The `polygon` component: writes `Shape2d::Polygon` and a [`PolygonMesh`]
/// into `Renderable2d`.
pub(crate) fn register_polygon_component(app: &mut App) {
    app.register_component(
        "polygon",
        ComponentDef {
            doc: "A filled, textured 2D polygon from a `mesh` asset's points and triangles, deformed by the rig `skeleton` names when the mesh carries skin weights.",
            schema: ComponentDef::parse_schema("polygon", POLYGON_SCHEMA),
            tags: &["2d", "render", "animation"],
            expects: &[],
            apply: Box::new(apply_polygon),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Renderable2d>(entity);
                Ok(())
            }),
            get: Box::new(polygon_of),
        },
    );
}

fn apply_polygon(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let ppu = params
        .get("pixels_per_unit")
        .and_then(as_f64)
        .map_or(crate::DEFAULT_PIXELS_PER_UNIT, |v| v as f32)
        .max(0.01);
    let polygon = resolve(eng, text("mesh"), text("texture"), text("skeleton"), ppu)?;
    set_polygon(eng, entity, Arc::new(polygon))?;
    set_color(eng, entity, color_from_params(params))
}

/// The geometry a `polygon` draws, with a missing or unreadable mesh warned
/// about and drawn as nothing: one bad reference must not take the scene
/// down, and the editor adds the component before it has any points.
fn resolve(
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
    let data = match balaur_core::assets::load_typed::<MeshData>(eng, &polygon.mesh)
        .and_then(|definition| balaur_core::mesh::load_from(eng, &definition))
    {
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
    polygon.indices = data.indices;
    polygon.uvs = if let Some(uvs) = data.uvs {
        uvs.iter().map(|uv| Vec2::new(uv[0], uv[1])).collect()
    } else {
        let size = texture_size(eng, &polygon.texture)?;
        polygon
            .positions
            .iter()
            .map(|&p| PolygonMesh::default_uv(p, ppu, size))
            .collect()
    };
    polygon.skin = data.skin;
    Ok(polygon)
}

/// The image's pixel size, read from its header in every build — the
/// resolved UVs are state a script reads, so headless must agree with
/// windowed. No texture is a unit square, which maps every point to (0.5, 0.5).
fn texture_size(eng: &Engine, texture: &str) -> Result<(u32, u32)> {
    if texture.is_empty() {
        return Ok((1, 1));
    }
    let bytes = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(texture)?;
    image::load_from_memory(&bytes)
        .map(|image| (image.width(), image.height()))
        .map_err(|e| anyhow!("reading the size of {texture}: {e}"))
}

fn polygon_of(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let renderable = world.get::<&Renderable2d>(entity).ok()?;
    if renderable.shape != Shape2d::Polygon {
        return None;
    }
    let polygon = renderable.polygon.as_ref()?;
    let mut map = toml::map::Map::new();
    map.insert("mesh".into(), toml::Value::String(polygon.mesh.clone()));
    map.insert(
        "texture".into(),
        toml::Value::String(polygon.texture.clone()),
    );
    map.insert(
        "pixels_per_unit".into(),
        toml::Value::Float(f64::from(polygon.pixels_per_unit)),
    );
    map.insert(
        "skeleton".into(),
        toml::Value::String(polygon.skeleton.clone()),
    );
    map.insert("color".into(), color_to_toml(renderable.color));
    Some(toml::Value::Table(map))
}
