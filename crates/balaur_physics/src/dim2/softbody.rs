//! `softbody2d`: the 2D half of [`crate::softbody`].
//!
//! The same material, the same tearing and the same plasticity, over
//! generators that lay particles out in a plane. A 2D body's cells are
//! triangles rather than tetrahedra, so `volumetric` triangulates an outline
//! where 3D tetrahedrizes a closed mesh, and `polygon` is the cheap way in:
//! the node's drawn shape becomes the body it is simulated as.
//!
//! What the solver produces goes back on the node as a
//! [`balaur_geometry::mesh::SolvedPolygon`], which is what the 2D renderer draws
//! in place of the authored positions.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{self, Component, Written};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId};

use crate::dim2::PhysicsState2d;
use crate::properties::{
    Binding, ContactPair, EdgeSpring, EdgeTearResistance, Edges, Indices, Orientation,
    ParticlePair, SoftKind2d, SoftRegion, SoftSolver, TensionOnly,
};
use crate::rapier2d::prelude::{
    SoftBodyBuilder as SoftBodyBuilder2, SoftBodyHandle as SoftBodyHandle2,
    SoftBodyParticleSettings as SoftBodyParticleSettings2, SoftBodySolver as SoftBodySolver2,
};
use crate::scalar::{self, Vector2};
use crate::shared::beside::{self, Beside};
use crate::shared::softbody::{self as cap, Family};
use crate::vocabulary::{component as c, keys as k, words as w};
use balaur_core::layers::Layer;

mod draw;

/// A deformable 2D body: particles linked by elastic constraints, laid out by `kind` and made of what the material rows say. A `polygon` on the same node is drawn from the solver's positions when the two agree on the vertex count, which the `polygon`, `triangle_mesh` and `volumetric` kinds give and a generator does not.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, PartialEq, Component)]
#[component(name = "softbody2d", on_remove = softbody2d_removed, after_write = after_write)]
pub struct Softbody2d {
    /// How the body's particles and elements are laid out
    #[property(default = SoftKind2d::Grid)]
    pub kind: SoftKind2d,
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
    /// Points and triangles for a polygon, triangle_mesh, polyline or volumetric body, and the skin a body with cells carries unless its soft_skin2d says otherwise: the same asset a polygon draws
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
    /// Parts of the body with a material of their own, each the particles it covers: `stiffness_scale` on the cells wholly inside, `tear_resistance` on its edges and cells, `shape_matching` towards its own frame, `pinned` to hold it still, and an edge spring of `hz` and `damping`, 0 hz keeping the body's. Each is a `soft_frame` body joints may attach to
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

/// A mesh the node's `softbody2d` meets the world through beside its own surface, deformed with the body: its triangles in 3D, the outline of its triangles in 2D.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_collision2d")]
pub struct SoftCollision2d {
    /// The mesh, in the node's space
    #[property(asset = "mesh")]
    pub collision_mesh: String,
    /// How the mesh follows the body: each vertex on the nearest particle, vertex i on particle i, or riding the cell that holds it, so a coarse cage of cells can carry a fine mesh
    pub collision_binding: Binding,
    /// Let the mesh collide with itself
    pub collision_self_contacts: bool,
}

impl Beside for SoftCollision2d {
    type Main = Softbody2d;

    fn reads(_: &Softbody2d) -> bool {
        true
    }
}

/// The `mesh` a `softbody2d` with cells carries as its skin: a grid, a volumetric body or a custom one with cells, whose particles are not the mesh's own.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_skin2d")]
pub struct SoftSkin2d {
    /// Keep the mesh asset's outline as what the cells carry and the node draws
    #[property(default = true)]
    pub skin: bool,
    /// Meet the world through the skin rather than the cells' boundary
    pub skin_collision: bool,
}

impl Beside for SoftSkin2d {
    type Main = Softbody2d;

    fn reads(main: &Softbody2d) -> bool {
        matches!(
            main.kind,
            SoftKind2d::Grid | SoftKind2d::Volumetric | SoftKind2d::Custom
        )
    }
}

/// The size and the cells of a `softbody2d` laid out as a grid: a sheet of particles.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_grid2d")]
pub struct SoftGrid2d {
    /// Whole size of the sheet
    #[property(default = [1.0, 1.0])]
    pub size: [f32; 2],
    /// How many cells along each axis of a grid
    #[property(default = [4.0, 4.0])]
    pub cells: [f32; 2],
}

impl Beside for SoftGrid2d {
    type Main = Softbody2d;

