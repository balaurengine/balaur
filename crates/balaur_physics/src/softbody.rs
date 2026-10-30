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
use crate::rapier3d::prelude::{SoftBodyBuilder, SoftBodyHandle, SoftBodySolver};
use crate::scalar::{self, Real, Vector};
use crate::shared::softbody::{self as cap, Family};
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

pub(crate) mod runtime;
mod wire;

/// The shape rows: how a body's particles and elements are laid out, and
/// the numbers each layout reads.
fn shape_schema() -> String {
    v::schema(&[
        (
            k::SIZE,
            r#"{ type = "vec3", default = [1.0, 1.0, 1.0], description = "Whole size along each axis: a box's block, or a cloth's sheet along x and z", group = "shape" }"#,
        ),
        (
            k::CELLS,
            r#"{ type = "vec3", default = [4.0, 4.0, 4.0], description = "How many cells along each axis, for box; a cloth reads the first two, and a cloth_tube reads them as particles around and cells along", group = "shape" }"#,
        ),
        (
            k::RADIUS,
            r#"{ type = "float", default = 0.5, min = 0.001, description = "Radius, for sphere and cloth_tube: a tube's at its start", group = "shape" }"#,
        ),
        (
            k::END_RADIUS,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "A cloth_tube's radius at its end; 0 takes radius", group = "shape" }"#,
        ),
        (
            k::AXIS,
            r#"{ type = "vec3", default = [0.0, 1.0, 0.0], description = "The segment a cloth_tube is wrapped around, relative to the node", group = "shape" }"#,
        ),
        (
            k::SUBDIVISIONS,
            r#"{ type = "int", default = 2, min = 0, max = 6, description = "How many times a sphere's icosahedron is refined; each level quadruples the triangles", group = "shape" }"#,
        ),
        (
            k::PARTICLE_COUNT,
            &format!(
                r#"{{ type = "int", default = {}, min = {}, description = "How many particles a rope is made of", group = "shape" }}"#,
                cap::DEFAULT_PARTICLES as i64,
                cap::MIN_CHAIN_PARTICLES as i64
            ),
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
            k::MESH,
            &format!(
                r#"{{ type = "asset", asset = "{}", default = "", description = "Geometry for a triangle_mesh or volumetric body, and the skin a body with cells carries when skin is on", group = "shape" }}"#,
                balaur_core::mesh::MESH_ASSET_TYPE
            ),
        ),
        (
            k::SKIN,
            r#"{ type = "bool", default = false, description = "Keep the mesh asset as the drawn surface and let the cells carry it, so a detail the cells cannot resolve survives; any kind with cells and a mesh takes it", group = "shape" }"#,
        ),
    ])
}

/// How a volumetric body's cells cover its mesh.
fn fill_schema() -> String {
    let solid = w::SOLID;
    let fills = v::options(w::FILL_MODES);
    v::schema(&[
        (
            k::FILL,
            &format!(
                r#"{{ type = "enum", default = "{solid}", options = [{fills}], description = "What a volumetric body's cells cover: the mesh's whole inside, which needs a closed mesh, or a crust along its surface alone, which does not", group = "shape" }}"#
            ),
        ),
        (
            k::CELL_SIZE,
            r#"{ type = "float", default = 0.25, min = 0.001, description = "How big one tetrahedron is when a volumetric body fills a mesh; smaller is finer, slower and stiffer to tear", group = "shape" }"#,
        ),
        (
            k::BOUNDARY_SUBDIVISIONS,
            r#"{ type = "int", default = 0, min = 0, max = 4, description = "How many times a solid fill's cells along the mesh's surface are halved, finer than cell_size", group = "shape" }"#,
        ),
        (
            k::SMOOTHING,
            r#"{ type = "int", default = 0, min = 0, description = "How many passes pull a solid fill's blocky boundary towards the mesh; 0 leaves it on the cell lattice", group = "shape" }"#,
        ),
        (
            k::SMOOTHING_GUARD,
            r#"{ type = "float", default = 0.15, min = 0.0, description = "How close to the mesh smoothing may pull the boundary, as a fraction of the cell size", group = "shape" }"#,
        ),
    ])
}

