//! `softbody3d`: a deformable body of particles linked by elastic
//! constraints, solved beside the rigid bodies on the same fixed step.
//!
//! The component says how the particles are laid out (`kind`, and the
//! component beside it that holds that kind's shape: `soft_grid3d`,
//! `soft_rope3d` and the rest) and what they are made of (the material
//! rows). Rapier
//! builds the body once from that; after it, the solver owns the positions
//! and the node's drawn vertices come back from it every step through
//! [`balaur_geometry::mesh::SolvedMesh`].
//!
//! A soft body is world-space like a rigid one: the generators are given the
//! node's composed global pose, and the read-back divides it out again so a
//! body under a turned parent draws where it is simulated.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{self, Component, Written};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::NodeId;

use crate::PhysicsState3d;
use crate::properties::{
    Binding, ContactPair, EdgeSpring, EdgeTearResistance, Fill, Indices, Orientation, ParticlePair,
    SoftKind3d, SoftRegion, SoftSolver, TensionOnly,
};
use crate::rapier3d::prelude::{SoftBodyBuilder, SoftBodyHandle, SoftBodySolver};
use crate::scalar::{self, Real, Vector};
use crate::shared::beside::{self, Beside};
use crate::shared::softbody::{self as cap, Family};
use crate::soft_material::SoftMaterial;
use crate::vocabulary::{component as c, keys as k, words as w};
use balaur_core::layers::Layer;

mod api;
pub(crate) mod runtime;
mod wire;

pub(crate) use api::install_softbody_api;

/// A deformable 3D body: particles linked by elastic constraints, laid out by `kind` and made of what the material rows say. The node is drawn from the solver's positions.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, PartialEq, Component)]
#[component(name = "softbody3d", on_remove = softbody3d_removed, after_write = after_write)]
pub struct Softbody3d {
    /// How the body's particles and elements are laid out
    #[property(default = SoftKind3d::Box)]
    pub kind: SoftKind3d,
    /// Take part in the simulation; off leaves the body where it is with no contacts or constraints until it is on again, without a rebuild
    #[property(default = true)]
    pub enabled: bool,
    /// What the whole body weighs, spread over its particles
    #[property(default = 1.0, min = 0.0)]
    pub mass: f32,
    /// How much gravity pulls on the particles
    #[property(default = 1.0)]
    pub gravity_scale: f32,
    /// What the body is drawn in when its node has nothing of its own to deform, as a cloth or a rope has not
    #[property(color, default = cap::DEFAULT_COLOR)]
    pub color: [f32; 4],
    /// What the body is made of: its cells, springs, volume, yield and tearing; empty takes a soft_material at its defaults
    #[property(asset = "soft_material")]
    pub soft_material: String,
    /// What the surface is made of: friction, bounce and how each combines; empty takes a physics_material at its defaults
    #[property(asset = "physics_material")]
    pub physics_material: String,
    /// Geometry for a triangle_mesh or volumetric body, and the skin a body with cells carries when its soft_skin3d asks for one
    #[property(asset = "mesh", group = "shape")]
    pub mesh: String,
    /// Structural edges added between particles the layout left apart, each as long as its two particles stand at the start
    #[property(group = "shape")]
    pub seams: Vec<ParticlePair>,
    /// The particles held where they are, by index: a cloth hangs from these, and `softbody_particles` says how many there are to choose from
    #[property(group = "particles")]
    pub pinned_particles: Indices,
    /// Each particle's own mass, by index; empty spreads `mass` over them evenly
    #[property(group = "particles")]
    pub masses: Vec<f32>,
    /// Air friction on the particles
    #[property(group = "particles", min = 0.0)]
    pub linear_damping: f32,
    /// Let the body stop being simulated once it settles
    #[property(default = true, group = "particles")]
    pub can_sleep: bool,
    /// Meet the world at all; off, the body passes through everything and only its pins and ties hold it
    #[property(default = true, group = "collision")]
    pub collides: bool,
    /// Report contacts and push nothing: the body passes through what it touches
    #[property(group = "collision")]
    pub sensor: bool,
    /// Let the body's own surface collide with itself, which stops a cloth passing through its own fold
    #[property(group = "collision")]
    pub self_collision: bool,
    /// How thick the particles are; 0 takes what the layout works out
    #[property(group = "collision", min = 0.0)]
    pub particle_radius: f32,
    /// How the surface meets what is inside it: solid encloses matter and pushes bodies out, shell holds bodies in, auto is solid when the surface is closed
    #[property(default = Orientation::Auto, group = "collision")]
    pub orientation: Orientation,
    /// The layers this collider is on
    #[property(flags, default = vec![Layer(1)], group = "filtering")]
    pub collision_layer: Vec<Layer>,
    /// The layers it collides with; empty means every layer
    #[property(flags, group = "filtering")]
    pub collision_mask: Vec<Layer>,
    /// Layers for the solver alone: a pair can be detected but not resolved
    #[property(flags, default = vec![Layer(1)], group = "filtering")]
    pub solver_layer: Vec<Layer>,
    /// Which solver layers this one pushes against; empty means all of them
    #[property(flags, group = "filtering")]
    pub solver_mask: Vec<Layer>,
    /// Which pairs of body kinds this collider is tested against; a sensor watching kinematic platforms needs more than the default
    #[property(flags, default = vec![ContactPair::DynamicDynamic, ContactPair::DynamicKinematic, ContactPair::DynamicStatic], group = "filtering")]
    pub contact_pairs: Vec<ContactPair>,
    /// How hard a contact must be before on_contact_force is called
    #[property(group = "events", min = 0.0)]
    pub contact_force_threshold: f32,
    /// Parts of the body with a material of their own, each the particles it covers: `soft_material` for its edge spring and shape matching, empty keeping the body's; `stiffness_scale` on the cells wholly inside, `tear_resistance` on its edges and cells, and `pinned` to hold it still. Each is a `soft_frame` body joints may attach to
    #[property(group = "overrides")]
    pub regions: Vec<SoftRegion>,
    /// Edges with a spring of their own, its frequency in hertz and its damping ratio, instead of the edge or bend rows', each named by the two particles it joins
    #[property(group = "overrides")]
    pub edge_springs: Vec<EdgeSpring>,
    /// Which edges resist stretching only, so the body folds freely and never pushes itself open: none, all, or the tension_only_edges listed
    #[property(default = TensionOnly::None, group = "overrides")]
    pub tension_only: TensionOnly,
    /// The edges that resist stretching only when tension_only is listed, each named by the two particles it joins
    #[property(group = "overrides")]
    pub tension_only_edges: Vec<ParticlePair>,
    /// Edges that tear sooner or later than the rest, each named by the two particles it joins: below 1 is a perforation, above 1 a seam
    #[property(group = "tearing")]
    pub tear_resistance: Vec<EdgeTearResistance>,
    /// The most edges that may tear in one step, which paces a crack; 0 is no limit
    #[property(group = "tearing", min = 0)]
    pub max_tears_per_step: i64,
    /// The smallest piece, in elements, a tear may split off; 0 lets rapier choose
    #[property(group = "tearing", min = 0)]
    pub min_piece: i64,
    /// Which solver runs the elasticity: sequential constraints, or an implicit Euler step over the whole body
    #[property(default = SoftSolver::Constraints, group = "solver")]
    pub solver: SoftSolver,
    /// Extra iterations inside each substep, for the same
    #[property(default = 3, group = "solver", max = 64, min = 0)]
    pub solver_iterations: i64,
    /// Extra solver substeps for this body and everything it touches
    #[property(group = "solver", max = 64, min = 0)]
    pub solver_substeps: i64,
    /// Which body wins a contact: a higher one is never pushed by a lower one
    #[property(group = "solver", max = 127, min = -127)]
    pub dominance: i64,
}