    fn reads(main: &Softbody2d) -> bool {
        main.kind == SoftKind2d::Grid
    }
}

/// The size of a `softbody2d` laid out as a circle: a disk of particles inside a rim.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_circle2d")]
pub struct SoftCircle2d {
    /// Radius
    #[property(default = 0.5, min = 0.001)]
    pub radius: f32,
    /// How many particles the rim of a circle is made of
    #[property(default = cap::DEFAULT_PARTICLES as i64, min = 2)]
    pub particle_count: i64,
}

impl Beside for SoftCircle2d {
    type Main = Softbody2d;

    fn reads(main: &Softbody2d) -> bool {
        main.kind == SoftKind2d::Circle
    }
}

/// Where a `softbody2d` laid out as a rope runs, and how many particles it is made of.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_rope2d")]
pub struct SoftRope2d {
    /// How many particles a rope is made of
    #[property(default = cap::DEFAULT_PARTICLES as i64, min = 2)]
    pub particle_count: i64,
    /// Where a rope starts, relative to the node
    #[property(name = "a")]
    pub first: [f32; 2],
    /// Where a rope ends, relative to the node
    #[property(name = "b", default = [0.0, -1.0])]
    pub second: [f32; 2],
}

impl Beside for SoftRope2d {
    type Main = Softbody2d;

    fn reads(main: &Softbody2d) -> bool {
        main.kind == SoftKind2d::Rope
    }
}

/// The corners of a `softbody2d` laid out as an outline: a ring of particles around an inside.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_outline2d")]
pub struct SoftOutline2d {
    /// The outline's corners counter-clockwise, relative to the node
    pub points: Vec<[f32; 2]>,
}

impl Beside for SoftOutline2d {
    type Main = Softbody2d;

    fn reads(main: &Softbody2d) -> bool {
        main.kind == SoftKind2d::Outline
    }
}

/// Which segments of its mesh a `softbody2d` laid out as a polyline joins.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_polyline2d")]
pub struct SoftPolyline2d {
    /// Which segments a polyline body joins: its mesh's points in order, or the edges of its mesh's triangles
    #[property(default = Edges::Chain)]
    pub edges: Edges,
}

impl Beside for SoftPolyline2d {
    type Main = Softbody2d;

    fn reads(main: &Softbody2d) -> bool {
        main.kind == SoftKind2d::Polyline
    }
}

/// How a volumetric `softbody2d` fills its mesh's outline with triangles.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_volumetric2d")]
pub struct SoftVolumetric2d {
    /// How big one triangle is when a volumetric body fills an outline; smaller is finer, slower and stiffer to tear
    #[property(default = 0.25, min = 0.001)]
    pub cell_size: f32,
    /// The smallest corner a volumetric body's triangles aim for; past 30 degrees the fill may not finish. Radians in the file
    #[property(default = DEFAULT_MIN_ANGLE, max = 30.0, min = 0.0, unit = "degrees")]
    pub min_angle: f32,
}

impl Beside for SoftVolumetric2d {
    type Main = Softbody2d;

    fn reads(main: &Softbody2d) -> bool {
        main.kind == SoftKind2d::Volumetric
    }
}

/// The particles and elements of a custom `softbody2d`, as the author lists them.
#[derive(Clone, PartialEq, Component)]
#[component(name = "soft_custom2d")]
pub struct SoftCustom2d {
    /// The particles of a custom body, relative to the node
    pub points: Vec<[f32; 2]>,
    /// A custom body's triangles, three particle indices each, counter-clockwise
    pub cell_indices: Vec<Vec<i64>>,
    /// A custom body's structural edges, each a pair of particle indices; empty takes the cells' edges
    pub edge_indices: Vec<Vec<i64>>,
    /// A custom body's bending edges, each a pair of particle indices
    pub bend_edge_indices: Vec<Vec<i64>>,
    /// A custom body's outward boundary segments, each a pair of particle indices: what holds its area and what it collides through; empty takes the cells' boundary
    pub surface_indices: Vec<Vec<i64>>,
}

impl Beside for SoftCustom2d {
    type Main = Softbody2d;

    fn reads(main: &Softbody2d) -> bool {
        main.kind == SoftKind2d::Custom
    }
}

/// The components whose rows a `softbody2d`'s build reads: its own, then
/// those beside it.
pub(crate) const PARAMS: &[&str] = &[
    c::SOFTBODY_2D,
    <SoftGrid2d as Component>::NAME,
    <SoftCircle2d as Component>::NAME,
    <SoftRope2d as Component>::NAME,
    <SoftOutline2d as Component>::NAME,
    <SoftPolyline2d as Component>::NAME,
    <SoftVolumetric2d as Component>::NAME,
    <SoftCustom2d as Component>::NAME,
    <SoftCollision2d as Component>::NAME,
    <SoftSkin2d as Component>::NAME,
];

/// What a node's soft body is built from: the component, and the
/// components beside it as its kind reads them.
#[derive(Clone, PartialEq)]
pub struct SoftAsked2d {
    pub body: Softbody2d,
    pub grid: SoftGrid2d,
    pub circle: SoftCircle2d,
    pub rope: SoftRope2d,
    pub outline: SoftOutline2d,
    pub polyline: SoftPolyline2d,
    pub volumetric: SoftVolumetric2d,
    pub custom: SoftCustom2d,
    pub collision: SoftCollision2d,
    pub skin: SoftSkin2d,
}

impl SoftAsked2d {
    /// What `entity` asks for, `None` without a `softbody2d`.
    pub fn of(world: &balaur_core::hecs::World, entity: Entity) -> Option<Self> {
        let body = (*world.get::<&Softbody2d>(entity).ok()?).clone();
        Some(Self::with_body(world, entity, body))
    }

