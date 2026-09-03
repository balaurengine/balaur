//! Asking the 3D world a question that is not already a collision.
//!
//! Rapier's whole `QueryPipeline` behind one options table per call, plus the
//! pairwise parry queries between two nodes' colliders.
//!
//! Two rules hold for everything here. **Order is ours, not rapier's**: the
//! pipeline walks a BVH, and BVH order is not something a replay may depend
//! on, so every list is sorted by distance and then by entity bits before it
//! crosses the binding seam. And **the world is borrowed while a query runs**:
//! a `predicate` that calls back into `physics3d` gets an error saying so
//! rather than a panic from the `RefCell`.

use crate::rapier3d::parry::query::{Ray, ShapeCastOptions};
use crate::rapier3d::prelude::{
    Collider, ColliderHandle, Group, InteractionGroups, InteractionTestMode, QueryFilter,
    QueryFilterFlags,
};
use crate::scalar::{self, Pose, Real};
use anyhow::{anyhow, Result};
use balaur_core::hecs::Entity;
use balaur_core::{entity_of, node_id_of, Engine};
use balaur_script::{Bindings, BindingsExt, CallbackHost, NodeId, Value};

use crate::vocabulary::{map, Opts};
use crate::PhysicsState;

/// Make sure the broad phase's tree matches the colliders before asking it
/// anything.
///
/// Rapier fills the tree during a step. A game that raycasts in `init` — to
/// drop a character onto the ground, say — has not stepped yet, and a game
/// that spawns a wall and immediately asks what it hits has stepped, but not
/// since. Both are ordinary, so both work: the tree is rebuilt here, once,
/// when something has changed.
pub(crate) fn ensure_queries(eng: &Engine) {
    let state = eng.resource::<PhysicsState>();
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

/// The entity a collider belongs to, from the id put on it when it was
/// inserted. One lookup rather than a scan of the whole collider map.
pub(crate) fn entity_of_collider(collider: &Collider) -> Option<Entity> {
    Entity::from_bits(collider.user_data as u64)
}

/// A `filter` table as rapier's `QueryFilter`.
///
/// The predicate is deliberately absent here and applied by the caller: it
/// needs the script host, and rapier's own predicate slot is a `&dyn Fn` with
/// a lifetime this function cannot hand back.
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
    if let Some(mask) = filter.get("mask") {
        let mut bits = 0u32;
        if let Value::List(items) = mask {
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

/// The nodes a filter's `exclude` and `exclude_body` name.
///
/// Rapier filters by handle; a script names nodes. Excluding the caller's own
/// node is the single most common thing a raycast needs, so it is worth the
/// two lines it costs here rather than in every script.
fn excluded(opts: &Opts<'_>, state: &PhysicsState) -> Vec<ColliderHandle> {
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

/// Whether a script predicate accepts this hit.
///
/// Called with the world *not* borrowed, so the predicate may ask the engine
/// anything it likes — including another query.
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

/// One hit, in the shape every query here returns.
fn hit_value(entity: Entity, point: [f32; 3], normal: [f32; 3], distance: Real) -> Value {
    map([
        ("node", Value::Node(entity.to_bits().get())),
        ("point", Value::Vec3(point)),
        ("normal", Value::Vec3(normal)),
        ("distance", Value::Num(f64::from(distance))),
    ])
}

/// The ray an options table describes.
fn ray_of(opts: &Opts<'_>) -> (Ray, Real, bool) {
    let from = opts.vec3("from", [0.0; 3]);
    let dir = opts.vec3("dir", [0.0, -1.0, 0.0]);
    (
        Ray::new(scalar::v3a(from), scalar::v3a(dir)),
        scalar::real(opts.f32("max", 1000.0)),
        opts.boolean("solid", true),
    )
}

/// Sorted by distance, then by entity bits: two machines must agree on the
/// order, and rapier's is its BVH's.
fn sort_hits(hits: &mut [(Entity, Real, [f32; 3], [f32; 3])]) {
    hits.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.to_bits().cmp(&b.0.to_bits()))
    });
}

pub(crate) fn install_query_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("raycast", &[], "(opts: table)", "The first collider a ray meets: `#{ from = [x, y, z], dir = [x, y, z], max = 100.0, filter = #{ exclude = node, only = \"dynamic\" } }`. Returns `#{ node, point, normal, distance }`, or nothing."),
        ("raycast_all", &[], "(opts: table)", "Every collider a ray meets, nearest first."),
    ]);
    m.function("raycast", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let (ray, max, solid) = ray_of(&opts);
        // Collect candidates with the world borrowed, then let the predicate
        // run with it released.
        let mut candidates = Vec::new();
        {
            let state = eng.resource::<PhysicsState>();
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
                let Some(entity) = entity_of_collider(collider) else {
                    continue;
                };
                let point = ray.point_at(hit.time_of_impact);
                candidates.push((
                    entity,
                    hit.time_of_impact,
                    scalar::a3(point),
                    scalar::a3(hit.normal),
                ));
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
            let state = eng.resource::<PhysicsState>();
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
                        scalar::a3(point),
                        scalar::a3(hit.normal),
                    ));
                }
            }
        }
        // Sorted first, then filtered: the predicate sees the hits in the
        // order a script will, and reading the node back out of a built value
        // to ask about it would be the long way round.
        sort_hits(&mut hits);
        let mut out = Vec::new();
        for (entity, distance, point, normal) in hits {
            if allowed(eng, &opts, entity)? {
                out.push(hit_value(entity, point, normal, distance));
            }
        }
        Ok(Value::List(out))
    });
}