/// A mesh the node's `softbody3d` meets the world through beside its own surface, deformed with the body: its triangles in 3D, the outline of its triangles in 2D.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_collision3d")]
pub struct SoftCollision3d {
    /// The mesh, in the node's space
    #[property(asset = "mesh")]
    pub collision_mesh: String,
    /// How the mesh follows the body: each vertex on the nearest particle, vertex i on particle i, or riding the cell that holds it, so a coarse cage of cells can carry a fine mesh
    pub collision_binding: Binding,
    /// Let the mesh collide with itself
    pub collision_self_contacts: bool,
}

impl Beside for SoftCollision3d {
    type Main = Softbody3d;

    fn reads(_: &Softbody3d) -> bool {
        true
    }
}

/// The `mesh` a `softbody3d` with cells carries as its skin: a box, a volumetric body or a custom one with cells, whose particles are not the mesh's own.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_skin3d")]
pub struct SoftSkin3d {
    /// Keep the mesh asset as the drawn surface and let the cells carry it, so a detail the cells cannot resolve survives
    pub skin: bool,
    /// Meet the world through the skin rather than the cells' boundary
    pub skin_collision: bool,
}

impl Beside for SoftSkin3d {
    type Main = Softbody3d;

    fn reads(main: &Softbody3d) -> bool {
        matches!(
            main.kind,
            SoftKind3d::Box | SoftKind3d::Volumetric | SoftKind3d::Custom
        )
    }
}

/// The size and the cells of a `softbody3d` laid out as a box or a cloth: a block of particles, or a sheet of them.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_grid3d")]
pub struct SoftGrid3d {
    /// Whole size along each axis: a box's block, or a cloth's sheet along x and z
    #[property(default = [1.0, 1.0, 1.0])]
    pub size: [f32; 3],
    /// How many cells along each axis, for box; a cloth reads the first two
    #[property(default = [4.0, 4.0, 4.0])]
    pub cells: [f32; 3],
}

impl Beside for SoftGrid3d {
    type Main = Softbody3d;

    fn reads(main: &Softbody3d) -> bool {
        matches!(main.kind, SoftKind3d::Box | SoftKind3d::Cloth)
    }
}

/// The size of a `softbody3d` laid out as a sphere: a refined icosahedron of particles.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_sphere3d")]
pub struct SoftSphere3d {
    /// Radius
    #[property(default = 0.5, min = 0.001)]
    pub radius: f32,
    /// How many times a sphere's icosahedron is refined; each level quadruples the triangles
    #[property(default = 2, max = 6, min = 0)]
    pub subdivisions: i64,
}

impl Beside for SoftSphere3d {
    type Main = Softbody3d;

    fn reads(main: &Softbody3d) -> bool {
        main.kind == SoftKind3d::Sphere
    }
}

/// The shape of a `softbody3d` laid out as a cloth_tube: a sleeve of cloth wrapped around a segment.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_cloth_tube3d")]
pub struct SoftClothTube3d {
    /// Radius at the tube's start
    #[property(default = 0.5, min = 0.001)]
    pub radius: f32,
    /// A cloth_tube's radius at its end; 0 takes radius
    #[property(min = 0.0)]
    pub end_radius: f32,
    /// The segment a cloth_tube is wrapped around, relative to the node
    #[property(default = [0.0, 1.0, 0.0])]
    pub axis: [f32; 3],
    /// Particles around the tube, then cells along it; the third is not read
    #[property(default = [4.0, 4.0, 4.0])]
    pub cells: [f32; 3],
}