/// A custom body's lists: its particles and the elements over them.
fn custom_schema() -> String {
    v::schema(&[
        (
            k::POINTS,
            r#"{ type = "list", of = { type = "vec3" }, default = [], description = "A custom body's particles, relative to the node", group = "shape" }"#,
        ),
        (
            k::CELL_INDICES,
            r#"{ type = "list", of = { type = "list", of = { type = "int" } }, default = [], description = "A custom body's tetrahedra, four particle indices each, positively oriented", group = "shape" }"#,
        ),
        (
            k::EDGE_INDICES,
            r#"{ type = "list", of = { type = "list", of = { type = "int" } }, default = [], description = "A custom body's structural edges, each a pair of particle indices; empty takes the cells' edges", group = "shape" }"#,
        ),
        (
            k::BEND_EDGE_INDICES,
            r#"{ type = "list", of = { type = "list", of = { type = "int" } }, default = [], description = "A custom body's bending edges, each a pair of particle indices", group = "shape" }"#,
        ),
        (
            k::DIHEDRAL_INDICES,
            r#"{ type = "list", of = { type = "list", of = { type = "int" } }, default = [], description = "A custom body's bending hinges between two triangles: the shared edge's two particles, then the two opposite ones", group = "shape" }"#,
        ),
        (
            k::SURFACE_INDICES,
            r#"{ type = "list", of = { type = "list", of = { type = "int" } }, default = [], description = "A custom body's outward triangles, three particle indices each: what holds its volume and what it collides through; empty takes the cells' boundary", group = "shape" }"#,
        ),
        (
            k::WIRE_INDICES,
            r#"{ type = "list", of = { type = "list", of = { type = "int" } }, default = [], description = "A custom body's segments it collides through when it has no surface, as a rope does, each a pair of particle indices", group = "shape" }"#,
        ),
    ])
}

crate::shared::softbody::material!(
    rapier = rapier3d,
    material = read_material,
    cell_model = read_cell_model,
    flow = threshold,
    springs = springs
);

cap::regions!(rapier = rapier3d, add = add_regions);
cap::collision_mesh!(
    rapier = rapier3d,
    bind = bind_collision_mesh,
    shape = |vertices, indices| {
        use crate::rapier3d::geometry::{SharedShape, TriMeshFlags};
        SharedShape::trimesh_with_flags(vertices, indices, TriMeshFlags::DEFORMABLE)
            .map_err(|why| anyhow!("`{}`: {why}", k::COLLISION_MESH))
    }
);

cap::surface!(
    rapier = rapier3d,
    layers = crate::collider::with_groups,
    events = crate::collider::with_events
);

/// Which solver runs the elasticity.
fn solver_of(params: &toml::Value) -> SoftBodySolver {
    if v::text(params, k::SOLVER, w::CONSTRAINTS) == w::FEM {
        SoftBodySolver::Fem
    } else {
        SoftBodySolver::Constraints
    }
}

cap::patch_in_place!(
    patch_body,
    crate::rapier3d::pipeline::PhysicsWorld,
    SoftBodyHandle,
    read_material,
    solver_of
);

