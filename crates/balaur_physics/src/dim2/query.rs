//! The 2D half of `crate::query`: the same questions, two dimensions.
//!
//! The ordering rule holds here too — every list is sorted by distance and
//! then by entity bits before it crosses the binding seam, because rapier
//! walks a BVH and a replay may not depend on BVH order.

use anyhow::{anyhow, Result};
use balaur_core::hecs::Entity;
use balaur_core::{entity_of, node_id_of, Engine};
use balaur_script::{Bindings, BindingsExt, CallbackHost, NodeId, Value};
use glamx::{Pose2, Vec2};
use rapier2d::parry::query::{Ray, ShapeCastOptions};
use rapier2d::prelude::{Collider, ColliderHandle, Group, InteractionGroups, QueryFilter};

use crate::dim2::PhysicsState2d;
use crate::vocabulary::{map, Opts};

pub(crate) fn entity_of_collider(collider: &Collider) -> Option<Entity> {
    Entity::from_bits(collider.user_data as u64)
}

fn filter_of<'a>(opts: &Opts<'_>, groups: &'a mut Option<InteractionGroups>) -> QueryFilter<'a> {
    let filter = Opts(opts.get("filter"));
    let mut out = QueryFilter::default();
    match filter.text("only") {
        Some("dynamic") => out = out.exclude_fixed().exclude_kinematic(),
        Some("kinematic") => out = out.exclude_fixed().exclude_dynamic(),
        Some("static") => out = out.exclude_dynamic().exclude_kinematic(),
        _ => {}
    }
    if !filter.boolean("sensors", true) {
        out = out.exclude_sensors();
    }
    if !filter.boolean("solids", true) {
        out = out.exclude_solids();
    }
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

fn hit_value(entity: Entity, point: [f32; 2], normal: [f32; 2], distance: f32) -> Value {
    map([
        ("node", Value::Node(entity.to_bits().get())),
        ("point", Value::Vec2(point)),
        ("normal", Value::Vec2(normal)),
        ("distance", Value::Num(f64::from(distance))),
    ])
}

fn ray_of(opts: &Opts<'_>) -> (Ray, f32, bool) {
    let from = opts.vec2("from", [0.0; 2]);
    let dir = opts.vec2("dir", [0.0, -1.0]);
    (
        Ray::new(Vec2::from(from), Vec2::from(dir)),
        opts.f32("max", 1000.0),
        opts.boolean("solid", true),
    )
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

pub(crate) fn install_query2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("raycast", &[], "(opts: table)", "The first collider a ray meets: `#{ from = [x, y], dir = [x, y], max = 100.0, filter = #{ exclude = node } }`. Returns `#{ node, point, normal, distance }`, or nothing."),
        ("raycast_all", &[], "(opts: table)", "Every collider a ray meets, nearest first."),
        ("shapecast", &[], "(opts: table)", "Sweep a shape along a direction until it hits something: a thick raycast."),
        ("nearest_point", &[], "(opts: table)", "The closest point on any collider to a world point."),
        ("point_hits", &[], "(opts: table)", "Every collider containing a world point: what a mouse click asks."),
        ("shape_hits", &[], "(opts: table)", "Every collider a shape overlaps where it stands."),
        ("box_hits", &[], "(opts: table)", "Every collider whose bounds meet an axis-aligned box."),
        ("distance", &[], "(a: node, b: node)", "The gap between two nodes' colliders, zero when they touch."),
        ("intersects", &[], "(a: node, b: node)", "Whether two nodes' colliders overlap right now, sensor or not."),
    ]);
    m.function("raycast", |eng: &Engine, opts: Value| {
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
                    candidates.push((entity, hit.time_of_impact, hit.point, hit.normal));
                }
            }
        }
        candidates.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.to_bits().cmp(&b.0.to_bits()))
        });
        for (entity, toi, point, normal) in candidates {
            if allowed(eng, &opts, entity)? {
                return Ok(hit_value(
                    entity,
                    [point.x, point.y],
                    [normal.x, normal.y],
                    toi,
                ));
            }
        }
        Ok(Value::Nil)
    });
    m.function("raycast_all", |eng: &Engine, opts: Value| {
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
                    hits.push((
                        entity,
                        hit.time_of_impact,
                        [hit.point.x, hit.point.y],
                        [hit.normal.x, hit.normal.y],
                    ));
                }
            }
        }
        hits.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.to_bits().cmp(&b.0.to_bits()))
        });
        let mut out = Vec::new();
        for (entity, toi, point, normal) in hits {
            if allowed(eng, &opts, entity)? {
                out.push(hit_value(entity, point, normal, toi));
            }
        }
        Ok(Value::List(out))
    });
    m.function("shapecast", |eng: &Engine, opts: Value| {
        let opts = Opts(Some(&opts));
        let params = shape_params(&opts)?;
        let builder = crate::dim2::collider::collider_builder(eng, &params)?;
        let from = Vec2::from(opts.vec2("from", [0.0; 2]));
        let dir = Vec2::from(opts.vec2("dir", [0.0, -1.0]));
        let options = ShapeCastOptions {
            max_time_of_impact: opts.f32("max", 1000.0),
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
            [hit.witness1.x, hit.witness1.y],
            [hit.normal1.x, hit.normal1.y],
            hit.time_of_impact,
        ))
    });
    m.function("nearest_point", |eng: &Engine, opts: Value| {
        let opts = Opts(Some(&opts));
        let point = Vec2::from(opts.vec2("point", [0.0; 2]));
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let found = state
            .world
            .query_pipeline_with_filter(filter)
            .project_point(point, opts.f32("max", 1000.0), opts.boolean("solid", true));
        let Some((handle, projection)) = found else {
            return Ok(Value::Nil);
        };
        let Some(entity) = entity_of_collider(&state.world.colliders[handle]) else {
            return Ok(Value::Nil);
        };
        let p = projection.point;
        Ok(map([
            ("node", Value::Node(entity.to_bits().get())),
            ("point", Value::Vec2([p.x, p.y])),
            ("inside", Value::Bool(projection.is_inside)),
            ("distance", Value::Num(f64::from((p - point).length()))),
        ]))
    });
    m.function("point_hits", |eng: &Engine, opts: Value| {
        let opts = Opts(Some(&opts));
        let point = Vec2::from(opts.vec2("point", [0.0; 2]));
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
    m.function("shape_hits", |eng: &Engine, opts: Value| {
        let opts = Opts(Some(&opts));
        let params = shape_params(&opts)?;
        let builder = crate::dim2::collider::collider_builder(eng, &params)?;
        let at = Vec2::from(opts.vec2("at", [0.0; 2]));
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
        let opts = Opts(Some(&opts));
        let min = Vec2::from(opts.vec2("min", [0.0; 2]));
        let max = Vec2::from(opts.vec2("max", [0.0; 2]));
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let mut hits: Vec<Entity> = state
            .world
            .query_pipeline_with_filter(filter)
            .intersect_aabb_conservative(rapier2d::prelude::Aabb::new(min, max))
            .filter_map(|(_, collider)| entity_of_collider(collider))
            .collect();
        Ok(node_list(&mut hits))
    });
    m.function("distance", |eng: &Engine, (a, b): (NodeId, NodeId)| {
        with_pair(eng, a, b, |first, second| {
            rapier2d::parry::query::distance(
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
            rapier2d::parry::query::intersection_test(
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
    let first = state
        .colliders
        .get(&a)
        .and_then(|h| h.first())
        .ok_or_else(|| anyhow!("the first node has no 2D collider"))?;
    let second = state
        .colliders
        .get(&b)
        .and_then(|h| h.first())
        .ok_or_else(|| anyhow!("the second node has no 2D collider"))?;
    f(
        &state.world.colliders[*first],
        &state.world.colliders[*second],
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
