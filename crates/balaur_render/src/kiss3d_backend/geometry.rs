//! Turning what a node says it draws into geometry kiss3d holds.
//!
//! Split from the backend for its length. A shape is spun by the mesher and
//! uploaded here, so the triangles a collider is fitted to are the ones the
//! renderer draws.

use balaur_core::App;
use glamx::{Vec2, Vec3};
use kiss3d::resource::GpuMesh3d;
use kiss3d::scene::SceneNode3d;

use super::MeshSkinSlot;
use crate::{Renderable3d, Shape3d};

/// Hand a mesh's triangles to kiss3d as a static node. Normals and UVs are
/// optional in the format; kiss3d computes normals from the faces when they
/// are absent, which is the right answer for a bare OBJ.
fn upload_geometry(scene: &mut SceneNode3d, data: &balaur_core::mesh::MeshData) -> SceneNode3d {
    let coords: Vec<Vec3> = data
        .positions
        .iter()
        .map(|p| Vec3::from_array(*p))
        .collect();
    let normals = data
        .normals
        .as_ref()
        .map(|ns| ns.iter().map(|n| Vec3::from_array(*n)).collect());
    let uvs = data
        .uvs
        .as_ref()
        .map(|us| us.iter().map(|u| Vec2::from_array(*u)).collect());
    let mut gpu = GpuMesh3d::new(coords, data.indices.clone(), normals, uvs, false);
    if let Some(colors) = &data.colors {
        gpu.set_colors(colors.clone());
    }
    scene.add_mesh(std::rc::Rc::new(std::cell::RefCell::new(gpu)), Vec3::ONE)
}

/// A 3D node as built: the node, a skinned mesh's rest and bindings, the
/// geometry its skinning material draws, and a model's levels of detail.
pub(crate) type Built3d = (
    SceneNode3d,
    Option<MeshSkinSlot>,
    Option<crate::skinned_3d::SkinnedMesh3d>,
    Option<crate::lods::Lods>,
);

/// Build the node a renderable's shape asks for, or `None` with nothing to
/// draw yet.
pub(crate) fn build_node(
    app: &App,
    scene: &mut SceneNode3d,
    renderable: &Renderable3d,
) -> Option<Built3d> {
    match renderable.shape {
        // Built by the mesher rather than by kiss3d: the triangles a collider
        // is fitted to and a ray is picked against are the ones uploaded here.
        Shape3d::Solid(solid) => Some((upload_geometry(scene, &solid.build()), None, None, None)),
        // A boolean's result, already worked out this tick.
        Shape3d::Built => renderable
            .built
            .as_deref()
            .filter(|mesh| !mesh.indices.is_empty())
            .map(|mesh| (upload_geometry(scene, mesh), None, None, None)),
        Shape3d::Mesh => upload_mesh(app, scene, renderable),
    }
}

/// Resolve a `mesh` asset and hand its triangles to kiss3d, with the skin to
/// deform them by when the asset carries one. `None` when the asset is
/// missing or unreadable, which is logged rather than fatal: one bad model
/// must not stop the frame.
fn upload_mesh(app: &App, scene: &mut SceneNode3d, renderable: &Renderable3d) -> Option<Built3d> {
    let reference = renderable.mesh.as_deref().filter(|r| !r.is_empty())?;
    let definition = match balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(
        &app.engine,
        reference,
    ) {
        Ok(definition) => definition,
        Err(err) => {
            tracing::error!("mesh '{reference}': {err:#}");
            return None;
        }
    };
    let data = match balaur_core::mesh::load_from(&app.engine, &definition) {
        Ok(data) => data,
        Err(err) => {
            tracing::error!("mesh '{reference}': {err:#}");
            return None;
        }
    };
    let coords: Vec<Vec3> = data
        .positions
        .iter()
        .map(|p| Vec3::new(p[0], p[1], p[2]))
        .collect();
    let faces: Vec<[u32; 3]> = data.indices.clone();
    // Normals and UVs are optional in the format; kiss3d computes normals from
    // the faces when they are absent, which is the right answer for a bare OBJ.
    let normals: Option<Vec<Vec3>> = data
        .normals
        .as_ref()
        .map(|ns| ns.iter().map(|n| Vec3::new(n[0], n[1], n[2])).collect());
    let uvs = data
        .uvs
        .as_ref()
        .map(|us| us.iter().map(|u| Vec2::new(u[0], u[1])).collect());
    // The geometry the skinning material draws from, kept before the skin is
    // moved into the slot. Nothing to skin with no triangles, and an empty
    // vertex buffer is one wgpu refuses to create.
    let geometry = data
        .skin
        .as_ref()
        .filter(|_| !coords.is_empty() && !faces.is_empty())
        .map(|skin| crate::skinned_3d::SkinnedMesh3d {
            positions: coords.clone(),
            normals: normals
                .clone()
                .unwrap_or_else(|| GpuMesh3d::compute_normals_array(&coords, &faces)),
            uvs: uvs.clone().unwrap_or_default(),
            joints: skin.joints.clone(),
            weights: skin.weights.clone(),
            indices: faces.clone(),
        });
    let lods = crate::lods::Lods::of(&app.engine, &data, &faces);
    // The shapes and the colours before the skin is moved out of the data.
    let morphs = crate::morph::targets_of(&data);
    let colors = data.colors.clone();
    let skin = data.skin.map(|skin| MeshSkinSlot {
        positions: coords.clone(),
        normals: normals.clone(),
        joints: skin.joints,
        weights: skin.weights,
        bones: skin.bones,
        inverse_bind: skin.inverse_bind,
        skeleton: renderable.skeleton.clone(),
    });
    // A skinned mesh is rewritten every frame; a rigid one is uploaded once.
    let mut gpu = GpuMesh3d::new(coords, faces, normals, uvs, skin.is_some());
    if let Some(targets) = morphs {
        gpu.set_morph_targets(targets);
    }
    if let Some(colors) = colors {
        gpu.set_colors(colors);
    }
    let mut node = scene.add_mesh(std::rc::Rc::new(std::cell::RefCell::new(gpu)), Vec3::ONE);
    crate::texture::attach_texture_3d(&app.engine, &mut node, &renderable.texture);
    Some((node, skin, geometry, lods))
}
