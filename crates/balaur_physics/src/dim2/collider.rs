//! `collider2d`: shapes, their material, and the overlap and contact
//! queries that read them back.

use crate::rapier2d::math::Vector;
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

use crate::rapier2d::prelude::{
    ActiveCollisionTypes, ActiveEvents, ActiveHooks, CoefficientCombineRule, Collider,
    ColliderBuilder as ColliderBuilder2, ColliderHandle, Group, InteractionGroups,
    InteractionTestMode, MassProperties, RigidBodyHandle, SharedShape,
};
use crate::scalar::{self, Pose2, Real, Rotation2};

use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::dim2::{PhysicsState2d, decompose, node_pose_2d};
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

crate::shared::collider::functions!(state = PhysicsState2d);

fn pose_relative_to(eng: &Engine, entity: Entity, body_node: Entity) -> Result<Pose2> {
    let here = node_pose_2d(eng, entity)?;
    if entity == body_node {
        return Ok(Pose2::IDENTITY);
    }
    let there = node_pose_2d(eng, body_node)?;
    let inverse = there.rotation.inverse();
    Ok(Pose2::from_parts(
        inverse * (here.translation - there.translation),
        inverse * here.rotation,
    ))
}

pub(crate) fn add_collider_at(
    eng: &Engine,
    entity: Entity,
    builder: ColliderBuilder2,
    offset: Pose2,
) -> Result<()> {
    let handle = if let Some((body_node, body)) = nearest_body(eng, entity) {
        let local = pose_relative_to(eng, entity, body_node)?;
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        state
            .world
            .insert_collider(builder.position(local * offset), Some(body))
    } else {
        let pose = node_pose_2d(eng, entity)?;
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        state
            .world
            .insert_collider(builder.position(pose * offset), None)
    };
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    state.world.colliders[handle].user_data = u128::from(entity.to_bits().get());
    if let Some(body) = state.world.colliders[handle].parent()
        && crate::dim2::body::has_total_mass(&state.world.bodies[body])
    {
        let world = &mut state.world;
        world.colliders[handle].set_density(0.0);
        world.bodies[body].recompute_mass_properties_from_colliders(&world.colliders);
    }
    state.colliders.entry(entity).or_default().push(handle);
    state.queries_ready = false;
    Ok(())
}

