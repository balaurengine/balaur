//! The 2D half of `crate::query`: the same questions, two dimensions.
//!
//! The ordering rule holds here too — every list is sorted by distance and
//! then by entity bits before it crosses the binding seam, because rapier
//! walks a BVH and a replay may not depend on BVH order.

use crate::rapier2d::parry::query::{Ray, ShapeCastOptions};
use crate::rapier2d::prelude::{
    Collider, ColliderHandle, Group, InteractionGroups, InteractionTestMode, QueryFilter,
    QueryFilterFlags,
};
use crate::scalar::{self, Pose2, Real};
use anyhow::{anyhow, Result};
use balaur_core::hecs::Entity;
use balaur_core::{entity_of, node_id_of, Engine};
use balaur_script::{Bindings, BindingsExt, CallbackHost, NodeId, Value};

use crate::dim2::PhysicsState2d;
use crate::vocabulary::{map, Opts};

crate::shared::query::functions!(
    state = PhysicsState2d,
    vector = Vec2,
    dimensions = 2,
    vocabulary = "collider2d"
);

fn ray_of(opts: &Opts<'_>) -> (Ray, Real, bool) {
    let from = opts.vec2("from", [0.0; 2]);
    let dir = opts.vec2("dir", [0.0, -1.0]);
    (
        Ray::new(scalar::v2a(from), scalar::v2a(dir)),
        scalar::real(opts.f32("max", 1000.0)),
        opts.boolean("solid", true),
    )
}

pub(crate) fn install_physics2d_query_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("raycast", &[], "(opts: table)", "The first collider a ray meets: `#{ from = [x, y], dir = [x, y], max = 100.0, filter = #{ exclude = node } }`. Returns `#{ node, point, normal, distance }`, or nothing."),
        ("raycast_all", &[], "(opts: table)", "Every collider a ray meets, nearest first."),
    ]);
    m.function("raycast", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let (ray, max, solid) = ray_of(&opts);
        let mut candidates = Vec::new();
        {
            let state = eng.resource::<PhysicsState2d>();
            let state = state.borrow();
            let mut groups = None;
            let filter = filter_of(&opts, &mut groups);
            let skip = excluded(&opts, &state);
            for (handle, collider, hit) in state
                .world
                .query_pipeline_with_filter(filter)
                .intersect_ray(ray, max, solid)
            {
                if skip.contains(&handle) {
                    continue;
                }
                if let Some(entity) = entity_of_collider(collider) {
                    let point = ray.point_at(hit.time_of_impact);
                    candidates.push((
                        entity,
                        hit.time_of_impact,
                        scalar::a2(point),
                        scalar::a2(hit.normal),
                    ));
                }
            }
        }
        sort_hits(&mut candidates);
        for (entity, toi, point, normal) in candidates {
            if allowed(eng, &opts, entity)? {
                return Ok(hit_value(entity, point, normal, toi));
            }
        }
        Ok(Value::Nil)
    });
    m.function("raycast_all", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let (ray, max, solid) = ray_of(&opts);
        let mut hits = Vec::new();
        {
            let state = eng.resource::<PhysicsState2d>();
            let state = state.borrow();
            let mut groups = None;
            let filter = filter_of(&opts, &mut groups);
            let skip = excluded(&opts, &state);
            for (handle, collider, hit) in state
                .world
                .query_pipeline_with_filter(filter)
                .intersect_ray(ray, max, solid)
            {
                if skip.contains(&handle) {
                    continue;
                }
                if let Some(entity) = entity_of_collider(collider) {
                    let point = ray.point_at(hit.time_of_impact);
                    hits.push((
                        entity,
                        hit.time_of_impact,
                        scalar::a2(point),
                        scalar::a2(hit.normal),
                    ));
                }
            }
        }
        sort_hits(&mut hits);
        let mut out = Vec::new();
        for (entity, toi, point, normal) in hits {
            if allowed(eng, &opts, entity)? {
                out.push(hit_value(entity, point, normal, toi));
            }
        }
        Ok(Value::List(out))
    });
}

/// Sweeping a 2D shape rather than a ray.
///
/// Split from [`install_physics2d_query_api`] under `MAX_FN_LINES`.
pub(crate) fn install_physics2d_shapecast_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "shapecast",
        &[],
        "(opts: table)",
        "Sweep a shape along a direction until it hits something: a thick raycast.",
    )]);
    m.function("shapecast", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let params = shape_params(&opts)?;
        let builder = crate::dim2::collider::collider_builder(eng, &params)?;
        let from = scalar::v2a(opts.vec2("from", [0.0; 2]));
        let dir = scalar::v2a(opts.vec2("dir", [0.0, -1.0]));
        let options = ShapeCastOptions {
            max_time_of_impact: scalar::real(opts.f32("max", 1000.0)),
            stop_at_penetration: opts.boolean("stop_at_penetration", true),
            ..ShapeCastOptions::default()
        };
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let hit = state.world.query_pipeline_with_filter(filter).cast_shape(
            &Pose2::from_translation(from),
            dir,
            builder.shape.as_ref(),
            options,
        );
        let Some((handle, hit)) = hit else {
            return Ok(Value::Nil);
        };
        let Some(entity) = entity_of_collider(&state.world.colliders[handle]) else {
            return Ok(Value::Nil);
        };
        Ok(hit_value(
            entity,
            scalar::a2(hit.witness1),
            scalar::a2(hit.normal1),
            hit.time_of_impact,
        ))
    });
}

