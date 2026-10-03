//! The `collider3d` component: every shape it can take, and the overlap
//! query that reads them back.

use crate::rapier3d::math::Vector;
use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::rapier3d::prelude::{
    ActiveCollisionTypes, ActiveEvents, ActiveHooks, CoefficientCombineRule, Collider,
    ColliderBuilder, ColliderHandle, Group, InteractionGroups, InteractionTestMode, MassProperties,
    RigidBodyHandle,
};
use crate::scalar::{self, Pose, Real};

use crate::vocabulary::{self as v, component as c, keys as k, words as w};
use crate::{PhysicsState3d, node_pose};

/// The collider described by `params`, in the `collider` schema's own
/// vocabulary, so a script table and a scene-file entry build the same thing.
pub(crate) fn collider_builder(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder> {
    let kind = v::text(params, k::KIND, w::BOX);
    let he = |i: usize| scalar::real(v::axis(params, k::SIZE, i, 1.0) / 2.0).max(0.01);
    let point = |key: &str, fallback: [f32; 3]| scalar::v3a(v::vec3(params, key, fallback));
    let radius = scalar::real(v::f(params, k::RADIUS, 0.5)).max(0.01);
    // rapier measures these from the centre. `height` runs tip to tip, so a
    // capsule's segment is what the two caps leave of it.
    let height = scalar::real(v::f(params, k::HEIGHT, 2.0));
    let half_height = height.max(0.01) / 2.0;
    let half_segment = (half_height - radius).max(0.0);
    // A rounded shape is a shape plus a border radius, not nine more kinds.
    // Ball and capsule are already round, so they ignore it.
    let border = scalar::real(v::f(params, k::EDGE_RADIUS, 0.0)).max(0.0);
    let rounded = border > 0.0;
    let builder = match kind {
        w::SPHERE => ColliderBuilder::ball(radius),
        w::BOX if rounded => ColliderBuilder::round_cuboid(he(0), he(1), he(2), border),
        w::BOX => ColliderBuilder::cuboid(he(0), he(1), he(2)),
        // `height = 0` hands the capsule's length to its two ends.
        w::CAPSULE if height <= 0.0 => ColliderBuilder::capsule_from_endpoints(
            point(k::A, [0.0, 0.0, 0.0]),
            point(k::B, [1.0, 0.0, 0.0]),
            radius,
        ),
        w::CAPSULE => match v::text(params, k::UP_AXIS, w::Y) {
            w::X => ColliderBuilder::capsule_x(half_segment, radius),
            w::Z => ColliderBuilder::capsule_z(half_segment, radius),
            _ => ColliderBuilder::capsule_y(half_segment, radius),
        },
        w::CYLINDER if rounded => ColliderBuilder::round_cylinder(half_height, radius, border),
        w::CYLINDER => ColliderBuilder::cylinder(half_height, radius),
        w::CONE if rounded => ColliderBuilder::round_cone(half_height, radius, border),
        w::CONE => ColliderBuilder::cone(half_height, radius),
        w::TRIANGLE if rounded => ColliderBuilder::round_triangle(
            point(k::A, [0.0, 0.0, 0.0]),
            point(k::B, [1.0, 0.0, 0.0]),
            point(k::C, [0.0, 1.0, 0.0]),
            border,
        ),
        w::TRIANGLE => ColliderBuilder::triangle(
            point(k::A, [0.0, 0.0, 0.0]),
            point(k::B, [1.0, 0.0, 0.0]),
            point(k::C, [0.0, 1.0, 0.0]),
        ),
        w::TRIANGLE_MESH
        | w::CONVEX_HULL
        | w::CONVEX_MESH
        | w::POLYLINE
        | w::CONVEX_DECOMPOSITION
        | w::VOXELIZED_POINTS
        | w::FIT => crate::shapes::mesh_collider(eng, params, kind, border)?,
        w::VOXELS => crate::shapes::voxel_collider(eng, params)?,
        w::VOXELIZED_MESH => crate::shapes::voxelized_mesh_collider(eng, params)?,
        w::HEIGHTFIELD => {
            crate::shapes::heightfield_collider(eng, params, point(k::SCALE, [1.0, 1.0, 1.0]))?
        }
        // An infinite plane, for a floor that needs no size and no triangles.
        w::WORLD_BOUNDARY => {
            let n = point(k::NORMAL, [0.0, 1.0, 0.0]);
            if n.length_squared() < 1.0e-12 {
                bail!("a world_boundary collider needs a non-zero `normal`");
            }
            ColliderBuilder::new(crate::rapier3d::prelude::SharedShape::halfspace(
                n.normalize(),
            ))
        }
        w::SEGMENT => {
            ColliderBuilder::segment(point(k::A, [0.0, 0.0, 0.0]), point(k::B, [1.0, 0.0, 0.0]))
        }
        other => return Err(anyhow!("unknown collider kind '{other}'")),
    };
    Ok(with_mass_properties(with_material(builder, params), params))
}

/// Everything a collider carries that is not its shape: what it is made of,
/// what it collides with, and what it reports.
///
/// Shared by both dimensions' builders through their own `collider_builder`,
/// because every one of these properties is dimension-free.
pub(crate) fn with_material(builder: ColliderBuilder, params: &toml::Value) -> ColliderBuilder {
    let mut builder = builder
        .restitution(scalar::real(v::f(params, k::RESTITUTION, 0.0).max(0.0)))
        .friction(scalar::real(v::f(params, k::FRICTION, 0.5)))
        .density(scalar::real(v::f(params, k::DENSITY, 1.0).max(0.0)))
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
        .collision_groups(interaction_groups(
            params,
            k::COLLISION_LAYER,
            k::COLLISION_MASK,
            k::COLLISION_TEST,
        ))
        .solver_groups(interaction_groups(
            params,
            k::SOLVER_LAYER,
            k::SOLVER_MASK,
            k::SOLVER_TEST,
        ))
        .active_collision_types(active_collision_types(params))
        .active_events(active_events(params))
        .active_hooks(active_hooks(params))
        .enabled(v::boolean(params, k::ENABLED, true))
        .sensor(v::boolean(params, k::SENSOR, false));
    // An explicit mass overrides what the density works out to, which is what
    // an author who typed a number in kilograms means.
    let mass = scalar::real(v::f(params, k::MASS, 0.0));
    if mass > 0.0 {
        builder = builder.mass(mass);
    }
    builder
}

/// `center_of_mass`, `inertia` and `inertia_rotation`, for a collider that
/// states any of them. The mass is `mass`, or what the density makes of the
/// shape; an `inertia` of 0 is the shape's own, about the stated centre.
fn with_mass_properties(builder: ColliderBuilder, params: &toml::Value) -> ColliderBuilder {
    let com = v::vec3(params, k::CENTER_OF_MASS, [0.0; 3]);
    let inertia = v::vec3(params, k::INERTIA, [0.0; 3]);
    if crate::body::is_default(&com) && crate::body::is_default(&inertia) {
        return builder;
    }
    let unit = builder.shape.mass_properties(1.0);
    let mass = match scalar::real(v::f(params, k::MASS, 0.0)) {
        stated if stated > 0.0 => stated,
        _ => scalar::real(v::f(params, k::DENSITY, 1.0).max(0.0)) * unit.mass(),
    };
    let com = scalar::v3a(com);
    let props = if crate::body::is_default(&inertia) {
        let mut shaped = unit;
        shaped.set_mass(mass, true);
        // The parallel-axis theorem, moving the shape's inertia to `com`.
        let d = com - shaped.local_com;
        let outer = crate::rapier3d::math::Matrix::from_cols(d * d.x, d * d.y, d * d.z);
        let shift =
            (crate::rapier3d::math::Matrix::from_diagonal(Vector::splat(d.length_squared()))
                - outer)
                * mass;
        MassProperties::with_inertia_matrix(com, mass, shaped.reconstruct_inertia_matrix() + shift)
    } else {
        let frame =
            crate::body::rotation_from_euler(v::vec3(params, k::INERTIA_ROTATION, [0.0; 3]));
        MassProperties::with_principal_inertia_frame(com, mass, scalar::v3a(inertia), frame)
    };
    builder.mass_properties(props)
}

/// What the contact hook needs of this collider, or `None` for one that
/// neither carries bodies through one way nor slides them along.
pub(crate) fn surface_of(params: &toml::Value) -> Option<crate::events::Surface> {
    let one_way = v::boolean(params, k::ONE_WAY, false).then(|| {
        let axis = scalar::v3a(v::vec3(params, k::ONE_WAY_AXIS, [0.0, 1.0, 0.0]));
        let angle = scalar::real(v::f(params, k::ONE_WAY_ANGLE, 0.1).max(0.0));
        (axis.try_normalize().unwrap_or(Vector::Y), angle)
    });
    let velocity = scalar::v3a(v::vec3(params, k::SURFACE_VELOCITY, [0.0; 3]));
    (one_way.is_some() || velocity != Vector::ZERO)
        .then_some(crate::events::Surface { one_way, velocity })
}

/// The layer and mask rows, as schema text. Named apart from the rest of
/// `shared_collider_schema` because a soft body's collider is filtered the
/// same way and has no density, no sensor and no contact skin.
pub(crate) fn shared_group_schema() -> String {
    let layers = v::layer_options();
    v::schema(&[
        (
            k::COLLISION_LAYER,
            &format!(
                r#"{{ type = "flags", default = ["1"], options = [{layers}], description = "The layers this body is on", group = "filtering" }}"#
            ),
        ),
        (
            k::COLLISION_MASK,
            &format!(
                r#"{{ type = "flags", default = [], options = [{layers}], description = "The layers it collides with; empty means every layer", group = "filtering" }}"#
            ),
        ),
    ])
}

/// The event rows, as schema text, for a soft body: its surface collider
/// reports the way a `collider3d` does.
pub(crate) fn shared_event_schema() -> String {
    let events = v::options(&v::flags::events().map(|(name, _)| name));
    v::schema(&[
        (
            k::EVENTS,
            &format!(
                r#"{{ type = "flags", default = [], options = [{events}], description = "What this body reports to its node's script: on_collision_enter and on_collision_exit, or on_contact_force", group = "filtering" }}"#
            ),
        ),
        (
            k::CONTACT_FORCE_THRESHOLD,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "How hard a contact must be before on_contact_force is called", group = "filtering" }"#,
        ),
    ])
}