/// The collider described by `params`, in the `collider2d` schema's own
/// vocabulary — so a script table and a scene-file entry build the same thing.
pub(crate) fn collider_builder(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder2> {
    let kind = v::text(params, k::KIND, w::RECTANGLE);
    let radius = scalar::real(v::f(params, k::RADIUS, 0.5)).max(0.01);
    // `height` runs tip to tip, as in 3D; the segment is what the caps leave.
    let half_segment =
        (scalar::real(v::f(params, k::HEIGHT, 2.0).max(0.01)) / 2.0 - radius).max(0.0);
    let he = |i: usize| scalar::real(v::axis(params, k::SIZE, i, 1.0) / 2.0).max(0.01);
    let point = |key: &str, fallback: [f32; 2]| scalar::v2a(v::vec2(params, key, fallback));
    let border = scalar::real(v::f(params, k::EDGE_RADIUS, 0.0)).max(0.0);
    let rounded = border > 0.0;
    let mut mass = None;
    let builder = match kind {
        w::CIRCLE => ColliderBuilder2::ball(radius),
        w::RECTANGLE if rounded => ColliderBuilder2::round_cuboid(he(0), he(1), border),
        w::RECTANGLE => ColliderBuilder2::cuboid(he(0), he(1)),
        w::CAPSULE => ColliderBuilder2::capsule_y(half_segment, radius),
        w::TRIANGLE if rounded => ColliderBuilder2::round_triangle(
            point(k::A, [0.0, 0.0]),
            point(k::B, [1.0, 0.0]),
            point(k::C, [0.0, 1.0]),
            border,
        ),
        w::TRIANGLE => ColliderBuilder2::triangle(
            point(k::A, [0.0, 0.0]),
            point(k::B, [1.0, 0.0]),
            point(k::C, [0.0, 1.0]),
        ),
        w::SEGMENT => ColliderBuilder2::segment(point(k::A, [0.0, 0.0]), point(k::B, [1.0, 0.0])),
        w::WORLD_BOUNDARY => {
            let n = point(k::NORMAL, [0.0, 1.0]);
            if n.length_squared() < 1.0e-12 {
                return Err(anyhow!(
                    "a world_boundary collider needs a non-zero `normal`"
                ));
            }
            ColliderBuilder2::new(crate::rapier2d::prelude::SharedShape::halfspace(
                n.normalize(),
            ))
        }
        w::TRIANGLE_MESH | w::CONVEX_HULL | w::POLYLINE => mesh_collider(eng, params, kind)?,
        w::CONVEX_DECOMPOSITION => {
            let (shape, weight) = decomposition_collider(eng, params)?;
            mass = weight;
            shape
        }
        w::VOXELS => voxel_collider(eng, params)?,
        w::HEIGHTFIELD => heightfield_collider(eng, params)?,
        other => return Err(anyhow!("unknown collider2d kind '{other}'")),
    };
    let builder = with_material(builder, params);
    Ok(match mass {
        // `mass` pins the total itself, and wins where it is written.
        Some(mass) if v::f(params, k::MASS, 0.0) <= 0.0 => builder.mass_properties(mass),
        _ => builder,
    })
}

/// A 2D shape from a `mesh` asset, reading the x and y of its points: the
/// same asset a `polygon` draws, so the outline a player sees and the one they
/// collide with are one authored thing.
fn mesh_collider(eng: &Engine, params: &toml::Value, kind: &str) -> Result<ColliderBuilder2> {
    let (points, indices) = mesh_of(eng, params, kind)?;
    match kind {
        // The flags 3D passes, for the same reason: without them a body
        // catches on the seam between two triangles of flat ground.
        w::TRIANGLE_MESH => {
            let mut flags = crate::rapier2d::prelude::TriMeshFlags::empty();
            if v::boolean(params, k::FIX_INTERNAL_EDGES, true) {
                flags |= crate::rapier2d::prelude::TriMeshFlags::FIX_INTERNAL_EDGES;
            }
            if v::boolean(params, k::WELD_VERTICES, false) {
                flags |= crate::rapier2d::prelude::TriMeshFlags::MERGE_DUPLICATE_VERTICES
                    | crate::rapier2d::prelude::TriMeshFlags::DELETE_DEGENERATE_TRIANGLES
                    | crate::rapier2d::prelude::TriMeshFlags::DELETE_BAD_TOPOLOGY_TRIANGLES;
            }
            if v::boolean(params, k::ORIENTED, false) {
                flags |= crate::rapier2d::prelude::TriMeshFlags::ORIENTED;
            }
            ColliderBuilder2::trimesh_with_flags(points, indices, flags)
                .map_err(|e| anyhow!("that mesh cannot be a triangle_mesh collider: {e}"))
        }
        w::CONVEX_HULL => ColliderBuilder2::convex_hull(&points)
            .ok_or_else(|| anyhow!("those {} points have no hull", points.len())),
        _ => {
            if points.len() < 2 {
                return Err(anyhow!(
                    "a polyline collider needs at least two points, not {}",
                    points.len()
                ));
            }
            // `oriented` is opt-in: it reads the winding to decide which side
            // is solid, so it is wrong on a chain wound the other way.
            if v::boolean(params, k::ORIENTED, false) {
                return Ok(ColliderBuilder2::oriented_polyline(points, None));
            }
            Ok(ColliderBuilder2::polyline(points, None))
        }
    }
}

/// Put a 2D collider on the layers `params` names, for a builder whose other
/// rows its owner has already set (see `crate::collider::with_groups`).
pub(crate) fn with_groups_2d(builder: ColliderBuilder2, params: &toml::Value) -> ColliderBuilder2 {
    builder.collision_groups(InteractionGroups::new(
        Group::from_bits_truncate(v::layer_bits(params, k::COLLISION_LAYER, false)),
        Group::from_bits_truncate(v::layer_bits(params, k::COLLISION_MASK, true)),
        InteractionTestMode::And,
    ))
}

/// Report what `params` asks for, as `crate::collider::with_events` does in 3D.
pub(crate) fn with_events_2d(builder: ColliderBuilder2, params: &toml::Value) -> ColliderBuilder2 {
    let events = ActiveEvents::from_bits_truncate(v::bits(params, k::EVENTS, &v::flags::events()));
    let threshold = scalar::real(v::f(params, k::CONTACT_FORCE_THRESHOLD, 0.0));
    builder
        .active_events(events)
        .contact_force_event_threshold(threshold)
}

/// The `mesh` asset's points as 2D, with the triangles over them.
fn mesh_of(eng: &Engine, params: &toml::Value, kind: &str) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
    let reference = params
        .get(k::MESH)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a {kind} collider2d needs a `mesh` asset"))?;
    let definition =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)?;
    let mesh = balaur_core::mesh::load_from(eng, &definition)?;
    let points = mesh
        .positions
        .iter()
        .map(|p| scalar::v2(p[0], p[1]))
        .collect();
    Ok((points, mesh.indices.clone()))
}