/// Sweeping a shape rather than a ray: a thick raycast, and how a camera
/// avoids walls.
///
/// Split from [`install_query_api`] under `MAX_FN_LINES`.
pub(crate) fn install_shapecast_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("shapecast", &[], "(opts: table)", "Sweep a shape along a direction until it hits something: a thick raycast, and how a camera avoids walls."),
    ]);
    m.function("shapecast", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let params = shape_params(&opts)?;
        let builder = crate::collider::collider_builder(eng, &params)?;
        let from = scalar::v3a(opts.vec3("from", [0.0; 3]));
        let dir = scalar::v3a(opts.vec3("dir", [0.0, -1.0, 0.0]));
        let options = ShapeCastOptions {
            max_time_of_impact: scalar::real(opts.f32("max", 1000.0)),
            stop_at_penetration: opts.boolean("stop_at_penetration", true),
            ..ShapeCastOptions::default()
        };
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let hit = state.world.query_pipeline_with_filter(filter).cast_shape(
            &Pose::from_translation(from),
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
            scalar::a3(hit.witness1),
            scalar::a3(hit.normal1),
            hit.time_of_impact,
        ))
    });
}

/// The queries that ask about a place rather than a line: what is at a point,
/// inside a shape, or within a box.
///
/// Split from [`install_query_api`] under `MAX_FN_LINES`.
pub(crate) fn install_volume_query_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("nearest_point", &[], "(opts: table)", "The closest point on any collider to a world point."),
        ("point_hits", &[], "(opts: table)", "Every collider containing a world point."),
        ("shape_hits", &[], "(opts: table)", "Every collider a shape overlaps where it stands: an explosion's reach, a melee arc."),
        ("box_hits", &[], "(opts: table)", "Every collider whose bounds meet an axis-aligned box; cheaper and looser than shape_hits."),
    ]);
    m.function("nearest_point", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let point = scalar::v3a(opts.vec3("point", [0.0; 3]));
        let state = eng.resource::<PhysicsState>();
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
            ("point", Value::Vec3(scalar::a3(p))),
            ("inside", Value::Bool(projection.is_inside)),
            ("distance", Value::Num(f64::from((p - point).length()))),
        ]))
    });
    m.function("point_hits", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let point = scalar::v3a(opts.vec3("point", [0.0; 3]));
        let state = eng.resource::<PhysicsState>();
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
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let params = shape_params(&opts)?;
        let builder = crate::collider::collider_builder(eng, &params)?;
        let at = scalar::v3a(opts.vec3("at", [0.0; 3]));
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let mut hits: Vec<Entity> = state
            .world
            .query_pipeline_with_filter(filter)
            .intersect_shape(Pose::from_translation(at), builder.shape.as_ref())
            .filter_map(|(_, collider)| entity_of_collider(collider))
            .collect();
        Ok(node_list(&mut hits))
    });
    m.function("box_hits", |eng: &Engine, opts: Value| {
        ensure_queries(eng);
        let opts = Opts(Some(&opts));
        let min = scalar::v3a(opts.vec3("min", [0.0; 3]));
        let max = scalar::v3a(opts.vec3("max", [0.0; 3]));
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let mut groups = None;
        let filter = filter_of(&opts, &mut groups);
        let aabb = crate::rapier3d::prelude::Aabb::new(min, max);
        let mut hits: Vec<Entity> = state
            .world
            .query_pipeline_with_filter(filter)
            .intersect_aabb_conservative(aabb)
            .filter_map(|(_, collider)| entity_of_collider(collider))
            .collect();
        Ok(node_list(&mut hits))
    });
}