impl Beside for SoftClothTube3d {
    type Main = Softbody3d;

    fn reads(main: &Softbody3d) -> bool {
        main.kind == SoftKind3d::ClothTube
    }
}

/// Where a `softbody3d` laid out as a rope runs, and how many particles it is made of.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_rope3d")]
pub struct SoftRope3d {
    /// How many particles a rope is made of
    #[property(default = cap::DEFAULT_PARTICLES as i64, min = 2)]
    pub particle_count: i64,
    /// Where a rope starts, relative to the node
    #[property(name = "a")]
    pub first: [f32; 3],
    /// Where a rope ends, relative to the node
    #[property(name = "b", default = [0.0, -1.0, 0.0])]
    pub second: [f32; 3],
}

impl Beside for SoftRope3d {
    type Main = Softbody3d;

    fn reads(main: &Softbody3d) -> bool {
        main.kind == SoftKind3d::Rope
    }
}

/// How a volumetric `softbody3d` fills its mesh with tetrahedra.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_volumetric3d")]
pub struct SoftVolumetric3d {
    /// What a volumetric body's cells cover: the mesh's whole inside, which needs a closed mesh, or a crust along its surface alone, which does not
    #[property(default = Fill::Solid)]
    pub fill: Fill,
    /// How big one tetrahedron is when a volumetric body fills a mesh; smaller is finer, slower and stiffer to tear
    #[property(default = 0.25, min = 0.001)]
    pub cell_size: f32,
    /// How many times a solid fill's cells along the mesh's surface are halved, finer than cell_size
    #[property(max = 4, min = 0)]
    pub boundary_subdivisions: i64,
    /// How many passes pull a solid fill's blocky boundary towards the mesh; 0 leaves it on the cell lattice
    #[property(min = 0)]
    pub smoothing: i64,
    /// How close to the mesh smoothing may pull the boundary, as a fraction of the cell size
    #[property(default = 0.15, min = 0.0)]
    pub smoothing_guard: f32,
}

impl Beside for SoftVolumetric3d {
    type Main = Softbody3d;

    fn reads(main: &Softbody3d) -> bool {
        main.kind == SoftKind3d::Volumetric
    }
}

/// The particles and elements of a custom `softbody3d`, as the author lists them.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_custom3d")]
pub struct SoftCustom3d {
    /// A custom body's particles, relative to the node
    pub points: Vec<[f32; 3]>,
    /// A custom body's tetrahedra, four particle indices each, positively oriented
    pub cell_indices: Vec<Vec<i64>>,
    /// A custom body's structural edges, each a pair of particle indices; empty takes the cells' edges
    pub edge_indices: Vec<Vec<i64>>,
    /// A custom body's bending edges, each a pair of particle indices
    pub bend_edge_indices: Vec<Vec<i64>>,
    /// A custom body's bending hinges between two triangles: the shared edge's two particles, then the two opposite ones
    pub dihedral_indices: Vec<Vec<i64>>,
    /// A custom body's outward triangles, three particle indices each: what holds its volume and what it collides through; empty takes the cells' boundary
    pub surface_indices: Vec<Vec<i64>>,
    /// A custom body's segments it collides through when it has no surface, as a rope does, each a pair of particle indices
    pub wire_indices: Vec<Vec<i64>>,
}

impl Beside for SoftCustom3d {
    type Main = Softbody3d;

    fn reads(main: &Softbody3d) -> bool {
        main.kind == SoftKind3d::Custom
    }
}

/// The components whose rows a `softbody3d`'s build reads: its own, then
/// those beside it.
pub(crate) const PARAMS: &[&str] = &[
    c::SOFTBODY_3D,
    <SoftGrid3d as Component>::NAME,
    <SoftSphere3d as Component>::NAME,
    <SoftClothTube3d as Component>::NAME,
    <SoftRope3d as Component>::NAME,
    <SoftVolumetric3d as Component>::NAME,
    <SoftCustom3d as Component>::NAME,
    <SoftCollision3d as Component>::NAME,
    <SoftSkin3d as Component>::NAME,
];

/// What a node's soft body is built from: the component, and the
/// components beside it as its kind reads them.
#[derive(Clone, PartialEq)]
pub struct SoftAsked3d {
    pub body: Softbody3d,
    pub grid: SoftGrid3d,
    pub sphere: SoftSphere3d,
    pub tube: SoftClothTube3d,
    pub rope: SoftRope3d,
    pub volumetric: SoftVolumetric3d,
    pub custom: SoftCustom3d,
    pub collision: SoftCollision3d,
    pub skin: SoftSkin3d,
}

impl SoftAsked3d {
    /// What `entity` asks for, `None` without a `softbody3d`.
    pub fn of(world: &balaur_core::hecs::World, entity: Entity) -> Option<Self> {
        let body = (*world.get::<&Softbody3d>(entity).ok()?).clone();
        Some(Self::with_body(world, entity, body))
    }

    /// `body` with the components beside it on `entity`.
    pub fn with_body(world: &balaur_core::hecs::World, entity: Entity, body: Softbody3d) -> Self {
        Self {
            grid: beside::held(world, entity, &body),
            sphere: beside::held(world, entity, &body),
            tube: beside::held(world, entity, &body),
            rope: beside::held(world, entity, &body),
            volumetric: beside::held(world, entity, &body),
            custom: beside::held(world, entity, &body),
            collision: beside::held(world, entity, &body),
            skin: beside::held(world, entity, &body),
            body,
        }
    }

