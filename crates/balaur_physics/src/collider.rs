//! The `collider3d` component: every shape it can take, and the overlap
//! query that reads them back.

use crate::rapier3d::math::Vector;
use anyhow::{anyhow, bail, Result};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine};
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::rapier3d::prelude::{
    ActiveCollisionTypes, ActiveEvents, ActiveHooks, CoefficientCombineRule, Collider,
    ColliderBuilder, Group, InteractionGroups, InteractionTestMode, RigidBodyHandle,
};
use crate::scalar::{self, Pose, Real};

use crate::vocabulary as v;
use crate::{node_pose, PhysicsState};

/// The geometry a mesh-backed collider names, through the same asset the
/// renderer uses.
fn collider_mesh(eng: &Engine, params: &toml::Value) -> Result<balaur_core::mesh::MeshData> {
    let reference = params
        .get("mesh")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a trimesh or convex_hull collider needs a `mesh` asset"))?;
    let definition =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)?;
    balaur_core::mesh::load_from(eng, &definition)
}

/// The collider kinds built from a `mesh` asset, so the model the renderer
/// draws and the shape it collides with stay one authored thing.
fn mesh_collider(eng: &Engine, params: &toml::Value, kind: &str) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, params)?;
    let points: Vec<Vector> = mesh.positions.iter().map(|p| scalar::v3a(*p)).collect();
    match kind {
        // The flags are what stop a character controller catching on the seam
        // between two triangles of a flat floor.
        "trimesh" => {
            let mut flags = crate::rapier3d::prelude::TriMeshFlags::empty();
            if v::boolean(params, "fix_internal_edges", true) {
                flags |= crate::rapier3d::prelude::TriMeshFlags::FIX_INTERNAL_EDGES;
            }
            if v::boolean(params, "clean", false) {
                flags |= crate::rapier3d::prelude::TriMeshFlags::MERGE_DUPLICATE_VERTICES
                    | crate::rapier3d::prelude::TriMeshFlags::DELETE_DEGENERATE_TRIANGLES
                    | crate::rapier3d::prelude::TriMeshFlags::DELETE_BAD_TOPOLOGY_TRIANGLES;
            }
            if v::boolean(params, "oriented", false) {
                flags |= crate::rapier3d::prelude::TriMeshFlags::ORIENTED;
            }
            ColliderBuilder::trimesh_with_flags(points, mesh.indices.clone(), flags)
                .map_err(|e| anyhow!("that mesh cannot be a trimesh collider: {e}"))
        }
        // The only way to get a *dynamic* concave shape: rapier's VHACD cuts
        // the mesh into convex pieces and keeps them as one compound.
        "convex_decomposition" => Ok(ColliderBuilder::convex_decomposition(
            &points,
            &mesh.indices,
        )),
        // A shape fitted to the mesh rather than made of it: a box, an
        // oriented box, or a hull, whichever `fit` asks for.
        "fit" => {
            let converter = match v::text(params, "fit", "convex_hull") {
                "aabb" => crate::rapier3d::prelude::MeshConverter::Aabb,
                "obb" => crate::rapier3d::prelude::MeshConverter::Obb,
                "convex_decomposition" => {
                    crate::rapier3d::prelude::MeshConverter::ConvexDecomposition
                }
                _ => crate::rapier3d::prelude::MeshConverter::ConvexHull,
            };
            ColliderBuilder::converted_trimesh(points, mesh.indices.clone(), converter)
                .map_err(|e| anyhow!("that mesh cannot be fitted: {e}"))
        }
        "convex_hull" => Ok(ColliderBuilder::convex_hull(&points).unwrap_or_else(|| {
            // Degenerate input (every point on one line or plane) has no hull.
            // The node keeps a collider rather than losing one silently.
            let (min, max) = mesh.bounds().unwrap_or(([-0.5; 3], [0.5; 3]));
            tracing::warn!(
                "convex_hull: those {} points are degenerate; using their bounding box",
                points.len()
            );
            ColliderBuilder::cuboid(
                scalar::real(((max[0] - min[0]) / 2.0).max(0.01)),
                scalar::real(((max[1] - min[1]) / 2.0).max(0.01)),
                scalar::real(((max[2] - min[2]) / 2.0).max(0.01)),
            )
        })),
        _ => {
            if points.len() < 2 {
                bail!(
                    "a polyline collider needs at least two points, not {}",
                    points.len()
                );
            }
            // `None` chains the points in order, which is what a mesh's vertex
            // list means; a closed loop repeats the first point at the end.
            Ok(ColliderBuilder::polyline(points, None))
        }
    }
}

/// A voxel grid from a `voxels` asset: filled cells on a lattice, editable
/// from a script while the game runs.
fn voxel_collider(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder> {
    let reference = params
        .get("voxels")
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a voxels collider needs a `voxels` asset"))?;
    let grid = balaur_core::assets::load_typed::<balaur_core::voxels::VoxelsData>(eng, reference)?;
    let cells: Vec<crate::rapier3d::math::IVector> = grid
        .cells
        .iter()
        .map(|c| crate::rapier3d::math::IVector::new(c[0], c[1], c[2]))
        .collect();
    let size = scalar::v3a(grid.size);
    Ok(ColliderBuilder::voxels(size, &cells))
}

/// A voxel grid built from a mesh, so a model can become destructible terrain
/// without anyone authoring a cell list.
fn voxelized_mesh_collider(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, params)?;
    let points: Vec<Vector> = mesh.positions.iter().map(|p| scalar::v3a(*p)).collect();
    let size = scalar::real(v::f(params, "voxel_size", 0.25).max(0.001));
    let fill = if v::text(params, "fill", "solid") == "surface" {
        crate::rapier3d::parry::transformation::voxelization::FillMode::SurfaceOnly
    } else {
        crate::rapier3d::parry::transformation::voxelization::FillMode::FloodFill {
            detect_cavities: false,
        }
    };
    Ok(ColliderBuilder::voxelized_mesh(
        &points,
        &mesh.indices,
        size,
        fill,
    ))
}