/// The 2D queries that ask about a place rather than a line.
///
/// Split from [`install_physics2d_query_api`] under `MAX_FN_LINES`.
pub(crate) fn install_physics2d_volume_query_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "nearest_point",
            &[],
            "(opts: table)",
            "The closest point on any collider to a world point.",
        ),
        (
            "point_hits",
            &[],
            "(opts: table)",
            "Every collider containing a world point: what a mouse click asks.",
        ),
        (
            "shape_hits",
            &[],
            "(opts: table)",
            "Every collider a shape overlaps where it stands.",
        ),
        (
            "box_hits",
            &[],
            "(opts: table)",
            "Every collider whose bounds meet an axis-aligned box.",
        ),
        (
            "distance",
            &[],
            "(a: node, b: node)",
            "The gap between two nodes' colliders, zero when they touch.",
        ),
        (
            "intersects",
            &[],
            "(a: node, b: node)",
            "Whether two nodes' colliders overlap right now, sensor or not.",
        ),
    ]);
    m.function("nearest_point", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let point = scalar::v2a(opts.vec2("point", [0.0; 2]));
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let found = state
            .world
            .query_pipeline_with_filter(filter)
            .project_point(
                point,
                scalar::real(opts.f32("max", 1000.0)),
                opts.boolean("solid", true),
            );
        let Some((handle, projection)) = found else {
            return Ok(Value::Nil);
        };
        let Some(entity) = entity_of_collider(&state.world.colliders[handle]) else {
            return Ok(Value::Nil);
        };
        let p = projection.point;
        Ok(map([
            ("node", Value::Node(entity.to_bits().get())),
            ("point", Value::Vec2(scalar::a2(p))),
            ("inside", Value::Bool(projection.is_inside)),
            ("distance", Value::Num(f64::from((p - point).length()))),
        ]))
    });
    m.function("point_hits", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let point = scalar::v2a(opts.vec2("point", [0.0; 2]));
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let mut hits: Vec<Entity> = state
            .world
            .query_pipeline_with_filter(filter)
            .intersect_point(point)
            .filter_map(|(_, collider)| entity_of_collider(collider))
            .collect();
        Ok(node_list(&mut hits))
    });
}

/// The 2D queries that take a shape rather than a point.
///
/// Split from [`install_physics2d_volume_query_api`] under `MAX_FN_LINES`.
pub(crate) fn install_physics2d_shape_query_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[]);
    m.function("shape_hits", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let params = shape_params(&opts)?;
        let builder = crate::dim2::collider::collider_builder(eng, &params)?;
        let at = scalar::v2a(opts.vec2("at", [0.0; 2]));
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let mut hits: Vec<Entity> = state
            .world
            .query_pipeline_with_filter(filter)
            .intersect_shape(Pose2::from_translation(at), builder.shape.as_ref())
            .filter_map(|(_, collider)| entity_of_collider(collider))
            .collect();
        Ok(node_list(&mut hits))
    });
    m.function("box_hits", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let min = scalar::v2a(opts.vec2("min", [0.0; 2]));
        let max = scalar::v2a(opts.vec2("max", [0.0; 2]));
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let mut hits: Vec<Entity> = state
            .world
            .query_pipeline_with_filter(filter)
            .intersect_aabb_conservative(crate::rapier2d::prelude::Aabb::new(min, max))
            .filter_map(|(_, collider)| entity_of_collider(collider))
            .collect();
        Ok(node_list(&mut hits))
    });
}

/// The 2D questions asked about two nodes rather than about the world.
///
/// Split from [`install_physics2d_volume_query_api`] under `MAX_FN_LINES`.
pub(crate) fn install_physics2d_pair_query_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[]);
    m.function("distance", |eng: &Engine, (a, b): (NodeId, NodeId)| {
        with_pair(eng, a, b, |first, second| {
            crate::rapier2d::parry::query::distance(
                first.position(),
                first.shape(),
                second.position(),
                second.shape(),
            )
            .map(|d| Value::Num(f64::from(d)))
            .map_err(|e| anyhow!("those two shapes cannot be measured: {e}"))
        })
    });
    m.function("intersects", |eng: &Engine, (a, b): (NodeId, NodeId)| {
        with_pair(eng, a, b, |first, second| {
            crate::rapier2d::parry::query::intersection_test(
                first.position(),
                first.shape(),
                second.position(),
                second.shape(),
            )
            .map(Value::Bool)
            .map_err(|e| anyhow!("those two shapes cannot be tested: {e}"))
        })
    });
}

fn with_pair(
    eng: &Engine,
    a: NodeId,
    b: NodeId,
    f: impl FnOnce(&Collider, &Collider) -> Result<Value>,
) -> Result<Value> {
    let (a, b) = (entity_of(a)?, entity_of(b)?);
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let first = crate::dim2::collider::first_collider(&state, a)
        .map_err(|why| anyhow!("the first node has no 2D collider: {why}"))?;
    let second = crate::dim2::collider::first_collider(&state, b)
        .map_err(|why| anyhow!("the second node has no 2D collider: {why}"))?;
    f(
        &state.world.colliders[first],
        &state.world.colliders[second],
    )
}
