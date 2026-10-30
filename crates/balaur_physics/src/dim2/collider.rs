//! `collider2d`: shapes, their material, and the overlap and contact
//! queries that read them back.

use crate::rapier2d::math::Vector;
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{self, Component, Written};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

use crate::rapier2d::prelude::{
    ActiveCollisionTypes, ActiveEvents, ActiveHooks, CoefficientCombineRule,
    ColliderBuilder as ColliderBuilder2, ColliderHandle, Group, InteractionGroups,
    InteractionTestMode, MassProperties, RigidBodyHandle,
};
use crate::scalar::{self, Pose2, Real, Rotation2};
use crate::shared::beside::{self, Beside};

use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::dim2::{PhysicsState2d, node_pose_2d, shapes};
use crate::properties::{
    Axis2d, ColliderEvent, ColliderKind2d, ContactPair, Decomposition2d, Edges2d, Fill, Fit2d,
    LayerTest,
};
use crate::vocabulary::{self as v, component as c, keys as k, words as w};
use balaur_core::layers::Layer;

/// The node's 2D collision shape, chosen by `kind`. It belongs to the node's `body2d` or the nearest body above it; without one it is static geometry.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, Component)]
#[component(name = "collider2d")]
pub struct Collider2d {
    /// Collision shape
    #[property(default = ColliderKind2d::Rectangle)]
    pub kind: ColliderKind2d,
    /// Whole size along each axis, when kind is rectangle
    #[property(default = [1.0, 1.0])]
    pub size: [f32; 2],
    /// Circle radius, when kind is circle or capsule
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Length tip to tip, when kind is capsule; height 0 runs the capsule from a to b instead
    #[property(default = 2.0, min = 0.0)]
    pub height: f32,
    /// Detects overlaps without colliding: bodies pass through and are reported
    pub sensor: bool,
    /// Collide at all; a disabled collider keeps its shape and costs nothing
    #[property(default = true)]
    pub enabled: bool,
    /// What the surface is made of: friction, bounce and how each combines; empty takes a physics_material at its defaults
    #[property(asset = "physics_material")]
    pub physics_material: String,
    /// Where the shape sits relative to the node
    #[property(group = "shape")]
    pub offset: [f32; 2],
    /// How the shape is turned relative to the node; radians in the file
    #[property(group = "shape", unit = "degrees")]
    pub offset_rotation: f32,
    /// The axis a capsule lies along
    #[property(default = Axis2d::Y, group = "shape")]
    pub up_axis: Axis2d,
    /// Rounds a rectangle, triangle, convex_hull, convex_polygon or a convex_decomposition's pieces by this radius, so it slides over seams instead of catching on them
    #[property(group = "shape", min = 0.0)]
    pub edge_radius: f32,
    /// First corner, when kind is triangle or segment, and a capsule's first end when its height is 0
    #[property(name = "a", group = "shape")]
    pub first: [f32; 2],
    /// Second corner, when kind is triangle or segment, and a capsule's other end when its height is 0
    #[property(name = "b", default = [1.0, 0.0], group = "shape")]
    pub second: [f32; 2],
    /// Third corner, when kind is triangle
    #[property(name = "c", default = [0.0, 1.0], group = "shape")]
    pub third: [f32; 2],
    /// Which way the infinite line faces, when kind is world_boundary
    #[property(default = [0.0, 1.0], group = "shape")]
    pub normal: [f32; 2],
    /// The layers this collider is on
    #[property(flags, default = vec![Layer(1)], group = "filtering")]
    pub collision_layer: Vec<Layer>,
    /// The layers it collides with; empty means every layer
    #[property(flags, group = "filtering")]
    pub collision_mask: Vec<Layer>,
    /// Whether a pair is tested when both colliders' layers accept the other, or when either does; two colliders that differ use both
    #[property(default = LayerTest::Both, group = "filtering")]
    pub collision_test: LayerTest,
    /// Layers for the solver alone: a pair can be detected but not resolved
    #[property(flags, default = vec![Layer(1)], group = "filtering")]
    pub solver_layer: Vec<Layer>,
    /// Which solver layers this one pushes against; empty means all of them
    #[property(flags, group = "filtering")]
    pub solver_mask: Vec<Layer>,
    /// The same choice for the solver layers
    #[property(default = LayerTest::Both, group = "filtering")]
    pub solver_test: LayerTest,
    /// Which pairs of body kinds this collider is tested against; a sensor watching kinematic platforms needs more than the default
    #[property(flags, default = vec![ContactPair::DynamicDynamic, ContactPair::DynamicKinematic, ContactPair::DynamicStatic], group = "filtering")]
    pub contact_pairs: Vec<ContactPair>,
    /// A platform bodies land on from the side one_way_axis names and pass through from the other
    #[property(group = "contacts")]
    pub one_way: bool,
    /// The side a one-way platform holds bodies on, in the collider's own axes: [0, 1] lands them from above and lets them up through from below
    #[property(default = [0.0, 1.0], group = "contacts")]
    pub one_way_axis: [f32; 2],
    /// How far a contact's normal may lean from one_way_axis and still hold the body; radians in the file
    #[property(
        default = 0.1,
        group = "contacts",
        max = 180.0,
        min = 0.0,
        unit = "degrees"
    )]
    pub one_way_angle: f32,
    /// How fast the surface slides along itself, in the collider's own axes: a conveyor belt carries what rests on it
    #[property(group = "contacts")]
    pub surface_velocity: [f32; 2],
    /// A margin the solver treats as already touching; stops thin shapes tunnelling and jittering
    #[property(group = "contacts", min = 0.0)]
    pub collision_margin: f32,
    /// Take neighbouring triangles into account for a triangle_mesh's contacts, so a body does not catch on the seam between two of them
    #[property(default = true, group = "contacts")]
    pub fix_internal_edges: bool,
    /// fix_internal_edges for a mesh hit from both sides: a contact from behind a triangle is kept and smoothed, not dropped
    #[property(group = "contacts")]
    pub two_sided_edges: bool,
    /// Mass per volume, so the shape's size sets its mass; 0 makes a collider that adds no mass to its body
    #[property(default = 1.0, group = "mass", min = 0.0)]
    pub density: f32,
    /// Mass in kilograms, overriding what density works out to; 0 keeps the density
    #[property(group = "mass", min = 0.0)]
    pub mass: f32,
    /// Where this collider's mass sits, in its own space; read with inertia, and both 0 keep the shape's own
    #[property(group = "mass")]
    pub center_of_mass: [f32; 2],
    /// This collider's resistance to spin; 0 with a center_of_mass takes the shape's own about that centre
    #[property(group = "mass", min = 0.0)]
    pub inertia: f32,
    /// What this collider reports to its node's script: on_collision_enter and on_collision_exit, or on_contact_force
    #[property(flags, group = "events")]
    pub events: Vec<ColliderEvent>,
    /// How hard a contact must be before on_contact_force is called
    #[property(group = "events", min = 0.0)]
    pub contact_force_threshold: f32,
    /// Points and triangles for a triangle_mesh, convex_hull, convex_polygon, convex_decomposition, polyline, fit, voxelized_mesh or voxelized_points collider: the same asset a polygon draws
    #[property(asset = "mesh", group = "mesh")]
    pub mesh: String,
    /// A row of heights, when kind is heightfield: a side-scroller's ground, with the asset's holes cut out
    #[property(asset = "heightfield", group = "mesh")]
    pub heightfield: String,
    /// Width and height scale of a heightfield
    #[property(default = [1.0, 1.0], group = "mesh")]
    pub scale: [f32; 2],
    /// Filled cells, when kind is voxels; a script may dig into them while the game runs
    #[property(asset = "voxels", group = "mesh")]
    pub voxels: String,
    /// How big one cell is: 0 keeps a voxels asset's own cell size, and is 0.25 for voxelized_mesh and voxelized_points
    #[property(group = "mesh", min = 0.0)]
    pub voxel_size: f32,
}