/// Report what `params` asks for, for a builder whose other rows its owner
/// has already set.
pub(crate) fn with_events(builder: ColliderBuilder, params: &toml::Value) -> ColliderBuilder {
    let threshold = scalar::real(v::f(params, k::CONTACT_FORCE_THRESHOLD, 0.0));
    builder
        .active_events(active_events(params))
        .contact_force_event_threshold(threshold)
}

/// Put a collider on the layers `params` names, for a builder whose other
/// rows its owner has already set.
pub(crate) fn with_groups(builder: ColliderBuilder, params: &toml::Value) -> ColliderBuilder {
    builder.collision_groups(interaction_groups(
        params,
        k::COLLISION_LAYER,
        k::COLLISION_MASK,
        k::COLLISION_TEST,
    ))
}

crate::shared::collider::functions!(state = PhysicsState3d, refit = crate::body::refit_mass);

/// Which body-type pairs this collider is tested against. Rapier leaves
/// static-static and kinematic-kinematic off, and a scene that wants a sensor
/// on the ground to notice a kinematic platform has to say so.
pub(crate) fn active_collision_types(params: &toml::Value) -> ActiveCollisionTypes {
    ActiveCollisionTypes::from_bits_truncate(v::bits(
        params,
        k::CONTACT_PAIRS,
        &v::flags::collision_types(),
    ))
}

