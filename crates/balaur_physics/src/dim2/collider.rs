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
    InteractionTestMode, MassProperties, RigidBodyHandle,
};
use crate::scalar::{self, Pose2, Real, Rotation2};

use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::dim2::{PhysicsState2d, node_pose_2d, shapes};
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

crate::shared::collider::functions!(
    state = PhysicsState2d,
    refit = crate::dim2::body::refit_mass
);

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
    // A pose the builder already holds sits inside the offset, as in 3D.
    let built = builder.position;
    let handle = if let Some((body_node, body)) = nearest_body(eng, entity) {
        let local = pose_relative_to(eng, entity, body_node)?;
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        state
            .world
            .insert_collider(builder.position(local * offset * built), Some(body))
    } else {
        let pose = node_pose_2d(eng, entity)?;
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        state
            .world
            .insert_collider(builder.position(pose * offset * built), None)
    };
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    state.world.colliders[handle].user_data = u128::from(entity.to_bits().get());
    if let Some(body) = state.world.colliders[handle].parent()
        && crate::dim2::body::has_total_mass(&state.world.bodies[body])
    {
        state.world.colliders[handle].set_density(0.0);
        crate::dim2::body::refit_mass(&mut state, body);
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
    let height = scalar::real(v::f(params, k::HEIGHT, 2.0));
    let half_segment = (height.max(0.01) / 2.0 - radius).max(0.0);
    let he = |i: usize| scalar::real(v::axis(params, k::SIZE, i, 1.0) / 2.0).max(0.01);
    let point = |key: &str, fallback: [f32; 2]| scalar::v2a(v::vec2(params, key, fallback));
    let border = scalar::real(v::f(params, k::EDGE_RADIUS, 0.0)).max(0.0);
    let rounded = border > 0.0;
    let mut mass = None;
    let builder = match kind {
        w::CIRCLE => ColliderBuilder2::ball(radius),
        w::RECTANGLE if rounded => ColliderBuilder2::round_cuboid(he(0), he(1), border),
        w::RECTANGLE => ColliderBuilder2::cuboid(he(0), he(1)),
        // `height = 0` hands the capsule's length to its two ends, as in 3D.
        w::CAPSULE if height <= 0.0 => ColliderBuilder2::capsule_from_endpoints(
            point(k::A, [0.0, 0.0]),
            point(k::B, [1.0, 0.0]),
            radius,
        ),
        w::CAPSULE if v::text(params, k::UP_AXIS, w::Y) == w::X => {
            ColliderBuilder2::capsule_x(half_segment, radius)
        }
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
        w::TRIANGLE_MESH
        | w::CONVEX_HULL
        | w::CONVEX_POLYGON
        | w::POLYLINE
        | w::FIT
        | w::VOXELIZED_MESH
        | w::VOXELIZED_POINTS => shapes::mesh_collider(eng, params, kind, border)?,
        w::CONVEX_DECOMPOSITION => {
            let (shape, weight) = shapes::decomposition_collider(eng, params)?;
            mass = weight;
            shape
        }
        w::VOXELS => shapes::voxel_collider(eng, params)?,
        w::HEIGHTFIELD => shapes::heightfield_collider(eng, params)?,
        other => return Err(anyhow!("unknown collider2d kind '{other}'")),
    };
    let builder = with_material(builder, params);
    Ok(match mass {
        // `mass` pins the total itself, and wins where it is written.
        Some(mass) if v::f(params, k::MASS, 0.0) <= 0.0 => builder.mass_properties(mass),
        _ => with_mass_properties(builder, params),
    })
}

/// `center_of_mass` and `inertia`, for a collider that states either, as in
/// 3D: the mass is `mass` or what the density makes of the shape, and an
/// `inertia` of 0 is the shape's own about the stated centre.
fn with_mass_properties(builder: ColliderBuilder2, params: &toml::Value) -> ColliderBuilder2 {
    let com = v::vec2(params, k::CENTER_OF_MASS, [0.0; 2]);
    let inertia = scalar::real(v::f(params, k::INERTIA, 0.0));
    if crate::body::is_default(&com) && inertia == 0.0 {
        return builder;
    }
    let mass = match scalar::real(v::f(params, k::MASS, 0.0)) {
        stated if stated > 0.0 => stated,
        _ => {
            let unit = builder.shape.mass_properties(1.0);
            scalar::real(v::f(params, k::DENSITY, 1.0).max(0.0)) * unit.mass()
        }
    };
    mass_at(builder, scalar::v2a(com), mass, inertia)
}