/// How the node's `collider2d` turns its `mesh` into a shape: the shape a fit takes, the edges a polyline keeps, how a convex_polygon and a voxelized outline are read, and what a triangle_mesh tidies first.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, Component)]
#[component(name = "mesh_collider2d")]
pub struct MeshCollider2d {
    /// The shape fitted to the mesh, when kind is fit; a fitted box keeps the pose it was fitted at
    #[property(default = Fit2d::ConvexHull)]
    pub fit: Fit2d,
    /// Which edges a polyline takes from its mesh: the points in order, the outline of its triangles, or every edge of them
    #[property(default = Edges2d::Chain)]
    pub edges: Edges2d,
    /// Treat a triangle_mesh or polyline as one-sided: the winding decides which side is solid, counter-clockwise enclosing the solid
    pub oriented: bool,
    /// Keep the points that lie on a straight edge of a convex_polygon instead of dropping them
    pub keep_collinear: bool,
    /// Whether voxelizing an outline fills its inside or only its edge, for voxelized_mesh and a vhacd or voxels decomposition
    #[property(default = Fill::Solid)]
    pub fill: Fill,
    /// When a solid fill floods an outline, leave the holes it walls off empty
    pub fill_cavities: bool,
    /// When a solid fill floods an outline, handle the places it crosses itself
    pub fix_self_intersections: bool,
    /// Merge vertices at exactly the same place when building a triangle_mesh
    #[property(group = "mesh cleanup")]
    pub merge_vertices: bool,
    /// Drop triangles that name one vertex twice; merges vertices too
    #[property(group = "mesh cleanup")]
    pub drop_degenerate_triangles: bool,
    /// Drop a triangle whose three vertices another one already names; merges vertices too
    #[property(group = "mesh cleanup")]
    pub drop_duplicate_triangles: bool,
    /// Drop the triangles that stop the mesh's edge topology from being built
    #[property(group = "mesh cleanup")]
    pub drop_bad_topology: bool,
    /// Build the mesh's half-edge topology
    #[property(group = "mesh cleanup")]
    pub topology: bool,
    /// Work out which triangles form each separate piece of the mesh
    #[property(group = "mesh cleanup")]
    pub connected_components: bool,
}