/// Terrain from a `heightfield` asset. The extent belongs to the collider, so
/// one grid can be placed at several sizes.
fn heightfield_collider(
    eng: &Engine,
    params: &toml::Value,
    extent: Vector,
) -> Result<ColliderBuilder> {
    let reference = params
        .get("heightfield")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a heightfield collider needs a `heightfield` asset"))?;
    let field = balaur_core::assets::load_typed::<balaur_core::heightfield::HeightfieldData>(
        eng, reference,
    )?;
    // The asset checked its own shape, so Array2's assert cannot fire. Through
    // rapier's re-export, so parry cannot drift from rapier's own version.
    // The asset is f32; a f64 build widens each height here, once, on load.
    let heights: Vec<Real> = field.heights.iter().map(|h| scalar::real(*h)).collect();
    let grid = crate::rapier3d::parry::utils::Array2::new(field.rows, field.columns, heights);
    Ok(ColliderBuilder::heightfield(grid, extent))
}

/// The collider described by `params`, in the `collider` schema's own
/// vocabulary — so a script table and a scene-file entry build the same thing.
pub(crate) fn collider_builder(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder> {
    let kind = params
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("cuboid");
    let f = |key: &str, default: f64| {
        params
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    let he = |i: usize| -> Real {
        params
            .get("half_extents")
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(0.5) as Real
    };
    let point = |key: &str, fallback: [f32; 3]| {
        let read = |i: usize| {
            params
                .get(key)
                .and_then(|v| v.as_array())
                .and_then(|a| a.get(i))
                .and_then(balaur_core::components::as_f64)
                .map(|v| v as Real)
        };
        Vector::new(
            read(0).unwrap_or(scalar::real(fallback[0])),
            read(1).unwrap_or(scalar::real(fallback[1])),
            read(2).unwrap_or(scalar::real(fallback[2])),
        )
    };
    let radius = scalar::real(f("radius", 0.5)).max(0.01);
    // rapier measures these from the centre; `height` is the whole straight
    // part, matching what the `shape` component means by it.
    let half_height = (scalar::real(f("height", 1.0)).max(0.01)) / 2.0;
    // A rounded shape is a shape plus a border radius, not nine more kinds.
    // Ball and capsule are already round, so they ignore it.
    let border = scalar::real(f("border", 0.0)).max(0.0);
    let rounded = border > 0.0;
    let builder = match kind {
        "ball" => ColliderBuilder::ball(radius),
        "cuboid" if rounded => {
            ColliderBuilder::round_cuboid(he(0).max(0.01), he(1).max(0.01), he(2).max(0.01), border)
        }
        "cuboid" => ColliderBuilder::cuboid(he(0).max(0.01), he(1).max(0.01), he(2).max(0.01)),
        "capsule" => ColliderBuilder::capsule_y(half_height, radius),
        "cylinder" if rounded => ColliderBuilder::round_cylinder(half_height, radius, border),
        "cylinder" => ColliderBuilder::cylinder(half_height, radius),
        "cone" if rounded => ColliderBuilder::round_cone(half_height, radius, border),
        "cone" => ColliderBuilder::cone(half_height, radius),
        "triangle" if rounded => ColliderBuilder::round_triangle(
            point("a", [0.0, 0.0, 0.0]),
            point("b", [1.0, 0.0, 0.0]),
            point("c", [0.0, 1.0, 0.0]),
            border,
        ),
        "triangle" => ColliderBuilder::triangle(
            point("a", [0.0, 0.0, 0.0]),
            point("b", [1.0, 0.0, 0.0]),
            point("c", [0.0, 1.0, 0.0]),
        ),
        "trimesh" | "convex_hull" | "polyline" | "convex_decomposition" | "fit" => {
            mesh_collider(eng, params, kind)?
        }
        "voxels" => voxel_collider(eng, params)?,
        "voxelized_mesh" => voxelized_mesh_collider(eng, params)?,
        "heightfield" => heightfield_collider(eng, params, point("scale", [1.0, 1.0, 1.0]))?,
        // An infinite plane, for a floor that needs no size and no triangles.
        "halfspace" => {
            let n = point("normal", [0.0, 1.0, 0.0]);
            if n.length_squared() < 1.0e-12 {
                bail!("a halfspace collider needs a non-zero `normal`");
            }
            ColliderBuilder::new(crate::rapier3d::prelude::SharedShape::halfspace(
                n.normalize(),
            ))
        }
        "segment" => {
            ColliderBuilder::segment(point("a", [0.0, 0.0, 0.0]), point("b", [1.0, 0.0, 0.0]))
        }
        other => return Err(anyhow!("unknown collider kind '{other}'")),
    };
    Ok(with_material(builder, params))
}

/// Everything a collider carries that is not its shape: what it is made of,
/// what it collides with, and what it reports.
///
/// Shared by both dimensions' builders through their own `collider_builder`,
/// because every one of these properties is dimension-free.
pub(crate) fn with_material(builder: ColliderBuilder, params: &toml::Value) -> ColliderBuilder {
    let mut builder = builder
        .restitution(scalar::real(v::f(params, "restitution", 0.0)))
        .friction(scalar::real(v::f(params, "friction", 0.5)))
        .density(scalar::real(v::f(params, "density", 1.0).max(0.001)))
        .friction_combine_rule(combine_rule(v::text(params, "friction_combine", "average")))
        .restitution_combine_rule(combine_rule(v::text(
            params,
            "restitution_combine",
            "average",
        )))
        .contact_skin(scalar::real(v::f(params, "contact_skin", 0.0).max(0.0)))
        .contact_force_event_threshold(scalar::real(v::f(params, "contact_force_threshold", 0.0)))
        .collision_groups(interaction_groups(params, "layers", "mask"))
        .solver_groups(interaction_groups(params, "solver_layers", "solver_mask"))
        .active_collision_types(active_collision_types(params))
        .active_events(active_events(params))
        .active_hooks(active_hooks(params))
        .enabled(v::boolean(params, "enabled", true))
        .sensor(v::boolean(params, "sensor", false));
    // An explicit mass overrides what the density works out to, which is what
    // an author who typed a number in kilograms means.
    let mass = scalar::real(v::f(params, "mass", 0.0));
    if mass > 0.0 {
        builder = builder.mass(mass);
    }
    builder
}

fn combine_rule(name: &str) -> CoefficientCombineRule {
    match name {
        "min" => CoefficientCombineRule::Min,
        "multiply" => CoefficientCombineRule::Multiply,
        "max" => CoefficientCombineRule::Max,
        "clamped_sum" => CoefficientCombineRule::ClampedSum,
        "geometric_mean" => CoefficientCombineRule::GeometricMean,
        _ => CoefficientCombineRule::Average,
    }
}

/// The 32 collision layers, as a `flags` property of layer numbers.
///
/// An empty `mask` means *every* layer: the alternative is 32 strings in every
/// scene file that wants the default, and the default is what most colliders
/// want.
pub(crate) fn interaction_groups(
    params: &toml::Value,
    memberships_key: &str,
    filter_key: &str,
) -> InteractionGroups {
    InteractionGroups::new(
        Group::from_bits_truncate(v::layer_bits(params, memberships_key, false)),
        Group::from_bits_truncate(v::layer_bits(params, filter_key, true)),
        // A collider is in a pair when *both* sides accept the other, which is
        // what every engine's layer/mask pair means.
        InteractionTestMode::And,
    )
}

/// Which body-type pairs this collider is tested against. Rapier leaves
/// static-static and kinematic-kinematic off, and a scene that wants a sensor
/// on the ground to notice a kinematic platform has to say so.
pub(crate) fn active_collision_types(params: &toml::Value) -> ActiveCollisionTypes {
    ActiveCollisionTypes::from_bits_truncate(v::bits(
        params,
        "active_collisions",
        &v::flags::collision_types(),
    ))
}

/// Rapier reports nothing by default, for speed. A collider opts in here, and
/// `crate::events` turns what it reports into a call on the node's script.
pub(crate) fn active_events(params: &toml::Value) -> ActiveEvents {
    ActiveEvents::from_bits_truncate(v::bits(params, "events", &v::flags::events()))
}

/// The hooks a collider asks rapier to call mid-step. `one_way` needs contact
/// modification, so asking for the platform asks for the hook.
pub(crate) fn active_hooks(params: &toml::Value) -> ActiveHooks {
    let mut hooks = ActiveHooks::from_bits_truncate(v::bits(params, "hooks", &v::flags::hooks()));
    // A one-way platform is contact modification with the answer written for
    // you, so asking for the platform asks for the hook.
    if v::boolean(params, "one_way", false) {
        hooks |= ActiveHooks::MODIFY_SOLVER_CONTACTS;
    }
    hooks
}

/// Build and insert the collider described by `params`, replacing any
/// existing one (attached to the entity's body when it has one).
pub(crate) fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let builder = collider_builder(eng, params)?;
    let offset = Pose::from_parts(scalar::v3a(v::vec3(params, "offset", [0.0; 3])), {
        let r = v::vec3(params, "offset_rotation", [0.0; 3]);
        scalar::rotation_of(glamx::Quat::from_euler(
            glamx::EulerRot::XYZ,
            r[0],
            r[1],
            r[2],
        ))
    });
    remove_colliders(eng, entity);
    add_collider_at(eng, entity, builder, offset)?;
    {
        let state = eng.resource::<PhysicsState>();
        state
            .borrow_mut()
            .collider_params
            .insert(entity, params.clone());
    }
    if v::boolean(params, "one_way", false) {
        let axis = v::vec3(params, "one_way_axis", [0.0, 1.0, 0.0]);
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        let handles = state.colliders.get(&entity).cloned().unwrap_or_default();
        for handle in handles {
            state.world.colliders[handle].user_data = encode_one_way(entity.to_bits().get(), axis);
        }
    }
    Ok(())
}

/// The nearest body at or above `entity` in the scene tree, and the node that
/// owns it.
///
/// This is what makes a compound shape authorable: a capsule and a sensor
/// sphere as two child nodes under one body, each with its own transform and
/// each pickable in the editor. Rapier has `compound`, but a compound shape is
/// one collider with one material and no way to tell which part was hit.
fn nearest_body(eng: &Engine, entity: Entity) -> Option<(Entity, RigidBodyHandle)> {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let world = eng.world();
    let mut current = entity;
    loop {
        if let Some(handle) = state.bodies.get(&current) {
            return Some((current, *handle));
        }
        current = world.get::<&balaur_core::scene::Parent>(current).ok()?.0;
    }
}

/// Where `entity` sits in `body_node`'s frame, so a child collider lands where
/// its node is rather than on top of the body.
fn pose_relative_to(eng: &Engine, entity: Entity, body_node: Entity) -> Result<Pose> {
    let here = node_pose(eng, entity)?;
    if entity == body_node {
        return Ok(Pose::IDENTITY);
    }
    let there = node_pose(eng, body_node)?;
    let inverse = there.rotation.inverse();
    Ok(Pose::from_parts(
        inverse * (here.translation - there.translation),
        inverse * here.rotation,
    ))
}

/// A one-way platform's direction, packed above the entity id in a collider's
/// `user_data`.
///
/// A hook cannot reach the component the axis was authored in — the step holds
/// the world — so it travels with the collider. Six directions rather than a
/// vector, because a platform's axis is a cardinal one in every game that has
/// ever wanted this, and the encoding costs three bits.
pub(crate) fn encode_one_way(entity_bits: u64, axis: [f32; 3]) -> u128 {
    let mut index = 0;
    for (i, value) in axis.iter().enumerate() {
        if value.abs() > axis[index].abs() {
            index = i;
        }
    }
    let sign = u128::from(axis[index] < 0.0);
    let code = (1 + index as u128 * 2 + sign) & 0b111;
    u128::from(entity_bits) | (code << 64)
}

/// The axis [`encode_one_way`] packed, or `None` for an ordinary collider.
pub(crate) fn decode_one_way(user_data: u128) -> Option<Vector> {
    let code = ((user_data >> 64) & 0b111) as u8;
    if code == 0 {
        return None;
    }
    let sign: Real = if (code - 1) % 2 == 1 { -1.0 } else { 1.0 };
    Some(match (code - 1) / 2 {
        0 => Vector::new(sign, 0.0, 0.0),
        1 => Vector::new(0.0, sign, 0.0),
        _ => Vector::new(0.0, 0.0, sign),
    })
}

/// Insert a collider for `entity`, attached to the nearest ancestor body.
///
/// `offset` is the collider's own offset from its node, on top of wherever the
/// node itself sits.
pub(crate) fn add_collider_at(
    eng: &Engine,
    entity: Entity,
    builder: ColliderBuilder,
    offset: Pose,
) -> Result<()> {
    let handle = if let Some((body_node, body)) = nearest_body(eng, entity) {
        let local = pose_relative_to(eng, entity, body_node)?;
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        warn_if_hollow_and_dynamic(&state, body, &builder);
        state
            .world
            .insert_collider(builder.position(local * offset), Some(body))
    } else {
        // No body anywhere above: static world geometry at the node's pose.
        let pose = node_pose(eng, entity)?;
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        state
            .world
            .insert_collider(builder.position(pose * offset), None)
    };
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    // The entity behind a handle, in one lookup rather than a scan of every
    // collider the world holds: every query result and every event needs it.
    state.world.colliders[handle].user_data = u128::from(entity.to_bits().get());
    state.colliders.entry(entity).or_default().push(handle);
    // The broad phase has not seen this one yet.
    state.queries_ready = false;
    Ok(())
}

/// Sitting on the node itself, which is what a script-built collider means.
pub(crate) fn add_collider(eng: &Engine, entity: Entity, builder: ColliderBuilder) -> Result<()> {
    add_collider_at(eng, entity, builder, Pose::IDENTITY)
}

/// A hollow shape has no interior, so rapier cannot derive an inertia tensor
/// for it. The body still simulates, badly; saying so beats leaving someone to
/// wonder why it tumbles.
fn warn_if_hollow_and_dynamic(
    state: &PhysicsState,
    body: RigidBodyHandle,
    builder: &ColliderBuilder,
) {
    if state
        .world
        .bodies
        .get(body)
        .is_some_and(crate::rapier3d::prelude::RigidBody::is_dynamic)
        && matches!(
            builder.shape.as_typed_shape(),
            crate::rapier3d::prelude::TypedShape::TriMesh(_)
                | crate::rapier3d::prelude::TypedShape::Polyline(_)
                | crate::rapier3d::prelude::TypedShape::HeightField(_)
                | crate::rapier3d::prelude::TypedShape::HalfSpace(_)
        )
    {
        tracing::warn!(
            "a dynamic body with a trimesh, polyline, heightfield or halfspace collider has no \
             well-defined mass; give it a convex_hull or a primitive, or make it static"
        );
    }
}

pub(crate) fn remove_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    if let Some(handles) = state.colliders.swap_remove(&entity) {
        for handle in handles {
            state.world.remove_collider(handle);
        }
        state.collider_params.swap_remove(&entity);
        state.queries_ready = false;
    }
}