/// A collider of `mass` centred on `com`, in its own space: `inertia` when it
/// is above 0, else the shape's own about that centre.
pub(crate) fn mass_at(
    builder: ColliderBuilder2,
    com: Vector,
    mass: Real,
    inertia: Real,
) -> ColliderBuilder2 {
    let inertia = if inertia > 0.0 {
        inertia
    } else {
        let mut shaped = builder.shape.mass_properties(1.0);
        shaped.set_mass(mass, true);
        shaped.principal_inertia() + mass * (com - shaped.local_com).length_squared()
    };
    builder.mass_properties(MassProperties::new(com, mass, inertia))
}

/// Put a 2D collider on the layers `params` names, for a builder whose other
/// rows its owner has already set (see `crate::collider::with_groups`).
pub(crate) fn with_groups_2d(builder: ColliderBuilder2, params: &toml::Value) -> ColliderBuilder2 {
    builder.collision_groups(interaction_groups(
        params,
        k::COLLISION_LAYER,
        k::COLLISION_MASK,
        k::COLLISION_TEST,
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

/// The 2D half of `crate::collider::with_material`. The flag tables are
/// shared (`crate::vocabulary::flags`); only the types they are poured into
/// are per-dimension.
pub(crate) fn with_material(builder: ColliderBuilder2, params: &toml::Value) -> ColliderBuilder2 {
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
    if let Some(surface) = surface_of(params, [0.0, 1.0]) {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let handles = state.colliders.get(&entity).cloned().unwrap_or_default();
        state
            .surfaces
            .extend(handles.into_iter().map(|h| (h, surface)));
    }
    Ok(())
}

/// What the contact hook needs of this collider, as in 3D. `up` is the axis a
/// platform holds bodies on when `one_way_axis` is left out.
pub(crate) fn surface_of(
    params: &toml::Value,
    up: [f32; 2],
) -> Option<crate::dim2::events::Surface> {
    let one_way = v::boolean(params, k::ONE_WAY, false).then(|| {
        let axis = scalar::v2a(v::vec2(params, k::ONE_WAY_AXIS, up));
        let angle = scalar::real(v::f(params, k::ONE_WAY_ANGLE, 0.1).max(0.0));
        (axis.try_normalize().unwrap_or(Vector::Y), angle)
    });
    let velocity = scalar::v2a(v::vec2(params, k::SURFACE_VELOCITY, [0.0; 2]));
    (one_way.is_some() || velocity != Vector::ZERO)
        .then_some(crate::dim2::events::Surface { one_way, velocity })
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
    let body = collider.parent().and_then(|b| state.world.bodies.get(b));
    read_material(
        collider,
        body.is_some_and(crate::dim2::body::has_total_mass),
        &mut map,
    );
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
        let (a, b) = (capsule.segment.a, capsule.segment.b);
        let along = b - a;
        let straight = along.length();
        let centred = (a + b).length() <= 1.0e-5 * straight.max(1.0);
        let axis = [(w::X, along.x), (w::Y, along.y)]
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
                map.insert(k::A.into(), vec2(a.x, a.y));
                map.insert(k::B.into(), vec2(b.x, b.y));
            }
        }
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

/// Largest contact normal impulse currently applied to the node's body, every
/// collider rapier attached to it included (0 when untouched). A node with no
/// body answers for its own colliders. Gameplay uses this for impact damage.
pub(crate) fn max_contact_impulse(eng: &Engine, entity: Entity) -> Real {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let body = state
        .bodies
        .get(&entity)
        .and_then(|handle| state.world.bodies.get(*handle));
    let handles = match body {
        Some(body) => body.colliders(),
        None => state.colliders.get(&entity).map_or(&[][..], Vec::as_slice),
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
    // rapier2d's own defaults, which are not parry's 3D ones.
    let vhacd = crate::rapier2d::parry::transformation::vhacd::VHACDParameters::default();
    let axes = v::options(w::CAPSULE_AXES_2D);
    let y = w::Y;
    let fills = v::options(w::FILL_MODES);
    let solid = w::SOLID;
    let fits = v::options(w::FIT_MODES_2D);
    let hull = w::CONVEX_HULL;
    let edges = v::options(w::EDGE_MODES_2D);
    let chain = w::CHAIN;
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{default}", options = [{shapes}], description = "Collision shape" }}"#)),
            (k::RADIUS, r#"{ type = "float", default = 0.5, min = 0.01, description = "Circle radius, when kind is circle or capsule" }"#),
            (k::HEIGHT, r#"{ type = "float", default = 2.0, min = 0.0, description = "Length tip to tip, when kind is capsule; height 0 runs the capsule from a to b instead" }"#),
            (k::UP_AXIS, &format!(r#"{{ type = "enum", default = "{y}", options = [{axes}], description = "The axis a capsule lies along", group = "shape" }}"#)),
            (k::SIZE, r#"{ type = "vec2", default = [1.0, 1.0], description = "Whole size along each axis, when kind is rectangle" }"#),
            (k::EDGE_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "Rounds a rectangle, triangle, convex_hull, convex_polygon or a convex_decomposition's pieces by this radius, so it slides over seams instead of catching on them", group = "shape" }"#),
            (k::A, r#"{ type = "vec2", default = [0.0, 0.0], description = "First corner, when kind is triangle or segment, and a capsule's first end when its height is 0", group = "shape" }"#),
            (k::B, r#"{ type = "vec2", default = [1.0, 0.0], description = "Second corner, when kind is triangle or segment, and a capsule's other end when its height is 0", group = "shape" }"#),
            (k::C, r#"{ type = "vec2", default = [0.0, 1.0], description = "Third corner, when kind is triangle", group = "shape" }"#),
            (k::NORMAL, r#"{ type = "vec2", default = [0.0, 1.0], description = "Which way the infinite line faces, when kind is world_boundary", group = "shape" }"#),
            (k::MESH, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "Points and triangles for a triangle_mesh, convex_hull, convex_polygon, convex_decomposition, polyline, fit, voxelized_mesh or voxelized_points collider: the same asset a polygon draws", group = "shape" }}"#, balaur_core::mesh::MESH_ASSET_TYPE)),
            (k::EDGES, &format!(r#"{{ type = "enum", default = "{chain}", options = [{edges}], description = "Which edges a polyline takes from its mesh: the points in order, the outline of its triangles, or every edge of them", group = "shape" }}"#)),
            (k::KEEP_COLLINEAR, r#"{ type = "bool", default = false, description = "Keep the points that lie on a straight edge of a convex_polygon instead of dropping them", group = "shape" }"#),
            (k::FIT, &format!(r#"{{ type = "enum", default = "{hull}", options = [{fits}], description = "The shape fitted to the mesh, when kind is fit; a fitted box keeps the pose it was fitted at", group = "shape" }}"#)),
            (k::HEIGHTFIELD, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "A row of heights, when kind is heightfield: a side-scroller's ground, with the asset's holes cut out", group = "shape" }}"#, balaur_core::heightfield::HEIGHTFIELD_ASSET_TYPE)),
            (k::SCALE, r#"{ type = "vec2", default = [1.0, 1.0], description = "Width and height scale of a heightfield", group = "shape" }"#),
            (k::VOXELS, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "Filled cells, when kind is voxels; a script may dig into them while the game runs", group = "shape" }}"#, balaur_core::voxels::VOXELS_ASSET_TYPE)),
            (k::VOXEL_SIZE, r#"{ type = "float", default = 0.0, min = 0.0, description = "How big one cell is: 0 keeps a voxels asset's own cell size, and is 0.25 for voxelized_mesh and voxelized_points", group = "shape" }"#),
            (k::FILL, &format!(r#"{{ type = "enum", default = "{solid}", options = [{fills}], description = "Whether voxelizing an outline fills its inside or only its edge, for voxelized_mesh and a vhacd or voxels decomposition", group = "shape" }}"#)),
            (k::FILL_CAVITIES, r#"{ type = "bool", default = false, description = "When a solid fill floods an outline, leave the holes it walls off empty", group = "shape" }"#),
            (k::FIX_SELF_INTERSECTIONS, r#"{ type = "bool", default = false, description = "When a solid fill floods an outline, handle the places it crosses itself", group = "shape" }"#),
            (k::FIX_INTERNAL_EDGES, r#"{ type = "bool", default = true, description = "Take neighbouring triangles into account for a triangle_mesh's contacts, so a body does not catch on the seam between two of them", group = "contacts" }"#),
            (k::ORIENTED, r#"{ type = "bool", default = false, description = "Treat a triangle_mesh or polyline as one-sided: the winding decides which side is solid, counter-clockwise enclosing the solid", group = "shape" }"#),
            (k::OVERLAP, r#"{ type = "float", default = 0.9, min = 0.0, max = 1.0, description = "How far an exact convex_decomposition piece grows through each seam it shares, so nothing wedges into one: 0 leaves the plain pieces, 1 grows flush with the face that stops it", group = "decomposition" }"#),
            (k::METHOD, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How a convex_decomposition is cut: exact, over the mesh's own triangles; vhacd, into hulls over a voxelised outline; or voxels, into voxel parts", group = "decomposition" }}"#, w::EXACT, v::options(w::DECOMPOSITION_METHODS))),
            (k::CENTER_OF_MASS, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where this collider's mass sits, in its own space; read with inertia, and both 0 keep the shape's own", group = "mass" }"#),
            (k::INERTIA, r#"{ type = "float", default = 0.0, min = 0.0, description = "This collider's resistance to spin; 0 with a center_of_mass takes the shape's own about that centre", group = "mass" }"#),
            (k::OFFSET, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where the shape sits relative to the node", group = "shape" }"#),
            (k::OFFSET_ROTATION, r#"{ type = "float", default = 0.0, unit = "degrees", description = "How the shape is turned relative to the node; radians in the file", group = "shape" }"#),
            (k::ONE_WAY_AXIS, r#"{ type = "vec2", default = [0.0, 1.0], description = "The side a one-way platform holds bodies on, in the collider's own axes: [0, 1] lands them from above and lets them up through from below", group = "contacts" }"#),
            (k::SURFACE_VELOCITY, r#"{ type = "vec2", default = [0.0, 0.0], description = "How fast the surface slides along itself, in the collider's own axes: a conveyor belt carries what rests on it", group = "contacts" }"#),
        ]),
        crate::collider::vhacd_schema(vhacd.resolution, scalar::f32_of(vhacd.concavity), vhacd.max_convex_hulls),
        crate::collider::trimesh_schema(),
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
        ("set_voxel_size", &[c::COLLIDER_2D], "", "Resize every cell of a voxel collider, keeping which cells are filled."),
        ("voxel_size", &[c::COLLIDER_2D], "", "How big one cell of a voxel collider is, along each axis."),
        ("crop_voxels", &[c::COLLIDER_2D], "", "Empty every cell outside the cells from `(min_x, min_y)` to `(max_x, max_y)`, both included. A range holding no filled cell leaves the grid as it was."),
        ("combine_voxels", &[c::COLLIDER_2D], "", "Tell two voxel colliders on one lattice about each other's cells, so a body sliding from one onto the other does not catch on the seam. Both must share a cell size and a rotation."),
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
    install_voxel_2d_edit_api(m);
}

/// The whole-grid edits: resizing, cropping and joining two grids' seams.
///
/// Split from [`install_voxel_2d_api`] under `MAX_FN_LINES`.
fn install_voxel_2d_edit_api(m: &mut dyn Bindings<Engine>) {
    m.function(
        "set_voxel_size",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            let size = scalar::v2(x.max(0.001), y.max(0.001));
            with_voxels(eng, node, |voxels| voxels.set_voxel_size(size))
        },
    );
    m.function("voxel_size", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let handle = first_collider(&state, entity)?;
        let voxels = state.world.colliders[handle]
            .shape()
            .as_voxels()
            .ok_or_else(|| anyhow!("this node's collider is not a voxel grid"))?;
        Ok(balaur_script::Value::Vec2(scalar::a2(voxels.voxel_size())))
    });
    m.function(
        "crop_voxels",
        |eng: &Engine, (node, min_x, min_y, max_x, max_y): (NodeId, i32, i32, i32, i32)| {
            with_voxels(eng, node, |voxels| {
                voxels.crop(scalar::cell2(min_x, min_y), scalar::cell2(max_x, max_y));
            })
        },
    );
    m.function(
        "combine_voxels",
        |eng: &Engine, (node, other): (NodeId, NodeId)| combine_voxels(eng, node, other),
    );
}

/// Merge the neighbour states of two voxel grids: `other`'s cell `key` sits at
/// `key + shift` on `node`'s lattice.
fn combine_voxels(eng: &Engine, node: NodeId, other: NodeId) -> Result<()> {
    let (a, b) = (
        balaur_core::entity_of(node)?,
        balaur_core::entity_of(other)?,
    );
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let (first, second) = (first_collider(&state, a)?, first_collider(&state, b)?);
    let not_voxels = || anyhow!("both nodes need a voxel collider");
    let here = &state.world.colliders[first];
    let there = &state.world.colliders[second];
    let mut theirs = there.shape().as_voxels().ok_or_else(not_voxels)?.clone();
    let size = here
        .shape()
        .as_voxels()
        .ok_or_else(not_voxels)?
        .voxel_size();
    let offset = here.position().inverse() * there.position().translation;
    let shift = scalar::cell2(
        (offset.x / size.x).round() as i32,
        (offset.y / size.y).round() as i32,
    );
    let ours = state.world.colliders[first]
        .shape_mut()
        .as_voxels_mut()
        .ok_or_else(not_voxels)?;
    ours.combine_voxel_states(&mut theirs, shift);
    if let Some(slot) = state.world.colliders[second].shape_mut().as_voxels_mut() {
        *slot = theirs;
    }
    state.shape_revision = state.shape_revision.wrapping_add(1);
    Ok(())
}