    /// All of it as one table, the way a snapshot keeps it: a row two
    /// components share is the one the kind reads.
    pub(crate) fn to_table(&self) -> toml::Value {
        let body = &self.body;
        let mut table = derived::to_table(body);
        beside::table_in(&mut table, &self.grid, SoftGrid3d::reads(body));
        beside::table_in(&mut table, &self.sphere, SoftSphere3d::reads(body));
        beside::table_in(&mut table, &self.tube, SoftClothTube3d::reads(body));
        beside::table_in(&mut table, &self.rope, SoftRope3d::reads(body));
        beside::table_in(&mut table, &self.volumetric, SoftVolumetric3d::reads(body));
        beside::table_in(&mut table, &self.custom, SoftCustom3d::reads(body));
        beside::table_in(&mut table, &self.collision, SoftCollision3d::reads(body));
        beside::table_in(&mut table, &self.skin, SoftSkin3d::reads(body));
        table
    }

    /// All of it out of one table, as [`Self::to_table`] wrote it.
    pub(crate) fn from_table(params: &toml::Value) -> Result<Self> {
        let body: Softbody3d = derived::from_table(params)?;
        Ok(Self {
            grid: beside::from_table(params, &body)?,
            sphere: beside::from_table(params, &body)?,
            tube: beside::from_table(params, &body)?,
            rope: beside::from_table(params, &body)?,
            volumetric: beside::from_table(params, &body)?,
            custom: beside::from_table(params, &body)?,
            collision: beside::from_table(params, &body)?,
            skin: beside::from_table(params, &body)?,
            body,
        })
    }
}

crate::shared::softbody::material!(
    rapier = rapier3d,
    material = read_material,
    cell_model = read_cell_model
);

cap::regions!(rapier = rapier3d, add = add_regions);
cap::only_in_place!(only_in_place, SoftAsked3d);
cap::collision_mesh!(
    rapier = rapier3d,
    body = Softbody3d,
    collision = SoftCollision3d,
    bind = bind_collision_mesh,
    shape = |vertices, indices| {
        use crate::rapier3d::geometry::{SharedShape, TriMeshFlags};
        SharedShape::trimesh_with_flags(vertices, indices, TriMeshFlags::DEFORMABLE)
            .map_err(|why| anyhow!("`{}`: {why}", k::COLLISION_MESH))
    }
);

cap::surface!(
    rapier = rapier3d,
    body = Softbody3d,
    groups = crate::collider::groups_of,
    pairs = crate::collider::collision_types_of
);

/// Which solver runs the elasticity.
fn solver_of(solver: SoftSolver) -> SoftBodySolver {
    match solver {
        SoftSolver::Fem => SoftBodySolver::Fem,
        SoftSolver::Constraints => SoftBodySolver::Constraints,
    }
}

cap::patch_in_place!(
    patch_body,
    crate::rapier3d::pipeline::PhysicsWorld,
    SoftBodyHandle,
    Softbody3d,
    read_material,
    solver_of
);

/// The rows every layout shares, applied after the generator has laid the
/// particles out.
fn with_settings(
    mut builder: SoftBodyBuilder,
    asked: &SoftAsked3d,
    materials: &cap::Materials,
) -> Result<SoftBodyBuilder> {
    let body = &asked.body;
    let made = &*materials.body;
    let seams = cap::seams(&body.seams, builder.positions.len())?;
    if !seams.is_empty() {
        builder = builder.add_edges(seams);
    }
    builder = builder
        .material(read_material(made, body.max_tears_per_step, body.min_piece))
        .cell_model(read_cell_model(made))
        .volume_preservation(made.volume_preservation)
        .volume_factor(made.volume_factor)
        .self_contacts(body.self_collision)
        .linear_damping(body.linear_damping)
        .gravity_scale(body.gravity_scale)
        .additional_solver_iterations(cap::iterations(body.solver_substeps))
        .additional_pgs_iterations(cap::iterations(body.solver_iterations))
        .can_sleep(body.can_sleep)
        .solver(solver_of(body.solver))
        .skin_collision(asked.skin.skin_collision)
        .surface_collider(surface_collider(body, &materials.surface));
    if body.mass > 0.0 {
        builder = builder.mass(body.mass);
    }
    if body.particle_radius > 0.0 {
        builder = builder.particle_radius(body.particle_radius);
    }
    if let Some(oriented) = cap::oriented(body.orientation) {
        builder = builder.oriented(oriented);
    }
    // After the generator, which picks its own for a mesh or a polyline.
    if let Some(matching) = made.shape_matching {
        builder = builder.shape_matching(matching);
    }
    if body.tension_only == TensionOnly::All {
        builder = builder.tension_only();
    }
    builder = builder.pinned_particles(body.pinned_particles.0.clone());
    let settings = crate::rapier3d::prelude::SoftBodyParticleSettings {
        dominance_group: body.dominance.clamp(-127, 127) as i8,
        ..builder.particle_settings
    };
    builder = builder.particle_settings(settings);
    let ask = cap::EdgeAsk {
        masses: &body.masses,
        tear_resistance: &body.tear_resistance,
        edge_springs: &body.edge_springs,
        tension_only: body.tension_only,
        tension_only_edges: &body.tension_only_edges,
    };
    let rows = cap::edge_rows(
        &ask,
        builder.positions.len(),
        &builder.edges,
        &builder.bend_edges,
    )?;
    // After `mass`, which spreads one total over the particles evenly.
    if !rows.masses.is_empty() {
        builder = builder.masses(rows.masses.clone());
    }
    if !rows.tear.is_empty() {
        builder = builder.edge_tear_resistance(rows.tear.iter().map(|(i, r)| (*i, *r)));
    }
    for (edge, hz, damping) in rows.springs {
        let spring = crate::rapier3d::prelude::SpringCoefficients::new(hz, damping);
        builder.edge_softness.push((edge, spring));
    }
    builder.tension_only_edges.extend(rows.tension_only);
    if !body.collides {
        builder = builder.no_surface_collider();
    }
    Ok(builder)
}

