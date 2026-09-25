//! `softbody3d`: a deformable body of particles linked by elastic
//! constraints, solved beside the rigid bodies on the same fixed step.
//!
//! The component says how the particles are laid out (`kind` and the shape
//! rows beside it) and what they are made of (the material rows). Rapier
//! builds the body once from that; after it, the solver owns the positions
//! and the node's drawn vertices come back from it every step through
//! [`balaur_core::mesh::SolvedMesh`].
//!
//! A soft body is world-space like a rigid one: the generators are given the
//! node's composed global pose, and the read-back divides it out again so a
//! body under a turned parent draws where it is simulated.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::PhysicsState3d;
use crate::rapier3d::prelude::{ColliderBuilder, SoftBodyBuilder, SoftBodyHandle, SoftBodySolver};
use crate::scalar::{self, Real, Vector};
use crate::shared::softbody as cap;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

/// The shape rows: how a body's particles and elements are laid out, and the
/// numbers each layout reads.
fn shape_schema() -> String {
    let kinds = v::options(w::SOFT_KINDS);
    let default = w::SOFT_CUBOID;
    v::schema(&[
        (
            k::KIND,
            &format!(
                r#"{{ type = "enum", default = "{default}", options = [{kinds}], description = "How the body's particles and elements are laid out" }}"#
            ),
        ),
        (
            k::HALF_EXTENTS,
            r#"{ type = "vec3", default = [0.5, 0.5, 0.5], description = "Half-sizes of the block, when kind is cuboid", group = "shape" }"#,
        ),
        (
            k::CELLS,
            r#"{ type = "vec3", default = [4.0, 4.0, 4.0], description = "How many cells along each axis, for cuboid; a cloth reads the first two, and a cloth_tube reads them as particles around and cells along", group = "shape" }"#,
        ),
        (
            k::SIZE,
            r#"{ type = "vec3", default = [1.0, 0.0, 1.0], description = "The two edges a cloth is spanned over, as the sheet's extent along x and z", group = "shape" }"#,
        ),
        (
            k::RADIUS,
            r#"{ type = "float", default = 0.5, min = 0.001, description = "Radius, for sphere and cloth_tube", group = "shape" }"#,
        ),
        (
            k::SUBDIVISIONS,
            r#"{ type = "float", default = 2.0, min = 0.0, max = 6.0, description = "How many times a sphere's icosahedron is refined; each level quadruples the triangles", group = "shape" }"#,
        ),
        (
            k::A,
            r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where a rope starts, relative to the node", group = "shape" }"#,
        ),
        (
            k::B,
            r#"{ type = "vec3", default = [0.0, -1.0, 0.0], description = "Where a rope ends, relative to the node", group = "shape" }"#,
        ),
        (
            k::PARTICLES,
            &format!(
                r#"{{ type = "float", default = {:?}, min = {:?}, description = "How many particles a rope is made of", group = "shape" }}"#,
                cap::DEFAULT_PARTICLES,
                cap::MIN_CHAIN_PARTICLES
            ),
        ),
        (
            k::AXIS,
            r#"{ type = "vec3", default = [0.0, 1.0, 0.0], description = "The segment a cloth_tube is wrapped around, relative to the node", group = "shape" }"#,
        ),
        (
            k::MESH,
            &format!(
                r#"{{ type = "asset", asset = "{}", default = "", description = "Geometry for a trimesh, polyline or volumetric body", group = "shape" }}"#,
                balaur_core::mesh::MESH_ASSET_TYPE
            ),
        ),
        (
            k::CELL_SIZE,
            r#"{ type = "float", default = 0.25, min = 0.001, description = "How big one tetrahedron is when a volumetric body fills a mesh; smaller is finer, slower and stiffer to tear", group = "shape" }"#,
        ),
        (
            k::SKIN,
            r#"{ type = "bool", default = false, description = "Keep the mesh as the drawn surface and let the cells carry it, so a detail the cell size cannot resolve survives", group = "shape" }"#,
        ),
        (
            k::SKIN_COLLISION,
            r#"{ type = "bool", default = false, description = "Meet the world through the skin rather than the cells' boundary", group = "shape" }"#,
        ),
    ])
}

