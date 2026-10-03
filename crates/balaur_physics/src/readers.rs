//! Where a 3D collider is tested and where it sits on its body, and the
//! holes a script cuts in a heightfield. Split from `crate::collider` under
//! `MAX_FILE_LINES`.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::PhysicsState3d;
use crate::collider::{first_collider, with_first_collider};
use crate::scalar::{self, Real};
use crate::vocabulary::{component as c, keys as k};

/// A box as a script reads one: its two opposite corners, flattened.
fn corners(aabb: &crate::rapier3d::prelude::Aabb) -> (Real, Real, Real, Real, Real, Real) {
    (
        aabb.mins.x,
        aabb.mins.y,
        aabb.mins.z,
        aabb.maxs.x,
        aabb.maxs.y,
        aabb.maxs.z,
    )
}

/// The boxes a collider is tested with, and where it sits on its body.
///
/// Split from `crate::collider::install_collider_reader_api` under `MAX_FN_LINES`.
pub(crate) fn install_collider_placement_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("collision_aabb", &[c::COLLIDER_3D], "", "The box the narrow phase tests the collider in: its shape's box grown by its collision_margin and the world's prediction distance."),
        ("broad_phase_aabb", &[c::COLLIDER_3D], "", "The box the broad phase files the collider under, which also reaches ahead by its body's speculative_distance."),
        ("pose_in_body", &[c::COLLIDER_3D], "", "Where the collider sits in its body's frame, as `#{ position, rotation }`; nothing for a collider with no body."),
    ]);
    m.function("collision_aabb", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let collider = &state.world.colliders[first_collider(&state, entity)?];
        let prediction = state.world.integration_parameters.prediction_distance();
        Ok(corners(&collider.compute_collision_aabb(prediction)))
    });
    m.function("broad_phase_aabb", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let collider = &state.world.colliders[first_collider(&state, entity)?];
        let params = &state.world.integration_parameters;
        Ok(corners(
            &collider.compute_broad_phase_aabb(params, &state.world.bodies),
        ))
    });
    m.function("pose_in_body", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |collider| {
            use balaur_script::Value;
            Ok(collider.position_wrt_parent().map_or(Value::Nil, |pose| {
                crate::vocabulary::map([
                    (k::POSITION, Value::Vec3(scalar::a3(pose.translation))),
                    (
                        k::ROTATION,
                        Value::Vec3(crate::body::euler_of(pose.rotation)),
                    ),
                ])
            }))
        })
    });
}

/// Cutting holes in a heightfield while the game runs, and reading them back.
pub(crate) fn install_heightfield_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_heightfield_hole", &[c::COLLIDER_3D], "", "Remove or restore the ground of one heightfield cell, named by the row and column of its first corner."),
        ("heightfield_hole", &[c::COLLIDER_3D], "", "Whether a heightfield cell has no ground."),
    ]);
    m.function(
        "set_heightfield_hole",
        |eng: &Engine, (node, row, column, hole): (NodeId, i64, i64, bool)| {
            use crate::rapier3d::parry::shape::HeightFieldCellStatus as Cell;
            let entity = balaur_core::entity_of(node)?;
            let state = eng.resource::<PhysicsState3d>();
            let mut state = state.borrow_mut();
            let handle = first_collider(&state, entity)?;
            let field = state.world.colliders[handle]
                .shape_mut()
                .as_heightfield_mut()
                .ok_or_else(|| anyhow!("this node's collider is not a heightfield"))?;
            let (row, column) = heightfield_cell(field.num_cells_ij(), row, column)?;
            let status = if hole {
                Cell::CELL_REMOVED
            } else {
                Cell::empty()
            };
            field.set_cell_status(row, column, status);
            // Nothing else about the world says a hole opened until something
            // falls into it.
            state.shape_revision = state.shape_revision.wrapping_add(1);
            state.queries_ready = false;
            Ok(())
        },
    );
    m.function(
        "heightfield_hole",
        |eng: &Engine, (node, row, column): (NodeId, i64, i64)| {
            use crate::rapier3d::parry::shape::HeightFieldCellStatus as Cell;
            with_first_collider(eng, node, |collider| {
                let field = collider
                    .shape()
                    .as_heightfield()
                    .ok_or_else(|| anyhow!("this node's collider is not a heightfield"))?;
                let (row, column) = heightfield_cell(field.num_cells_ij(), row, column)?;
                Ok(field.cell_status(row, column).contains(Cell::CELL_REMOVED))
            })
        },
    );
}

/// A cell a script named, checked against the grid: parry indexes without a
/// check, and a panic is a poor way to learn the grid is smaller.
fn heightfield_cell(cells: (usize, usize), row: i64, column: i64) -> Result<(usize, usize)> {
    let inside = |n: i64, limit: usize| usize::try_from(n).ok().filter(|n| *n < limit);
    match (inside(row, cells.0), inside(column, cells.1)) {
        (Some(row), Some(column)) => Ok((row, column)),
        _ => Err(anyhow!(
            "this heightfield has cells [0, 0] to [{}, {}], not [{row}, {column}]",
            cells.0.saturating_sub(1),
            cells.1.saturating_sub(1)
        )),
    }
}