/// The shape half of a `collider3d`'s params.
///
/// `None` for the asset-backed kinds: rapier keeps the geometry, not the
/// file it came from, so there is nothing to write back.
fn collider_shape_params(
    shape: &dyn crate::rapier3d::geometry::Shape,
) -> Option<toml::map::Map<String, toml::Value>> {
    let f = |v: Real| toml::Value::Float(f64::from(v));
    let vec3 = |x: Real, y: Real, z: Real| toml::Value::Array(vec![f(x), f(y), f(z)]);
    let mut map = toml::map::Map::new();
    if let Some(ball) = shape.as_ball() {
        map.insert("kind".into(), "ball".into());
        map.insert("radius".into(), f(ball.radius));
        return Some(map);
    }
    if let Some(cuboid) = shape.as_cuboid() {
        map.insert("kind".into(), "cuboid".into());
        let he = cuboid.half_extents;
        map.insert("half_extents".into(), vec3(he.x, he.y, he.z));
        return Some(map);
    }
    if let Some(capsule) = shape.as_capsule() {
        map.insert("kind".into(), "capsule".into());
        map.insert("radius".into(), f(capsule.radius));
        // `height` is the straight part: the segment, caps excluded.
        let straight = (capsule.segment.b - capsule.segment.a).length();
        map.insert("height".into(), f(straight));
        return Some(map);
    }
    if let Some(cylinder) = shape.as_cylinder() {
        map.insert("kind".into(), "cylinder".into());
        map.insert("radius".into(), f(cylinder.radius));
        map.insert("height".into(), f(cylinder.half_height * 2.0));
        return Some(map);
    }
    if let Some(cone) = shape.as_cone() {
        map.insert("kind".into(), "cone".into());
        map.insert("radius".into(), f(cone.radius));
        map.insert("height".into(), f(cone.half_height * 2.0));
        return Some(map);
    }
    if let Some(tri) = shape.as_triangle() {
        map.insert("kind".into(), "triangle".into());
        map.insert("a".into(), vec3(tri.a.x, tri.a.y, tri.a.z));
        map.insert("b".into(), vec3(tri.b.x, tri.b.y, tri.b.z));
        map.insert("c".into(), vec3(tri.c.x, tri.c.y, tri.c.z));
        return Some(map);
    }
    if let Some(segment) = shape.as_segment() {
        map.insert("kind".into(), "segment".into());
        map.insert("a".into(), vec3(segment.a.x, segment.a.y, segment.a.z));
        map.insert("b".into(), vec3(segment.b.x, segment.b.y, segment.b.z));
        return Some(map);
    }
    if let Some(halfspace) = shape.as_halfspace() {
        map.insert("kind".into(), "halfspace".into());
        let n = halfspace.normal;
        map.insert("normal".into(), vec3(n.x, n.y, n.z));
        return Some(map);
    }
    // The rounded shapes report the shape they wrap plus the border that
    // rounded it, which is exactly how the schema spells them.
    if let Some(round) = shape.as_round_cuboid() {
        let he = round.inner_shape.half_extents;
        map.insert("kind".into(), "cuboid".into());
        map.insert("half_extents".into(), vec3(he.x, he.y, he.z));
        map.insert("border".into(), f(round.border_radius));
        return Some(map);
    }
    if let Some(round) = shape.as_round_cylinder() {
        map.insert("kind".into(), "cylinder".into());
        map.insert("radius".into(), f(round.inner_shape.radius));
        map.insert("height".into(), f(round.inner_shape.half_height * 2.0));
        map.insert("border".into(), f(round.border_radius));
        return Some(map);
    }
    if let Some(round) = shape.as_round_cone() {
        map.insert("kind".into(), "cone".into());
        map.insert("radius".into(), f(round.inner_shape.radius));
        map.insert("height".into(), f(round.inner_shape.half_height * 2.0));
        map.insert("border".into(), f(round.border_radius));
        return Some(map);
    }
    None
}

