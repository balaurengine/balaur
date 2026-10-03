//! What a 2D collider weighs and where it is, the call that replaces one, and
//! the heightfield's holes: `crate::collider`'s readers in two dimensions.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::dim2::PhysicsState2d;
use crate::dim2::collider::{apply_collider, first_collider};
use crate::rapier2d::prelude::{Aabb, Collider};
use crate::scalar::{self, Real};
use crate::vocabulary::{component as c, keys as k, map};

/// A node's first 2D collider, for the readers that ask one question about it.
fn with_first_collider<R>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&PhysicsState2d, &Collider) -> Result<R>,
) -> Result<R> {
    let entity = balaur_core::entity_of(node)?;
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = first_collider(&state, entity)?;
    f(&state, &state.world.colliders[handle])
}

/// A box as a script reads one: its two opposite corners, flattened.
fn corners(aabb: &Aabb) -> (Real, Real, Real, Real) {
    (aabb.mins.x, aabb.mins.y, aabb.maxs.x, aabb.maxs.y)
}

/// Replacing a 2D collider, and the boxes it covers.
pub(crate) fn install_collider2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_collider", &[c::COLLIDER_2D], "", "Replace the node's collider from a `collider2d` table: `kind`, `radius`, `size`, `friction`, and the rest of the component's own vocabulary."),
        ("aabb", &[c::COLLIDER_2D], "", "The world-space box the collider currently occupies, as its two opposite corners."),
        ("swept_aabb", &[c::COLLIDER_2D], "", "The box the collider covers over the next fixed step, from where it is to where its body's velocity and forces carry it."),
        ("collision_aabb", &[c::COLLIDER_2D], "", "The box the narrow phase tests the collider in: its shape's box grown by its collision_margin and the world's prediction distance."),
        ("broad_phase_aabb", &[c::COLLIDER_2D], "", "The box the broad phase files the collider under, which also reaches ahead by its body's speculative_distance."),
    ]);
    m.function(
        "set_collider",
        |eng: &Engine, (node, params): (NodeId, Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            apply_collider(eng, balaur_core::entity_of(node)?, &params)
        },
    );
    m.function("aabb", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| {
            Ok(corners(&collider.compute_aabb()))
        })
    });
    m.function("swept_aabb", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |state, collider| {
            // Unclamped by speculative_distance, so the box spans the whole step.
            let dt = scalar::real(balaur_core::fixed_dt());
            let next = collider
                .parent()
                .and_then(|parent| state.world.bodies.get(parent))
                .zip(collider.position_wrt_parent())
                .map_or(*collider.position(), |(body, local)| {
                    body.predict_position_using_velocity_and_forces(dt) * local
                });
            Ok(corners(&collider.compute_swept_aabb(&next)))
        })
    });
    m.function("collision_aabb", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |state, collider| {
            let prediction = state.world.integration_parameters.prediction_distance();
            Ok(corners(&collider.compute_collision_aabb(prediction)))
        })
    });
    m.function("broad_phase_aabb", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |state, collider| {
            let params = &state.world.integration_parameters;
            Ok(corners(
                &collider.compute_broad_phase_aabb(params, &state.world.bodies),
            ))
        })
    });
}