/// Rapier reports nothing by default, for speed. A collider opts in here, and
/// `crate::events` turns what it reports into a call on the node's script.
pub(crate) fn active_events(params: &toml::Value) -> ActiveEvents {
    ActiveEvents::from_bits_truncate(v::bits(params, k::EVENTS, &v::flags::events()))
}

/// Build and insert the collider described by `params`, replacing any
/// existing one (attached to the entity's body when it has one).
pub(crate) fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let builder = collider_builder(eng, params)?;
    let offset = Pose::from_parts(scalar::v3a(v::vec3(params, k::OFFSET, [0.0; 3])), {
        let r = v::vec3(params, k::OFFSET_ROTATION, [0.0; 3]);
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
        let state = eng.resource::<PhysicsState3d>();
        state
            .borrow_mut()
            .collider_params
            .insert(entity, params.clone());
    }
    if let Some(surface) = surface_of(params) {
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let handles = state.colliders.get(&entity).cloned().unwrap_or_default();
        state
            .surfaces
            .extend(handles.into_iter().map(|h| (h, surface)));
    }
    Ok(())
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

/// Insert a collider for `entity`, attached to the nearest ancestor body.
///
/// `offset` is the collider's own offset from its node, on top of wherever the
/// node itself sits; a pose the builder already holds, as a fitted box's, sits
/// inside both.
pub(crate) fn add_collider_at(
    eng: &Engine,
    entity: Entity,
    builder: ColliderBuilder,
    offset: Pose,
) -> Result<()> {
    let fitted = builder.position;
    let handle = if let Some((body_node, body)) = nearest_body(eng, entity) {
        let local = pose_relative_to(eng, entity, body_node)?;
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        warn_if_hollow_and_dynamic(&state, body, &builder);
        state
            .world
            .insert_collider(builder.position(local * offset * fitted), Some(body))
    } else {
        // No body anywhere above: static world geometry at the node's pose.
        let pose = node_pose(eng, entity)?;
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        state
            .world
            .insert_collider(builder.position(pose * offset * fitted), None)
    };
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    // The entity behind a handle, in one lookup rather than a scan of every
    // collider the world holds: every query result and every event needs it.
    state.world.colliders[handle].user_data = u128::from(entity.to_bits().get());
    if let Some(body) = state.world.colliders[handle].parent()
        && crate::body::has_total_mass(&state.world.bodies[body])
    {
        state.world.colliders[handle].set_density(0.0);
        crate::body::refit_mass(&mut state, body);
    }
    state.colliders.entry(entity).or_default().push(handle);
    // The broad phase has not seen this one yet.
    state.queries_ready = false;
    Ok(())
}

/// A hollow shape has no interior, so rapier cannot derive an inertia tensor
/// for it. The body still simulates, badly; saying so beats leaving someone to
/// wonder why it tumbles.
fn warn_if_hollow_and_dynamic(
    state: &PhysicsState3d,
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
            "a dynamic body with a triangle_mesh, polyline, heightfield or world_boundary collider has no \
             well-defined mass; give it a convex_hull or a primitive, or make it static"
        );
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
        map.insert(k::KIND.into(), w::SPHERE.into());
        map.insert(k::RADIUS.into(), f(ball.radius));
        return Some(map);
    }
    if let Some(cuboid) = shape.as_cuboid() {
        map.insert(k::KIND.into(), w::BOX.into());
        let he = cuboid.half_extents * 2.0;
        map.insert(k::SIZE.into(), vec3(he.x, he.y, he.z));
        return Some(map);
    }
    if let Some(capsule) = shape.as_capsule() {
        map.insert(k::KIND.into(), w::CAPSULE.into());
        map.insert(k::RADIUS.into(), f(capsule.radius));
        let (a, b) = (capsule.segment.a, capsule.segment.b);
        let along = b - a;
        let straight = along.length();
        let centred = (a + b).length() <= 1.0e-5 * straight.max(1.0);
        let axis = [(w::X, along.x), (w::Y, along.y), (w::Z, along.z)]
            .into_iter()
            .find(|(_, n)| *n > 0.0 && (*n - straight).abs() <= 1.0e-5 * straight);
        match axis {
            Some((word, _)) if centred => {
                map.insert(k::UP_AXIS.into(), word.into());
                map.insert(k::HEIGHT.into(), f(straight + 2.0 * capsule.radius));
            }
            // A capsule with no length keeps the axis it was written with.
            _ if straight == 0.0 => {
                map.insert(k::HEIGHT.into(), f(2.0 * capsule.radius));
            }
            _ => {
                map.insert(k::HEIGHT.into(), f(0.0));
                map.insert(k::A.into(), vec3(a.x, a.y, a.z));
                map.insert(k::B.into(), vec3(b.x, b.y, b.z));
            }
        }
        return Some(map);
    }
    if let Some(cylinder) = shape.as_cylinder() {
        map.insert(k::KIND.into(), w::CYLINDER.into());
        map.insert(k::RADIUS.into(), f(cylinder.radius));
        map.insert(k::HEIGHT.into(), f(cylinder.half_height * 2.0));
        return Some(map);
    }
    if let Some(cone) = shape.as_cone() {
        map.insert(k::KIND.into(), w::CONE.into());
        map.insert(k::RADIUS.into(), f(cone.radius));
        map.insert(k::HEIGHT.into(), f(cone.half_height * 2.0));
        return Some(map);
    }
    if let Some(tri) = shape.as_triangle() {
        map.insert(k::KIND.into(), w::TRIANGLE.into());
        map.insert(k::A.into(), vec3(tri.a.x, tri.a.y, tri.a.z));
        map.insert(k::B.into(), vec3(tri.b.x, tri.b.y, tri.b.z));
        map.insert(k::C.into(), vec3(tri.c.x, tri.c.y, tri.c.z));
        return Some(map);
    }
    if let Some(segment) = shape.as_segment() {
        map.insert(k::KIND.into(), w::SEGMENT.into());
        map.insert(k::A.into(), vec3(segment.a.x, segment.a.y, segment.a.z));
        map.insert(k::B.into(), vec3(segment.b.x, segment.b.y, segment.b.z));
        return Some(map);
    }
    if let Some(halfspace) = shape.as_halfspace() {
        map.insert(k::KIND.into(), w::WORLD_BOUNDARY.into());
        let n = halfspace.normal;
        map.insert(k::NORMAL.into(), vec3(n.x, n.y, n.z));
        return Some(map);
    }
    // The rounded shapes report the shape they wrap plus the border that
    // rounded it, which is exactly how the schema spells them.
    if let Some(round) = shape.as_round_cuboid() {
        let he = round.inner_shape.half_extents * 2.0;
        map.insert(k::KIND.into(), w::BOX.into());
        map.insert(k::SIZE.into(), vec3(he.x, he.y, he.z));
        map.insert(k::EDGE_RADIUS.into(), f(round.border_radius));
        return Some(map);
    }
    if let Some(round) = shape.as_round_cylinder() {
        map.insert(k::KIND.into(), w::CYLINDER.into());
        map.insert(k::RADIUS.into(), f(round.inner_shape.radius));
        map.insert(k::HEIGHT.into(), f(round.inner_shape.half_height * 2.0));
        map.insert(k::EDGE_RADIUS.into(), f(round.border_radius));
        return Some(map);
    }
    if let Some(round) = shape.as_round_cone() {
        map.insert(k::KIND.into(), w::CONE.into());
        map.insert(k::RADIUS.into(), f(round.inner_shape.radius));
        map.insert(k::HEIGHT.into(), f(round.inner_shape.half_height * 2.0));
        map.insert(k::EDGE_RADIUS.into(), f(round.border_radius));
        return Some(map);
    }
    None
}