/// The rows every layout shares, applied after the generator has laid the
/// particles out.
fn with_settings(
    mut builder: SoftBodyBuilder,
    params: &toml::Value,
    materials: &cap::Materials,
) -> Result<SoftBodyBuilder> {
    let made = &materials.body.0;
    let seams = cap::seams(params, builder.positions.len())?;
    if !seams.is_empty() {
        builder = builder.add_edges(seams);
    }
    builder = builder
        .material(read_material(params, made))
        .cell_model(read_cell_model(made))
        .volume_preservation(v::boolean(made, k::VOLUME_PRESERVATION, true))
        .volume_factor(scalar::real(v::f(made, k::VOLUME_FACTOR, 1.0)))
        .self_contacts(v::boolean(params, k::SELF_COLLISION, false))
        .linear_damping(scalar::real(v::f(params, k::LINEAR_DAMPING, 0.0)))
        .gravity_scale(scalar::real(v::f(params, k::GRAVITY_SCALE, 1.0)))
        .additional_solver_iterations(v::f(params, k::SOLVER_SUBSTEPS, 0.0).max(0.0) as usize)
        .additional_pgs_iterations(v::f(params, k::SOLVER_ITERATIONS, 3.0).max(0.0) as usize)
        .can_sleep(v::boolean(params, k::CAN_SLEEP, true))
        .solver(solver_of(params))
        .skin_collision(v::boolean(params, k::SKIN_COLLISION, false))
        .surface_collider(surface_collider(params, &materials.surface));
    let mass = v::f(params, k::MASS, 1.0);
    if mass > 0.0 {
        builder = builder.mass(scalar::real(mass));
    }
    let radius = v::f(params, k::PARTICLE_RADIUS, 0.0);
    if radius > 0.0 {
        builder = builder.particle_radius(scalar::real(radius));
    }
    if let Some(oriented) = cap::oriented(params) {
        builder = builder.oriented(oriented);
    }
    // After the generator, which picks its own for a mesh or a polyline.
    if let Some(matching) = cap::shape_matching(made) {
        builder = builder.shape_matching(matching);
    }
    if cap::tension(params) == cap::Tension::All {
        builder = builder.tension_only();
    }
    builder = builder.pinned_particles(v::indices(params, k::PINNED_PARTICLES));
    let settings = crate::rapier3d::prelude::SoftBodyParticleSettings {
        dominance_group: v::f(params, k::DOMINANCE, 0.0).clamp(-127.0, 127.0) as i8,
        ..builder.particle_settings
    };
    builder = builder.particle_settings(settings);
    let rows = cap::edge_rows(
        params,
        builder.positions.len(),
        &builder.edges,
        &builder.bend_edges,
    )?;
    // After `mass`, which spreads one total over the particles evenly.
    if !rows.masses.is_empty() {
        builder = builder.masses(rows.masses.iter().map(|m| scalar::real(*m)).collect());
    }
    if !rows.tear.is_empty() {
        builder =
            builder.edge_tear_resistance(rows.tear.iter().map(|(i, r)| (*i, scalar::real(*r))));
    }
    for (edge, hz, damping) in rows.springs {
        let spring = crate::rapier3d::prelude::SpringCoefficients::new(
            scalar::real(hz),
            scalar::real(damping),
        );
        builder.edge_softness.push((edge, spring));
    }
    builder.tension_only_edges.extend(rows.tension_only);
    if !v::boolean(params, k::COLLIDES, true) {
        builder = builder.no_surface_collider();
    }
    Ok(builder)
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
    mesh_in_world(eng, reference, pose)
}

/// The `collision_mesh`, in world space, when there is one.
fn collision_mesh(
    eng: &Engine,
    params: &toml::Value,
    pose: scalar::Pose,
) -> Result<Option<cap::Mesh<Vector>>> {
    let reference = v::text(params, k::COLLISION_MESH, "");
    if reference.is_empty() {
        return Ok(None);
    }
    mesh_in_world(eng, reference, pose).map(Some)
}

fn mesh_in_world(
    eng: &Engine,
    reference: &str,
    pose: scalar::Pose,
) -> Result<(Vec<Vector>, Vec<[u32; 3]>)> {
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
    materials: &cap::Materials,
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
            w::BOX => axis(0) * axis(1) * axis(2),
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
        w::BOX => SoftBodyBuilder::cuboid(
            at,
            scalar::v3a(v::vec3(params, k::SIZE, [1.0, 1.0, 1.0])) / 2.0,
            axis(0) as usize,
            axis(1) as usize,
            axis(2) as usize,
        ),
        w::SPHERE => SoftBodyBuilder::sphere(
            at,
            scalar::real(v::f(params, k::RADIUS, 0.5)),
            v::f(params, k::SUBDIVISIONS, 2.0).clamp(0.0, 6.0) as usize,
        ),
        w::CLOTH => cloth(
            params,
            &materials.body.0,
            pose,
            axis(0) as usize,
            axis(1) as usize,
        ),
        w::CLOTH_TUBE => {
            let start = v::f(params, k::RADIUS, 0.5);
            let end = match v::f(params, k::END_RADIUS, 0.0) {
                given if given > 0.0 => given,
                _ => start,
            };
            SoftBodyBuilder::cloth_tube(
                at,
                along(k::AXIS, [0.0, 1.0, 0.0]),
                scalar::real(start),
                scalar::real(end),
                ring(cells[0]) as usize,
                axis(1) as usize,
            )
        }
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
            SoftBodyBuilder::volumetric_with(&points, &indices, &volume_parameters(params, size))
                .ok_or_else(|| {
                    anyhow!("that mesh encloses nothing at a cell size of {size}: it has to be closed, and big enough to hold a cell")
                })?
        }
        w::TRIANGLE_MESH => {
            let (points, indices) = source_mesh(eng, params, pose)?;
            SoftBodyBuilder::trimesh(points, indices)
                .ok_or_else(|| anyhow!("that mesh has no triangles to make a soft surface of"))?
        }
        w::CUSTOM => custom(params, pose)?,
        other => return Err(anyhow!("unknown soft-body kind '{other}'")),
    };
    cap::refuse_past_cap(builder.positions.len() as f64, kind)?;
    let builder = skinned(eng, params, pose, kind, builder)?;
    with_settings(builder, params, materials)
}