impl Beside for MeshCollider2d {
    type Main = Collider2d;

    fn reads(main: &Collider2d) -> bool {
        matches!(
            main.kind,
            ColliderKind2d::TriangleMesh
                | ColliderKind2d::Polyline
                | ColliderKind2d::Fit
                | ColliderKind2d::ConvexPolygon
                | ColliderKind2d::VoxelizedMesh
                | ColliderKind2d::ConvexDecomposition
        )
    }
}

/// How the node's `collider2d` cuts its mesh into convex pieces, for a convex_decomposition.
#[derive(Clone, Component)]
#[component(name = "convex_decomposition2d")]
pub struct ConvexDecomposition2d {
    /// How a convex_decomposition is cut: exact, over the mesh's own triangles; vhacd, into hulls over a voxelised outline; or voxels, into voxel parts
    #[property(default = Decomposition2d::Exact)]
    pub method: Decomposition2d,
    /// How far an exact convex_decomposition piece grows through each seam it shares, so nothing wedges into one: 0 leaves the plain pieces, 1 grows flush with the face that stops it
    #[property(default = 0.9, max = 1.0, min = 0.0)]
    pub overlap: f32,
    /// How deep a dent a piece may keep before it is cut again
    #[property(default = vhacd().concavity, min = 0.0)]
    pub max_concavity: f32,
    /// The most pieces a decomposition is asked to leave; parry 0.31 passes it on unread, so it limits nothing yet
    #[property(default = i64::from(vhacd().max_convex_hulls), min = 1)]
    pub max_convex_hulls: i64,
    /// How fine the voxel grid a decomposition cuts is
    #[property(default = i64::from(vhacd().resolution), min = 1)]
    pub resolution: i64,
    /// Estimate each piece's hull while cutting rather than building it exactly
    #[property(default = vhacd().convex_hull_approximation)]
    pub approximate_hulls: bool,
    /// How coarsely a piece's hull is sampled while choosing a cut; 1 uses every point
    #[property(default = i64::from(vhacd().convex_hull_downsampling), min = 1)]
    pub hull_downsampling: i64,
    /// How coarsely the cutting planes are searched first; 1 tries every one
    #[property(default = i64::from(vhacd().plane_downsampling), min = 1)]
    pub plane_downsampling: i64,
    /// How much a cut prefers a plane of symmetry
    #[property(default = vhacd().alpha, max = 1.0, min = 0.0)]
    pub symmetry_bias: f32,
    /// How much a cut prefers an axis of revolution
    #[property(default = vhacd().beta, max = 1.0, min = 0.0)]
    pub revolution_bias: f32,
}

impl Beside for ConvexDecomposition2d {
    type Main = Collider2d;

    fn reads(main: &Collider2d) -> bool {
        main.kind == ColliderKind2d::ConvexDecomposition
    }
}