    /// `body` with the components beside it on `entity`.
    pub fn with_body(world: &balaur_core::hecs::World, entity: Entity, body: Softbody2d) -> Self {
        Self {
            grid: beside::held(world, entity, &body),
            circle: beside::held(world, entity, &body),
            rope: beside::held(world, entity, &body),
            outline: beside::held(world, entity, &body),
            polyline: beside::held(world, entity, &body),
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
        beside::table_in(&mut table, &self.grid, SoftGrid2d::reads(body));
        beside::table_in(&mut table, &self.circle, SoftCircle2d::reads(body));
        beside::table_in(&mut table, &self.rope, SoftRope2d::reads(body));
        beside::table_in(&mut table, &self.outline, SoftOutline2d::reads(body));
        beside::table_in(&mut table, &self.polyline, SoftPolyline2d::reads(body));
        beside::table_in(&mut table, &self.volumetric, SoftVolumetric2d::reads(body));
        beside::table_in(&mut table, &self.custom, SoftCustom2d::reads(body));
        beside::table_in(&mut table, &self.collision, SoftCollision2d::reads(body));
        beside::table_in(&mut table, &self.skin, SoftSkin2d::reads(body));
        table
    }

    /// All of it out of one table, as [`Self::to_table`] wrote it.
    pub(crate) fn from_table(params: &toml::Value) -> Result<Self> {
        let body: Softbody2d = derived::from_table(params)?;
        Ok(Self {
            grid: beside::from_table(params, &body)?,
            circle: beside::from_table(params, &body)?,
            rope: beside::from_table(params, &body)?,
            outline: beside::from_table(params, &body)?,
            polyline: beside::from_table(params, &body)?,
            volumetric: beside::from_table(params, &body)?,
            custom: beside::from_table(params, &body)?,
            collision: beside::from_table(params, &body)?,
            skin: beside::from_table(params, &body)?,
            body,
        })
    }
}

crate::shared::softbody::material!(
    rapier = rapier2d,
    material = read_material_2d,
    cell_model = read_cell_model_2d
);

/// The `mesh` asset's points as 2D, with the triangles over them, in world
/// space around `pose`.
fn source_mesh(
    eng: &Engine,
    reference: &str,
    pose: scalar::Pose2,
) -> Result<(Vec<Vector2>, Vec<[u32; 3]>)> {
    if reference.is_empty() {
        return Err(anyhow!("this soft-body kind needs a `mesh` asset"));
    }
    mesh_in_world(eng, reference, pose)
}

/// The `collision_mesh`, in world space, when there is one.
fn collision_mesh(
    eng: &Engine,
    collision: &SoftCollision2d,
    pose: scalar::Pose2,
) -> Result<Option<cap::Mesh<Vector2>>> {
    if collision.collision_mesh.is_empty() {
        return Ok(None);
    }
    mesh_in_world(eng, &collision.collision_mesh, pose).map(Some)
}

fn mesh_in_world(
    eng: &Engine,
    reference: &str,
    pose: scalar::Pose2,
) -> Result<(Vec<Vector2>, Vec<[u32; 3]>)> {
    let definition =
        balaur_core::assets::load_typed::<balaur_geometry::mesh::MeshData>(eng, reference)?;
    let mesh = balaur_geometry::mesh::load_from(eng, &definition)?;
    let points = mesh
        .positions
        .iter()
        .map(|p| pose * scalar::v2(p[0], p[1]))
        .collect();
    Ok((points, mesh.indices.clone()))
}

/// The outline of a mesh, as the boundary segments of its triangles: the
/// edges belonging to one triangle only, sorted so the result never depends
/// on the order the triangles came in.
fn outline(indices: &[[u32; 3]]) -> Vec<[u32; 2]> {
    let mut counts: std::collections::BTreeMap<[u32; 2], (u32, [u32; 2])> =
        std::collections::BTreeMap::new();
    for tri in indices {
        for i in 0..3 {
            let (a, b) = (tri[i], tri[(i + 1) % 3]);
            let key = [a.min(b), a.max(b)];
            let entry = counts.entry(key).or_insert((0, [a, b]));
            entry.0 += 1;
        }
    }
    counts
        .into_values()
        .filter(|(count, _)| *count == 1)
        .map(|(_, edge)| edge)
        .collect()
}

/// Every edge of the triangles, once, in a stable order.
fn triangle_edges(indices: &[[u32; 3]]) -> Vec<[u32; 2]> {
    let mut edges = std::collections::BTreeSet::new();
    for tri in indices {
        for i in 0..3 {
            let (a, b) = (tri[i], tri[(i + 1) % 3]);
            edges.insert([a.min(b), a.max(b)]);
        }
    }
    edges.into_iter().collect()
}

/// What to change for fewer particles, by what a kind reads.
fn fewer(kind: SoftKind2d) -> cap::Fewer {
    match kind {
        SoftKind2d::Volumetric => cap::Fewer::CellSize,
        SoftKind2d::Rope | SoftKind2d::Circle => cap::Fewer::ParticleCount,
        SoftKind2d::TriangleMesh | SoftKind2d::Polygon | SoftKind2d::Polyline => {
            cap::Fewer::MeshPoints
        }
        SoftKind2d::Custom | SoftKind2d::Outline => cap::Fewer::Points,
        SoftKind2d::Grid => cap::Fewer::Cells,
    }
}

/// Lay the particles out the way `kind` asks, in world space around `pose`.
fn build_layout(
    eng: &Engine,
    asked: &SoftAsked2d,
    materials: &cap::Materials,
    pose: scalar::Pose2,
) -> Result<SoftBodyBuilder2> {
    let kind = asked.body.kind;
    let mesh = asked.body.mesh.as_str();
    let at = pose.translation;
    let cells = asked.grid.cells;
    let axis = |i: usize| cap::particles_along(cells[i]);
    let circle = || cap::particle_count(asked.circle.particle_count, cap::MIN_RING_PARTICLES);
    let rope = || cap::particle_count(asked.rope.particle_count, cap::MIN_CHAIN_PARTICLES);
    cap::refuse_past_cap(
        match kind {
            SoftKind2d::Grid => axis(0) * axis(1),
            SoftKind2d::Circle => circle(),
            SoftKind2d::Rope => rope(),
            _ => 0.0,
        },
        fewer(kind),
    )?;
    let builder = match kind {
        // As in 3D: the schema counts cells, rapier counts the particles
        // between them.
        SoftKind2d::Grid => SoftBodyBuilder2::grid(
            at,
            scalar::v2a(asked.grid.size) / 2.0,
            axis(0) as usize,
            axis(1) as usize,
        ),
        SoftKind2d::Circle => SoftBodyBuilder2::disk(at, asked.circle.radius, circle() as usize),
        SoftKind2d::Rope => SoftBodyBuilder2::rope(
            pose * scalar::v2a(asked.rope.first),
            pose * scalar::v2a(asked.rope.second),
            rope() as usize,
        ),
        // A hoop around an inside; the triangle edges hold too, or a vertex
        // inside the outline is a particle nothing holds, and it falls out.
        SoftKind2d::Polygon => {
            let (points, indices) = source_mesh(eng, mesh, pose)?;
            let border = outline(&indices);
            SoftBodyBuilder2::polyline(points, Some(border))
                .map(|hoop| hoop.edges(triangle_edges(&indices)))
                .ok_or_else(|| anyhow!("that mesh has no outline to make a soft polygon of"))?
        }
        // `chain` joins the points in order; `mesh` takes its triangles' edges.
        SoftKind2d::Polyline => {
            let (points, indices) = source_mesh(eng, mesh, pose)?;
            let segments = (asked.polyline.edges == Edges::Mesh).then(|| triangle_edges(&indices));
            SoftBodyBuilder2::polyline(points, segments)
                .ok_or_else(|| anyhow!("that mesh has no points to make a soft wire of"))?
        }
        SoftKind2d::Outline => {
            let points: Vec<Vector2> = asked
                .outline
                .points
                .iter()
                .map(|p| pose * scalar::v2a(*p))
                .collect();
            if points.len() < 3 {
                return Err(anyhow!(
                    "an outline soft body needs three or more `points`, counter-clockwise"
                ));
            }
            SoftBodyBuilder2::polygon(points)
        }
        SoftKind2d::Custom => custom(&asked.custom, pose)?,
        // The triangles the asset already carries, kept as the body's cells.
        SoftKind2d::TriangleMesh => {
            let (points, indices) = source_mesh(eng, mesh, pose)?;
            SoftBodyBuilder2::trimesh(points, indices)
                .ok_or_else(|| anyhow!("that mesh has no triangles to make a soft body of"))?
        }
        // The approximate triangulation: the outline is covered with cells of
        // `cell_size`, and the mesh rides them as a skin, which is what draws.
        SoftKind2d::Volumetric => {
            let (points, indices) = source_mesh(eng, mesh, pose)?;
            let border = outline(&indices);
            let size = asked.volumetric.cell_size;
            cap::refuse_past_cap(cap::grid_particles(&extents(&points), size), fewer(kind))?;
            let mut fill = crate::rapier2d::parry::transformation::VolumeMeshParameters::new(size);
            fill.min_angle = asked.volumetric.min_angle;
            SoftBodyBuilder2::volumetric_with(&points, &border, &fill).ok_or_else(|| {
                anyhow!("that outline encloses nothing at a cell size of {size}: it has to be closed, and big enough to hold a cell")
            })?
        }
    };
    cap::refuse_past_cap(builder.positions.len() as f64, fewer(kind))?;
    let builder = skinned(eng, asked, pose, builder)?;
    with_settings(builder, asked, materials)
}

/// parry's own smallest angle for a 2D fill, 30 degrees.
const DEFAULT_MIN_ANGLE: f32 = std::f32::consts::FRAC_PI_6;

/// A body of the particles and elements the author listed.
fn custom(listed: &SoftCustom2d, pose: scalar::Pose2) -> Result<SoftBodyBuilder2> {
    let points: Vec<Vector2> = listed
        .points
        .iter()
        .map(|p| pose * scalar::v2a(*p))
        .collect();
    let n = points.len();
    let edges = cap::tuples::<2>(&listed.edge_indices, k::EDGE_INDICES, n)?;
    let bend = cap::tuples::<2>(&listed.bend_edge_indices, k::BEND_EDGE_INDICES, n)?;
    let surface = cap::tuples::<2>(&listed.surface_indices, k::SURFACE_INDICES, n)?;
    let cells = cap::tuples::<3>(&listed.cell_indices, k::CELL_INDICES, n)?;
    if n == 0 || (edges.is_empty() && cells.is_empty() && surface.is_empty()) {
        return Err(anyhow!(
            "a custom soft body needs `points` and something to join them: cell_indices, edge_indices or surface_indices"
        ));
    }
    let mut builder = SoftBodyBuilder2::new(points)
        .bend_edges(bend)
        .surface(surface)
        .cells(cells);
    if !edges.is_empty() {
        builder = builder.edges(edges);
    }
    Ok(builder)
}

/// The `mesh` asset's outline held by the cells as the body's skin, when
/// its kind reads a `soft_skin2d`, that asks for one, and the layout made
/// cells.
fn skinned(
    eng: &Engine,
    asked: &SoftAsked2d,
    pose: scalar::Pose2,
    builder: SoftBodyBuilder2,
) -> Result<SoftBodyBuilder2> {
    let body = &asked.body;
    if !SoftSkin2d::reads(body)
        || !asked.skin.skin
        || body.mesh.is_empty()
        || builder.cells.is_empty()
    {
        return Ok(builder);
    }
    let (points, indices) = source_mesh(eng, &body.mesh, pose)?;
    Ok(builder.skin(points, outline(&indices)))
}

/// How wide the points spread along each axis.
fn extents(points: &[Vector2]) -> [f32; 2] {
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for point in points {
        for (axis, value) in scalar::a2(*point).into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    [0, 1].map(|axis| (high[axis] - low[axis]).max(0.0))
}

cap::regions!(rapier = rapier2d, add = add_regions_2d);
cap::only_in_place!(only_in_place, SoftAsked2d);
cap::collision_mesh!(
    rapier = rapier2d,
    body = Softbody2d,
    collision = SoftCollision2d,
    bind = bind_collision_mesh_2d,
    shape = |vertices, indices: Vec<[u32; 3]>| {
        use crate::rapier2d::geometry::{Polyline, PolylineFlags, SharedShape};
        Ok::<_, anyhow::Error>(SharedShape::new(Polyline::with_flags(
            vertices,
            Some(outline(&indices)),
            PolylineFlags::DEFORMABLE,
        )))
    }
);

cap::surface!(
    rapier = rapier2d,
    body = Softbody2d,
    groups = crate::dim2::collider::groups_of,
    pairs = crate::dim2::collider::collision_types_of
);

/// Which solver runs the elasticity.
fn solver_of_2d(solver: SoftSolver) -> SoftBodySolver2 {
    match solver {
        SoftSolver::Fem => SoftBodySolver2::Fem,
        SoftSolver::Constraints => SoftBodySolver2::Constraints,
    }
}

cap::patch_in_place!(
    patch_body_2d,
    crate::rapier2d::pipeline::PhysicsWorld,
    SoftBodyHandle2,
    Softbody2d,
    read_material_2d,
    solver_of_2d
);

/// The rows every layout shares, as 3D's are read, against rapier2d's own
/// types.
fn with_settings(
    mut builder: SoftBodyBuilder2,
    asked: &SoftAsked2d,
    materials: &cap::Materials,
) -> Result<SoftBodyBuilder2> {
    let body = &asked.body;
    let made = &*materials.body;
    let seams = cap::seams(&body.seams, builder.positions.len())?;
    if !seams.is_empty() {
        builder = builder.add_edges(seams);
    }
    builder = builder
        .material(read_material_2d(
            made,
            body.max_tears_per_step,
            body.min_piece,
        ))
        .cell_model(read_cell_model_2d(made))
        .volume_preservation(made.volume_preservation)
        .volume_factor(made.volume_factor)
        .self_contacts(body.self_collision)
        .linear_damping(body.linear_damping)
        .gravity_scale(body.gravity_scale)
        .additional_solver_iterations(cap::iterations(body.solver_substeps))
        .additional_pgs_iterations(cap::iterations(body.solver_iterations))
        .can_sleep(body.can_sleep)
        .solver(solver_of_2d(body.solver))
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
    let settings = SoftBodyParticleSettings2 {
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
        let spring = crate::rapier2d::prelude::SpringCoefficients::new(hz, damping);
        builder.edge_softness.push((edge, spring));
    }
    builder.tension_only_edges.extend(rows.tension_only);
    if !body.collides {
        builder = builder.no_surface_collider();
    }
    Ok(builder)
}

cap::stamp_colliders!(
    stamp_colliders_2d,
    crate::rapier2d::pipeline::PhysicsWorld,
    SoftBodyHandle2
);

pub(crate) fn apply_softbody_2d(eng: &Engine, entity: Entity, asked: &SoftAsked2d) -> Result<()> {
    let materials =
        cap::Materials::of(eng, &asked.body.soft_material, &asked.body.physics_material)?;
    if patched_2d(eng, entity, asked, &materials) {
        return Ok(());
    }
    build_softbody_2d(eng, entity, asked, &materials)
}

/// Build the node's soft body from rest, replacing whatever it had: what a
/// script's `set_softbody` asks for even when nothing changed.
fn build_softbody_2d(
    eng: &Engine,
    entity: Entity,
    asked: &SoftAsked2d,
    materials: &cap::Materials,
) -> Result<()> {
    let pose = crate::dim2::node_pose_2d(eng, entity)?;
    let builder =
        build_layout(eng, asked, materials, pose)?.user_data(u128::from(entity.to_bits().get()));
    let collision = collision_mesh(eng, &asked.collision, pose)?;
    remove_softbody_2d(eng, entity);
    let added = {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let handle = state.world.soft_bodies.insert(
            builder,
            &mut state.world.bodies,
            &mut state.world.colliders,
        );
        state.soft_bodies.insert(entity, handle);
        state.soft_params.insert(entity, asked.clone());
        let added = add_regions_2d(&mut state.world, handle, &asked.body.regions).and_then(|()| {
            bind_collision_mesh_2d(
                &mut state.world,
                handle,
                &asked.body,
                &asked.collision,
                collision,
                materials,
            )
        });
        stamp_colliders_2d(&mut state.world, handle, entity);
        added
    };
    if let Err(why) = added {
        remove_softbody_2d(eng, entity);
        return Err(why);
    }
    write_solved_polygon(eng, entity);
    Ok(())
}

/// As the 3D `patched`: a write the live body takes as it stands.
fn patched_2d(
    eng: &Engine,
    entity: Entity,
    asked: &SoftAsked2d,
    materials: &cap::Materials,
) -> bool {
    let state = eng.resource::<PhysicsState2d>();
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
    patch_body_2d(&mut state.world, handle, &asked.body, materials);
    state.soft_params.insert(entity, asked.clone());
    true
}

pub(crate) fn remove_softbody_2d(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState2d>();
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

/// Hand this step's geometry to whatever draws the node, in the node's own
/// space (see [`crate::softbody::write_solved_mesh`]).
pub(crate) fn write_solved_polygon(eng: &Engine, entity: Entity) {
    let Ok(pose) = crate::dim2::node_pose_2d(eng, entity) else {
        return;
    };
    let held = eng
        .world()
        .get::<&draw::SkinTriangles>(entity)
        .ok()
        .map(|t| t.0.clone());
    let mut fresh = None;
    let ((positions, indices), color) = {
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let Some(&handle) = state.soft_bodies.get(&entity) else {
            return;
        };
        let Some(body) = state.world.soft_bodies.get(handle) else {
            return;
        };
        let asked = state.soft_params.get(&entity);
        let skinned = body
            .meshes()
            .any(crate::rapier2d::dynamics::SoftCollisionMesh::is_skinned);
        let mut drawn = draw::drawn(body, pose.inverse(), || {
            held.unwrap_or_else(|| {
                let loaded = asked
                    .and_then(|asked| {
                        source_mesh(eng, &asked.body.mesh, scalar::Pose2::IDENTITY).ok()
                    })
                    .map(|(_, triangles)| triangles)
                    .unwrap_or_default();
                fresh = Some(loaded.clone());
                loaded
            })
        });
        // What tore off draws beside the body, unless the body is a skin a
        // polygon bends by: that polygon has room for its own points alone.
        if !skinned {
            for piece in state.world.soft_bodies.family(handle).into_iter().skip(1) {
                let Some(body) = state.world.soft_bodies.get(piece) else {
                    continue;
                };
                let (points, triangles) = draw::drawn(body, pose.inverse(), Vec::new);
                let offset = drawn.0.len() as u32;
                drawn.0.extend(points);
                drawn
                    .1
                    .extend(triangles.iter().map(|t| t.map(|i| i + offset)));
            }
        }
        let color = asked.map_or(cap::DEFAULT_COLOR, |asked| asked.body.color);
        (drawn, color)
    };
    if let Some(triangles) = fresh {
        let _ = eng.insert_one(entity, draw::SkinTriangles(triangles));
    }
    if let Ok(mut solved) = eng
        .world()
        .get::<&mut balaur_geometry::mesh::SolvedPolygon>(entity)
    {
        solved.update(positions, indices);
        solved.color = color;
        return;
    }
    let mut solved = balaur_geometry::mesh::SolvedPolygon::default();
    solved.update(positions, indices);
    solved.color = color;
    let _ = eng.insert_one(entity, solved);
}

pub(crate) fn write_every_solved_polygon(eng: &Engine) {
    let entities: Vec<Entity> = {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        for (&entity, &handle) in &state.soft_bodies {
            stamp_colliders_2d(&mut state.world, handle, entity);
        }
        state.soft_bodies.keys().copied().collect()
    };
    for entity in entities {
        write_solved_polygon(eng, entity);
    }
}

/// What is off about a 2D soft body: a node that also draws something rigid,
/// a polygon of another mesh than the body hands over, and a hovering radius.
fn softbody_warnings_2d(eng: &Engine, entity: Entity) -> Vec<balaur_core::warnings::Warning> {
    use balaur_core::warnings::Warning;
    let mut out = Vec::new();
    let present = balaur_core::components::present_on(eng, entity);
    if let Some(drawer) = c::RIGID_2D_DRAWERS
        .iter()
        .find(|d| present.iter().any(|p| p == *d))
    {
        out.push(Warning::whole(format!(
            "the node also draws a {drawer}, which stays rigid while the body moves: draw it as a polygon instead"
        )));
    }
    if let Some(drawn) = polygon_points(eng, entity) {
        let handed = eng
            .world()
            .get::<&balaur_geometry::mesh::SolvedPolygon>(entity)
            .map_or(drawn, |solved| solved.positions.len());
        if handed != drawn {
            out.push(Warning::on(
                k::MESH,
                format!(
                    "the polygon has {drawn} points and the body hands over {handed}, so the polygon does not bend: build the body from the polygon's mesh"
                ),
            ));
        }
    }
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let authored = state
        .soft_params
        .get(&entity)
        .map_or(0.0, |asked| asked.body.particle_radius);
    let body = state
        .soft_bodies
        .get(&entity)
        .and_then(|&handle| state.world.soft_bodies.get(handle));
    if let Some(body) = body.filter(|_| authored <= 0.0) {
        let points: Vec<Vector2> = body.particle_positions().collect();
        let widest = extents(&points).into_iter().fold(0.0, f32::max);
        out.extend(cap::hovering(body.particle_radius(), widest));
    }
    out
}

/// How many points the node's own polygon draws, when it has one.
fn polygon_points(eng: &Engine, entity: Entity) -> Option<usize> {
    let polygon = balaur_core::components::get(eng, entity, c::POLYGON)?;
    let reference = polygon.get(k::MESH).and_then(toml::Value::as_str)?;
    let mesh = balaur_geometry::mesh::resolved(eng, reference).ok()?;
    Some(mesh.positions.len())
}
/// The soft body the component asks for, patched in place or built again.
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let asked = SoftAsked2d::of(&eng.world(), entity);
    let Some(asked) = asked else {
        return Ok(());
    };
    apply_softbody_2d(eng, entity, &asked)
}

/// What a removed `Softbody2d` leaves behind, taken with it.
fn softbody2d_removed(eng: &Engine, entity: Entity) {
    remove_softbody_2d(eng, entity);
    let _ = eng.remove_one::<balaur_geometry::mesh::SolvedPolygon>(entity);
    let _ = eng.remove_one::<draw::SkinTriangles>(entity);
}

/// `softbody2d`: what was asked, held on the node; the body and the table it
/// was built from are in [`crate::PhysicsState2d`].
pub(crate) fn register_softbody_component_2d(reg: &mut Registry<'_>) {
    let tags: &'static [&'static str] = &[
        balaur_core::components::tag::DIM_2D,
        balaur_core::components::tag::PHYSICS,
    ];
    let main: &'static [&'static str] = &[c::SOFTBODY_2D];
    beside::register::<SoftGrid2d>(reg, main, tags, after_write);
    beside::register::<SoftCircle2d>(reg, main, tags, after_write);
    beside::register::<SoftRope2d>(reg, main, tags, after_write);
    beside::register::<SoftOutline2d>(reg, main, tags, after_write);
    beside::register::<SoftPolyline2d>(reg, main, tags, after_write);
    beside::register::<SoftVolumetric2d>(reg, main, tags, after_write);
    beside::register::<SoftCustom2d>(reg, main, tags, after_write);
    beside::register::<SoftCollision2d>(reg, main, tags, after_write);
    beside::register::<SoftSkin2d>(reg, main, tags, after_write);
    // After the components beside it, so a scene gives the node those first.
    reg.register_component(
        Softbody2d::NAME,
        ComponentDef {
            events: crate::vocabulary::hook::SOFT_BODY,
            warnings: Some(Box::new(softbody_warnings_2d)),
            // It draws the node, so the tree's Rendering filter finds it too.
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
                balaur_core::components::tag::RENDER,
            ],
            ..ComponentDef::of::<Softbody2d>()
        },
    );
}