pub(crate) fn get_collider_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let handle = state.colliders.get(&entity)?.first()?;
    let collider = state.world.colliders.get(*handle)?;
    // What it was authored from, under what rapier can report: the asset
    // names and the build-time choices survive, and anything a script has
    // changed since shows through.
    let mut map = state
        .collider_params
        .get(&entity)
        .and_then(|params| params.as_table().cloned())
        .unwrap_or_default();
    if let Some(shape) = collider_shape_params(collider.shape()) {
        map.extend(shape);
    }
    read_material(collider, &mut map);
    Some(toml::Value::Table(map))
}

/// The non-shape half of a collider, read back off it. The inverse of
/// [`with_material`], property for property, so the inspector round-trips.
pub(crate) fn read_material(collider: &Collider, map: &mut toml::map::Map<String, toml::Value>) {
    let f = |value: Real| toml::Value::Float(f64::from(value));
    map.insert("restitution".into(), f(collider.restitution()));
    map.insert("friction".into(), f(collider.friction()));
    map.insert("density".into(), f(collider.density()));
    map.insert("mass".into(), f(collider.mass()));
    map.insert("contact_skin".into(), f(collider.contact_skin()));
    map.insert(
        "contact_force_threshold".into(),
        f(collider.contact_force_event_threshold()),
    );
    map.insert("sensor".into(), collider.is_sensor().into());
    map.insert("enabled".into(), collider.is_enabled().into());
    map.insert(
        "friction_combine".into(),
        combine_name(collider.friction_combine_rule()).into(),
    );
    map.insert(
        "restitution_combine".into(),
        combine_name(collider.restitution_combine_rule()).into(),
    );
    let groups = collider.collision_groups();
    map.insert("layers".into(), v::layer_names(groups.memberships.bits()));
    map.insert("mask".into(), v::layer_names(groups.filter.bits()));
    let solver = collider.solver_groups();
    map.insert(
        "solver_layers".into(),
        v::layer_names(solver.memberships.bits()),
    );
    map.insert("solver_mask".into(), v::layer_names(solver.filter.bits()));
    map.insert(
        "events".into(),
        v::names(collider.active_events().bits(), &v::flags::events()),
    );
    map.insert(
        "hooks".into(),
        v::names(collider.active_hooks().bits(), &v::flags::hooks()),
    );
    map.insert(
        "active_collisions".into(),
        v::names(
            collider.active_collision_types().bits(),
            &v::flags::collision_types(),
        ),
    );
}

