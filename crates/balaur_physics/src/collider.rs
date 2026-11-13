//! The `collider3d` component: every shape it can take, and the overlap
//! query that reads them back.

use crate::rapier3d::math::Vector;
use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::components::derived::{self, Component, Written};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId};

use crate::collider_shape::ColliderShape3d;
use crate::rapier3d::prelude::{
    ActiveCollisionTypes, ActiveHooks, CoefficientCombineRule, Collider, ColliderBuilder,
    ColliderHandle, Group, InteractionGroups, InteractionTestMode, MassProperties, RigidBodyHandle,
};
use crate::scalar::{self, Pose};
use crate::shared::beside::{self, Beside};

use crate::properties::{Axis3d, ContactPair, Decomposition3d, Fill, LayerTest};
use crate::vocabulary::{component as c, keys as k};
use balaur_core::layers::Layer;

use crate::vocabulary::words;
use crate::vocabulary::words::functions as f;
use crate::{PhysicsState3d, node_pose};

/// The node's 3D collision shape, chosen by `kind`. It belongs to the node's `body3d` or the nearest body above it; without one it is static geometry.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, Component)]
#[component(
    name = "collider3d",
    on_remove = collider3d_removed,
    events = crate::vocabulary::hook::COLLIDER,
    tags = [ balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
    after_write = after_write,
)]
pub struct Collider3d {
    /// The collision shape, and the dimensions or the source that kind reads
    pub shape: ColliderShape3d,
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
    pub offset: [f32; 3],
    /// How the shape is turned relative to the node, as `rotation_euler` turns one; Euler radians in the file
    #[property(group = "shape", unit = "degrees")]
    pub offset_rotation: [f32; 3],
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
    /// Which pairs of body kinds this collider is tested against; a sensor is tested against every kind whatever this says
    #[property(flags, default = vec![ContactPair::DynamicDynamic, ContactPair::DynamicKinematic, ContactPair::DynamicStatic], group = "filtering")]
    pub contact_pairs: Vec<ContactPair>,
    /// A platform bodies land on from the side one_way_axis names and pass through from the other
    #[property(group = "contacts")]
    pub one_way: bool,
    /// The side a one-way platform holds bodies on, in the collider's own axes: [0, 1, 0] lands them from above and lets them up through from below
    #[property(default = [0.0, 1.0, 0.0], group = "contacts")]
    pub one_way_axis: [f32; 3],
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
    pub surface_velocity: [f32; 3],
    /// A margin the solver treats as already touching; stops thin shapes tunnelling and jittering
    #[property(group = "contacts", min = 0.0)]
    pub collision_margin: f32,
    /// Mass per volume, so the shape's size sets its mass; 0 makes a collider that adds no mass to its body
    #[property(default = 1.0, group = "mass", min = 0.0)]
    pub density: f32,
    /// Mass in kilograms, overriding what density works out to; 0 keeps the density
    #[property(group = "mass", min = 0.0)]
    pub mass: f32,
    /// Where this collider's mass sits, in its own space; read with inertia, and both 0 keep the shape's own
    #[property(group = "mass")]
    pub center_of_mass: [f32; 3],
    /// This collider's resistance to spin about each axis; 0 with a center_of_mass takes the shape's own about that centre
    #[property(group = "mass")]
    pub inertia: [f32; 3],
    /// Turns the axes inertia is measured about, as `rotation_euler` does; read with a non-zero inertia. Euler radians in the file
    #[property(group = "mass", unit = "degrees")]
    pub inertia_rotation: [f32; 3],
    /// How hard a contact must be before on_contact_force is called
    #[property(group = "events", min = 0.0)]
    pub contact_force_threshold: f32,
}