/// A concave polygon as convex pieces that overlap across their seams: the
/// only shape a concave *dynamic* 2D body can have and not wedge a thin one.
///
/// The mass is the ungrown pieces', since the grown ones share the ground
/// they overlap on and would weigh it twice.
fn decomposition_collider(
    eng: &Engine,
    params: &toml::Value,
) -> Result<(ColliderBuilder2, Option<MassProperties>)> {
    let (points, indices) = mesh_of(eng, params, w::CONVEX_DECOMPOSITION)?;
    let border = scalar::real(v::f(params, k::EDGE_RADIUS, 0.0)).max(0.0);
    if v::text(params, k::METHOD, w::EXACT) == w::VHACD {
        return Ok((vhacd_collider(params, &points, &indices, border), None));
    }
    let pieces = decompose::pieces(&points, &indices);
    let overlap = v::f(params, k::OVERLAP, 0.9);
    let density = scalar::real(v::f(params, k::DENSITY, 1.0).max(0.001));
    let mut weights = pieces.iter().map(|piece| {
        let ring: Vec<Vector> = piece.iter().map(|&at| points[at as usize]).collect();
        MassProperties::from_convex_polygon(density, &ring)
    });
    let mass = weights
        .next()
        .map(|first| weights.fold(first, |sum, next| sum + next));
    let shapes: Vec<_> = decompose::grown(&points, &pieces, overlap)
        .into_iter()
        .filter_map(|piece| Some((Pose2::IDENTITY, convex_piece(piece, border)?)))
        .collect();
    if shapes.is_empty() {
        return Err(anyhow!(
            "that mesh has no area, so it cannot be cut into convex pieces"
        ));
    }
    Ok((ColliderBuilder2::compound(shapes), mass))
}

/// One piece as a shape, rounded when the collider asked for a border.
fn convex_piece(piece: Vec<Vector>, border: Real) -> Option<SharedShape> {
    if border > 0.0 {
        SharedShape::round_convex_polyline(piece, border)
    } else {
        SharedShape::convex_polyline(piece)
    }
}

/// The approximate cut, for an outline dense enough that the exact one's
/// quadratic merge shows. rapier voxelises the outline, so it takes segments.
fn vhacd_collider(
    params: &toml::Value,
    points: &[Vector],
    indices: &[[u32; 3]],
    border: Real,
) -> ColliderBuilder2 {
    let mut tuning = crate::rapier2d::parry::transformation::vhacd::VHACDParameters::default();
    tuning.resolution = v::f(params, k::RESOLUTION, tuning.resolution as f32).max(1.0) as u32;
    tuning.concavity = scalar::real(v::f(
        params,
        k::MAX_CONCAVITY,
        scalar::f32_of(tuning.concavity),
    ));
    tuning.max_convex_hulls =
        v::f(params, k::MAX_CONVEX_HULLS, tuning.max_convex_hulls as f32).max(1.0) as u32;
    let outline = boundary(indices);
    if border > 0.0 {
        return ColliderBuilder2::round_convex_decomposition_with_params(
            points, &outline, &tuning, border,
        );
    }
    ColliderBuilder2::convex_decomposition_with_params(points, &outline, &tuning)
}