crate::softbody::runtime::runtime_api!(
    install = install_runtime_2d,
    state = PhysicsState2d,
    rapier = rapier2d,
    handle = SoftBodyHandle2,
    joint = crate::dim2::joint::JointHandle2d::Impulse,
    stamp = stamp_colliders_2d,
    component = c::SOFTBODY_2D,
    dims = 2,
    point = Vector2,
    vector = scalar::v2a,
    value = balaur_script::Value::Vec2,
    array = scalar::a2
);

pub(crate) fn install_softbody_api_2d(m: &mut Module<Engine>) {
    m.describe(&[
        ("softbody_area", &[c::SOFTBODY_2D], "", "How much area the body and every piece torn off it enclose right now, against `softbody_rest_area` for how far it is squeezed."),
        ("softbody_rest_area", &[c::SOFTBODY_2D], "", "How much they enclose at rest."),
        ("softbody_center", &[c::SOFTBODY_2D], "", "The centre of mass of the body and its pieces, which is where it is when a deformable body has no one position."),
    ]);
    m.function("softbody_area", |eng: &Engine, node: NodeId| {
        with_family_2d(eng, node, |bodies| {
            Ok(bodies.iter().map(|b| b.volume()).sum::<f32>())
        })
    });
    m.function("softbody_rest_area", |eng: &Engine, node: NodeId| {
        with_family_2d(eng, node, |bodies| {
            Ok(bodies.iter().map(|b| b.rest_volume()).sum::<f32>())
        })
    });
    m.function("softbody_center", |eng: &Engine, node: NodeId| {
        with_family_2d(eng, node, |bodies| {
            let mass: scalar::Real = bodies.iter().map(|b| b.mass()).sum();
            let weighted: Vector2 = bodies.iter().map(|b| b.center_of_mass() * b.mass()).sum();
            let center = if mass > 0.0 {
                weighted / mass
            } else {
                Vector2::ZERO
            };
            Ok(balaur_script::Value::Vec2(scalar::a2(center)))
        })
    });

    install_runtime_2d(m);
}

/// Run `f` over the node's body and every piece torn off it.
fn with_family_2d<T>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&[&crate::rapier2d::prelude::SoftBody]) -> Result<T>,
) -> Result<T> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = *state
        .soft_bodies
        .get(&entity)
        .ok_or_else(|| anyhow!("node has no soft body"))?;
    let set = &state.world.soft_bodies;
    let bodies: Vec<_> = set
        .family(handle)
        .into_iter()
        .filter_map(|piece| set.get(piece))
        .collect();
    anyhow::ensure!(!bodies.is_empty(), "node has no soft body");
    f(&bodies)
}

/// The handle type the 2D snapshot carries per node.
pub(crate) type SoftRef2d = SoftBodyHandle2;