/// A sheet spanned across x and z around the node, `nu` by `nv` particles,
/// with springs of its own along the warp, the weft and the shear when any
/// woven key is set.
fn cloth(
    params: &toml::Value,
    made: &toml::Value,
    pose: scalar::Pose,
    nu: usize,
    nv: usize,
) -> SoftBodyBuilder {
    let at = pose.translation;
    let size = v::vec3(params, k::SIZE, [1.0, 1.0, 1.0]);
    // The sheet is spanned from a corner, so the node's own position
    // is its middle like every other layout's.
    let du = pose.rotation * scalar::v3(size[0], 0.0, 0.0);
    let dv = pose.rotation * scalar::v3(0.0, 0.0, size[2]);
    let origin = at - (du + dv) * 0.5;
    let (step_u, step_v) = (du / (nu.max(2) - 1) as Real, dv / (nv.max(2) - 1) as Real);
    let woven = [k::WARP_HZ, k::WEFT_HZ, k::SHEAR_HZ].map(|key| v::f(made, key, 0.0));
    let damped =
        [k::WARP_DAMPING, k::WEFT_DAMPING, k::SHEAR_DAMPING].map(|key| v::f(made, key, 0.0));
    let mut built = if woven.iter().chain(&damped).any(|f| *f > 0.0) {
        let edge = v::f(made, k::EDGE_HZ, 30.0);
        let edge_damping = v::f(made, k::EDGE_DAMPING, 1.0);
        let spring = |i: usize| {
            let or = |own: f32, fallback: f32| if own > 0.0 { own } else { fallback };
            crate::rapier3d::prelude::SpringCoefficients::new(
                scalar::real(or(woven[i], edge)),
                scalar::real(or(damped[i], edge_damping)),
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
const MAX_SUBDIVISIONS: f32 = 4.0;

/// How a volumetric body fills its mesh.
fn volume_parameters(
    params: &toml::Value,
    size: Real,
) -> crate::rapier3d::parry::transformation::VolumeMeshParameters {
    use crate::rapier3d::parry::transformation::{MeshEnclosure, VolumeMeshParameters};
    let mut out = VolumeMeshParameters::new(size);
    out.enclosure = if v::text(params, k::FILL, w::SOLID) == w::SURFACE {
        MeshEnclosure::Crust
    } else {
        MeshEnclosure::Cover
    };
    out.cover_smoothing = v::f(params, k::SMOOTHING, 0.0).max(0.0) as u32;
    out.cover_guard = scalar::real(v::f(params, k::SMOOTHING_GUARD, 0.15).max(0.0));
    out.cover_subdivisions =
        v::f(params, k::BOUNDARY_SUBDIVISIONS, 0.0).clamp(0.0, MAX_SUBDIVISIONS) as u32;
    out
}

/// A body of the particles and elements the author listed.
fn custom(params: &toml::Value, pose: scalar::Pose) -> Result<SoftBodyBuilder> {
    let points: Vec<Vector> = cap::points::<3>(params, k::POINTS)
        .into_iter()
        .map(|p| pose * scalar::v3a(p))
        .collect();
    let n = points.len();
    let edges = cap::tuples::<2>(params, k::EDGE_INDICES, n)?;
    let bend = cap::tuples::<2>(params, k::BEND_EDGE_INDICES, n)?;
    let surface = cap::tuples::<3>(params, k::SURFACE_INDICES, n)?;
    let wire = cap::tuples::<2>(params, k::WIRE_INDICES, n)?;
    let cells = cap::tuples::<4>(params, k::CELL_INDICES, n)?;
    let dihedrals = cap::tuples::<4>(params, k::DIHEDRAL_INDICES, n)?;
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

/// The `mesh` asset held by the cells as the body's skin, when `skin` asks
/// for one, the body has cells, and its particles are not the mesh's own.
fn skinned(
    eng: &Engine,
    params: &toml::Value,
    pose: scalar::Pose,
    kind: &str,
    builder: SoftBodyBuilder,
) -> Result<SoftBodyBuilder> {
    let named = params
        .get(k::MESH)
        .and_then(toml::Value::as_str)
        .is_some_and(|reference| !reference.is_empty());
    if !v::boolean(params, k::SKIN, false)
        || !named
        || builder.cells.is_empty()
        || kind == w::TRIANGLE_MESH
    {
        return Ok(builder);
    }
    let (points, indices) = source_mesh(eng, params, pose)?;
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
pub(crate) fn apply_softbody(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let materials = cap::Materials::of(eng, params)?;
    if patched(eng, entity, params, &materials) {
        return Ok(());
    }
    build_softbody(eng, entity, params, &materials)
}

/// Build the node's soft body from rest, replacing whatever it had: what a
/// script's `set_softbody` asks for even when nothing changed.
fn build_softbody(
    eng: &Engine,
    entity: Entity,
    params: &toml::Value,
    materials: &cap::Materials,
) -> Result<()> {
    let pose = crate::node_pose(eng, entity)?;
    let kind = v::text(params, k::KIND, w::BOX).to_string();
    let builder = build_layout(eng, params, materials, pose, &kind)?
        .user_data(u128::from(entity.to_bits().get()));
    let collision = collision_mesh(eng, params, pose)?;
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
        state.soft_params.insert(entity, params.clone());
        let added = add_regions(&mut state.world, handle, params).and_then(|()| {
            bind_collision_mesh(&mut state.world, handle, params, collision, materials)
        });
        stamp_colliders(&mut state.world, handle, entity);
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
fn patched(eng: &Engine, entity: Entity, params: &toml::Value, materials: &cap::Materials) -> bool {
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let (Some(built), Some(&handle)) = (
        state.soft_params.get(&entity),
        state.soft_bodies.get(&entity),
    ) else {
        return false;
    };
    if !cap::only_in_place(built, params) || state.world.soft_bodies.get(handle).is_none() {
        return false;
    }
    patch_body(&mut state.world, handle, params, materials);
    state.soft_params.insert(entity, params.clone());
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
    // Not the mass, whose sum over the particles rounds off what was asked
    // for, nor the radius, whose 0 means "worked out from the layout".
    let solver = match body.solver() {
        SoftBodySolver::Fem => w::FEM,
        SoftBodySolver::Constraints => w::CONSTRAINTS,
    };
    table.insert(k::SOLVER.into(), toml::Value::String(solver.into()));
    table.insert(k::ENABLED.into(), toml::Value::Boolean(body.is_enabled()));
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
                    let radius = scalar::f32_of(body.particle_radius());
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

pub(crate) fn register_softbody_component(reg: &mut Registry<'_>) {
    let schema = [
        cap::primary_schema(w::SOFT_KINDS, w::BOX),
        shape_schema(),
        fill_schema(),
        cap::seams_schema(),
        custom_schema(),
        cap::particles_schema(),
        cap::collision_schema(),
        cap::filtering_schema(),
        crate::collider::events_schema(),
        cap::overrides_schema(),
        cap::tearing_schema(),
        cap::solver_schema(),
    ]
    .join("\n");
    reg.register_component(
        c::SOFTBODY_3D,
        ComponentDef {
            events: crate::vocabulary::hook::SOFT_BODY,
            warnings: Some(Box::new(softbody_warnings)),
            doc: "A deformable 3D body: particles linked by elastic constraints, laid out by `kind` and made of what the material rows say. The node is drawn from the solver's positions.",
            schema: ComponentDef::parse_schema(c::SOFTBODY_3D, &schema),
            // It draws the node, so the tree's Rendering filter finds it too.
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::PHYSICS,
                balaur_core::components::tag::RENDER,
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

/// What a script may ask a soft body and do to it; the calls on particles,
/// forces and edges are `install_runtime`.
pub(crate) fn install_softbody_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_softbody", &[c::SOFTBODY_3D], "", "Build the node's soft body from a `softbody3d` table: `kind`, the shape rows, and the material rows."),
        ("softbody_volume", &[c::SOFTBODY_3D], "", "How much space the body and every piece torn off it enclose right now, against `softbody_rest_volume` for how far it is squeezed."),
        ("softbody_rest_volume", &[c::SOFTBODY_3D], "", "How much they enclose at rest."),
        ("softbody_center", &[c::SOFTBODY_3D], "", "The centre of mass of the body and its pieces, which is where it is when a deformable body has no one position."),
        ("softbody_dihedrals", &[c::SOFTBODY_3D], "() -> [[int]]", "Every bending hinge between two surface triangles, as four particle indices: the shared edge, then the two opposite particles."),
        ("softbody_dihedral", &[c::SOFTBODY_3D], "(index: int) -> map", "Everything one hinge holds: `#{ particles, rest_angle, initial_rest_angle, plastic_set }`, angles in radians; `plastic_set` is how far its rest angle has flowed from the one it was built with."),
    ]);
    m.function(
        "set_softbody",
        |eng: &Engine, (node, params): (NodeId, balaur_script::Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            let materials = cap::Materials::of(eng, &params)?;
            build_softbody(eng, entity_of(node)?, &params, &materials)
        },
    );
    m.function("softbody_volume", |eng: &Engine, node: NodeId| {
        with_family(eng, node, |bodies| {
            Ok(scalar::f32_of(bodies.iter().map(|(b, _)| b.volume()).sum()))
        })
    });
    m.function("softbody_rest_volume", |eng: &Engine, node: NodeId| {
        with_family(eng, node, |bodies| {
            Ok(scalar::f32_of(
                bodies.iter().map(|(b, _)| b.rest_volume()).sum(),
            ))
        })
    });
    m.function("softbody_center", |eng: &Engine, node: NodeId| {
        with_family(eng, node, |bodies| {
            let mass: Real = bodies.iter().map(|(b, _)| b.mass()).sum();
            let weighted: Vector = bodies
                .iter()
                .map(|(b, _)| b.center_of_mass() * b.mass())
                .sum();
            let center = if mass > 0.0 {
                weighted / mass
            } else {
                Vector::ZERO
            };
            Ok(balaur_script::Value::Vec3(scalar::a3(center)))
        })
    });
    m.function("softbody_dihedrals", |eng: &Engine, node: NodeId| {
        with_family(eng, node, |bodies| {
            let mut out = Vec::new();
            for (body, offset) in bodies {
                for hinge in body.dihedrals() {
                    let at = |i: u32| {
                        balaur_script::Value::Int(
                            i64::from(i) + i64::try_from(*offset).unwrap_or(i64::MAX),
                        )
                    };
                    out.push(balaur_script::Value::List(hinge.vertices.map(at).to_vec()));
                }
            }
            Ok(balaur_script::Value::List(out))
        })
    });
    m.function(
        "softbody_dihedral",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_family(eng, node, |bodies| {
                let mut left = usize::try_from(index)
                    .map_err(|_| anyhow!("this body has no dihedral {index}"))?;
                for (body, offset) in bodies {
                    let Some(hinge) = body.dihedrals().get(left) else {
                        left -= body.dihedrals().len();
                        continue;
                    };
                    let number = |x: Real| balaur_script::Value::Num(f64::from(scalar::f32_of(x)));
                    let at = |i: u32| {
                        balaur_script::Value::Int(
                            i64::from(i) + i64::try_from(*offset).unwrap_or(i64::MAX),
                        )
                    };
                    return Ok(balaur_script::Value::Map(vec![
                        (
                            k::PARTICLES.into(),
                            balaur_script::Value::List(hinge.vertices.map(at).to_vec()),
                        ),
                        (k::REST_ANGLE.into(), number(hinge.rest_angle)),
                        (
                            k::INITIAL_REST_ANGLE.into(),
                            number(hinge.initial_rest_angle()),
                        ),
                        (k::PLASTIC_SET.into(), number(hinge.plastic_set())),
                    ]));
                }
                Err(anyhow!("this body has no dihedral {index}"))
            })
        },
    );

    install_runtime(m);
}

/// Run `f` over the node's body and every piece torn off it, each with the
/// family-wide index of its first particle.
fn with_family<T>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&[(&crate::rapier3d::prelude::SoftBody, usize)]) -> Result<T>,
) -> Result<T> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let handle = *state
        .soft_bodies
        .get(&entity)
        .ok_or_else(|| anyhow!("node has no soft body"))?;
    let set = &state.world.soft_bodies;
    let mut offset = 0;
    let mut bodies = Vec::new();
    for piece in set.family(handle) {
        if let Some(body) = set.get(piece) {
            bodies.push((body, offset));
            offset += body.num_particles();
        }
    }
    anyhow::ensure!(!bodies.is_empty(), "node has no soft body");
    f(&bodies)
}

/// The handle type the snapshot carries per node.
pub(crate) type SoftRef3d = SoftBodyHandle;
