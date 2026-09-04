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

/// The 2D twin of `crate::query::ensure_queries`: rapier fills the broad
/// phase during a step, and a game asks questions before its first one.
pub(crate) fn ensure_queries(eng: &Engine) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    if state.queries_ready {
        return;
    }
    let state = &mut *state;
    let handles: Vec<ColliderHandle> = state.world.colliders.iter().map(|(h, _)| h).collect();
    let params = state.world.integration_parameters;
    let mut pairs = Vec::new();
    state.world.broad_phase.update(
        &params,
        &state.world.colliders,
        &state.world.bodies,
        &handles,
        &[],
        &mut pairs,
    );
    state.queries_ready = true;
}

pub(crate) fn entity_of_collider(collider: &Collider) -> Option<Entity> {
    Entity::from_bits(collider.user_data as u64)
}

fn filter_of<'a>(opts: &Opts<'_>, groups: &'a mut Option<InteractionGroups>) -> QueryFilter<'a> {
    let filter = Opts(opts.get("filter"));
    let mut flags = QueryFilterFlags::empty();
    match filter.text("only") {
        Some("dynamic") => flags |= QueryFilterFlags::ONLY_DYNAMIC,
        Some("kinematic") => flags |= QueryFilterFlags::ONLY_KINEMATIC,
        Some("static") => flags |= QueryFilterFlags::ONLY_FIXED,
        _ => {}
    }
    if !filter.boolean("sensors", true) {
        flags |= QueryFilterFlags::EXCLUDE_SENSORS;
    }
    if !filter.boolean("solids", true) {
        flags |= QueryFilterFlags::EXCLUDE_SOLIDS;
    }
    let mut out = QueryFilter::from(flags);
    if let Some(Value::List(items)) = filter.get("mask") {
        let mut bits = 0u32;
        for item in items {
            let layer = match item {
                Value::Int(i) => Some(*i as u32),
                Value::Num(n) => Some(*n as u32),
                Value::Str(s) => s.parse().ok(),
                _ => None,
            };
            if let Some(layer) = layer.filter(|l| *l < 32) {
                bits |= 1 << layer;
            }
        }
        *groups = Some(InteractionGroups::new(
            Group::ALL,
            Group::from_bits_truncate(if bits == 0 { u32::MAX } else { bits }),
            InteractionTestMode::And,
        ));
        if let Some(groups) = groups.as_ref() {
            out = out.groups(*groups);
        }
    }
    out
}

fn excluded(opts: &Opts<'_>, state: &PhysicsState2d) -> Vec<ColliderHandle> {
    let filter = Opts(opts.get("filter"));
    let mut out = Vec::new();
    for key in ["exclude", "exclude_body"] {
        let Some(bits) = filter.node(key) else {
            continue;
        };
        let Some(entity) = Entity::from_bits(bits) else {
            continue;
        };
        if let Some(handles) = state.colliders.get(&entity) {
            out.extend(handles.iter().copied());
        }
    }
    out
}

fn allowed(eng: &Engine, opts: &Opts<'_>, entity: Entity) -> Result<bool> {
    let Some(Value::Callback(id)) = opts
        .get("filter")
        .and_then(|f| Opts(Some(f)).get("predicate"))
    else {
        return Ok(true);
    };
    match eng.invoke(*id, &[Value::Node(entity.to_bits().get())])? {
        Value::Bool(false) | Value::Nil => Ok(false),
        _ => Ok(true),
    }
}

fn hit_value(entity: Entity, point: [f32; 2], normal: [f32; 2], distance: Real) -> Value {
    map([
        ("node", Value::Node(entity.to_bits().get())),
        ("point", Value::Vec2(point)),
        ("normal", Value::Vec2(normal)),
        ("distance", Value::Num(f64::from(distance))),
    ])
}

fn ray_of(opts: &Opts<'_>) -> (Ray, Real, bool) {
    let from = opts.vec2("from", [0.0; 2]);
    let dir = opts.vec2("dir", [0.0, -1.0]);
    (
        Ray::new(scalar::v2a(from), scalar::v2a(dir)),
        scalar::real(opts.f32("max", 1000.0)),
        opts.boolean("solid", true),
    )
}

/// Sorted by distance, then by entity bits, as in 3D.
fn sort_hits(hits: &mut [(Entity, Real, [f32; 2], [f32; 2])]) {
    // `total_cmp`, not `partial_cmp`: a NaN distance makes the latter a
    // non-order, which `sort_by` is allowed to panic on.
    hits.sort_by(|a, b| {
        a.1.total_cmp(&b.1)
            .then_with(|| a.0.to_bits().cmp(&b.0.to_bits()))
    });
}

fn node_list(hits: &mut Vec<Entity>) -> Value {
    hits.sort_unstable_by_key(|e| e.to_bits());
    hits.dedup();
    Value::List(
        hits.iter()
            .map(|e| Value::Node(e.to_bits().get()))
            .collect(),
    )
}

fn shape_params(opts: &Opts<'_>) -> Result<toml::Value> {
    let shape = opts
        .get("shape")
        .ok_or_else(|| anyhow!("this query needs a `shape` table, in collider2d's vocabulary"))?;
    balaur_core::node_api::to_toml(shape)
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

/// Nodes whose colliders intersect this node's, sorted by entity bits.
pub fn overlaps(eng: &Engine, entity: Entity) -> Vec<Entity> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let Some(handles) = state.colliders.get(&entity) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for &handle in handles {
        for (h1, _, h2, _, intersecting) in state.world.intersection_pairs_with(handle) {
            if !intersecting {
                continue;
            }
            let other = if h1 == handle { h2 } else { h1 };
            if let Some(found) = entity_of_collider(&state.world.colliders[other]) {
                if found != entity {
                    hits.push(found);
                }
            }
        }
    }
    hits.sort_unstable_by_key(|e| e.to_bits());
    hits.dedup();
    hits
}

pub(crate) fn overlaps_value(eng: &Engine, node: NodeId) -> Result<Vec<NodeId>> {
    Ok(overlaps(eng, entity_of(node)?)
        .into_iter()
        .map(node_id_of)
        .collect())
}