/// Everything a soft body carries that is not its layout: what it is made of,
/// how it yields, when it tears, and what its particles weigh. Shared with
/// `softbody2d`, which lays its particles out differently and is made of the
/// same things.
pub(crate) fn shared_softbody_schema() -> String {
    let models = v::options(w::CELL_MODELS);
    let volume = w::VOLUME;
    let solvers = v::options(w::SOFT_SOLVERS);
    let constraints = w::CONSTRAINTS;
    let flows = v::options(w::PLASTIC_FLOWS);
    let both = w::BOTH;
    [
        v::schema(&[
            (k::EDGE_FREQUENCY, r#"{ type = "float", default = 30.0, min = 0.0, max = 10000.0, description = "The frequency of the spring a structural edge is solved as, in hertz; higher is stiffer", group = "stiffness" }"#),
            (k::EDGE_DAMPING, r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "The damping ratio of that spring; 1 settles without overshooting", group = "stiffness" }"#),
            (k::BEND_FREQUENCY, r#"{ type = "float", default = 10.0, min = 0.0, max = 10000.0, description = "The same for the bending edges, which are what stop a cloth folding flat", group = "stiffness" }"#),
            (k::BEND_DAMPING, r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "The damping ratio of the bending springs", group = "stiffness" }"#),
            (k::VOLUME_FREQUENCY, r#"{ type = "float", default = 30.0, min = 0.0, max = 10000.0, description = "The same for the constraints holding a cell's volume, and for the whole-body one", group = "stiffness" }"#),
            (k::VOLUME_DAMPING, r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "The damping ratio of the volume constraints", group = "stiffness" }"#),
            (k::SHAPE_MATCHING_FREQUENCY, r#"{ type = "float", default = 10.0, min = 0.0, max = 10000.0, description = "The same for shape matching, which pulls the body back towards the shape it was built in", group = "stiffness" }"#),
            (k::SHAPE_MATCHING_DAMPING, r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "The damping ratio of the shape-matching constraints", group = "stiffness" }"#),
            (k::CELL_MODEL, &format!(r#"{{ type = "enum", default = "{volume}", options = [{models}], description = "What a cell resists with: a volume constraint for a cheap jelly, or an elastic model a Young modulus parameterises", group = "elasticity" }}"#)),
            (k::YOUNG_MODULUS, r#"{ type = "float", default = 10000.0, min = 0.0, description = "Stiffness of the elastic cells, as force per unit area; a finer mesh does not get stiffer for it", group = "elasticity" }"#),
            (k::POISSON_RATIO, r#"{ type = "float", default = 0.3, min = 0.0, max = 0.499, description = "How much an elastic cell bulges sideways when squeezed; towards 0.5 it stops changing volume at all", group = "elasticity" }"#),
            (k::ELASTIC_DAMPING, r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "Damping ratio of the elastic cells", group = "elasticity" }"#),
            (k::SOLVER, &format!(r#"{{ type = "enum", default = "{constraints}", options = [{solvers}], description = "Which solver runs the elasticity: sequential constraints, or an implicit Euler step over the whole body", group = "elasticity" }}"#)),
            (k::DEFORMATION_DAMPING, r#"{ type = "float", default = 0.0, min = 0.0, max = 1000.0, description = "How fast the particles are pulled towards the body's own rigid motion, which settles a residual sway without slowing the body down", group = "elasticity" }"#),
            (k::PLASTIC_YIELD, r#"{ type = "float", default = 0.0, min = 0.0, description = "The cell strain past which the rest shape flows towards the current one; 0 is perfectly elastic", group = "plasticity" }"#),
            (k::PLASTIC_CREEP, r#"{ type = "float", default = 1.0, min = 0.0, description = "How fast, per second, the strain past the yield is absorbed into the rest shape", group = "plasticity" }"#),
            (k::PLASTIC_MAX, r#"{ type = "float", default = 1.0, min = 0.0, description = "The most permanent deformation a cell may take, so a crushed cell cannot flow to a sliver", group = "plasticity" }"#),
            (k::EDGE_PLASTIC_YIELD, r#"{ type = "float", default = 0.0, min = 0.0, description = "The edge strain past which its rest length flows towards its current length", group = "plasticity" }"#),
            (k::EDGE_PLASTIC_CREEP, r#"{ type = "float", default = 1.0, min = 0.0, description = "How fast, per second, an edge's excess strain is absorbed into its rest length", group = "plasticity" }"#),
            (k::EDGE_PLASTIC_MAX, r#"{ type = "float", default = 0.5, min = 0.0, description = "The largest permanent set an edge may take, as a fraction of its first length", group = "plasticity" }"#),
            (k::EDGE_PLASTIC_FLOW, &format!(r#"{{ type = "enum", default = "{both}", options = [{flows}], description = "Whether an edge sets under a squeeze, a stretch, or both: clay dents but does not stay stretched", group = "plasticity" }}"#)),
            (k::TEAR_STRAIN, r#"{ type = "float", default = 0.0, min = 0.0, description = "The stretch past which an element breaks, as a fraction of its rest length; 0 is unbreakable", group = "tearing" }"#),
            (k::TEAR_FORCE, r#"{ type = "float", default = 0.0, min = 0.0, description = "The pull past which an edge breaks; 0 is unbreakable. Either criterion tears an edge", group = "tearing" }"#),
            (k::TEAR_SMOOTHING, r#"{ type = "float", default = 0.0, min = 0.0, description = "Over how many seconds a load is averaged before it is tested, so one hard frame does not tear a body", group = "tearing" }"#),
            (k::INTERIOR_STRENGTH, r#"{ type = "float", default = 1.0, min = 1.0, description = "How many times tougher an undamaged inside element is than a surface one, so cracks start at the surface and run inward", group = "tearing" }"#),
            (k::MAX_TEARS, r#"{ type = "float", default = 0.0, min = 0.0, description = "The most edges that may tear in one step, which paces a crack; 0 is no limit", group = "tearing" }"#),
            (k::MIN_PIECE, r#"{ type = "float", default = 0.0, min = 0.0, description = "The smallest piece, in elements, a tear may split off; 0 lets rapier choose", group = "tearing" }"#),
            (k::VOLUME_PRESERVATION, r#"{ type = "bool", default = true, description = "Hold the volume each closed piece of the body encloses; an open sheet or a rope encloses none, and a hoop without it caves in", group = "volume" }"#),
            (k::VOLUME_FACTOR, r#"{ type = "float", default = 1.0, min = 0.0, description = "What that volume is held at, as a multiple of the rest volume; above 1 inflates the body", group = "volume" }"#),
            (k::SHAPE_MATCHING, r#"{ type = "bool", default = false, description = "Pull the body back towards the shape it was built in, which is what keeps a jelly a jelly", group = "volume" }"#),
            (k::TENSION_ONLY, r#"{ type = "bool", default = false, description = "Let the edges resist stretching only, so the body folds freely and never pushes itself open", group = "volume" }"#),
            (k::MASS, r#"{ type = "float", default = 1.0, min = 0.0, description = "What the whole body weighs, spread over its particles", group = "particles" }"#),
            (k::PINNED, r#"{ type = "list", of = { type = "int" }, default = [], description = "The particles held where they are, by index: a cloth hangs from these, and `softbody_particles` says how many there are to choose from", group = "particles" }"#),
            (k::PARTICLE_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "How thick the particles are; 0 takes what the layout works out", group = "particles" }"#),
            (k::SELF_CONTACTS, r#"{ type = "bool", default = false, description = "Let the body's own surface collide with itself, which stops a cloth passing through its own fold", group = "particles" }"#),
            (k::ORIENTED, r#"{ type = "bool", default = false, description = "Treat the surface as closed and outward-facing, so its inside holds bodies in instead of pushing them out", group = "particles" }"#),
            (k::LINEAR_DAMPING, r#"{ type = "float", default = 0.0, min = 0.0, description = "Air friction on the particles", group = "particles" }"#),
            (k::GRAVITY_SCALE, r#"{ type = "float", default = 1.0, description = "How much gravity pulls on the particles", group = "particles" }"#),
            (k::SOLVER_ITERATIONS, r#"{ type = "float", default = 0.0, min = 0.0, max = 64.0, description = "Extra solver substeps for this body and everything it touches", group = "particles" }"#),
            (k::PGS_ITERATIONS, r#"{ type = "float", default = 3.0, min = 0.0, max = 64.0, description = "Extra iterations inside each substep, for the same", group = "particles" }"#),
            (k::CAN_SLEEP, r#"{ type = "bool", default = true, description = "Let the body stop being simulated once it settles", group = "particles" }"#),
            (k::DOMINANCE, r#"{ type = "int", default = 0, min = -127, max = 127, description = "Which body wins a contact: a higher one is never pushed by a lower one", group = "particles" }"#),
            (k::COLOR, &format!(r#"{{ type = "color", default = {:?}, description = "What the body is drawn in when its node has nothing of its own to deform, as a cloth or a rope has not", group = "surface" }}"#, cap::DEFAULT_COLOR)),
            (k::FRICTION, r#"{ type = "float", default = 0.5, min = 0.0, description = "Surface friction of the body's collider; 0 is ice", group = "surface" }"#),
            (k::RESTITUTION, r#"{ type = "float", default = 0.0, min = 0.0, max = 1.0, description = "Bounciness of the body's collider", group = "surface" }"#),
        ]),
        crate::collider::shared_group_schema(),
    ]
    .join("\n")
}

crate::shared::softbody::material!(
    rapier = rapier3d,
    material = read_material,
    cell_model = read_cell_model,
    flow = threshold,
    springs = springs
);

/// The collider every soft body meets the world through, which carries the
/// same surface and filtering rows a `collider3d` does.
fn surface_collider(params: &toml::Value) -> ColliderBuilder {
    let builder = ColliderBuilder::ball(1.0)
        .friction(scalar::real(v::f(params, k::FRICTION, 0.5)))
        .restitution(scalar::real(v::f(params, k::RESTITUTION, 0.0)));
    crate::collider::with_groups(builder, params)
}

/// The rows every layout shares, applied after the generator has laid the
/// particles out.
fn with_settings(mut builder: SoftBodyBuilder, params: &toml::Value) -> SoftBodyBuilder {
    builder = builder
        .material(read_material(params))
        .cell_model(read_cell_model(params))
        .volume_preservation(v::boolean(params, k::VOLUME_PRESERVATION, true))
        .volume_factor(scalar::real(v::f(params, k::VOLUME_FACTOR, 1.0)))
        .shape_matching(v::boolean(params, k::SHAPE_MATCHING, false))
        .self_contacts(v::boolean(params, k::SELF_CONTACTS, false))
        .linear_damping(scalar::real(v::f(params, k::LINEAR_DAMPING, 0.0)))
        .gravity_scale(scalar::real(v::f(params, k::GRAVITY_SCALE, 1.0)))
        .additional_solver_iterations(v::f(params, k::SOLVER_ITERATIONS, 0.0).max(0.0) as usize)
        .additional_pgs_iterations(v::f(params, k::PGS_ITERATIONS, 3.0).max(0.0) as usize)
        .can_sleep(v::boolean(params, k::CAN_SLEEP, true))
        .solver(if v::text(params, k::SOLVER, w::CONSTRAINTS) == w::FEM {
            SoftBodySolver::Fem
        } else {
            SoftBodySolver::Constraints
        })
        .skin_collision(v::boolean(params, k::SKIN_COLLISION, false))
        .surface_collider(surface_collider(params));
    let mass = v::f(params, k::MASS, 1.0);
    if mass > 0.0 {
        builder = builder.mass(scalar::real(mass));
    }
    let radius = v::f(params, k::PARTICLE_RADIUS, 0.0);
    if radius > 0.0 {
        builder = builder.particle_radius(scalar::real(radius));
    }
    if v::boolean(params, k::ORIENTED, false) {
        builder = builder.oriented(true);
    }
    if v::boolean(params, k::TENSION_ONLY, false) {
        builder = builder.tension_only();
    }
    builder = builder.pinned_particles(v::indices(params, k::PINNED));
    let settings = crate::rapier3d::prelude::SoftBodyParticleSettings {
        dominance_group: v::f(params, k::DOMINANCE, 0.0).clamp(-127.0, 127.0) as i8,
        ..builder.particle_settings
    };
    builder.particle_settings(settings)
}

/// The mesh a layout is built out of, in world space.
fn source_mesh(
    eng: &Engine,
    params: &toml::Value,
    pose: scalar::Pose,
) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
    let reference = params
        .get(k::MESH)
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("this soft-body kind needs a `mesh` asset"))?;
    let definition =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)?;
    let mesh = balaur_core::mesh::load_from(eng, &definition)?;
    let points = mesh
        .positions
        .iter()
        .map(|p| pose * scalar::v3a(*p))
        .collect();
    Ok((points, mesh.indices.clone()))
}

/// Lay the particles out the way `kind` asks, in world space around `pose`.
fn build_layout(
    eng: &Engine,
    params: &toml::Value,
    pose: scalar::Pose,
    kind: &str,
) -> Result<SoftBodyBuilder> {
    let at = pose.translation;
    let local = |key: &str, default: [f32; 3]| pose * scalar::v3a(v::vec3(params, key, default));
    let along =
        |key: &str, default: [f32; 3]| pose.rotation * scalar::v3a(v::vec3(params, key, default));
    let cells = v::vec3(params, k::CELLS, [4.0, 4.0, 4.0]);
    let axis = |i: usize| cap::particles_along(cells[i]);
    let ring = |around: f32| f64::from(around.max(cap::MIN_RING_PARTICLES)).floor();
    cap::refuse_past_cap(
        match kind {
            w::SOFT_CUBOID => axis(0) * axis(1) * axis(2),
            w::CLOTH => axis(0) * axis(1),
            w::CLOTH_TUBE => ring(cells[0]) * axis(1),
            w::ROPE_SOFT => cap::particle_count(params, cap::MIN_CHAIN_PARTICLES),
            _ => 0.0,
        },
        kind,
    )?;
    let builder = match kind {
        // Rapier counts the particles along an axis; the schema counts the
        // cells between them, which is the number an author means.
        w::SOFT_CUBOID => SoftBodyBuilder::cuboid(
            at,
            scalar::v3a(v::vec3(params, k::HALF_EXTENTS, [0.5, 0.5, 0.5])),
            axis(0) as usize,
            axis(1) as usize,
            axis(2) as usize,
        ),
        w::SPHERE => SoftBodyBuilder::sphere(
            at,
            scalar::real(v::f(params, k::RADIUS, 0.5)),
            v::f(params, k::SUBDIVISIONS, 2.0).clamp(0.0, 6.0) as usize,
        ),
        w::CLOTH => {
            let (nu, nv) = (axis(0) as usize, axis(1) as usize);
            let size = v::vec3(params, k::SIZE, [1.0, 0.0, 1.0]);
            // The sheet is spanned from a corner, so the node's own position
            // is its middle like every other layout's.
            let du = pose.rotation * scalar::v3(size[0], 0.0, 0.0);
            let dv = pose.rotation * scalar::v3(0.0, 0.0, size[2]);
            let origin = at - (du + dv) * 0.5;
            let mut built = SoftBodyBuilder::cloth(
                origin,
                du / (nu.max(2) - 1) as Real,
                dv / (nv.max(2) - 1) as Real,
                nu,
                nv,
            );
            // Spanned +x then +z, rapier winds the sheet's front underneath
            // it (`du x dv` is -y); a flat cloth is looked at from above.
            for triangle in &mut built.surface {
                triangle.swap(1, 2);
            }
            built
        }
        w::CLOTH_TUBE => SoftBodyBuilder::cloth_tube(
            at,
            along(k::AXIS, [0.0, 1.0, 0.0]),
            scalar::real(v::f(params, k::RADIUS, 0.5)),
            scalar::real(v::f(params, k::RADIUS, 0.5)),
            ring(cells[0]) as usize,
            axis(1) as usize,
        ),
        w::ROPE_SOFT => SoftBodyBuilder::rope(
            local(k::A, [0.0, 0.0, 0.0]),
            local(k::B, [0.0, -1.0, 0.0]),
            cap::particle_count(params, cap::MIN_CHAIN_PARTICLES) as usize,
        ),
        // The approximate tetrahedrization: the mesh is covered with cells of
        // `cell_size` and the body is those cells.
        w::VOLUMETRIC => {
            let (points, indices) = source_mesh(eng, params, pose)?;
            let size = scalar::real(v::f(params, k::CELL_SIZE, 0.25));
            cap::refuse_past_cap(cap::grid_particles(&extents(&points), size), kind)?;
            let built = if v::boolean(params, k::SKIN, false) {
                SoftBodyBuilder::volumetric_skinned(&points, &indices, size)
            } else {
                SoftBodyBuilder::volumetric(&points, &indices, size)
            };
            built.ok_or_else(|| {
                anyhow!("that mesh encloses nothing at a cell size of {size}: it has to be closed, and big enough to hold a cell")
            })?
        }
        w::SURFACE_MESH => {
            let (points, indices) = source_mesh(eng, params, pose)?;
            SoftBodyBuilder::trimesh(points, indices)
                .ok_or_else(|| anyhow!("that mesh has no triangles to make a soft surface of"))?
        }
        other => return Err(anyhow!("unknown soft-body kind '{other}'")),
    };
    cap::refuse_past_cap(builder.positions.len() as f64, kind)?;
    Ok(with_settings(builder, params))
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

/// Build the node's soft body, replacing whatever it had.
pub(crate) fn apply_softbody(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let pose = crate::node_pose(eng, entity)?;
    let kind = v::text(params, k::KIND, w::SOFT_CUBOID).to_string();
    let builder =
        build_layout(eng, params, pose, &kind)?.user_data(u128::from(entity.to_bits().get()));
    remove_softbody(eng, entity);
    {
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let handle = state.world.soft_bodies.insert(
            builder,
            &mut state.world.bodies,
            &mut state.world.colliders,
        );
        state.soft_bodies.insert(entity, handle);
        state.soft_params.insert(entity, params.clone());
    }
    // The node draws from the solver, so it has geometry to draw before the
    // first step rather than a frame of nothing.
    write_solved_mesh(eng, entity);
    Ok(())
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
    world.soft_bodies.remove(
        handle,
        &mut world.islands,
        &mut world.bodies,
        &mut world.colliders,
        &mut world.impulse_joints,
        &mut world.multibody_joints,
    );
}

/// What the component reads back: what was authored, under the few numbers
/// the solver is the authority on.
pub(crate) fn get_softbody_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let authored = state.soft_params.get(&entity)?.clone();
    let handle = *state.soft_bodies.get(&entity)?;
    let body = state.world.soft_bodies.get(handle)?;
    let toml::Value::Table(mut table) = authored else {
        return None;
    };
    let number = |value: Real| toml::Value::Float(f64::from(scalar::f32_of(value)));
    // Not the mass, whose sum over the particles rounds off what was asked
    // for, nor the radius, whose 0 means "worked out from the layout".
    table.insert(k::VOLUME_FACTOR.into(), number(body.volume_factor()));
    let solver = match body.solver() {
        SoftBodySolver::Fem => w::FEM,
        SoftBodySolver::Constraints => w::CONSTRAINTS,
    };
    table.insert(k::SOLVER.into(), toml::Value::String(solver.into()));
    table.insert(
        k::VOLUME_PRESERVATION.into(),
        toml::Value::Boolean(body.volume_preservation_enabled()),
    );
    Some(toml::Value::Table(table))
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
        let color = drawn_color(state.soft_params.get(&entity));
        let Some(body) = state.world.soft_bodies.get(handle) else {
            return;
        };
        match body.collision_mesh() {
            Some(mesh) => (
                mesh.vertex_positions(body)
                    .map(|p| scalar::a3(inverse * p))
                    .collect::<Vec<_>>(),
                mesh.indices().to_vec(),
                color,
            ),
            // A body with no collider still draws: its boundary is what a
            // generator laid out, and the particles are its vertices.
            None => (
                body.particle_positions()
                    .map(|p| scalar::a3(inverse * p))
                    .collect(),
                body.boundary().to_vec(),
                color,
            ),
        }
    };
    let mut world = eng.world_mut();
    if let Ok(mut solved) = world.get::<&mut balaur_core::mesh::SolvedMesh>(entity) {
        solved.update(positions, indices);
        solved.color = color;
        return;
    }
    let mut solved = balaur_core::mesh::SolvedMesh::default();
    solved.update(positions, indices);
    solved.color = color;
    let _ = world.insert_one(entity, solved);
}

/// The colour a body draws in when its node has nothing of its own.
pub(crate) fn drawn_color(params: Option<&toml::Value>) -> [f32; 4] {
    params.map_or(cap::DEFAULT_COLOR, |params| {
        v::color(params, k::COLOR, cap::DEFAULT_COLOR)
    })
}

/// Hand every soft body's positions over, which is what the step does once
/// rapier has finished with them.
pub(crate) fn write_every_solved_mesh(eng: &Engine) {
    let entities: Vec<Entity> = {
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.soft_bodies.keys().copied().collect()
    };
    for entity in entities {
        write_solved_mesh(eng, entity);
    }
}

pub(crate) fn register_softbody_component(reg: &mut Registry<'_>) {
    let schema = [shape_schema(), shared_softbody_schema()].join("\n");
    reg.register_component(
        c::SOFTBODY_3D,
        ComponentDef {
            warnings: Some(Box::new(softbody_warnings)),
            doc: "A deformable 3D body: particles linked by elastic constraints, laid out by `kind` and made of what the material rows say. The node is drawn from the solver's positions.",
            schema: ComponentDef::parse_schema(c::SOFTBODY_3D, &schema),
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &[],
            apply: Box::new(apply_softbody),
            remove: Box::new(|eng, entity| {
                remove_softbody(eng, entity);
                let _ = eng
                    .world_mut()
                    .remove_one::<balaur_core::mesh::SolvedMesh>(entity);
                Ok(())
            }),
            get: Box::new(get_softbody_params),
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
        .map_or(0.0, |params| v::f(params, k::PARTICLE_RADIUS, 0.0));
    let body = state
        .soft_bodies
        .get(&entity)
        .and_then(|&handle| state.world.soft_bodies.get(handle));
    let Some(body) = body.filter(|_| authored <= 0.0) else {
        return Vec::new();
    };
    let points: Vec<Vector> = body.particle_positions().collect();
    let widest = extents(&points).into_iter().fold(0.0, f32::max);
    cap::hovering(scalar::f32_of(body.particle_radius()), widest)
        .into_iter()
        .collect()
}

/// What a script may ask a soft body, and the two things it may do to one.
pub(crate) fn install_softbody_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_softbody", &[c::SOFTBODY_3D], "", "Build the node's soft body from a `softbody3d` table: `kind`, the shape rows, and the material rows."),
        ("softbody_particles", &[c::SOFTBODY_3D], "", "How many particles the body ended up with, which a generator decides rather than the author."),
        ("softbody_position", &[c::SOFTBODY_3D], "", "Where one particle is, in world space."),
        ("softbody_volume", &[c::SOFTBODY_3D], "", "How much space the body encloses right now, against `softbody_rest_volume` for how far it is squeezed."),
        ("softbody_rest_volume", &[c::SOFTBODY_3D], "", "How much it encloses at rest."),
        ("softbody_center", &[c::SOFTBODY_3D], "", "The body's centre of mass, which is where it is when a deformable body has no one position."),
        ("pin_particle", &[c::SOFTBODY_3D], "", "Hold one particle where it is, which is how a cloth hangs from a hook."),
    ]);
    m.function(
        "set_softbody",
        |eng: &Engine, (node, params): (NodeId, balaur_script::Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            apply_softbody(eng, entity_of(node)?, &params)
        },
    );
    m.function("softbody_particles", |eng: &Engine, node: NodeId| {
        with_softbody(eng, node, |body| {
            Ok(i64::try_from(body.num_particles()).unwrap_or(i64::MAX))
        })
    });
    m.function(
        "softbody_position",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_softbody(eng, node, |body| {
                let index = usize::try_from(index)
                    .ok()
                    .filter(|i| *i < body.num_particles())
                    .ok_or_else(|| anyhow!("this body has no particle {index}"))?;
                Ok(balaur_script::Value::Vec3(scalar::a3(
                    body.particle_position(index),
                )))
            })
        },
    );
    m.function("softbody_volume", |eng: &Engine, node: NodeId| {
        with_softbody(eng, node, |body| Ok(scalar::f32_of(body.volume())))
    });
    m.function("softbody_rest_volume", |eng: &Engine, node: NodeId| {
        with_softbody(eng, node, |body| Ok(scalar::f32_of(body.rest_volume())))
    });
    m.function("softbody_center", |eng: &Engine, node: NodeId| {
        with_softbody(eng, node, |body| {
            Ok(balaur_script::Value::Vec3(scalar::a3(
                body.center_of_mass(),
            )))
        })
    });
    m.function(
        "pin_particle",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            let entity = entity_of(node)?;
            let state = eng.resource::<PhysicsState3d>();
            let mut state = state.borrow_mut();
            let state = &mut *state;
            let handle = *state
                .soft_bodies
                .get(&entity)
                .ok_or_else(|| anyhow!("node has no soft body"))?;
            let body = state
                .world
                .soft_bodies
                .get_mut(handle)
                .ok_or_else(|| anyhow!("node has no soft body"))?;
            let index = usize::try_from(index)
                .ok()
                .filter(|i| *i < body.num_particles())
                .ok_or_else(|| anyhow!("this body has no particle {index}"))?;
            let at = body.particle_position(index);
            body.set_particle_kinematic_target(index, at);
            Ok(())
        },
    );
}

fn with_softbody<T>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&crate::rapier3d::prelude::SoftBody) -> Result<T>,
) -> Result<T> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let handle = *state
        .soft_bodies
        .get(&entity)
        .ok_or_else(|| anyhow!("node has no soft body"))?;
    let body = state
        .world
        .soft_bodies
        .get(handle)
        .ok_or_else(|| anyhow!("node has no soft body"))?;
    f(body)
}

/// The handle type the snapshot carries per node.
pub(crate) type SoftRef3d = SoftBodyHandle;