pub(crate) fn get_collider_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState3d>();
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
    // A `fit` box is a cuboid to rapier; reading it back as one would lose the
    // mesh it was fitted to and the pose it was fitted at.
    let authored_kind = map.get(k::KIND).and_then(toml::Value::as_str);
    if let Some(shape) = collider_shape_params(collider.shape())
        && authored_kind
            .is_none_or(|kind| shape.get(k::KIND).and_then(toml::Value::as_str) == Some(kind))
    {
        map.extend(shape);
    }
    let body = collider.parent().and_then(|b| state.world.bodies.get(b));
    read_material(
        collider,
        body.is_some_and(crate::body::has_total_mass),
        &mut map,
    );
    Some(toml::Value::Table(map))
}

/// A node's first collider, for the readers that ask one question about it.
pub(crate) fn with_first_collider<R>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&Collider) -> Result<R>,
) -> Result<R> {
    let entity = balaur_core::entity_of(node)?;
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let handle = first_collider(&state, entity)?;
    f(&state.world.colliders[handle])
}

/// A voxel collider's grid, for editing in place.
///
/// Every edit bumps the collider's revision, which is what puts a dug hole in
/// the digest: nothing else about the world changes until something falls into
/// it, and two machines that disagree about a hole must not agree about the
/// frame.
pub(crate) fn with_voxels(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&mut crate::rapier3d::parry::shape::Voxels),
) -> Result<()> {
    let entity = balaur_core::entity_of(node)?;
    let state = eng.resource::<PhysicsState3d>();
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

/// A collider's shape as points and triangles, through parry's own
/// tessellation: which every shape has, voxels included.
fn collider_mesh_value(eng: &Engine, node: NodeId) -> Result<balaur_script::Value> {
    let entity = balaur_core::entity_of(node)?;
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let handle = first_collider(&state, entity)?;
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
        (k::POINTS, balaur_script::Value::List(points)),
        (k::INDICES, balaur_script::Value::List(indices)),
    ]))
}