/// The mesh a layout is built out of, in world space.
fn source_mesh(
    eng: &Engine,
    reference: &str,
    pose: scalar::Pose,
) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
    if reference.is_empty() {
        return Err(anyhow!("this soft-body kind needs a `mesh` asset"));
    }
    mesh_in_world(eng, reference, pose)
}

/// The `collision_mesh`, in world space, when there is one.
fn collision_mesh(
    eng: &Engine,
    collision: &SoftCollision3d,
    pose: scalar::Pose,
) -> Result<Option<cap::Mesh<Vector>>> {
    if collision.collision_mesh.is_empty() {
        return Ok(None);
    }
    mesh_in_world(eng, &collision.collision_mesh, pose).map(Some)
}

fn mesh_in_world(
    eng: &Engine,
    reference: &str,
    pose: scalar::Pose,
) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
    let definition =
        balaur_core::assets::load_typed::<balaur_geometry::mesh::MeshData>(eng, reference)?;
    let mesh = balaur_geometry::mesh::load_from(eng, &definition)?;
    let points = mesh
        .positions
        .iter()
        .map(|p| pose * scalar::v3a(*p))
        .collect();
    Ok((points, mesh.indices.clone()))
}

/// What to change for fewer particles, by what a kind reads.
fn fewer(kind: SoftKind3d) -> cap::Fewer {
    match kind {
        SoftKind3d::Volumetric => cap::Fewer::CellSize,
        SoftKind3d::Rope => cap::Fewer::ParticleCount,
        SoftKind3d::Sphere => cap::Fewer::Subdivisions,
        SoftKind3d::TriangleMesh => cap::Fewer::MeshPoints,
        SoftKind3d::Custom => cap::Fewer::Points,
        SoftKind3d::Box | SoftKind3d::Cloth | SoftKind3d::ClothTube => cap::Fewer::Cells,
    }
}

/// Lay the particles out the way `kind` asks, in world space around `pose`.
fn build_layout(
    eng: &Engine,
    asked: &SoftAsked3d,
    materials: &cap::Materials,
    pose: scalar::Pose,
) -> Result<SoftBodyBuilder> {
    let kind = asked.body.kind;
    let at = pose.translation;
    let local = |point: [f32; 3]| pose * scalar::v3a(point);
    let cells = if kind == SoftKind3d::ClothTube {
        asked.tube.cells
    } else {
        asked.grid.cells
    };
    let axis = |i: usize| cap::particles_along(cells[i]);
    let ring = |around: f32| f64::from(around.max(cap::MIN_RING_PARTICLES)).floor();
    let rope = || cap::particle_count(asked.rope.particle_count, cap::MIN_CHAIN_PARTICLES);
    cap::refuse_past_cap(
        match kind {
            SoftKind3d::Box => axis(0) * axis(1) * axis(2),
            SoftKind3d::Cloth => axis(0) * axis(1),
            SoftKind3d::ClothTube => ring(cells[0]) * axis(1),
            SoftKind3d::Rope => rope(),
            _ => 0.0,
        },
        fewer(kind),
    )?;
    let builder = match kind {
        // Rapier counts the particles along an axis; the schema counts the
        // cells between them, which is the number an author means.
        SoftKind3d::Box => SoftBodyBuilder::cuboid(
            at,
            scalar::v3a(asked.grid.size) / 2.0,
            axis(0) as usize,
            axis(1) as usize,
            axis(2) as usize,
        ),
        SoftKind3d::Sphere => SoftBodyBuilder::sphere(
            at,
            asked.sphere.radius,
            asked.sphere.subdivisions.clamp(0, 6) as usize,
        ),
        SoftKind3d::Cloth => cloth(
            &asked.grid,
            &materials.body,
            pose,
            axis(0) as usize,
            axis(1) as usize,
        ),
        SoftKind3d::ClothTube => {
            let tube = &asked.tube;
            let end = if tube.end_radius > 0.0 {
                tube.end_radius
            } else {
                tube.radius
            };
            SoftBodyBuilder::cloth_tube(
                at,
                pose.rotation * scalar::v3a(tube.axis),
                tube.radius,
                end,
                ring(cells[0]) as usize,
                axis(1) as usize,
            )
        }
        SoftKind3d::Rope => SoftBodyBuilder::rope(
            local(asked.rope.first),
            local(asked.rope.second),
            rope() as usize,
        ),
        // The approximate tetrahedrization: the mesh is covered with cells of
        // `cell_size` and the body is those cells.
        SoftKind3d::Volumetric => {
            let (points, indices) = source_mesh(eng, &asked.body.mesh, pose)?;
            let size = asked.volumetric.cell_size;
            cap::refuse_past_cap(cap::grid_particles(&extents(&points), size), fewer(kind))?;
            SoftBodyBuilder::volumetric_with(
                &points,
                &indices,
                &volume_parameters(&asked.volumetric, size),
            )
            .ok_or_else(|| {
                anyhow!("that mesh encloses nothing at a cell size of {size}: it has to be closed, and big enough to hold a cell")
            })?
        }
        SoftKind3d::TriangleMesh => {
            let (points, indices) = source_mesh(eng, &asked.body.mesh, pose)?;
            SoftBodyBuilder::trimesh(points, indices)
                .ok_or_else(|| anyhow!("that mesh has no triangles to make a soft surface of"))?
        }
        SoftKind3d::Custom => custom(&asked.custom, pose)?,
    };
    cap::refuse_past_cap(builder.positions.len() as f64, fewer(kind))?;
    let builder = skinned(eng, asked, pose, builder)?;
    with_settings(builder, asked, materials)
}