/// The outline of a triangulated mesh: every edge only one triangle uses,
/// which is what a 2D decomposition over a polyline needs.
fn boundary(indices: &[[u32; 3]]) -> Vec<[u32; 2]> {
    let mut edges: std::collections::BTreeMap<(u32, u32), [u32; 2]> =
        std::collections::BTreeMap::new();
    for &[a, b, c] in indices {
        for edge in [[a, b], [b, c], [c, a]] {
            let key = (edge[0].min(edge[1]), edge[0].max(edge[1]));
            if edges.remove(&key).is_none() {
                edges.insert(key, edge);
            }
        }
    }
    edges.into_values().collect()
}

/// A 2D heightfield is one row of heights: a side-scroller's ground.
/// A voxel grid from a `voxels` asset, the 2D twin of the 3D kind. parry
/// classifies each cell from its neighbours, so a body sliding along a wall
/// of them cannot catch on the seam between two.
fn voxel_collider(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder2> {
    let reference = params
        .get(k::VOXELS)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a voxels collider2d needs a `voxels` asset"))?;
    let grid = balaur_core::assets::load_typed::<balaur_core::voxels::VoxelsData>(eng, reference)?;
    let cells: Vec<crate::rapier2d::math::IVector> = grid
        .cells
        .iter()
        .map(|c| scalar::cell2(c[0], c[1]))
        .collect();
    Ok(ColliderBuilder2::voxels(
        scalar::v2(grid.size[0], grid.size[1]),
        &cells,
    ))
}

fn heightfield_collider(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder2> {
    let reference = params
        .get(k::HEIGHTFIELD)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a heightfield collider2d needs a `heightfield` asset"))?;
    let field = balaur_core::assets::load_typed::<balaur_core::heightfield::HeightfieldData>(
        eng, reference,
    )?;
    Ok(ColliderBuilder2::heightfield(
        field.heights.iter().map(|h| scalar::real(*h)).collect(),
        scalar::v2a(v::vec2(params, k::SCALE, [1.0, 1.0])),
    ))
}

/// The 2D half of `crate::collider::with_material`. The flag tables are
/// shared (`crate::vocabulary::flags`); only the types they are poured into
/// are per-dimension.
pub(crate) fn with_material(builder: ColliderBuilder2, params: &toml::Value) -> ColliderBuilder2 {
    let mut builder = builder
        .restitution(scalar::real(v::f(params, k::RESTITUTION, 0.0)))
        .friction(scalar::real(v::f(params, k::FRICTION, 0.5)))
        .density(scalar::real(v::f(params, k::DENSITY, 1.0).max(0.001)))
        .friction_combine_rule(combine_rule(v::text(
            params,
            k::FRICTION_COMBINE,
            w::AVERAGE,
        )))
        .restitution_combine_rule(combine_rule(v::text(
            params,
            k::RESTITUTION_COMBINE,
            w::AVERAGE,
        )))
        .contact_skin(scalar::real(
            v::f(params, k::COLLISION_MARGIN, 0.0).max(0.0),
        ))
        .contact_force_event_threshold(scalar::real(v::f(params, k::CONTACT_FORCE_THRESHOLD, 0.0)))
        .collision_groups(InteractionGroups::new(
            Group::from_bits_truncate(v::layer_bits(params, k::COLLISION_LAYER, false)),
            Group::from_bits_truncate(v::layer_bits(params, k::COLLISION_MASK, true)),
            InteractionTestMode::And,
        ))
        .solver_groups(InteractionGroups::new(
            Group::from_bits_truncate(v::layer_bits(params, k::SOLVER_LAYER, false)),
            Group::from_bits_truncate(v::layer_bits(params, k::SOLVER_MASK, true)),
            InteractionTestMode::And,
        ))
        .active_collision_types(ActiveCollisionTypes::from_bits_truncate(v::bits(
            params,
            k::CONTACT_PAIRS,
            &v::flags::collision_types(),
        )))
        .active_events(ActiveEvents::from_bits_truncate(v::bits(
            params,
            k::EVENTS,
            &v::flags::events(),
        )))
        .active_hooks(active_hooks(params))
        .enabled(v::boolean(params, k::ENABLED, true))
        .sensor(v::boolean(params, k::SENSOR, false));
    let mass = scalar::real(v::f(params, k::MASS, 0.0));
    if mass > 0.0 {
        builder = builder.mass(mass);
    }
    builder
}

/// Build and insert the collider described by `params`, replacing any
/// existing one.
pub(crate) fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let builder = collider_builder(eng, params)?;
    let offset = Pose2::from_parts(
        scalar::v2a(v::vec2(params, k::OFFSET, [0.0; 2])),
        Rotation2::from_angle(scalar::real(v::f(params, k::OFFSET_ROTATION, 0.0))),
    );
    remove_colliders(eng, entity);
    add_collider_at(eng, entity, builder, offset)?;
    {
        let state = eng.resource::<PhysicsState2d>();
        state
            .borrow_mut()
            .collider_params
            .insert(entity, params.clone());
    }
    if v::boolean(params, k::ONE_WAY, false) {
        // The axis rides in the collider's `user_data`, where the hook can
        // read it mid-step; 2D packs the same three bits 3D does.
        let axis = v::vec2(params, k::ONE_WAY_AXIS, [0.0, 1.0]);
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let handles = state.colliders.get(&entity).cloned().unwrap_or_default();
        for handle in handles {
            state.world.colliders[handle].user_data =
                crate::collider::encode_one_way(entity.to_bits().get(), [axis[0], axis[1], 0.0]);
        }
    }
    Ok(())
}

pub(crate) fn get_collider_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = state.colliders.get(&entity)?.first()?;
    let collider = state.world.colliders.get(*handle)?;
    // What it was authored from, under what rapier can report, as in 3D: the
    // asset names, the offset and `one_way` survive a re-save.
    let mut map = state
        .collider_params
        .get(&entity)
        .and_then(|params| params.as_table().cloned())
        .unwrap_or_default();
    if let Some(shape) = shape_params(collider) {
        map.extend(shape);
    }
    read_material(collider, &mut map);
    Some(toml::Value::Table(map))
}