fn combine_name(rule: CoefficientCombineRule) -> &'static str {
    match rule {
        CoefficientCombineRule::Min => "min",
        CoefficientCombineRule::Multiply => "multiply",
        CoefficientCombineRule::Max => "max",
        CoefficientCombineRule::ClampedSum => "clamped_sum",
        CoefficientCombineRule::GeometricMean => "geometric_mean",
        CoefficientCombineRule::Average => "average",
    }
}

/// A node's first collider, for the readers that ask one question about it.
fn with_first_collider<R>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&Collider) -> Result<R>,
) -> Result<R> {
    let entity = balaur_core::entity_of(node)?;
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let handle = *state
        .colliders
        .get(&entity)
        .and_then(|handles| handles.first())
        .ok_or_else(|| anyhow!("node has no collider"))?;
    f(&state.world.colliders[handle])
}

/// A voxel collider's grid, for editing in place.
///
/// Every edit bumps the collider's revision, which is what puts a dug hole in
/// the digest: nothing else about the world changes until something falls into
/// it, and two machines that disagree about a hole must not agree about the
/// frame.
fn with_voxels(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&mut crate::rapier3d::parry::shape::Voxels),
) -> Result<()> {
    let entity = balaur_core::entity_of(node)?;
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let handle = *state
        .colliders
        .get(&entity)
        .and_then(|handles| handles.first())
        .ok_or_else(|| anyhow!("node has no collider"))?;
    let collider = &mut state.world.colliders[handle];
    let voxels = collider
        .shape_mut()
        .as_voxels_mut()
        .ok_or_else(|| anyhow!("this node's collider is not a voxel grid"))?;
    f(voxels);
    state.shape_revision = state.shape_revision.wrapping_add(1);
    Ok(())
}