/// The questions asked about two nodes rather than about the world: how far
/// apart, where they nearly touch, and what they are touching with.
///
/// Split from [`install_query_api`] under `MAX_FN_LINES`.
pub(crate) fn install_pair_query_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("distance", &[], "(a: node, b: node)", "The gap between two nodes' colliders, zero when they touch or overlap."),
        ("closest_points", &[], "(a: node, b: node)", "The nearest point on each of two nodes' colliders."),
        ("intersects", &[], "(a: node, b: node)", "Whether two nodes' colliders overlap right now, sensor or not."),
        ("contacts", &["collider3d"], "", "Every contact point on this node's collider this step: `#{ node, point, normal, impulse }` each. Empty for a sensor, which has no contacts by definition."),
        ("max_contact_impulse", &["collider3d"], "", "The hardest contact this node took in the last step, zero when nothing touched it: a damage threshold in one number."),
        ("time_of_impact", &[], "(a: node, b: node, opts: table)", "When two moving colliders would meet, given each one's velocity: `#{ velocity_a = [..], velocity_b = [..], max = 1.0 }`. Nothing when they never do."),
    ]);
    m.function("distance", |eng: &Engine, (a, b): (NodeId, NodeId)| {
        with_pair(eng, a, b, |world, first, second| {
            crate::rapier3d::parry::query::distance(
                first.position(),
                first.shape(),
                second.position(),
                second.shape(),
            )
            .map_err(|e| anyhow!("those two shapes cannot be measured: {e}"))
            .map(|d| {
                let _ = world;
                Value::Num(f64::from(d))
            })
        })
    });
    m.function(
        "closest_points",
        |eng: &Engine, (a, b): (NodeId, NodeId)| {
            with_pair(eng, a, b, |_, first, second| {
                let found = crate::rapier3d::parry::query::closest_points(
                    first.position(),
                    first.shape(),
                    second.position(),
                    second.shape(),
                    Real::MAX,
                )
                .map_err(|e| anyhow!("those two shapes have no closest points: {e}"))?;
                Ok(match found {
                    crate::rapier3d::parry::query::ClosestPoints::Intersecting => Value::Nil,
                    crate::rapier3d::parry::query::ClosestPoints::WithinMargin(p, q) => map([
                        ("a", Value::Vec3(scalar::a3(p))),
                        ("b", Value::Vec3(scalar::a3(q))),
                    ]),
                    crate::rapier3d::parry::query::ClosestPoints::Disjoint => Value::Nil,
                })
            })
        },
    );
    m.function(
        "time_of_impact",
        |eng: &Engine, (a, b, opts): (NodeId, NodeId, Value)| {
            let opts = Opts(Some(&opts));
            let (va, vb) = (
                scalar::v3a(opts.vec3("velocity_a", [0.0; 3])),
                scalar::v3a(opts.vec3("velocity_b", [0.0; 3])),
            );
            let options = ShapeCastOptions {
                max_time_of_impact: scalar::real(opts.f32("max", 1.0)),
                stop_at_penetration: opts.boolean("stop_at_penetration", true),
                ..ShapeCastOptions::default()
            };
            with_pair(eng, a, b, |_, first, second| {
                let hit = crate::rapier3d::parry::query::cast_shapes(
                    first.position(),
                    va,
                    first.shape(),
                    second.position(),
                    vb,
                    second.shape(),
                    options,
                )
                .map_err(|e| anyhow!("those two shapes cannot be swept: {e}"))?;
                Ok(hit.map_or(Value::Nil, |hit| {
                    map([
                        ("distance", Value::Num(f64::from(hit.time_of_impact))),
                        ("point", Value::Vec3(scalar::a3(hit.witness1))),
                        ("normal", Value::Vec3(scalar::a3(hit.normal1))),
                    ])
                }))
            })
        },
    );
}