/// Everything a collider carries besides its shape, as schema text. Shared
/// with `collider2d`: a material is dimension-free.
pub(crate) fn shared_collider_schema() -> String {
    let combine = v::options(w::COMBINE_RULES);
    let average = w::AVERAGE;
    let material = v::schema(&[
        (
            k::RESTITUTION,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Bounciness: 0 is a dead stop, 1 a full rebound, and above 1 each bounce gains energy", group = "surface" }"#,
        ),
        (
            k::FRICTION,
            r#"{ type = "float", default = 0.5, min = 0.0, description = "Surface friction; 0 is ice", group = "surface" }"#,
        ),
        (
            k::DENSITY,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "Mass per volume, so the shape's size sets its mass; 0 makes a collider that adds no mass to its body", group = "mass" }"#,
        ),
        (
            k::MASS,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Mass in kilograms, overriding what density works out to; 0 keeps the density", group = "mass" }"#,
        ),
        (
            k::FRICTION_COMBINE,
            &format!(
                r#"{{ type = "enum", default = "{average}", options = [{combine}], description = "How this surface's friction combines with the other one's", group = "surface" }}"#
            ),
        ),
        (
            k::RESTITUTION_COMBINE,
            &format!(
                r#"{{ type = "enum", default = "{average}", options = [{combine}], description = "How this surface's bounciness combines with the other one's", group = "surface" }}"#
            ),
        ),
        (
            k::COLLISION_MARGIN,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "A margin the solver treats as already touching; stops thin shapes tunnelling and jittering", group = "contacts" }"#,
        ),
        (
            k::SENSOR,
            r#"{ type = "bool", default = false, description = "Detects overlaps without colliding: bodies pass through and are reported" }"#,
        ),
        (
            k::ENABLED,
            r#"{ type = "bool", default = true, description = "Collide at all; a disabled collider keeps its shape and costs nothing" }"#,
        ),
    ]);
    [material, shared_filter_schema()].join("\n")
}

/// The layer, test-mode, event and contact rows every collider takes, split
/// from [`shared_collider_schema`] under `MAX_FN_LINES`.
fn shared_filter_schema() -> String {
    let layers = v::layer_options();
    let tests = v::options(w::TEST_MODES);
    let both = w::BOTH;
    let events = v::options(&v::flags::events().map(|(name, _)| name));
    let collisions = v::options(&v::flags::collision_types().map(|(name, _)| name));
    let watched = v::options(w::DEFAULT_COLLISIONS);
    v::schema(&[
        (
            k::COLLISION_LAYER,
            &format!(
                r#"{{ type = "flags", default = ["1"], options = [{layers}], description = "The layers this collider is on", group = "filtering" }}"#
            ),
        ),
        (
            k::COLLISION_MASK,
            &format!(
                r#"{{ type = "flags", default = [], options = [{layers}], description = "The layers it collides with; empty means every layer", group = "filtering" }}"#
            ),
        ),
        (
            k::SOLVER_LAYER,
            &format!(
                r#"{{ type = "flags", default = ["1"], options = [{layers}], description = "Layers for the solver alone: a pair can be detected but not resolved", group = "filtering" }}"#
            ),
        ),
        (
            k::SOLVER_MASK,
            &format!(
                r#"{{ type = "flags", default = [], options = [{layers}], description = "Which solver layers this one pushes against; empty means all of them", group = "filtering" }}"#
            ),
        ),
        (
            k::COLLISION_TEST,
            &format!(
                r#"{{ type = "enum", default = "{both}", options = [{tests}], description = "Whether a pair is tested when both colliders' layers accept the other, or when either does; two colliders that differ use both", group = "filtering" }}"#
            ),
        ),
        (
            k::SOLVER_TEST,
            &format!(
                r#"{{ type = "enum", default = "{both}", options = [{tests}], description = "The same choice for the solver layers", group = "filtering" }}"#
            ),
        ),
        (
            k::EVENTS,
            &format!(
                r#"{{ type = "flags", default = [], options = [{events}], description = "What this collider reports to its node's script: on_collision_enter and on_collision_exit, or on_contact_force", group = "filtering" }}"#
            ),
        ),
        (
            k::CONTACT_FORCE_THRESHOLD,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "How hard a contact must be before on_contact_force is called", group = "contacts" }"#,
        ),
        (
            k::CONTACT_PAIRS,
            &format!(
                r#"{{ type = "flags", default = [{watched}], options = [{collisions}], description = "Which pairs of body kinds this collider is tested against; a sensor watching kinematic platforms needs more than the default", group = "filtering" }}"#
            ),
        ),
        (
            k::ONE_WAY,
            r#"{ type = "bool", default = false, description = "A platform bodies land on from the side one_way_axis names and pass through from the other", group = "contacts" }"#,
        ),
        (
            k::ONE_WAY_ANGLE,
            r#"{ type = "float", default = 0.1, min = 0.0, max = 180.0, unit = "degrees", description = "How far a contact's normal may lean from one_way_axis and still hold the body; radians in the file", group = "contacts" }"#,
        ),
    ])
}

