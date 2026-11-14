//! What a 2D collider weighs and where it is, the call that replaces one, and
//! the heightfield's holes: `crate::collider`'s readers in two dimensions.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_script::{Module, NodeId, Value};

use crate::dim2::PhysicsState2d;
use crate::dim2::collider::first_collider;
use crate::rapier2d::prelude::{Aabb, Collider};
use crate::scalar;
use crate::vocabulary::words;
use crate::vocabulary::words::functions as f;
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

/// A box as a script reads one, the `Aabb` record: its lowest and highest corners.
fn aabb_value(aabb: &Aabb) -> Value {
    map([
        (k::MIN, Value::Vec2(scalar::a2(aabb.mins))),
        (k::MAX, Value::Vec2(scalar::a2(aabb.maxs))),
    ])
}

/// Replacing a 2D collider, and the boxes it covers.
pub(crate) fn install_collider2d_api(m: &mut Module<Engine>) {
    m.describe(&[
        (words::AABB, &[c::COLLIDER_2D], "(node: node) -> Aabb", "The world-space box the collider currently occupies, from its lowest corner to its highest."),
        (f::SWEPT_AABB, &[c::COLLIDER_2D], "(node: node) -> Aabb", "The box the collider covers over the next fixed step, from where it is to where its body's velocity and forces carry it."),
        (f::COLLISION_AABB, &[c::COLLIDER_2D], "(node: node) -> Aabb", "The box the narrow phase tests the collider in: its shape's box grown by its collision_margin and the world's prediction distance."),
        (f::BROAD_PHASE_AABB, &[c::COLLIDER_2D], "(node: node) -> Aabb", "The box the broad phase files the collider under, which also reaches ahead by its body's speculative_distance."),
    ]);
    m.function(words::AABB, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| {
            Ok(aabb_value(&collider.compute_aabb()))
        })
    });
    m.function(f::SWEPT_AABB, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |state, collider| {
            // Unclamped by speculative_distance, so the box spans the whole step.
            let dt = balaur_core::fixed_dt();
            let next = collider
                .parent()
                .and_then(|parent| state.world.bodies.get(parent))
                .zip(collider.position_wrt_parent())
                .map_or(*collider.position(), |(body, local)| {
                    body.predict_position_using_velocity_and_forces(dt) * local
                });
            Ok(aabb_value(&collider.compute_swept_aabb(&next)))
        })
    });
    m.function(f::COLLISION_AABB, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |state, collider| {
            let prediction = state.world.integration_parameters.prediction_distance();
            Ok(aabb_value(&collider.compute_collision_aabb(prediction)))
        })
    });
    m.function(f::BROAD_PHASE_AABB, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |state, collider| {
            let params = &state.world.integration_parameters;
            Ok(aabb_value(
                &collider.compute_broad_phase_aabb(params, &state.world.bodies),
            ))
        })
    });
}

/// What a 2D collider weighs, how much room it takes, its shape as points, and
/// the handles rapier knows it by.
pub(crate) fn install_collider2d_reader_api(m: &mut Module<Engine>) {
    m.records(&[
        (
            "Aabb",
            "{ min: Vec2, max: Vec2 }",
            "An axis-aligned box in world space, from its lowest corner to its highest.",
        ),
        (
            "ColliderMesh",
            "{ points: [Vec2], indices: [int] }",
            "A voxel collider's outline as points and the two point indices of each segment, one after the other.",
        ),
        (
            "Handles",
            "{ body: [int]?, colliders: [[int]] }",
            "The rapier handles behind a node, each an index and a generation; `body` is nil on a node with no body.",
        ),
        (
            "MassProperties",
            "{ mass: float, center_of_mass: Vec2, inertia: float }",
            "What a collider adds to its body, in its own space.",
        ),
    ]);
    m.describe(&[
        (f::COLLIDER_TOTAL_MASS, &[c::COLLIDER_2D], "", "What this collider weighs, density and size together."),
        (f::COLLIDER_VOLUME, &[c::COLLIDER_2D], "", "How much area the shape encloses."),
        (f::COLLIDER_MASS_PROPERTIES, &[c::COLLIDER_2D], "(node: node) -> MassProperties", "What this collider adds to its body, in its own space."),
        (f::COLLIDER_MESH, &[c::COLLIDER_2D], "(node: node) -> ColliderMesh", "A voxel collider's outline as points and the segments between them, for drawing it or for spawning the pieces it broke into."),
        (f::POSE_IN_BODY, &[c::COLLIDER_2D], "(node: node) -> Pose?", "Where the collider sits in its body's frame; nothing for a collider with no body."),
        (f::HANDLES, &[c::COLLIDER_2D], "(node: node) -> Handles", "The rapier handles behind this node, its body and its colliders. For matching a log line against rapier's own output."),
    ]);
    m.function(f::COLLIDER_TOTAL_MASS, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| Ok(collider.mass()))
    });
    m.function(f::COLLIDER_VOLUME, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| Ok(collider.volume()))
    });
    m.function(f::COLLIDER_MASS_PROPERTIES, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| {
            let props = collider.mass_properties();
            Ok(map([
                (k::MASS, Value::Num(f64::from(props.mass()))),
                (k::CENTER_OF_MASS, Value::Vec2(scalar::a2(props.local_com))),
                (k::INERTIA, Value::Num(f64::from(props.principal_inertia()))),
            ]))
        })
    });
    m.function(f::COLLIDER_MESH, |eng: &Engine, node: NodeId| {
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
    m.function(f::POSE_IN_BODY, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |_, collider| {
            Ok(collider.position_wrt_parent().map_or(Value::Nil, |pose| {
                map([
                    (k::POSITION, Value::Vec2(scalar::a2(pose.translation))),
                    (k::ROTATION, Value::Num(f64::from(pose.rotation.angle()))),
                ])
            }))
        })
    });
    m.function(f::HANDLES, |eng: &Engine, node: NodeId| {
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
pub(crate) fn install_heightfield_2d_api(m: &mut Module<Engine>) {
    m.describe(&[
        (f::SET_HEIGHTFIELD_HOLE, &[c::COLLIDER_2D], "", "Remove or restore one segment of a heightfield's ground, numbered from the first height."),
        (f::HEIGHTFIELD_HOLE, &[c::COLLIDER_2D], "", "Whether a heightfield segment has no ground."),
    ]);
    m.function(
        f::SET_HEIGHTFIELD_HOLE,
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
        f::HEIGHTFIELD_HOLE,
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