/// The shape half of a `collider2d`'s params. `None` for the asset-backed
/// kinds: rapier keeps the geometry, not the file it came from.
fn shape_params(collider: &Collider) -> Option<toml::map::Map<String, toml::Value>> {
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let vec2 = |x: Real, y: Real| toml::Value::Array(vec![f(x), f(y)]);
    let shape = collider.shape();
    let mut map = toml::map::Map::new();
    if let Some(ball) = shape.as_ball() {
        map.insert(k::KIND.into(), w::CIRCLE.into());
        map.insert(k::RADIUS.into(), f(ball.radius));
        return Some(map);
    }
    if let Some(capsule) = shape.as_capsule() {
        map.insert(k::KIND.into(), w::CAPSULE.into());
        map.insert(k::RADIUS.into(), f(capsule.radius));
        let straight = (capsule.segment.b - capsule.segment.a).length();
        map.insert(k::HEIGHT.into(), f(straight + 2.0 * capsule.radius));
        return Some(map);
    }
    if let Some(cuboid) = shape.as_cuboid() {
        map.insert(k::KIND.into(), w::RECTANGLE.into());
        let he = cuboid.half_extents * 2.0;
        map.insert(k::SIZE.into(), vec2(he.x, he.y));
        return Some(map);
    }
    if let Some(round) = shape.as_round_cuboid() {
        map.insert(k::KIND.into(), w::RECTANGLE.into());
        let he = round.inner_shape.half_extents * 2.0;
        map.insert(k::SIZE.into(), vec2(he.x, he.y));
        map.insert(k::EDGE_RADIUS.into(), f(round.border_radius));
        return Some(map);
    }
    if let Some(tri) = shape.as_triangle() {
        map.insert(k::KIND.into(), w::TRIANGLE.into());
        map.insert(k::A.into(), vec2(tri.a.x, tri.a.y));
        map.insert(k::B.into(), vec2(tri.b.x, tri.b.y));
        map.insert(k::C.into(), vec2(tri.c.x, tri.c.y));
        return Some(map);
    }
    if let Some(segment) = shape.as_segment() {
        map.insert(k::KIND.into(), w::SEGMENT.into());
        map.insert(k::A.into(), vec2(segment.a.x, segment.a.y));
        map.insert(k::B.into(), vec2(segment.b.x, segment.b.y));
        return Some(map);
    }
    if let Some(halfspace) = shape.as_halfspace() {
        map.insert(k::KIND.into(), w::WORLD_BOUNDARY.into());
        map.insert(
            k::NORMAL.into(),
            vec2(halfspace.normal.x, halfspace.normal.y),
        );
        return Some(map);
    }
    None
}