/// The VHACD rows both dimensions' `convex_decomposition` takes, over parry's
/// own defaults for that dimension.
pub(crate) fn vhacd_schema(resolution: u32, concavity: f32, max_hulls: u32) -> String {
    let defaults = crate::rapier3d::parry::transformation::vhacd::VHACDParameters::default();
    v::schema(&[
        (
            k::RESOLUTION,
            &format!(
                r#"{{ type = "int", default = {resolution}, min = 1, description = "How fine the voxel grid a decomposition cuts is", group = "decomposition" }}"#
            ),
        ),
        (
            k::MAX_CONCAVITY,
            &format!(
                r#"{{ type = "float", default = {concavity:?}, min = 0.0, description = "How deep a dent a piece may keep before it is cut again", group = "decomposition" }}"#
            ),
        ),
        (
            k::MAX_CONVEX_HULLS,
            &format!(
                r#"{{ type = "int", default = {max_hulls}, min = 1, description = "The most pieces a decomposition is asked to leave; parry 0.31 passes it on unread, so it limits nothing yet", group = "decomposition" }}"#
            ),
        ),
        (
            k::SYMMETRY_BIAS,
            &format!(
                r#"{{ type = "float", default = {:?}, min = 0.0, max = 1.0, description = "How much a cut prefers a plane of symmetry", group = "decomposition" }}"#,
                defaults.alpha
            ),
        ),
        (
            k::REVOLUTION_BIAS,
            &format!(
                r#"{{ type = "float", default = {:?}, min = 0.0, max = 1.0, description = "How much a cut prefers an axis of revolution", group = "decomposition" }}"#,
                defaults.beta
            ),
        ),
        (
            k::PLANE_DOWNSAMPLING,
            &format!(
                r#"{{ type = "int", default = {}, min = 1, description = "How coarsely the cutting planes are searched first; 1 tries every one", group = "decomposition" }}"#,
                defaults.plane_downsampling
            ),
        ),
        (
            k::HULL_DOWNSAMPLING,
            &format!(
                r#"{{ type = "int", default = {}, min = 1, description = "How coarsely a piece's hull is sampled while choosing a cut; 1 uses every point", group = "decomposition" }}"#,
                defaults.convex_hull_downsampling
            ),
        ),
        (
            k::APPROXIMATE_HULLS,
            &format!(
                r#"{{ type = "bool", default = {}, description = "Estimate each piece's hull while cutting rather than building it exactly", group = "decomposition" }}"#,
                defaults.convex_hull_approximation
            ),
        ),
    ])
}

/// The `triangle_mesh` cleanup rows both dimensions take.
pub(crate) fn trimesh_schema() -> String {
    v::schema(&[
        (
            k::MERGE_VERTICES,
            r#"{ type = "bool", default = false, description = "Merge vertices at exactly the same place when building a triangle_mesh", group = "mesh cleanup" }"#,
        ),
        (
            k::DROP_DEGENERATE_TRIANGLES,
            r#"{ type = "bool", default = false, description = "Drop triangles that name one vertex twice; merges vertices too", group = "mesh cleanup" }"#,
        ),
        (
            k::DROP_DUPLICATE_TRIANGLES,
            r#"{ type = "bool", default = false, description = "Drop a triangle whose three vertices another one already names; merges vertices too", group = "mesh cleanup" }"#,
        ),
        (
            k::DROP_BAD_TOPOLOGY,
            r#"{ type = "bool", default = false, description = "Drop the triangles that stop the mesh's edge topology from being built", group = "mesh cleanup" }"#,
        ),
        (
            k::TOPOLOGY,
            r#"{ type = "bool", default = false, description = "Build the mesh's half-edge topology", group = "mesh cleanup" }"#,
        ),
        (
            k::CONNECTED_COMPONENTS,
            r#"{ type = "bool", default = false, description = "Work out which triangles form each separate piece of the mesh", group = "mesh cleanup" }"#,
        ),
        (
            k::TWO_SIDED_EDGES,
            r#"{ type = "bool", default = false, description = "fix_internal_edges for a mesh hit from both sides: a contact from behind a triangle is kept and smoothed, not dropped", group = "contacts" }"#,
        ),
    ])
}