/// How the node's `collider3d` turns its `mesh` into a shape: how a voxelized mesh or a decomposition's grid is filled, and what a triangle_mesh tidies first.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, Component)]
#[component(name = "mesh_collider3d")]
pub struct MeshCollider3d {
    /// Treat the triangle_mesh as a closed, outward-facing surface, which makes inside and outside meaningful
    pub oriented: bool,
    /// Whether voxelizing a mesh fills its inside or only its shell, for voxelized_mesh and a convex_decomposition's voxel grid
    #[property(default = Fill::Solid)]
    pub fill: Fill,
    /// When a solid fill floods a mesh, leave the cavities a closed surface walls off empty
    pub fill_cavities: bool,
    /// Merge vertices at exactly the same place when building a triangle_mesh
    #[property(default = true, group = "mesh cleanup")]
    pub merge_vertices: bool,
    /// Drop triangles that name one vertex twice; merges vertices too
    #[property(default = true, group = "mesh cleanup")]
    pub drop_degenerate_triangles: bool,
    /// Drop a triangle whose three vertices another one already names; merges vertices too
    #[property(default = true, group = "mesh cleanup")]
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

impl Beside for MeshCollider3d {
    type Main = Collider3d;

    fn reads(main: &Collider3d) -> bool {
        matches!(
            main.shape,
            ColliderShape3d::TriangleMesh(_)
                | ColliderShape3d::VoxelizedMesh(_)
                | ColliderShape3d::ConvexDecomposition(_)
        )
    }
}

/// How the node's `collider3d` cuts its mesh into convex pieces, for a convex_decomposition or a fit to one.
#[derive(Clone, Component)]
#[component(name = "convex_decomposition3d")]
pub struct ConvexDecomposition3d {
    /// How a convex_decomposition is cut: into convex hulls, or into voxel parts
    #[property(default = Decomposition3d::Vhacd)]
    pub method: Decomposition3d,
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

impl Beside for ConvexDecomposition3d {
    type Main = Collider3d;

    fn reads(main: &Collider3d) -> bool {
        matches!(
            main.shape,
            ColliderShape3d::ConvexDecomposition(_) | ColliderShape3d::Fit(_)
        )
    }
}

/// parry's own decomposition tuning, which a `convex_decomposition3d`'s rows start at.
fn vhacd() -> crate::rapier3d::parry::transformation::vhacd::VHACDParameters {
    crate::rapier3d::parry::transformation::vhacd::VHACDParameters::default()
}

/// What a node's collider is built from: the component, and the components
/// beside it as its shape's kind reads them.
#[derive(Clone)]
pub struct Asked3d {
    pub collider: Collider3d,
    pub mesh: MeshCollider3d,
    pub cut: ConvexDecomposition3d,
}

impl Asked3d {
    /// What `entity` asks for, `None` without a `collider3d`.
    pub fn of(world: &balaur_core::hecs::World, entity: Entity) -> Option<Self> {
        let collider = (*world.get::<&Collider3d>(entity).ok()?).clone();
        Some(Self {
            mesh: beside::held(world, entity, &collider),
            cut: beside::held(world, entity, &collider),
            collider,
        })
    }

    /// The three as one table, the way a snapshot keeps them.
    pub fn to_table(&self) -> toml::Value {
        let mut table = derived::to_table(&self.collider);
        for beside in [derived::to_table(&self.mesh), derived::to_table(&self.cut)] {
            if let (Some(rows), toml::Value::Table(more)) = (table.as_table_mut(), beside) {
                rows.extend(more);
            }
        }
        table
    }