/// rapier2d's own decomposition tuning, which is not parry's 3D one: what a
/// `convex_decomposition2d`'s rows start at.
fn vhacd() -> crate::rapier2d::parry::transformation::vhacd::VHACDParameters {
    crate::rapier2d::parry::transformation::vhacd::VHACDParameters::default()
}

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
    let material = crate::material::of(eng, params)?;
    let builder = with_material(builder, params, &material);
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
pub(crate) fn with_material(
    builder: ColliderBuilder2,
    params: &toml::Value,
    material: &crate::material::PhysicsMaterial,
) -> ColliderBuilder2 {
    let mut builder = builder
        .restitution(scalar::real(material.restitution))
        .friction(scalar::real(material.friction))
        .density(scalar::real(v::f(params, k::DENSITY, 1.0).max(0.0)))
        .friction_combine_rule(combine_rule(&material.friction_combine))
        .restitution_combine_rule(combine_rule(&material.restitution_combine))
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

/// Build again each collider and soft body naming a material file once the
/// asset cache has moved, so a material saved in the editor shows on every
/// body naming it, as a re-read tile map's cells do.
pub(crate) fn reread_materials(eng: &Engine) {
    let stale: Vec<(Entity, toml::Value, bool)> = {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let generation = balaur_core::assets::generation(eng);
        if state.materials_read == generation {
            return;
        }
        state.materials_read = generation;
        let surface = [k::PHYSICS_MATERIAL];
        let both = [k::PHYSICS_MATERIAL, k::SOFT_MATERIAL];
        let colliders = state
            .collider_params
            .iter()
            .filter(|(_, p)| crate::material::names_a_file(p, &surface));
        let soft = state
            .soft_params
            .iter()
            .filter(|(_, p)| crate::material::names_a_file(p, &both));
        colliders
            .map(|(entity, params)| (*entity, params.clone(), false))
            .chain(soft.map(|(entity, params)| (*entity, params.clone(), true)))
            .collect()
    };
    for (entity, params, soft) in stale {
        let rebuilt = if soft {
            crate::dim2::softbody::apply_softbody_2d(eng, entity, &params)
        } else {
            apply_collider(eng, entity, &params)
        };
        if let Err(why) = rebuilt {
            tracing::warn!("physics_material: {why:#}");
        }
    }
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

/// What a node's live collider was built from, as the table its builder
/// reads: `None` when it has none.
pub(crate) fn authored_collider(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    state.colliders.get(&entity)?.first()?;
    state.collider_params.get(&entity).cloned()
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

/// What the node's `collider2d` asks for, with the rows of the components
/// beside it written in; `None` when it has none.
fn asked(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let held = world.get::<&Collider2d>(entity).ok()?;
    let mut params = derived::to_table(&*held);
    beside::write_in::<MeshCollider2d>(&world, entity, &held, &mut params);
    beside::write_in::<ConvexDecomposition2d>(&world, entity, &held, &mut params);
    Some(params)
}

/// The collider the component asks for, built again.
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let Some(params) = asked(eng, entity) else {
        return Ok(());
    };
    apply_collider(eng, entity, &params)
}

/// `collider2d`: as `collider3d`, the collider itself in
/// [`crate::PhysicsState2d`].
pub(crate) fn register_collider2d_component(reg: &mut Registry<'_>) {
    let tags: &'static [&'static str] = &[
        balaur_core::components::tag::DIM_2D,
        balaur_core::components::tag::PHYSICS,
    ];
    beside::register::<MeshCollider2d>(reg, &[c::COLLIDER_2D], tags, after_write);
    beside::register::<ConvexDecomposition2d>(reg, &[c::COLLIDER_2D], tags, after_write);
    // After the components beside it, so a scene gives the node those first.
    reg.register_component(
        Collider2d::NAME,
        ComponentDef {
            events: crate::vocabulary::hook::COLLIDER,
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Collider2d>(entity);
                remove_colliders(eng, entity);
                Ok(())
            }),
            ..ComponentDef::of::<Collider2d>(Some(after_write))
        },
    );
}

/// The components whose rows a `collider2d`'s build reads: its own, then
/// those beside it.
pub(crate) const PARAMS: &[&str] = &[
    c::COLLIDER_2D,
    <MeshCollider2d as Component>::NAME,
    <ConvexDecomposition2d as Component>::NAME,
];

/// Replace a node's collider from a `collider2d` table, read as a write of
/// the component reads it.
pub(crate) fn set_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let asked: Collider2d = crate::properties::asked_of(eng, params)?;
    eng.world_mut()
        .insert_one(entity, asked)
        .map_err(|_| anyhow!("node is dead"))?;
    after_write(eng, entity, Written::Changed)
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