/// A collider's shape as points and triangles, through parry's own
/// tessellation — which every shape has, voxels included.
fn collider_mesh_value(eng: &Engine, node: NodeId) -> Result<balaur_script::Value> {
    let entity = balaur_core::entity_of(node)?;
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let handle = *state
        .colliders
        .get(&entity)
        .and_then(|handles| handles.first())
        .ok_or_else(|| anyhow!("node has no collider"))?;
    let (points, indices) = state.world.colliders[handle]
        .shape()
        .as_voxels()
        .map(crate::rapier3d::parry::shape::Voxels::to_trimesh)
        .ok_or_else(|| {
            anyhow!("only a voxel collider can be turned into a mesh so far; ask for another shape")
        })?;
    let points = points
        .into_iter()
        .map(|p| balaur_script::Value::Vec3(scalar::a3(p)))
        .collect();
    let indices = indices
        .into_iter()
        .flat_map(|t| t.into_iter().map(|i| balaur_script::Value::Int(i.into())))
        .collect();
    Ok(crate::vocabulary::map([
        ("points", balaur_script::Value::List(points)),
        ("indices", balaur_script::Value::List(indices)),
    ]))
}

/// Everything a collider carries besides its shape, as schema text. Shared
/// with `collider2d`: a material is dimension-free.
pub(crate) fn shared_collider_schema() -> String {
    let layers = v::layer_options();
    format!(
        r#"restitution = {{ type = "float", default = 0.0, min = 0.0, max = 1.0, description = "Bounciness: 0 is a dead stop, 1 a full rebound" }}
friction = {{ type = "float", default = 0.5, min = 0.0, description = "Surface friction; 0 is ice" }}
density = {{ type = "float", default = 1.0, min = 0.001, description = "Mass per volume, so the shape's size sets its mass" }}
mass = {{ type = "float", default = 0.0, min = 0.0, description = "Mass in kilograms, overriding what density works out to; 0 keeps the density" }}
friction_combine = {{ type = "enum", default = "average", options = ["average", "min", "multiply", "max", "clamped_sum", "geometric_mean"], description = "How this surface's friction combines with the other one's" }}
restitution_combine = {{ type = "enum", default = "average", options = ["average", "min", "multiply", "max", "clamped_sum", "geometric_mean"], description = "How this surface's bounciness combines with the other one's" }}
contact_skin = {{ type = "float", default = 0.0, min = 0.0, description = "A margin the solver treats as already touching; stops thin shapes tunnelling and jittering" }}
sensor = {{ type = "bool", default = false, description = "Detects overlaps without colliding: bodies pass through and are reported" }}
enabled = {{ type = "bool", default = true, description = "Collide at all; a disabled collider keeps its shape and costs nothing" }}
layers = {{ type = "flags", default = ["0"], options = [{layers}], description = "The layers this collider is on" }}
mask = {{ type = "flags", default = [], options = [{layers}], description = "The layers it collides with; empty means every layer" }}
solver_layers = {{ type = "flags", default = ["0"], options = [{layers}], description = "Layers for the solver alone: a pair can be detected but not resolved" }}
solver_mask = {{ type = "flags", default = [], options = [{layers}], description = "Which solver layers this one pushes against; empty means all of them" }}
events = {{ type = "flags", default = [], options = ["collision", "contact_force"], description = "What this collider reports to its node's script: on_collision_start and on_collision_stop, or on_contact_force" }}
contact_force_threshold = {{ type = "float", default = 0.0, min = 0.0, description = "How hard a contact must be before on_contact_force is called" }}
hooks = {{ type = "flags", default = [], options = ["filter_contact", "filter_overlap", "modify_contacts"], description = "Mid-step questions this collider asks its node's script; each costs a call per candidate pair per step" }}
active_collisions = {{ type = "flags", default = ["dynamic_dynamic", "dynamic_kinematic", "dynamic_static"], options = ["dynamic_dynamic", "dynamic_kinematic", "dynamic_static", "kinematic_kinematic", "kinematic_static", "static_static"], description = "Which pairs of body kinds this collider is tested against; a sensor watching kinematic platforms needs more than the default" }}
one_way = {{ type = "bool", default = false, description = "A platform bodies pass through from below and land on from above" }}
"#
    )
}