/// A sheet spanned across x and z around the node, `nu` by `nv` particles,
/// with springs of its own along the warp, the weft and the shear when any
/// woven row is set.
fn cloth(
    grid: &SoftGrid3d,
    made: &SoftMaterial,
    pose: scalar::Pose,
    nu: usize,
    nv: usize,
) -> SoftBodyBuilder {
    let at = pose.translation;
    let size = grid.size;
    // The sheet is spanned from a corner, so the node's own position
    // is its middle like every other layout's.
    let du = pose.rotation * scalar::v3(size[0], 0.0, 0.0);
    let dv = pose.rotation * scalar::v3(0.0, 0.0, size[2]);
    let origin = at - (du + dv) * 0.5;
    let (step_u, step_v) = (du / (nu.max(2) - 1) as Real, dv / (nv.max(2) - 1) as Real);
    let woven = [made.warp, made.weft, made.shear];
    let mut built = if woven.iter().any(|s| s.hz > 0.0 || s.damping > 0.0) {
        let spring = |i: usize| {
            let or = |own: f32, fallback: f32| if own > 0.0 { own } else { fallback };
            crate::rapier3d::prelude::SpringCoefficients::new(
                or(woven[i].hz, made.edge.hz),
                or(woven[i].damping, made.edge.damping),
            )
        };
        SoftBodyBuilder::cloth_anisotropic(
            origin,
            step_u,
            step_v,
            nu,
            nv,
            spring(0),
            spring(1),
            spring(2),
        )
    } else {
        SoftBodyBuilder::cloth(origin, step_u, step_v, nu, nv)
    };
    // Spanned +x then +z, rapier winds the sheet's front underneath
    // it (`du x dv` is -y); a flat cloth is looked at from above.
    for triangle in &mut built.surface {
        triangle.swap(1, 2);
    }
    built
}

/// The most times `boundary_subdivisions` halves a cell: each halving
/// quadruples the cells along the surface.
const MAX_SUBDIVISIONS: i64 = 4;

/// How a volumetric body fills its mesh.
fn volume_parameters(
    fill: &SoftVolumetric3d,
    size: Real,
) -> crate::rapier3d::parry::transformation::VolumeMeshParameters {
    use crate::rapier3d::parry::transformation::{MeshEnclosure, VolumeMeshParameters};
    let mut out = VolumeMeshParameters::new(size);
    out.enclosure = match fill.fill {
        Fill::Surface => MeshEnclosure::Crust,
        Fill::Solid => MeshEnclosure::Cover,
    };
    out.cover_smoothing = u32::try_from(fill.smoothing.max(0)).unwrap_or(u32::MAX);
    out.cover_guard = fill.smoothing_guard.max(0.0);
    out.cover_subdivisions = fill.boundary_subdivisions.clamp(0, MAX_SUBDIVISIONS) as u32;
    out
}

/// A body of the particles and elements the author listed.
fn custom(listed: &SoftCustom3d, pose: scalar::Pose) -> Result<SoftBodyBuilder> {
    let points: Vec<Vector> = listed
        .points
        .iter()
        .map(|p| pose * scalar::v3a(*p))
        .collect();
    let n = points.len();
    let edges = cap::tuples::<2>(&listed.edge_indices, k::EDGE_INDICES, n)?;
    let bend = cap::tuples::<2>(&listed.bend_edge_indices, k::BEND_EDGE_INDICES, n)?;
    let surface = cap::tuples::<3>(&listed.surface_indices, k::SURFACE_INDICES, n)?;
    let wire = cap::tuples::<2>(&listed.wire_indices, k::WIRE_INDICES, n)?;
    let cells = cap::tuples::<4>(&listed.cell_indices, k::CELL_INDICES, n)?;
    let dihedrals = cap::tuples::<4>(&listed.dihedral_indices, k::DIHEDRAL_INDICES, n)?;
    if n == 0 || (edges.is_empty() && cells.is_empty() && surface.is_empty() && wire.is_empty()) {
        return Err(anyhow!(
            "a custom soft body needs `points` and something to join them: cell_indices, edge_indices, surface_indices or wire_indices"
        ));
    }
    let mut builder = SoftBodyBuilder::new(points)
        .bend_edges(bend)
        .surface(surface)
        .wire(wire)
        .dihedrals(dihedrals)
        .cells(cells);
    if !edges.is_empty() {
        builder = builder.edges(edges);
    }
    Ok(builder)
}