/// What the world holds: every body, and every body that is awake.
///
/// Split from [`install_pair_query_api`] under `MAX_FN_LINES`.
pub(crate) fn install_world_list_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("bodies", &[], "()", "Every node with a rigid body, sorted."),
        ("active_bodies", &[], "()", "Every node whose body is awake this step: what a game loops over when it wants to touch only what is moving."),
    ]);
    m.function("bodies", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let mut nodes: Vec<Entity> = state.bodies.keys().copied().collect();
        Ok(node_list(&mut nodes))
    });
    // Awake bodies only: a game that walks every body each frame to read a
    // position is doing the one thing sleeping was meant to save.
    m.function("active_bodies", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let mut nodes: Vec<Entity> = state
            .bodies
            .iter()
            .filter(|(_, handle)| {
                state
                    .world
                    .bodies
                    .get(**handle)
                    .is_some_and(|body| !body.is_sleeping())
            })
            .map(|(entity, _)| *entity)
            .collect();
        Ok(node_list(&mut nodes))
    });
    m.function("contacts", |eng: &Engine, node: NodeId| {
        contact_list(eng, node)
    });
    m.function("max_contact_impulse", |eng: &Engine, node: NodeId| {
        max_contact_impulse(eng, node)
    });
    m.function("intersects", |eng: &Engine, (a, b): (NodeId, NodeId)| {
        with_pair(eng, a, b, |_, first, second| {
            crate::rapier3d::parry::query::intersection_test(
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

/// Every contact point on a node's colliders, in the order rapier holds them
/// within a pair and by entity bits between pairs.
fn contact_list(eng: &Engine, node: NodeId) -> Result<Value> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let Some(handles) = state.colliders.get(&entity) else {
        return Ok(Value::List(Vec::new()));
    };
    let mut out: Vec<(u64, Value)> = Vec::new();
    for &handle in handles {
        for pair in state.world.contact_pairs_with(handle) {
            let other_handle = if pair.collider1 == handle {
                pair.collider2
            } else {
                pair.collider1
            };
            let Some(other) = entity_of_collider(&state.world.colliders[other_handle]) else {
                continue;
            };
            for manifold in &pair.manifolds {
                let normal = manifold.data.normal;
                for point in &manifold.points {
                    let p = point.local_p1;
                    out.push((
                        other.to_bits().get(),
                        map([
                            ("node", Value::Node(other.to_bits().get())),
                            ("point", Value::Vec3(scalar::a3(p))),
                            ("normal", Value::Vec3(scalar::a3(normal))),
                            ("impulse", Value::Num(f64::from(point.data.impulse))),
                        ]),
                    ));
                }
            }
        }
    }
    out.sort_by_key(|(bits, _)| *bits);
    Ok(Value::List(out.into_iter().map(|(_, v)| v).collect()))
}

/// The hardest contact a node took, which is what a damage threshold reads.
/// 2D has had this since it shipped; 3D gets it here.
fn max_contact_impulse(eng: &Engine, node: NodeId) -> Result<Real> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let Some(handles) = state.colliders.get(&entity) else {
        return Ok(0.0);
    };
    let mut max: Real = 0.0;
    for &handle in handles {
        for pair in state.world.contact_pairs_with(handle) {
            for manifold in &pair.manifolds {
                for point in &manifold.points {
                    max = max.max(point.data.impulse.abs());
                }
            }
        }
    }
    Ok(max)
}

/// Node lists cross the seam sorted by entity bits, for the same reason hit
/// lists cross it sorted by distance.
fn node_list(hits: &mut Vec<Entity>) -> Value {
    hits.sort_unstable_by_key(|e| e.to_bits());
    hits.dedup();
    Value::List(
        hits.iter()
            .map(|e| Value::Node(e.to_bits().get()))
            .collect(),
    )
}

/// The `shape` half of a shapecast or shape query, in the `collider3d`
/// vocabulary — so the shape you cast is spelled like the shape you attach.
fn shape_params(opts: &Opts<'_>) -> Result<toml::Value> {
    let shape = opts
        .get("shape")
        .ok_or_else(|| anyhow!("this query needs a `shape` table, in collider3d's vocabulary"))?;
    balaur_core::node_api::to_toml(shape)
}

/// Both nodes' first colliders, for the pairwise parry queries.
fn with_pair(
    eng: &Engine,
    a: NodeId,
    b: NodeId,
    f: impl FnOnce(&crate::rapier3d::pipeline::PhysicsWorld, &Collider, &Collider) -> Result<Value>,
) -> Result<Value> {
    let (a, b) = (entity_of(a)?, entity_of(b)?);
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let first = state
        .colliders
        .get(&a)
        .and_then(|h| h.first())
        .ok_or_else(|| anyhow!("the first node has no collider"))?;
    let second = state
        .colliders
        .get(&b)
        .and_then(|h| h.first())
        .ok_or_else(|| anyhow!("the second node has no collider"))?;
    f(
        &state.world,
        &state.world.colliders[*first],
        &state.world.colliders[*second],
    )
}

/// Nodes whose colliders intersect this node's, sorted by entity bits.
///
/// Rapier tracks an intersection pair only when one side is a sensor, which is
/// why this is not the same question as `shape_hits`.
pub fn overlaps(eng: &Engine, entity: Entity) -> Vec<Entity> {
    let state = eng.resource::<PhysicsState>();
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

/// `physics3d.overlaps`, as a node list.
pub(crate) fn overlaps_value(eng: &Engine, node: NodeId) -> Result<Vec<NodeId>> {
    Ok(overlaps(eng, entity_of(node)?)
        .into_iter()
        .map(node_id_of)
        .collect())
}