/// The `collider3d` key. Not backed by a component type either: it writes
/// into [`crate::PhysicsState`].
pub(crate) fn register_collider_component(app: &mut App) {
    let schema = format!(
        r#"kind = {{ type = "enum", default = "cuboid", options = ["ball", "cuboid", "capsule", "cylinder", "cone", "triangle", "segment", "halfspace", "trimesh", "convex_hull", "convex_decomposition", "polyline", "heightfield", "voxels", "voxelized_mesh", "fit"], description = "Collision shape" }}
radius = {{ type = "float", default = 0.5, min = 0.01, description = "Radius, for ball, capsule, cylinder and cone" }}
height = {{ type = "float", default = 1.0, min = 0.01, description = "Length along y of the straight part, for capsule, cylinder and cone" }}
half_extents = {{ type = "vec3", default = [0.5, 0.5, 0.5], description = "Half-sizes of the cuboid, when kind is cuboid" }}
border = {{ type = "float", default = 0.0, min = 0.0, description = "Rounds a cuboid, cylinder, cone or triangle by this radius; a rounded shape slides over seams instead of catching on them" }}
a = {{ type = "vec3", default = [0.0, 0.0, 0.0], description = "First corner, when kind is triangle or segment" }}
b = {{ type = "vec3", default = [1.0, 0.0, 0.0], description = "Second corner, when kind is triangle or segment" }}
c = {{ type = "vec3", default = [0.0, 1.0, 0.0], description = "Third corner, when kind is triangle" }}
normal = {{ type = "vec3", default = [0.0, 1.0, 0.0], description = "Which way the infinite plane faces, when kind is halfspace" }}
mesh = {{ type = "asset", asset = "mesh", default = "", description = "Geometry for a trimesh, convex_hull or polyline collider" }}
heightfield = {{ type = "asset", asset = "heightfield", default = "", description = "Terrain grid, when kind is heightfield" }}
voxels = {{ type = "asset", asset = "voxels", default = "", description = "Filled cells, when kind is voxels; a script may dig into them while the game runs" }}
voxel_size = {{ type = "float", default = 0.25, min = 0.001, description = "How big one cell is, when kind is voxelized_mesh" }}
fill = {{ type = "enum", default = "solid", options = ["solid", "surface"], description = "Whether voxelizing a mesh fills its inside or only its shell" }}
fit = {{ type = "enum", default = "convex_hull", options = ["convex_hull", "aabb", "obb", "convex_decomposition"], description = "The shape fitted to the mesh, when kind is fit" }}
fix_internal_edges = {{ type = "bool", default = true, description = "Smooth the seams between a trimesh's triangles, so a character does not catch on flat ground" }}
clean = {{ type = "bool", default = false, description = "Drop duplicate vertices and degenerate triangles when building a trimesh" }}
oriented = {{ type = "bool", default = false, description = "Treat the trimesh as a closed, outward-facing surface, which makes inside and outside meaningful" }}
scale = {{ type = "vec3", default = [1.0, 1.0, 1.0], description = "Cell size and height scale of a heightfield" }}
one_way_axis = {{ type = "vec3", default = [0.0, 1.0, 0.0], description = "The direction a one-way platform lets bodies through from" }}
offset = {{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where the shape sits relative to the node" }}
offset_rotation = {{ type = "vec3", default = [0.0, 0.0, 0.0], description = "How the shape is turned relative to the node, in radians" }}
{}"#,
        shared_collider_schema()
    );
    app.register_component(
        "collider3d",
        ComponentDef {
            doc: "The shape the node collides with in 3D. On a node with a `body3d` it is that body's shape; on a node without one it is immovable world geometry. A collider on a child node belongs to the nearest body above it, which is how one body carries several shapes.",
            schema: ComponentDef::parse_schema("collider3d", &schema),
            tags: &["3d", "physics"],
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

/// Collider calls that are not about creating one: replacing the shape, and
/// asking where it is.
pub(crate) fn install_collider_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_collider", &["collider3d"], "", "Replace the node's collider from a `collider3d` table: `kind`, `radius`, `half_extents`, `friction`, and the rest of the component's own vocabulary."),
    ]);
    m.function(
        "set_collider",
        |eng: &Engine, (node, params): (NodeId, balaur_script::Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            apply_collider(eng, balaur_core::entity_of(node)?, &params)
        },
    );
    m.function("aabb", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let handle = state
            .colliders
            .get(&entity)
            .and_then(|handles| handles.first())
            .ok_or_else(|| anyhow!("node has no collider"))?;
        let aabb = state.world.colliders[*handle].compute_aabb();
        Ok((
            aabb.mins.x,
            aabb.mins.y,
            aabb.mins.z,
            aabb.maxs.x,
            aabb.maxs.y,
            aabb.maxs.z,
        ))
    });
}

/// Editing a voxel grid, and reading one back as a mesh.
///
/// Voxels are the one shape a game changes rather than replaces, so they get
/// calls of their own. Split from [`install_collider_api`] under
/// `MAX_FN_LINES`.
pub(crate) fn install_voxel_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_voxel", &["collider3d"], "", "Fill or empty one cell of a voxel collider: digging a hole, or building a wall, while the game runs."),
        ("voxel", &["collider3d"], "", "Whether one cell of a voxel collider is filled."),
        ("voxel_at", &["collider3d"], "", "The cell a world position falls in, as three whole numbers."),
    ]);
    // Voxels are the one shape a game edits rather than replaces, so they get
    // calls of their own rather than going through `set_collider`.
    m.function(
        "set_voxel",
        |eng: &Engine, (node, x, y, z, filled): (NodeId, i32, i32, i32, bool)| {
            with_voxels(eng, node, |voxels| {
                voxels.set_voxel(scalar::cell(x, y, z), filled);
            })
        },
    );
    m.function(
        "voxel",
        |eng: &Engine, (node, x, y, z): (NodeId, i32, i32, i32)| {
            let entity = balaur_core::entity_of(node)?;
            let state = eng.resource::<PhysicsState>();
            let state = state.borrow();
            let handle = *state
                .colliders
                .get(&entity)
                .and_then(|handles| handles.first())
                .ok_or_else(|| anyhow!("node has no collider"))?;
            let voxels = state.world.colliders[handle]
                .shape()
                .as_voxels()
                .ok_or_else(|| anyhow!("this node's collider is not a voxel grid"))?;
            Ok(voxels
                .voxel_state(scalar::cell(x, y, z))
                .is_some_and(|state| !state.is_empty()))
        },
    );
    m.function(
        "voxel_at",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            let entity = balaur_core::entity_of(node)?;
            let state = eng.resource::<PhysicsState>();
            let state = state.borrow();
            let handle = *state
                .colliders
                .get(&entity)
                .and_then(|handles| handles.first())
                .ok_or_else(|| anyhow!("node has no collider"))?;
            let collider = &state.world.colliders[handle];
            let voxels = collider
                .shape()
                .as_voxels()
                .ok_or_else(|| anyhow!("this node's collider is not a voxel grid"))?;
            // The grid is in the collider's own space, so a world point has to
            // come home first.
            let local = collider.position().inverse() * scalar::v3(x, y, z);
            let cell = voxels.voxel_at_point(local);
            Ok((i64::from(cell.x), i64::from(cell.y), i64::from(cell.z)))
        },
    );
}

