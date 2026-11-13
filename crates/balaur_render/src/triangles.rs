//! `render.triangles_of`: the triangles a node draws, for a tool that places
//! things on a surface, as Godot's Populate Surface does.

use anyhow::{Result, bail};
use balaur_core::scene::GlobalTransform;
use balaur_core::{Engine, entity_of};
use balaur_geometry::mesh::MeshData;
use balaur_script::{Module, NodeId, Value};
use glamx::{Mat4, Vec3};

const TRIANGLES_OF: &str = "triangles_of";

fn matrix(at: &GlobalTransform) -> Mat4 {
    Mat4::from_scale_rotation_translation(at.scale, at.rotation, at.position)
}

/// The triangles `surface` draws, in the space of the node at `into`.
fn triangles_of(eng: &Engine, surface: NodeId, into: Mat4) -> Result<Vec<[Vec3; 3]>> {
    let entity = entity_of(surface)?;
    // What the surface draws, read under the borrow; a mesh asset is resolved
    // after it, since resolving reaches the asset cache.
    let (source, found) = {
        let world = eng.world();
        if let Ok(renderable) = world.get::<&crate::Renderable3d>(entity) {
            if let Some(built) = &renderable.built {
                (None, Some((**built).clone()))
            } else if let Some(source) = renderable.mesh.clone().filter(|s| !s.is_empty()) {
                (Some(source), None)
            } else {
                (None, renderable.shape.solid().map(|solid| solid.build()))
            }
        } else if let Ok(renderable) = world.get::<&crate::Renderable2d>(entity)
            && let Some(polygon) = &renderable.polygon
        {
            let flat = MeshData {
                positions: polygon.positions.iter().map(|p| [p.x, p.y, 0.0]).collect(),
                indices: polygon.indices.clone(),
                ..MeshData::default()
            };
            (None, Some(flat))
        } else {
            (None, None)
        }
    };
    let mesh = match (source, found) {
        (Some(source), _) => (*balaur_geometry::mesh::resolved(eng, &source)?).clone(),
        (None, Some(mesh)) => mesh,
        (None, None) => bail!("the surface carries no mesh, shape or polygon to populate"),
    };
    let world = eng.world();
    let surface_at = world
        .get::<&GlobalTransform>(entity)
        .map_or(Mat4::IDENTITY, |at| matrix(&at));
    let to_node = into.inverse() * surface_at;
    Ok(mesh
        .indices
        .iter()
        .filter_map(|[a, b, c]| {
            let corner = |i: u32| {
                mesh.positions
                    .get(i as usize)
                    .map(|p| to_node.transform_point3(Vec3::from_array(*p)))
            };
            Some([corner(*a)?, corner(*b)?, corner(*c)?])
        })
        .collect())
}

/// `render.triangles_of`.
pub(crate) fn install_triangles_api(m: &mut Module<Engine>) {
    m.describe(&[(
        TRIANGLES_OF,
        &[],
        "(node: node, space: node?) -> [Vec3]",
        "The triangles `node` draws, from its mesh, its shape or its 2D polygon, as three corners each in the space of `space`, or of the world where it is left out.",
    )]);
    m.function(
        TRIANGLES_OF,
        |eng: &Engine, (node, space): (NodeId, Option<NodeId>)| {
            let into = match space {
                Some(space) => {
                    let entity = entity_of(space)?;
                    eng.world()
                        .get::<&GlobalTransform>(entity)
                        .map_or(Mat4::IDENTITY, |at| matrix(&at))
                }
                None => Mat4::IDENTITY,
            };
            Ok(Value::List(
                triangles_of(eng, node, into)?
                    .iter()
                    .flatten()
                    .map(|corner| Value::Vec3(corner.to_array()))
                    .collect(),
            ))
        },
    );
}