/// The `collider3d` key. Not backed by a component type either: it writes
/// into [`crate::PhysicsState3d`].
pub(crate) fn register_collider_component(reg: &mut Registry<'_>) {
    let shapes = v::options(w::SHAPES);
    let default = w::BOX;
    let fills = v::options(w::FILL_MODES);
    let solid = w::SOLID;
    let hull = w::CONVEX_HULL;
    let fits = v::options(w::FIT_MODES);
    let axes = v::options(w::CAPSULE_AXES);
    let y = w::Y;
    let methods = v::options(w::DECOMPOSITION_METHODS_3D);
    let vhacd = w::VHACD;
    let edges = v::options(w::EDGE_MODES);
    let chain = w::CHAIN;
    let tuning = crate::rapier3d::parry::transformation::vhacd::VHACDParameters::default();
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{default}", options = [{shapes}], description = "Collision shape" }}"#)),
            (k::RADIUS, r#"{ type = "float", default = 0.5, min = 0.01, description = "Radius, for ball, capsule, cylinder and cone" }"#),
            (k::HEIGHT, r#"{ type = "float", default = 2.0, min = 0.0, description = "Length tip to tip, for capsule, cylinder and cone; a capsule with height 0 runs from a to b instead" }"#),
            (k::UP_AXIS, &format!(r#"{{ type = "enum", default = "{y}", options = [{axes}], description = "The axis a capsule lies along; cylinder and cone stand along y", group = "shape" }}"#)),
            (k::SIZE, r#"{ type = "vec3", default = [1.0, 1.0, 1.0], description = "Whole size along each axis, when kind is box" }"#),
            (k::EDGE_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "Rounds a box, cylinder, cone, triangle, convex_hull, convex_mesh or vhacd convex_decomposition by this radius; a rounded shape slides over seams instead of catching on them", group = "shape" }"#),
            (k::A, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "First corner, when kind is triangle or segment, and a capsule's first end when its height is 0", group = "shape" }"#),
            (k::B, r#"{ type = "vec3", default = [1.0, 0.0, 0.0], description = "Second corner, when kind is triangle or segment, and a capsule's other end when its height is 0", group = "shape" }"#),
            (k::C, r#"{ type = "vec3", default = [0.0, 1.0, 0.0], description = "Third corner, when kind is triangle", group = "shape" }"#),
            (k::NORMAL, r#"{ type = "vec3", default = [0.0, 1.0, 0.0], description = "Which way the infinite plane faces, when kind is world_boundary", group = "shape" }"#),
            (k::MESH, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "Geometry for a triangle_mesh, convex_hull, convex_mesh, convex_decomposition, polyline, fit, voxelized_mesh or voxelized_points collider", group = "shape" }}"#, balaur_core::mesh::MESH_ASSET_TYPE)),
            (k::EDGES, &format!(r#"{{ type = "enum", default = "{chain}", options = [{edges}], description = "Which edges a polyline takes from its mesh: the points in order, or every edge of its triangles", group = "shape" }}"#)),
            (k::HEIGHTFIELD, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "Terrain grid, when kind is heightfield; the asset's holes are cut out of it", group = "shape" }}"#, balaur_core::heightfield::HEIGHTFIELD_ASSET_TYPE)),
            (k::VOXELS, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "Filled cells, when kind is voxels; a script may dig into them while the game runs", group = "shape" }}"#, balaur_core::voxels::VOXELS_ASSET_TYPE)),
            (k::VOXEL_SIZE, r#"{ type = "float", default = 0.0, min = 0.0, description = "How big one cell is: 0 keeps a voxels asset's own cell size, and is 0.25 for voxelized_mesh and voxelized_points", group = "shape" }"#),
            (k::FILL, &format!(r#"{{ type = "enum", default = "{solid}", options = [{fills}], description = "Whether voxelizing a mesh fills its inside or only its shell, for voxelized_mesh and a convex_decomposition's voxel grid", group = "shape" }}"#)),
            (k::FILL_CAVITIES, r#"{ type = "bool", default = false, description = "When a solid fill floods a mesh, leave the cavities a closed surface walls off empty", group = "shape" }"#),
            (k::FIT, &format!(r#"{{ type = "enum", default = "{hull}", options = [{fits}], description = "The shape fitted to the mesh, when kind is fit", group = "shape" }}"#)),
            (k::METHOD, &format!(r#"{{ type = "enum", default = "{vhacd}", options = [{methods}], description = "How a convex_decomposition is cut: into convex hulls, or into voxel parts", group = "decomposition" }}"#)),
            (k::FIX_INTERNAL_EDGES, r#"{ type = "bool", default = true, description = "Smooth the seams between a triangle_mesh's triangles, so a character does not catch on flat ground", group = "contacts" }"#),
            (k::ORIENTED, r#"{ type = "bool", default = false, description = "Treat the triangle_mesh as a closed, outward-facing surface, which makes inside and outside meaningful", group = "shape" }"#),
            (k::SCALE, r#"{ type = "vec3", default = [1.0, 1.0, 1.0], description = "Cell size and height scale of a heightfield", group = "shape" }"#),
            (k::CENTER_OF_MASS, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where this collider's mass sits, in its own space; read with inertia, and both 0 keep the shape's own", group = "mass" }"#),
            (k::INERTIA, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "This collider's resistance to spin about each axis; 0 with a center_of_mass takes the shape's own about that centre", group = "mass" }"#),
            (k::INERTIA_ROTATION, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], unit = "degrees", description = "Turns the axes inertia is measured about, x first; read with a non-zero inertia. Euler radians in the file", group = "mass" }"#),
            (k::ONE_WAY_AXIS, r#"{ type = "vec3", default = [0.0, 1.0, 0.0], description = "The side a one-way platform holds bodies on, in the collider's own axes: [0, 1, 0] lands them from above and lets them up through from below", group = "contacts" }"#),
            (k::SURFACE_VELOCITY, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "How fast the surface slides along itself, in the collider's own axes: a conveyor belt carries what rests on it", group = "contacts" }"#),
            (k::OFFSET, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where the shape sits relative to the node", group = "shape" }"#),
            (k::OFFSET_ROTATION, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], unit = "degrees", description = "How the shape is turned relative to the node, x first; Euler radians in the file", group = "shape" }"#),
        ]),
        vhacd_schema(tuning.resolution, scalar::f32_of(tuning.concavity), tuning.max_convex_hulls),
        trimesh_schema(),
        shared_collider_schema(),
    ]
    .join("\n");
    reg.register_component(
        c::COLLIDER_3D,
        ComponentDef {
            events: crate::vocabulary::hook::COLLIDER,
            warnings: None,
            doc: "The node's 3D collision shape, chosen by `kind`. It belongs to the node's `body3d` or the nearest body above it; without one it is static geometry.",
            schema: ComponentDef::parse_schema(c::COLLIDER_3D, &schema),
            tags: &[balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
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
        ("set_collider", &[c::COLLIDER_3D], "", "Replace the node's collider from a `collider3d` table: `kind`, `radius`, `size`, `friction`, and the rest of the component's own vocabulary."),
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
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let handle = first_collider(&state, entity)?;
        let aabb = state.world.colliders[handle].compute_aabb();
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
        ("set_voxel", &[c::COLLIDER_3D], "", "Fill or empty one cell of a voxel collider: digging a hole, or building a wall, while the game runs."),
        ("voxel", &[c::COLLIDER_3D], "", "Whether one cell of a voxel collider is filled."),
        ("voxel_at", &[c::COLLIDER_3D], "", "The cell a world position falls in, as three whole numbers."),
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
            let state = eng.resource::<PhysicsState3d>();
            let state = state.borrow();
            let handle = first_collider(&state, entity)?;
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
            let state = eng.resource::<PhysicsState3d>();
            let state = state.borrow();
            let handle = first_collider(&state, entity)?;
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
        ("collider_mesh", &[c::COLLIDER_3D], "", "The collider's shape as points and triangles, including a voxel grid's, for drawing it or for spawning the pieces it broke into."),
        ("collider_mass", &[c::COLLIDER_3D], "", "What this collider weighs, density and size together."),
        ("collider_volume", &[c::COLLIDER_3D], "", "How much space the shape encloses."),
        ("swept_aabb", &[c::COLLIDER_3D], "", "The box the collider covers over the next fixed step, from where it is to where its body's velocity and forces carry it."),
        ("handles", &[c::COLLIDER_3D], "", "The rapier handles behind this node, its body and its colliders, as `#{ body, colliders }` of index and generation pairs. For matching a log line against rapier's own output."),
        ("collider_mass_properties", &[c::COLLIDER_3D], "", "What this collider adds to its body, in its own space: `#{ mass, center_of_mass, inertia, inertia_rotation }`."),
        ("aabb", &[c::COLLIDER_3D], "", "The world-space box the collider currently occupies, as its two opposite corners."),
    ]);
    m.function("collider_mesh", |eng: &Engine, node: NodeId| {
        collider_mesh_value(eng, node)
    });
    m.function("collider_volume", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |collider| Ok(collider.volume()))
    });
    m.function("swept_aabb", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let collider = &state.world.colliders[first_collider(&state, entity)?];
        // Rapier's own prediction, which its broad phase clamps to a body's
        // `speculative_distance`; unclamped here, so the box spans the whole step.
        let dt = scalar::real(balaur_core::fixed_dt());
        let next = collider
            .parent()
            .and_then(|parent| state.world.bodies.get(parent))
            .zip(collider.position_wrt_parent())
            .map_or(*collider.position(), |(body, local)| {
                body.predict_position_using_velocity_and_forces(dt) * local
            });
        let aabb = collider.compute_swept_aabb(&next);
        Ok((
            aabb.mins.x,
            aabb.mins.y,
            aabb.mins.z,
            aabb.maxs.x,
            aabb.maxs.y,
            aabb.maxs.z,
        ))
    });
    m.function("handles", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
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
            (k::BODY, body),
            (k::COLLIDERS, colliders),
        ]))
    });
    m.function("collider_mass", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let handle = first_collider(&state, entity)?;
        Ok(state.world.colliders[handle].mass())
    });
    m.function("collider_mass_properties", |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |collider| {
            use balaur_script::Value;
            let props = collider.mass_properties();
            Ok(crate::vocabulary::map([
                (k::MASS, Value::Num(f64::from(props.mass()))),
                (k::CENTER_OF_MASS, Value::Vec3(scalar::a3(props.local_com))),
                (
                    k::INERTIA,
                    Value::Vec3(scalar::a3(props.principal_inertia())),
                ),
                (
                    k::INERTIA_ROTATION,
                    Value::Vec3(crate::body::euler_of(props.principal_inertia_local_frame)),
                ),
            ]))
        })
    });
}