/// What a collider weighs, how much space it takes, where it is, and the
/// handles rapier knows it by.
///
/// Split from [`install_collider_api`] under `MAX_FN_LINES`.
pub(crate) fn install_collider_reader_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("collider_mesh", &["collider3d"], "", "The collider's shape as points and triangles — including a voxel grid's — for drawing it or for spawning the pieces it broke into."),
        ("collider_mass", &["collider3d"], "", "What this collider weighs, density and size together."),
        ("collider_volume", &["collider3d"], "", "How much space the shape encloses."),
        ("swept_aabb", &["collider3d"], "", "The box the collider covers over the next step, its motion included: what the broad phase actually tests."),
        ("handles", &["collider3d"], "", "The rapier handles behind this node — its body and its colliders — as `#{ body, colliders }` of index and generation pairs. For matching a log line against rapier's own output."),
        ("aabb", &["collider3d"], "", "The world-space box the collider currently occupies, as its two opposite corners."),
    ]);
    m.function("collider_mesh", |eng: &Engine, node: NodeId| {
        collider_mesh_value(eng, node)
    });
    m.function("collider_volume", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |collider| Ok(collider.volume()))
    });
    m.function("swept_aabb", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |collider| {
            let aabb = collider.compute_swept_aabb(collider.position());
            Ok((
                aabb.mins.x,
                aabb.mins.y,
                aabb.mins.z,
                aabb.maxs.x,
                aabb.maxs.y,
                aabb.maxs.z,
            ))
        })
    });
    m.function("handles", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let pair = |index: u32, generation: u32| {
            balaur_script::Value::List(vec![
                balaur_script::Value::Int(i64::from(index)),
                balaur_script::Value::Int(i64::from(generation)),
            ])
        };
        let body = state
            .bodies
            .get(&entity)
            .map_or(balaur_script::Value::Nil, |handle| {
                let (index, generation) = handle.into_raw_parts();
                pair(index, generation)
            });
        let colliders = state.colliders.get(&entity).map_or_else(
            || balaur_script::Value::List(Vec::new()),
            |handles| {
                balaur_script::Value::List(
                    handles
                        .iter()
                        .map(|handle| {
                            let (index, generation) = handle.into_raw_parts();
                            pair(index, generation)
                        })
                        .collect(),
                )
            },
        );
        Ok(crate::vocabulary::map([
            ("body", body),
            ("colliders", colliders),
        ]))
    });
    m.function("collider_mass", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let handle = state
            .colliders
            .get(&entity)
            .and_then(|handles| handles.first())
            .ok_or_else(|| anyhow!("node has no collider"))?;
        Ok(state.world.colliders[*handle].mass())
    });
}
