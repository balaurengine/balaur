//! The whole-grid edits on a 3D voxel collider: resizing its cells, cropping
//! it and joining two grids' seams, the twins of `physics2d`'s.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::PhysicsState3d;
use crate::collider::{first_collider, with_voxels};
use crate::scalar;
use crate::vocabulary::component as c;

pub(crate) fn install_voxel_edit_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_voxel_size", &[c::COLLIDER_3D], "", "Resize every cell of a voxel collider, keeping which cells are filled."),
        ("voxel_size", &[c::COLLIDER_3D], "", "How big one cell of a voxel collider is, along each axis."),
        ("crop_voxels", &[c::COLLIDER_3D], "", "Empty every cell outside the cells from `(min_x, min_y, min_z)` to `(max_x, max_y, max_z)`, both included. A range holding no filled cell leaves the grid as it was."),
        ("combine_voxels", &[c::COLLIDER_3D], "", "Tell two voxel colliders on one lattice about each other's cells, so a body sliding from one onto the other does not catch on the seam. Both must share a cell size and a rotation."),
    ]);
    m.function(
        "set_voxel_size",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            let size = scalar::v3(x.max(0.001), y.max(0.001), z.max(0.001));
            with_voxels(eng, node, |voxels| voxels.set_voxel_size(size))
        },
    );
    m.function("voxel_size", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let handle = first_collider(&state, entity)?;
        let voxels = state.world.colliders[handle]
            .shape()
            .as_voxels()
            .ok_or_else(|| anyhow!("this node's collider is not a voxel grid"))?;
        Ok(balaur_script::Value::Vec3(scalar::a3(voxels.voxel_size())))
    });
    m.function(
        "crop_voxels",
        |eng: &Engine,
         (node, min_x, min_y, min_z, max_x, max_y, max_z): (
            NodeId,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
        )| {
            with_voxels(eng, node, |voxels| {
                voxels.crop(
                    scalar::cell(min_x, min_y, min_z),
                    scalar::cell(max_x, max_y, max_z),
                );
            })
        },
    );
    m.function(
        "combine_voxels",
        |eng: &Engine, (node, other): (NodeId, NodeId)| combine_voxels(eng, node, other),
    );
}

/// Merge the neighbour states of two voxel grids: `other`'s cell `key` sits at
/// `key + shift` on `node`'s lattice.
fn combine_voxels(eng: &Engine, node: NodeId, other: NodeId) -> Result<()> {
    let (a, b) = (
        balaur_core::entity_of(node)?,
        balaur_core::entity_of(other)?,
    );
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let (first, second) = (first_collider(&state, a)?, first_collider(&state, b)?);
    let not_voxels = || anyhow!("both nodes need a voxel collider");
    let here = &state.world.colliders[first];
    let there = &state.world.colliders[second];
    let mut theirs = there.shape().as_voxels().ok_or_else(not_voxels)?.clone();
    let size = here
        .shape()
        .as_voxels()
        .ok_or_else(not_voxels)?
        .voxel_size();
    let offset = here.position().inverse() * there.position().translation;
    let shift = scalar::cell(
        (offset.x / size.x).round() as i32,
        (offset.y / size.y).round() as i32,
        (offset.z / size.z).round() as i32,
    );
    let ours = state.world.colliders[first]
        .shape_mut()
        .as_voxels_mut()
        .ok_or_else(not_voxels)?;
    ours.combine_voxel_states(&mut theirs, shift);
    if let Some(slot) = state.world.colliders[second].shape_mut().as_voxels_mut() {
        *slot = theirs;
    }
    state.shape_revision = state.shape_revision.wrapping_add(1);
    Ok(())
}