/// What a 2D collider weighs, how much room it takes, its shape as points, and
/// the handles rapier knows it by.
pub(crate) fn install_collider2d_reader_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("collider_mass", &[c::COLLIDER_2D], "", "What this collider weighs, density and size together."),
        ("collider_volume", &[c::COLLIDER_2D], "", "How much area the shape encloses."),
        ("collider_mass_properties", &[c::COLLIDER_2D], "", "What this collider adds to its body, in its own space: `#{ mass, center_of_mass, inertia }`."),
        ("collider_mesh", &[c::COLLIDER_2D], "", "A voxel collider's outline as points and the segments between them, `#{ points, indices }`, for drawing it or for spawning the pieces it broke into."),
        ("pose_in_body", &[c::COLLIDER_2D], "", "Where the collider sits in its body's frame, as `#{ position, rotation }`; nothing for a collider with no body."),
        ("handles", &[c::COLLIDER_2D], "", "The rapier handles behind this node, its body and its colliders, as `#{ body, colliders }` of index and generation pairs. For matching a log line against rapier's own output."),
    ]);
    m.function("collider_mass", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| Ok(collider.mass()))
    });
    m.function("collider_volume", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| Ok(collider.volume()))
    });
    m.function("collider_mass_properties", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| {
            let props = collider.mass_properties();
            Ok(map([
                (k::MASS, Value::Num(f64::from(props.mass()))),
                (k::CENTER_OF_MASS, Value::Vec2(scalar::a2(props.local_com))),
                (k::INERTIA, Value::Num(f64::from(props.principal_inertia()))),
            ]))
        })
    });
    m.function("collider_mesh", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| {
            let (points, segments) = collider
                .shape()
                .as_voxels()
                .map(crate::rapier2d::parry::shape::Voxels::to_polyline)
                .ok_or_else(|| {
                    anyhow!("only a voxel collider can be turned into points so far; ask for another shape")
                })?;
            let points = points.into_iter().map(|p| Value::Vec2(scalar::a2(p))).collect();
            let indices = segments
                .into_iter()
                .flat_map(|s| s.into_iter().map(|i| Value::Int(i.into())))
                .collect();
            Ok(map([
                (k::POINTS, Value::List(points)),
                (k::INDICES, Value::List(indices)),
            ]))
        })
    });
    m.function("pose_in_body", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| {
            Ok(collider.position_wrt_parent().map_or(Value::Nil, |pose| {
                map([
                    (k::POSITION, Value::Vec2(scalar::a2(pose.translation))),
                    (k::ROTATION, Value::Num(f64::from(pose.rotation.angle()))),
                ])
            }))
        })
    });
    m.function("handles", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let pair = |(index, generation): (u32, u32)| {
            Value::List(vec![
                Value::Int(i64::from(index)),
                Value::Int(i64::from(generation)),
            ])
        };
        let body = state
            .bodies
            .get(&entity)
            .map_or(Value::Nil, |handle| pair(handle.into_raw_parts()));
        let colliders = state.colliders.get(&entity).map_or_else(
            || Value::List(Vec::new()),
            |handles| Value::List(handles.iter().map(|h| pair(h.into_raw_parts())).collect()),
        );
        Ok(map([(k::BODY, body), (k::COLLIDERS, colliders)]))
    });
}

/// Cutting holes in a 2D heightfield while the game runs, and reading them.
pub(crate) fn install_heightfield_2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_heightfield_hole", &[c::COLLIDER_2D], "", "Remove or restore one segment of a heightfield's ground, numbered from the first height."),
        ("heightfield_hole", &[c::COLLIDER_2D], "", "Whether a heightfield segment has no ground."),
    ]);
    m.function(
        "set_heightfield_hole",
        |eng: &Engine, (node, segment, hole): (NodeId, i64, bool)| {
            let entity = balaur_core::entity_of(node)?;
            let state = eng.resource::<PhysicsState2d>();
            let mut state = state.borrow_mut();
            let handle = first_collider(&state, entity)?;
            let ground = state.world.colliders[handle]
                .shape_mut()
                .as_heightfield_mut()
                .ok_or_else(|| anyhow!("this node's collider is not a heightfield"))?;
            let segment = segment_of(ground.num_cells(), segment)?;
            ground.set_segment_removed(segment, hole);
            state.shape_revision = state.shape_revision.wrapping_add(1);
            state.queries_ready = false;
            Ok(())
        },
    );
    m.function(
        "heightfield_hole",
        |eng: &Engine, (node, segment): (NodeId, i64)| {
            with_first_collider(eng, node, |_, collider| {
                let ground = collider
                    .shape()
                    .as_heightfield()
                    .ok_or_else(|| anyhow!("this node's collider is not a heightfield"))?;
                Ok(ground.is_segment_removed(segment_of(ground.num_cells(), segment)?))
            })
        },
    );
}

/// A segment a script named, checked against the ground: parry indexes
/// without a check.
fn segment_of(count: usize, segment: i64) -> Result<usize> {
    usize::try_from(segment)
        .ok()
        .filter(|s| *s < count)
        .ok_or_else(|| {
            anyhow!(
                "this heightfield has segments 0 to {}, not {segment}",
                count.saturating_sub(1)
            )
        })
}