    /// The three out of one table, as [`Self::to_table`] wrote them.
    pub(crate) fn from_table(params: &toml::Value) -> Result<Self> {
        Ok(Self {
            collider: derived::from_table(params)?,
            mesh: derived::from_table(params)?,
            cut: derived::from_table(params)?,
        })
    }
}

/// The collider a node asks for: its `shape`, built with the components
/// beside it, and every row that is not the shape.
pub(crate) fn collider_builder(eng: &Engine, asked: &Asked3d) -> Result<ColliderBuilder> {
    let builder = shape_builder(eng, &asked.collider.shape, &asked.mesh, &asked.cut)?;
    let material = crate::material::named(eng, &asked.collider.physics_material)?;
    Ok(with_mass_properties(
        with_rows(builder, &asked.collider, &material),
        &asked.collider,
    ))
}

/// A shape on its own, for a query: the value a `collider3d`'s `shape` holds,
/// built with the components beside a collider at their defaults.
pub(crate) fn query_shape(eng: &Engine, shape: &toml::Value) -> Result<ColliderBuilder> {
    let shape = <ColliderShape3d as derived::Property>::from_value(shape)?;
    shape_builder(
        eng,
        &shape,
        &MeshCollider3d::default(),
        &ConvexDecomposition3d::default(),
    )
}

/// The shape a kind and its rows make. `mesh` and `cut` are the components
/// beside the collider, which the kinds built from a mesh read.
pub(crate) fn shape_builder(
    eng: &Engine,
    shape: &ColliderShape3d,
    mesh: &MeshCollider3d,
    cut: &ConvexDecomposition3d,
) -> Result<ColliderBuilder> {
    use ColliderShape3d as S;
    let radius_of = |radius: f32| radius.max(0.01);
    let half = |size: [f32; 3], i: usize| (size[i] / 2.0).max(0.01);
    // A rounded shape is a shape plus a border radius, not nine more kinds.
    let border_of = |edge: f32| edge.max(0.0);
    // rapier measures these from the centre. `height` runs tip to tip, so a
    // capsule's segment is what the two caps leave of it.
    let half_height = |height: f32| height.max(0.01) / 2.0;
    Ok(match shape {
        S::Sphere(ball) => ColliderBuilder::ball(radius_of(ball.radius)),
        S::Box(cuboid) => {
            let (x, y, z) = (
                half(cuboid.size, 0),
                half(cuboid.size, 1),
                half(cuboid.size, 2),
            );
            match border_of(cuboid.edge_radius) {
                border if border > 0.0 => ColliderBuilder::round_cuboid(x, y, z, border),
                _ => ColliderBuilder::cuboid(x, y, z),
            }
        }
        // `height = 0` hands the capsule's length to its two ends.
        S::Capsule(capsule) if capsule.height <= 0.0 => ColliderBuilder::capsule_from_endpoints(
            scalar::v3a(capsule.first),
            scalar::v3a(capsule.second),
            radius_of(capsule.radius),
        ),
        S::Capsule(capsule) => {
            let radius = radius_of(capsule.radius);
            let segment = (half_height(capsule.height) - radius).max(0.0);
            match capsule.up_axis {
                Axis3d::X => ColliderBuilder::capsule_x(segment, radius),
                Axis3d::Z => ColliderBuilder::capsule_z(segment, radius),
                Axis3d::Y => ColliderBuilder::capsule_y(segment, radius),
            }
        }
        S::Cylinder(round) => match border_of(round.edge_radius) {
            border if border > 0.0 => ColliderBuilder::round_cylinder(
                half_height(round.height),
                radius_of(round.radius),
                border,
            ),
            _ => ColliderBuilder::cylinder(half_height(round.height), radius_of(round.radius)),
        },
        S::Cone(round) => match border_of(round.edge_radius) {
            border if border > 0.0 => ColliderBuilder::round_cone(
                half_height(round.height),
                radius_of(round.radius),
                border,
            ),
            _ => ColliderBuilder::cone(half_height(round.height), radius_of(round.radius)),
        },
        S::Triangle(corners) => {
            let [a, b, c] = [corners.first, corners.second, corners.third].map(scalar::v3a);
            match border_of(corners.edge_radius) {
                border if border > 0.0 => ColliderBuilder::round_triangle(a, b, c, border),
                _ => ColliderBuilder::triangle(a, b, c),
            }
        }
        S::Segment(ends) => {
            ColliderBuilder::segment(scalar::v3a(ends.first), scalar::v3a(ends.second))
        }
        // An infinite plane, for a floor that needs no size and no triangles.
        S::WorldBoundary(boundary) => {
            let n = scalar::v3a(boundary.normal);
            if n.length_squared() < 1.0e-12 {
                bail!("a world_boundary collider needs a non-zero `normal`");
            }
            ColliderBuilder::new(crate::rapier3d::prelude::SharedShape::halfspace(
                n.normalize(),
            ))
        }
        S::TriangleMesh(source) => crate::shapes::triangle_mesh(eng, source, mesh)?,
        S::ConvexHull(source) => {
            crate::shapes::convex_hull(eng, &source.mesh, border_of(source.edge_radius))?
        }
        S::ConvexMesh(source) => {
            crate::shapes::convex_mesh(eng, &source.mesh, border_of(source.edge_radius))?
        }
        S::ConvexDecomposition(source) => crate::shapes::decomposition(
            eng,
            &source.mesh,
            cut,
            mesh,
            border_of(source.edge_radius),
        )?,
        S::Polyline(line) => crate::shapes::polyline(eng, &line.mesh, line.edges)?,
        S::Fit(fitted) => crate::shapes::fit(eng, &fitted.mesh, fitted.fit, cut, mesh)?,
        S::VoxelizedPoints(cells) => {
            crate::shapes::voxelized_points(eng, &cells.mesh, cells.voxel_size)?
        }
        S::Voxels(grid) => crate::shapes::voxel_collider(eng, &grid.voxels, grid.voxel_size)?,
        S::VoxelizedMesh(cells) => {
            crate::shapes::voxelized_mesh_collider(eng, &cells.mesh, cells.voxel_size, mesh)?
        }
        S::Heightfield(field) => crate::shapes::heightfield_collider(
            eng,
            &field.heightfield,
            scalar::v3a(field.scale),
            field.fix_internal_edges,
        )?,
    })
}

/// `center_of_mass`, `inertia` and `inertia_rotation`, for a collider that
/// states any of them. The mass is `mass`, or what the density makes of the
/// shape; an `inertia` of 0 is the shape's own, about the stated centre.
fn with_mass_properties(builder: ColliderBuilder, asked: &Collider3d) -> ColliderBuilder {
    let com = asked.center_of_mass;
    let inertia = asked.inertia;
    if crate::body::is_default(&com) && crate::body::is_default(&inertia) {
        return builder;
    }
    let unit = builder.shape.mass_properties(1.0);
    let mass = match asked.mass {
        stated if stated > 0.0 => stated,
        _ => asked.density.max(0.0) * unit.mass(),
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
        let frame = crate::body::rotation_from_euler(asked.inertia_rotation);
        MassProperties::with_principal_inertia_frame(com, mass, scalar::v3a(inertia), frame)
    };
    builder.mass_properties(props)
}

/// What the contact hook needs of this collider, or `None` for one that
/// neither carries bodies through one way nor slides them along.
pub(crate) fn surface_of(asked: &Collider3d) -> Option<crate::events::Surface> {
    let one_way = asked.one_way.then(|| {
        let axis = scalar::v3a(asked.one_way_axis);
        let angle = asked.one_way_angle.max(0.0);
        (axis.try_normalize().unwrap_or(Vector::Y), angle)
    });
    let velocity = scalar::v3a(asked.surface_velocity);
    (one_way.is_some() || velocity != Vector::ZERO)
        .then_some(crate::events::Surface { one_way, velocity })
}

crate::shared::collider::functions!(state = PhysicsState3d, refit = crate::body::refit_mass);

/// Build again each collider and soft body naming a material file once the
/// asset cache has moved, so a material saved in the editor shows on every
/// body naming it, as a re-read tile map's cells do.
pub(crate) fn reread_materials(eng: &Engine) {
    let stale: Vec<(Entity, Option<Asked3d>)> = {
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let generation = balaur_core::assets::generation(eng);
        if state.materials_read == generation {
            return;
        }
        state.materials_read = generation;
        let colliders = state
            .collider_params
            .iter()
            .filter(|(_, asked)| crate::material::is_a_file(&asked.collider.physics_material))
            .map(|(entity, asked)| (*entity, Some(asked.clone())));
        let soft = state
            .soft_params
            .iter()
            .filter(|(_, asked)| {
                crate::material::is_a_file(&asked.body.physics_material)
                    || crate::material::is_a_file(&asked.body.soft_material)
                    || crate::shared::softbody::regions_name_a_file(&asked.body.regions)
            })
            .map(|(entity, _)| (*entity, None));
        colliders.chain(soft).collect()
    };
    for (entity, asked) in stale {
        let rebuilt = if let Some(asked) = asked {
            apply_collider(eng, entity, &asked)
        } else {
            let params = eng
                .resource::<PhysicsState3d>()
                .borrow()
                .soft_params
                .get(&entity)
                .cloned();
            params.map_or(Ok(()), |params| {
                crate::softbody::reread_softbody(eng, entity, &params)
            })
        };
        if let Err(why) = rebuilt {
            tracing::warn!("physics_material: {why:#}");
        }
    }
}

/// Build and insert the collider a node asks for, replacing any existing one
/// (attached to the entity's body when it has one).
pub(crate) fn apply_collider(eng: &Engine, entity: Entity, asked: &Asked3d) -> Result<()> {
    let builder = collider_builder(eng, asked)?;
    let offset = Pose::from_parts(
        scalar::v3a(asked.collider.offset),
        crate::body::rotation_from_euler(asked.collider.offset_rotation),
    );
    remove_colliders(eng, entity);
    add_collider_at(eng, entity, builder, offset)?;
    {
        let state = eng.resource::<PhysicsState3d>();
        state
            .borrow_mut()
            .collider_params
            .insert(entity, asked.clone());
    }
    if let Some(surface) = surface_of(&asked.collider) {
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
    balaur_core::events::hearers_may_have_changed(eng, entity);
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

/// What a node's live collider was built from: `None` when it has none.
pub(crate) fn authored_collider(eng: &Engine, entity: Entity) -> Option<Asked3d> {
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    state.colliders.get(&entity)?.first()?;
    state.collider_params.get(&entity).cloned()
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

/// What the node's `collider3d` asks for, with the rows of the components
/// beside it written in; `None` when it has none.
/// The collider the component asks for, built again.
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let Some(asked) = Asked3d::of(&eng.world(), entity) else {
        return Ok(());
    };
    apply_collider(eng, entity, &asked)
}

/// What a removed `Collider3d` leaves behind, taken with it.
fn collider3d_removed(eng: &Engine, entity: Entity) {
    remove_colliders(eng, entity);
}

/// `collider3d`: the asked-for struct on the node, the collider itself in
/// [`crate::PhysicsState3d`].
pub(crate) fn register_collider_component(reg: &mut Registry<'_>) {
    let tags: &'static [&'static str] = &[
        balaur_core::components::tag::DIM_3D,
        balaur_core::components::tag::PHYSICS,
    ];
    beside::register::<MeshCollider3d>(reg, &[c::COLLIDER_3D], tags, after_write);
    beside::register::<ConvexDecomposition3d>(reg, &[c::COLLIDER_3D], tags, after_write);
    // After the components beside it, so a scene gives the node those first.
    reg.add_component::<Collider3d>();
}

/// The components whose rows a `collider3d`'s build reads: its own, then
/// those beside it.
pub(crate) const PARAMS: &[&str] = &[
    c::COLLIDER_3D,
    <MeshCollider3d as Component>::NAME,
    <ConvexDecomposition3d as Component>::NAME,
];

/// Collider calls that are not about creating one: asking where it is.
pub(crate) fn install_collider_api(m: &mut Module<Engine>) {
    m.describe(&[]);
    m.function(words::AABB, |eng: &Engine, node: NodeId| {
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
pub(crate) fn install_voxel_api(m: &mut Module<Engine>) {
    m.describe(&[
        (f::SET_VOXEL, &[c::COLLIDER_3D], "", "Fill or empty one cell of a voxel collider: digging a hole, or building a wall, while the game runs."),
        (f::VOXEL, &[c::COLLIDER_3D], "", "Whether one cell of a voxel collider is filled."),
        (f::VOXEL_AT, &[c::COLLIDER_3D], "", "The cell a world position falls in, as three whole numbers."),
    ]);
    // Voxels are the one shape a game edits rather than replaces, so they get
    // calls of their own rather than going through `set_collider`.
    m.function(
        f::SET_VOXEL,
        |eng: &Engine, (node, x, y, z, filled): (NodeId, i32, i32, i32, bool)| {
            with_voxels(eng, node, |voxels| {
                voxels.set_voxel(scalar::cell(x, y, z), filled);
            })
        },
    );
    m.function(
        f::VOXEL,
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
        f::VOXEL_AT,
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

/// The tables a collider's readers answer.
const READER_RECORDS: &[balaur_script::RecordDoc] = &[
    (
        "ColliderMesh",
        "{ points: [Vec3], indices: [int] }",
        "A collider's shape as points and the three point indices of each triangle, one after the other.",
    ),
    (
        "Handles",
        "{ body: [int]?, colliders: [[int]] }",
        "The rapier handles behind a node, each an index and a generation; `body` is nil on a node with no body.",
    ),
    (
        "MassProperties",
        "{ mass: float, center_of_mass: Vec3, inertia: Vec3, inertia_rotation: Vec3 }",
        "What a collider adds to its body, in its own space: its mass and where it sits, its inertia about each principal axis, and how those axes are turned, in Euler radians as `rotation_euler` reads them.",
    ),
];

/// What a collider weighs, how much space it takes, where it is, and the
/// handles rapier knows it by.
///
/// Split from [`install_collider_api`] under `MAX_FN_LINES`.
pub(crate) fn install_collider_reader_api(m: &mut Module<Engine>) {
    m.records(READER_RECORDS);
    m.describe(&[
        (f::COLLIDER_MESH, &[c::COLLIDER_3D], "(node: node) -> ColliderMesh", "The collider's shape as points and triangles, including a voxel grid's, for drawing it or for spawning the pieces it broke into."),
        (f::COLLIDER_MASS, &[c::COLLIDER_3D], "", "What this collider weighs, density and size together."),
        (f::COLLIDER_VOLUME, &[c::COLLIDER_3D], "", "How much space the shape encloses."),
        (f::SWEPT_AABB, &[c::COLLIDER_3D], "", "The box the collider covers over the next fixed step, from where it is to where its body's velocity and forces carry it."),
        (f::HANDLES, &[c::COLLIDER_3D], "(node: node) -> Handles", "The rapier handles behind this node, its body and its colliders. For matching a log line against rapier's own output."),
        (f::COLLIDER_MASS_PROPERTIES, &[c::COLLIDER_3D], "(node: node) -> MassProperties", "What this collider adds to its body, in its own space."),
        (words::AABB, &[c::COLLIDER_3D], "", "The world-space box the collider currently occupies, as its two opposite corners."),
    ]);
    m.function(f::COLLIDER_MESH, |eng: &Engine, node: NodeId| {
        collider_mesh_value(eng, node)
    });
    m.function(f::COLLIDER_VOLUME, |eng: &Engine, node: NodeId| {
        with_first_collider(eng, node, |collider| Ok(collider.volume()))
    });
    m.function(f::SWEPT_AABB, |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let collider = &state.world.colliders[first_collider(&state, entity)?];
        // Rapier's own prediction, which its broad phase clamps to a body's
        // `speculative_distance`; unclamped here, so the box spans the whole step.
        let dt = balaur_core::fixed_dt();
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
    m.function(f::HANDLES, |eng: &Engine, node: NodeId| {
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
    m.function(f::COLLIDER_MASS, |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let handle = first_collider(&state, entity)?;
        Ok(state.world.colliders[handle].mass())
    });
    m.function(f::COLLIDER_MASS_PROPERTIES, |eng: &Engine, node: NodeId| {
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
crate::properties::collider_rows!(Collider3d);