/// Largest contact normal impulse currently applied to the node's colliders
/// (0 when untouched). Gameplay uses this for impact damage.
pub(crate) fn max_contact_impulse(eng: &Engine, entity: Entity) -> Real {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let Some(handles) = state.colliders.get(&entity) else {
        return 0.0;
    };
    let mut max: Real = 0.0;
    for &handle in handles {
        for pair in state.world.narrow_phase.contact_pairs_with(handle) {
            for manifold in pair.manifolds() {
                for point in &manifold.points {
                    max = max.max(point.data.impulse.abs());
                }
            }
        }
    }
    max
}

/// The `collider2d` key, backed by no component type: it writes into
/// [`crate::PhysicsState2d`].
pub(crate) fn register_collider2d_component(reg: &mut Registry<'_>) {
    let shapes = v::options(w::SHAPES_2D);
    let default = w::RECTANGLE;
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{default}", options = [{shapes}], description = "Collision shape" }}"#)),
            (k::RADIUS, r#"{ type = "float", default = 0.5, min = 0.01, description = "Circle radius, when kind is circle or capsule" }"#),
            (k::HEIGHT, r#"{ type = "float", default = 2.0, min = 0.01, description = "Length along y, tip to tip, when kind is capsule" }"#),
            (k::SIZE, r#"{ type = "vec2", default = [1.0, 1.0], description = "Whole size along each axis, when kind is rectangle" }"#),
            (k::EDGE_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "Rounds a rect or triangle by this radius, so it slides over seams instead of catching on them", group = "shape" }"#),
            (k::A, r#"{ type = "vec2", default = [0.0, 0.0], description = "First corner, when kind is triangle or segment", group = "shape" }"#),
            (k::B, r#"{ type = "vec2", default = [1.0, 0.0], description = "Second corner, when kind is triangle or segment", group = "shape" }"#),
            (k::C, r#"{ type = "vec2", default = [0.0, 1.0], description = "Third corner, when kind is triangle", group = "shape" }"#),
            (k::NORMAL, r#"{ type = "vec2", default = [0.0, 1.0], description = "Which way the infinite line faces, when kind is world_boundary", group = "shape" }"#),
            (k::MESH, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "Points and triangles for a triangle_mesh, convex_hull, convex_decomposition or polyline collider: the same asset a polygon draws", group = "shape" }}"#, balaur_core::mesh::MESH_ASSET_TYPE)),
            (k::HEIGHTFIELD, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "A row of heights, when kind is heightfield: a side-scroller's ground", group = "shape" }}"#, balaur_core::heightfield::HEIGHTFIELD_ASSET_TYPE)),
            (k::SCALE, r#"{ type = "vec2", default = [1.0, 1.0], description = "Width and height scale of a heightfield", group = "shape" }"#),
            (k::VOXELS, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "Filled cells, when kind is voxels; a script may dig into them while the game runs", group = "shape" }}"#, balaur_core::voxels::VOXELS_ASSET_TYPE)),
            (k::FIX_INTERNAL_EDGES, r#"{ type = "bool", default = true, description = "Take neighbouring triangles into account for a triangle_mesh's contacts, so a body does not catch on the seam between two of them", group = "contacts" }"#),
            (k::WELD_VERTICES, r#"{ type = "bool", default = false, description = "Merge duplicate vertices and drop degenerate triangles when building a triangle_mesh", group = "shape" }"#),
            (k::ORIENTED, r#"{ type = "bool", default = false, description = "Treat a triangle_mesh or polyline as one-sided: the winding decides which side is solid, counter-clockwise enclosing the solid", group = "shape" }"#),
            (k::OVERLAP, r#"{ type = "float", default = 0.9, min = 0.0, max = 1.0, description = "How far a convex_decomposition piece grows through each seam it shares, so nothing wedges into one: 0 leaves the plain pieces, 1 grows flush with the face that stops it", group = "shape" }"#),
            (k::METHOD, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How a convex_decomposition is cut: exact, over the mesh's own triangles, or vhacd, which voxelises the outline", group = "shape" }}"#, w::EXACT, v::options(w::DECOMPOSITION_METHODS))),
            (k::RESOLUTION, r#"{ type = "int", default = 64, min = 1, description = "How fine the voxel grid is, when method is vhacd", group = "shape" }"#),
            (k::MAX_CONCAVITY, r#"{ type = "float", default = 0.01, min = 0.0, description = "How deep a dent a vhacd piece may keep before it is cut again", group = "shape" }"#),
            (k::MAX_CONVEX_HULLS, r#"{ type = "int", default = 1024, min = 1, description = "The most pieces a vhacd cut may leave", group = "shape" }"#),
            (k::OFFSET, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where the shape sits relative to the node", group = "shape" }"#),
            (k::OFFSET_ROTATION, r#"{ type = "float", default = 0.0, description = "How the shape is turned relative to the node, in radians", group = "shape" }"#),
            (k::ONE_WAY_AXIS, r#"{ type = "vec2", default = [0.0, 1.0], description = "The direction a one-way platform lets bodies through from", group = "contacts" }"#),
        ]),
        crate::collider::shared_collider_schema(),
    ]
    .join("\n");
    reg.register_component(
        c::COLLIDER_2D,
        ComponentDef {
            events: crate::vocabulary::hook::COLLIDER,
            warnings: None,
            doc: "The node's 2D collision shape, chosen by `kind`. It belongs to the node's `body2d` or the nearest body above it; without one it is static geometry.",
            schema: ComponentDef::parse_schema(c::COLLIDER_2D, &schema),
            tags: &[balaur_core::components::tag::DIM_2D, balaur_core::components::tag::PHYSICS],
            expects: &[],
            apply: Box::new(apply_collider),
            remove: Box::new(|eng, entity| {
                remove_colliders(eng, entity);
                Ok(())
            }),
            get: Box::new(get_collider_params),
        },
    );
}

/// One voxel grid under a node, for the calls that edit it.
fn with_voxels(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&mut crate::rapier2d::parry::shape::Voxels),
) -> Result<()> {
    let entity = balaur_core::entity_of(node)?;
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let handle = first_collider(&state, entity)?;
    let collider = &mut state.world.colliders[handle];
    let voxels = collider
        .shape_mut()
        .as_voxels_mut()
        .ok_or_else(|| anyhow!("this node's collider is not a voxel grid"))?;
    f(voxels);
    state.shape_revision = state.shape_revision.wrapping_add(1);
    Ok(())
}

/// Editing a 2D voxel grid, the twin of `crate::collider::install_voxel_api`.
pub(crate) fn install_voxel_2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_voxel", &[c::COLLIDER_2D], "", "Fill or empty one cell of a voxel collider: digging a hole, or building a wall, while the game runs."),
        ("voxel", &[c::COLLIDER_2D], "", "Whether one cell of a voxel collider is filled."),
        ("voxel_at", &[c::COLLIDER_2D], "", "The cell a world position falls in, as two whole numbers."),
    ]);
    m.function(
        "set_voxel",
        |eng: &Engine, (node, x, y, filled): (NodeId, i32, i32, bool)| {
            with_voxels(eng, node, |voxels| {
                voxels.set_voxel(scalar::cell2(x, y), filled);
            })
        },
    );
    m.function("voxel", |eng: &Engine, (node, x, y): (NodeId, i32, i32)| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let handle = first_collider(&state, entity)?;
        let voxels = state.world.colliders[handle]
            .shape()
            .as_voxels()
            .ok_or_else(|| anyhow!("this node's collider is not a voxel grid"))?;
        Ok(voxels
            .voxel_state(scalar::cell2(x, y))
            .is_some_and(|state| !state.is_empty()))
    });
    m.function(
        "voxel_at",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            let entity = balaur_core::entity_of(node)?;
            let state = eng.resource::<PhysicsState2d>();
            let state = state.borrow();
            let handle = first_collider(&state, entity)?;
            let collider = &state.world.colliders[handle];
            let voxels = collider
                .shape()
                .as_voxels()
                .ok_or_else(|| anyhow!("this node's collider is not a voxel grid"))?;
            // The grid is in the collider's own space, so a world point has to
            // come home first.
            let local = collider.position().inverse() * scalar::v2(x, y);
            let cell = voxels.voxel_at_point(local);
            Ok((i64::from(cell.x), i64::from(cell.y)))
        },
    );
}