/// The `mesh` asset held by the cells as the body's skin, when its kind
/// reads a `soft_skin3d`, that asks for one, and the layout made cells.
fn skinned(
    eng: &Engine,
    asked: &SoftAsked3d,
    pose: scalar::Pose,
    builder: SoftBodyBuilder,
) -> Result<SoftBodyBuilder> {
    let body = &asked.body;
    if !SoftSkin3d::reads(body)
        || !asked.skin.skin
        || body.mesh.is_empty()
        || builder.cells.is_empty()
    {
        return Ok(builder);
    }
    let (points, indices) = source_mesh(eng, &body.mesh, pose)?;
    Ok(builder.skin(points, indices))
}

/// How wide the points spread along each axis.
fn extents(points: &[Vector]) -> [f32; 3] {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for point in points {
        for (axis, value) in scalar::a3(*point).into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    [0, 1, 2].map(|axis| (high[axis] - low[axis]).max(0.0))
}

cap::stamp_colliders!(
    stamp_colliders,
    crate::rapier3d::pipeline::PhysicsWorld,
    SoftBodyHandle
);

/// Build the node's soft body, replacing whatever it had.
pub(crate) fn apply_softbody(eng: &Engine, entity: Entity, asked: &SoftAsked3d) -> Result<()> {
    let materials =
        cap::Materials::of(eng, &asked.body.soft_material, &asked.body.physics_material)?;
    if patched(eng, entity, asked, &materials) {
        return Ok(());
    }
    build_softbody(eng, entity, asked, &materials)
}

/// A body after a material file moved: in place, unless a region names one,
/// a region being made with the body.
pub(crate) fn reread_softbody(eng: &Engine, entity: Entity, asked: &SoftAsked3d) -> Result<()> {
    if !cap::regions_name_a_file(&asked.body.regions) {
        return apply_softbody(eng, entity, asked);
    }
    let materials =
        cap::Materials::of(eng, &asked.body.soft_material, &asked.body.physics_material)?;
    build_softbody(eng, entity, asked, &materials)
}

/// Build the node's soft body from rest, replacing whatever it had.
pub(super) fn build_softbody(
    eng: &Engine,
    entity: Entity,
    asked: &SoftAsked3d,
    materials: &cap::Materials,
) -> Result<()> {
    let pose = crate::node_pose(eng, entity)?;
    let builder =
        build_layout(eng, asked, materials, pose)?.user_data(u128::from(entity.to_bits().get()));
    let collision = collision_mesh(eng, &asked.collision, pose)?;
    remove_softbody(eng, entity);
    let added = {
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let handle = state.world.soft_bodies.insert(
            builder,
            &mut state.world.bodies,
            &mut state.world.colliders,
        );
        state.soft_bodies.insert(entity, handle);
        state.soft_params.insert(entity, asked.clone());
        let added =
            add_regions(eng, &mut state.world, handle, &asked.body.regions).and_then(|()| {
                bind_collision_mesh(
                    &mut state.world,
                    handle,
                    &asked.body,
                    &asked.collision,
                    collision,
                    materials,
                )
            });
        stamp_colliders(&mut state.world, handle, entity);
        balaur_core::events::hearers_may_have_changed(eng, entity);
        added
    };
    if let Err(why) = added {
        remove_softbody(eng, entity);
        return Err(why);
    }
    // The node draws from the solver, so it has geometry to draw before the
    // first step rather than a frame of nothing.
    write_solved_mesh(eng, entity);
    Ok(())
}

/// Take a write the live body can take as it stands; `false` when it has to
/// be built again.
fn patched(eng: &Engine, entity: Entity, asked: &SoftAsked3d, materials: &cap::Materials) -> bool {
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let (Some(built), Some(&handle)) = (
        state.soft_params.get(&entity),
        state.soft_bodies.get(&entity),
    ) else {
        return false;
    };
    if !only_in_place(built, asked)
        || materials.changes_shape_matching(eng, &built.body.soft_material)
        || state.world.soft_bodies.get(handle).is_none()
    {
        return false;
    }
    patch_body(&mut state.world, handle, &asked.body, materials);
    state.soft_params.insert(entity, asked.clone());
    true
}

pub(crate) fn remove_softbody(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    state.soft_params.shift_remove(&entity);
    let Some(handle) = state.soft_bodies.shift_remove(&entity) else {
        return;
    };
    let world = &mut state.world;
    for piece in world.soft_bodies.family(handle) {
        world.soft_bodies.remove(
            piece,
            &mut world.islands,
            &mut world.bodies,
            &mut world.colliders,
            &mut world.impulse_joints,
            &mut world.multibody_joints,
        );
    }
}

/// Hand this step's geometry to whatever draws the node.
///
/// The body's collision mesh rather than its particles: that is the surface
/// it presents to the world, and for a skinned body it is the authored mesh
/// the cells carry rather than the coarse cover around it.
///
/// In the node's own space: the renderer puts the node's transform back on
/// top, and a soft body under a turned parent has to draw where it is
/// simulated rather than twice-rotated.
pub(crate) fn write_solved_mesh(eng: &Engine, entity: Entity) {
    let Ok(pose) = crate::node_pose(eng, entity) else {
        return;
    };
    let inverse = pose.inverse();
    let (positions, indices, color) = {
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let Some(&handle) = state.soft_bodies.get(&entity) else {
            return;
        };
        let color = state
            .soft_params
            .get(&entity)
            .map_or(cap::DEFAULT_COLOR, |asked| asked.body.color);
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut indices: Vec<[u32; 3]> = Vec::new();
        // The body and whatever tore off it, drawn as one mesh.
        for piece in state.world.soft_bodies.family(handle) {
            let Some(body) = state.world.soft_bodies.get(piece) else {
                continue;
            };
            let offset = positions.len() as u32;
            let shifted = |t: &[u32; 3]| t.map(|i| i + offset);
            match body.collision_mesh() {
                Some(mesh) if mesh.is_wire() => {
                    let points: Vec<[f32; 3]> = mesh
                        .vertex_positions(body)
                        .map(|p| scalar::a3(inverse * p))
                        .collect();
                    let segments: Vec<[u32; 2]> =
                        mesh.indices().iter().map(|t| [t[0], t[1]]).collect();
                    let radius = body.particle_radius();
                    let (rings, faces) = wire::tube(&points, &segments, radius);
                    positions.extend(rings);
                    indices.extend(faces.iter().map(shifted));
                }
                Some(mesh) => {
                    positions.extend(mesh.vertex_positions(body).map(|p| scalar::a3(inverse * p)));
                    indices.extend(mesh.indices().iter().map(shifted));
                }
                None => {
                    // A body with no collider still draws: its boundary is what a
                    // generator laid out, and the particles are its vertices.
                    positions.extend(body.particle_positions().map(|p| scalar::a3(inverse * p)));
                    indices.extend(body.boundary().iter().map(shifted));
                }
            }
        }
        if positions.is_empty() {
            return;
        }
        (positions, indices, color)
    };
    if let Ok(mut solved) = eng
        .world()
        .get::<&mut balaur_geometry::mesh::SolvedMesh>(entity)
    {
        solved.update(positions, indices);
        solved.color = color;
        return;
    }
    let mut solved = balaur_geometry::mesh::SolvedMesh::default();
    solved.update(positions, indices);
    solved.color = color;
    let _ = eng.insert_one(entity, solved);
}

/// Hand every soft body's positions over, which is what the step does once
/// rapier has finished with them.
pub(crate) fn write_every_solved_mesh(eng: &Engine) {
    let entities: Vec<Entity> = {
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        // A tear makes pieces with colliders of their own, which need the
        // node as much as the body they came off.
        for (&entity, &handle) in &state.soft_bodies {
            stamp_colliders(&mut state.world, handle, entity);
        }
        state.soft_bodies.keys().copied().collect()
    };
    for entity in entities {
        write_solved_mesh(eng, entity);
    }
}

/// The soft body the component asks for, patched in place or built again.
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let asked = SoftAsked3d::of(&eng.world(), entity);
    let Some(asked) = asked else {
        return Ok(());
    };
    apply_softbody(eng, entity, &asked)
}

/// What a removed `Softbody3d` leaves behind, taken with it.
fn softbody3d_removed(eng: &Engine, entity: Entity) {
    remove_softbody(eng, entity);
    let _ = eng.remove_one::<balaur_geometry::mesh::SolvedMesh>(entity);
}

/// `softbody3d`: what was asked, held on the node; the body and the table it
/// was built from are in [`crate::PhysicsState3d`].
pub(crate) fn register_softbody_component(reg: &mut Registry<'_>) {
    let tags: &'static [&'static str] = &[
        balaur_core::components::tag::DIM_3D,
        balaur_core::components::tag::PHYSICS,
    ];
    let main: &'static [&'static str] = &[c::SOFTBODY_3D];
    beside::register::<SoftGrid3d>(reg, main, tags, after_write);
    beside::register::<SoftSphere3d>(reg, main, tags, after_write);
    beside::register::<SoftClothTube3d>(reg, main, tags, after_write);
    beside::register::<SoftRope3d>(reg, main, tags, after_write);
    beside::register::<SoftVolumetric3d>(reg, main, tags, after_write);
    beside::register::<SoftCustom3d>(reg, main, tags, after_write);
    beside::register::<SoftCollision3d>(reg, main, tags, after_write);
    beside::register::<SoftSkin3d>(reg, main, tags, after_write);
    // After the components beside it, so a scene gives the node those first.
    reg.register_component(
        Softbody3d::NAME,
        ComponentDef {
            events: crate::vocabulary::hook::SOFT_BODY,
            warnings: Some(Box::new(softbody_warnings)),
            // It draws the node, so the tree's Rendering filter finds it too.
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::PHYSICS,
                balaur_core::components::tag::RENDER,
            ],
            ..ComponentDef::of::<Softbody3d>()
        },
    );
}

/// What is off about a soft body that built: a particle radius worked out so
/// large that the body hovers.
fn softbody_warnings(eng: &Engine, entity: Entity) -> Vec<balaur_core::warnings::Warning> {
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let authored = state
        .soft_params
        .get(&entity)
        .map_or(0.0, |asked| asked.body.particle_radius);
    let body = state
        .soft_bodies
        .get(&entity)
        .and_then(|&handle| state.world.soft_bodies.get(handle));
    let Some(body) = body.filter(|_| authored <= 0.0) else {
        return Vec::new();
    };
    let points: Vec<Vector> = body.particle_positions().collect();
    let widest = extents(&points).into_iter().fold(0.0, f32::max);
    cap::hovering(body.particle_radius(), widest)
        .into_iter()
        .collect()
}

runtime::runtime_api!(
    install = install_runtime,
    state = PhysicsState3d,
    rapier = rapier3d,
    handle = SoftBodyHandle,
    joint = crate::joint::JointHandle3d::Impulse,
    stamp = stamp_colliders,
    component = c::SOFTBODY_3D,
    dims = 3,
    point = Vector,
    vector = scalar::v3a,
    value = balaur_script::Value::Vec3,
    array = scalar::a3
);

/// The handle type the snapshot carries per node.
pub(crate) type SoftRef3d = SoftBodyHandle;
