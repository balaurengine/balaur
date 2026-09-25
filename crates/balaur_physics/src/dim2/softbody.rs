//! `softbody2d`: the 2D half of [`crate::softbody`].
//!
//! The same material, the same tearing and the same plasticity, over
//! generators that lay particles out in a plane. A 2D body's cells are
//! triangles rather than tetrahedra, so `volumetric` triangulates an outline
//! where 3D tetrahedrizes a closed mesh, and `polygon` is the cheap way in:
//! the node's drawn shape becomes the body it is simulated as.
//!
//! What the solver produces goes back on the node as a
//! [`balaur_core::mesh::SolvedPolygon`], which is what the 2D renderer draws
//! in place of the authored positions.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::dim2::PhysicsState2d;
use crate::rapier2d::prelude::{
    ColliderBuilder as ColliderBuilder2, SoftBodyBuilder as SoftBodyBuilder2,
    SoftBodyHandle as SoftBodyHandle2, SoftBodyParticleSettings as SoftBodyParticleSettings2,
    SoftBodySolver as SoftBodySolver2,
};
use crate::scalar::{self, Vector2};
use crate::shared::softbody as cap;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

crate::shared::softbody::material!(
    rapier = rapier2d,
    material = read_material_2d,
    cell_model = read_cell_model_2d,
    flow = threshold_2d,
    springs = springs_2d
);

/// How a 2D body's particles and elements are laid out.
fn shape_schema() -> String {
    let kinds = v::options(w::SOFT_KINDS_2D);
    let default = w::GRID;
    v::schema(&[
        (
            k::KIND,
            &format!(
                r#"{{ type = "enum", default = "{default}", options = [{kinds}], description = "How the body's particles and elements are laid out" }}"#
            ),
        ),
        (
            k::HALF_EXTENTS,
            r#"{ type = "vec2", default = [0.5, 0.5], description = "Half-sizes of the sheet, when kind is grid", group = "shape" }"#,
        ),
        (
            k::CELLS,
            r#"{ type = "vec2", default = [4.0, 4.0], description = "How many cells along each axis of a grid", group = "shape" }"#,
        ),
        (
            k::RADIUS,
            r#"{ type = "float", default = 0.5, min = 0.001, description = "Radius, when kind is disk", group = "shape" }"#,
        ),
        (
            k::A,
            r#"{ type = "vec2", default = [0.0, 0.0], description = "Where a rope starts, relative to the node", group = "shape" }"#,
        ),
        (
            k::B,
            r#"{ type = "vec2", default = [0.0, -1.0], description = "Where a rope ends, relative to the node", group = "shape" }"#,
        ),
        (
            k::PARTICLES,
            r#"{ type = "float", default = 16.0, min = 2.0, description = "How many particles a rope or the rim of a disk is made of", group = "shape" }"#,
        ),
        (
            k::MESH,
            &format!(
                r#"{{ type = "asset", asset = "{}", default = "", description = "Points and triangles for a polygon, trimesh, polyline or volumetric body: the same asset a polygon draws", group = "shape" }}"#,
                balaur_core::mesh::MESH_ASSET_TYPE
            ),
        ),
        (
            k::CELL_SIZE,
            r#"{ type = "float", default = 0.25, min = 0.001, description = "How big one triangle is when a volumetric body fills an outline; smaller is finer, slower and stiffer to tear", group = "shape" }"#,
        ),
        (
            k::SKIN,
            r#"{ type = "bool", default = false, description = "Keep the outline as the drawn shape and let the cells carry it, so a detail the cell size cannot resolve survives", group = "shape" }"#,
        ),
    ])
}

/// The `mesh` asset's points as 2D, with the triangles over them, in world
/// space around `pose`.
fn source_mesh(
    eng: &Engine,
    params: &toml::Value,
    pose: scalar::Pose2,
) -> Result<(Vec<Vector2>, Vec<[u32; 3]>)> {
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

/// Lay the particles out the way `kind` asks, in world space around `pose`.
fn build_layout(
    eng: &Engine,
    params: &toml::Value,
    pose: scalar::Pose2,
    kind: &str,
) -> Result<SoftBodyBuilder2> {
    let at = pose.translation;
    let local = |key: &str, default: [f32; 2]| pose * scalar::v2a(v::vec2(params, key, default));
    let cells = v::vec2(params, k::CELLS, [4.0, 4.0]);
    let particles = f64::from(v::f(params, k::PARTICLES, 16.0).max(2.0)).floor();
    cap::refuse_past_cap(match kind {
        w::GRID => cap::particles_along(cells[0]) * cap::particles_along(cells[1]),
        w::DISK | w::ROPE_SOFT => particles,
        _ => 0.0,
    })?;
    let builder = match kind {
        // As in 3D: the schema counts cells, rapier counts the particles
        // between them.
        w::GRID => SoftBodyBuilder2::grid(
            at,
            scalar::v2a(v::vec2(params, k::HALF_EXTENTS, [0.5, 0.5])),
            cells[0].max(1.0) as usize + 1,
            cells[1].max(1.0) as usize + 1,
        ),
        w::DISK => SoftBodyBuilder2::disk(
            at,
            scalar::real(v::f(params, k::RADIUS, 0.5)),
            v::f(params, k::PARTICLES, 16.0).max(3.0) as usize,
        ),
        w::ROPE_SOFT => SoftBodyBuilder2::rope(
            local(k::A, [0.0, 0.0]),
            local(k::B, [0.0, -1.0]),
            v::f(params, k::PARTICLES, 16.0).max(2.0) as usize,
        ),
        // The outline alone: a hoop of edges around an inside, which is what
        // a 2D shape drawn by its border wants to be.
        w::SOFT_POLYGON => {
            let (points, indices) = source_mesh(eng, params, pose)?;
            let border = outline(&indices);
            SoftBodyBuilder2::polyline(points, Some(border))
                .ok_or_else(|| anyhow!("that mesh has no outline to make a soft polygon of"))?
        }
        w::POLYLINE => {
            let (points, _) = source_mesh(eng, params, pose)?;
            SoftBodyBuilder2::polyline(points, None)
                .ok_or_else(|| anyhow!("that mesh has no points to make a soft wire of"))?
        }
        // The triangles the asset already carries, kept as the body's cells.
        w::SURFACE_MESH => {
            let (points, indices) = source_mesh(eng, params, pose)?;
            SoftBodyBuilder2::trimesh(points, indices)
                .ok_or_else(|| anyhow!("that mesh has no triangles to make a soft body of"))?
        }
        // The approximate triangulation: the outline is covered with cells of
        // `cell_size` and the body is those cells.
        w::VOLUMETRIC => {
            let (points, indices) = source_mesh(eng, params, pose)?;
            let border = outline(&indices);
            let size = scalar::real(v::f(params, k::CELL_SIZE, 0.25));
            cap::refuse_past_cap(cap::grid_particles(&extents(&points), size))?;
            let built = if v::boolean(params, k::SKIN, false) {
                SoftBodyBuilder2::volumetric_skinned(&points, &border, size)
            } else {
                SoftBodyBuilder2::volumetric(&points, &border, size)
            };
            built.ok_or_else(|| {
                anyhow!("that outline encloses nothing at a cell size of {size}: it has to be closed, and big enough to hold a cell")
            })?
        }
        other => return Err(anyhow!("unknown soft-body kind '{other}'")),
    };
    cap::refuse_past_cap(builder.positions.len() as f64)?;
    Ok(with_settings(builder, params))
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

/// The rows every layout shares. The same reading as 3D's, against rapier2d's
/// own types.
fn with_settings(mut builder: SoftBodyBuilder2, params: &toml::Value) -> SoftBodyBuilder2 {
    builder = builder
        .material(read_material_2d(params))
        .cell_model(read_cell_model_2d(params))
        .volume_preservation(v::boolean(params, k::VOLUME_PRESERVATION, false))
        .volume_factor(scalar::real(v::f(params, k::VOLUME_FACTOR, 1.0)))
        .shape_matching(v::boolean(params, k::SHAPE_MATCHING, false))
        .self_contacts(v::boolean(params, k::SELF_CONTACTS, false))
        .linear_damping(scalar::real(v::f(params, k::LINEAR_DAMPING, 0.0)))
        .gravity_scale(scalar::real(v::f(params, k::GRAVITY_SCALE, 1.0)))
        .additional_solver_iterations(v::f(params, k::SOLVER_ITERATIONS, 0.0).max(0.0) as usize)
        .additional_pgs_iterations(v::f(params, k::PGS_ITERATIONS, 3.0).max(0.0) as usize)
        .can_sleep(v::boolean(params, k::CAN_SLEEP, true))
        .solver(if v::text(params, k::SOLVER, w::CONSTRAINTS) == w::FEM {
            SoftBodySolver2::Fem
        } else {
            SoftBodySolver2::Constraints
        })
        .surface_collider(
            crate::dim2::collider::with_groups_2d(ColliderBuilder2::ball(1.0), params)
                .friction(scalar::real(v::f(params, k::FRICTION, 0.5)))
                .restitution(scalar::real(v::f(params, k::RESTITUTION, 0.0))),
        );
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
    let settings = SoftBodyParticleSettings2 {
        dominance_group: v::f(params, k::DOMINANCE, 0.0).clamp(-127.0, 127.0) as i8,
        ..builder.particle_settings
    };
    builder.particle_settings(settings)
}

pub(crate) fn apply_softbody_2d(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let pose = crate::dim2::node_pose_2d(eng, entity)?;
    let kind = v::text(params, k::KIND, w::GRID).to_string();
    let builder =
        build_layout(eng, params, pose, &kind)?.user_data(u128::from(entity.to_bits().get()));
    remove_softbody_2d(eng, entity);
    {
        let state = eng.resource::<PhysicsState2d>();
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
    write_solved_polygon(eng, entity);
    Ok(())
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
    world.soft_bodies.remove(
        handle,
        &mut world.islands,
        &mut world.bodies,
        &mut world.colliders,
        &mut world.impulse_joints,
        &mut world.multibody_joints,
    );
}

pub(crate) fn get_softbody_params_2d(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let authored = state.soft_params.get(&entity)?.clone();
    let handle = *state.soft_bodies.get(&entity)?;
    let body = state.world.soft_bodies.get(handle)?;
    let toml::Value::Table(mut table) = authored else {
        return None;
    };
    let number = |value: scalar::Real| toml::Value::Float(f64::from(scalar::f32_of(value)));
    // Not the mass, whose sum over the particles rounds off what was asked
    // for, nor the radius, whose 0 means "worked out from the layout".
    table.insert(k::VOLUME_FACTOR.into(), number(body.volume_factor()));
    let solver = match body.solver() {
        SoftBodySolver2::Fem => w::FEM,
        SoftBodySolver2::Constraints => w::CONSTRAINTS,
    };
    table.insert(k::SOLVER.into(), toml::Value::String(solver.into()));
    table.insert(
        k::VOLUME_PRESERVATION.into(),
        toml::Value::Boolean(body.volume_preservation_enabled()),
    );
    Some(toml::Value::Table(table))
}

/// Hand this step's particle positions to whatever draws the node, in the
/// node's own space (see [`crate::softbody::write_solved_mesh`]).
pub(crate) fn write_solved_polygon(eng: &Engine, entity: Entity) {
    let Ok(pose) = crate::dim2::node_pose_2d(eng, entity) else {
        return;
    };
    let inverse = pose.inverse();
    let (positions, indices) = {
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let Some(&handle) = state.soft_bodies.get(&entity) else {
            return;
        };
        let Some(body) = state.world.soft_bodies.get(handle) else {
            return;
        };
        // The cells are what a 2D body is drawn as: a filled shape, not the
        // outline its collision mesh reports.
        (
            body.particle_positions()
                .map(|p| scalar::a2(inverse * p))
                .collect::<Vec<_>>(),
            body.cells().iter().map(|cell| cell.vertices).collect(),
        )
    };
    let mut world = eng.world_mut();
    if let Ok(mut solved) = world.get::<&mut balaur_core::mesh::SolvedPolygon>(entity) {
        solved.update(positions, indices);
        return;
    }
    let mut solved = balaur_core::mesh::SolvedPolygon::default();
    solved.update(positions, indices);
    let _ = world.insert_one(entity, solved);
}

pub(crate) fn write_every_solved_polygon(eng: &Engine) {
    let entities: Vec<Entity> = {
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        state.soft_bodies.keys().copied().collect()
    };
    for entity in entities {
        write_solved_polygon(eng, entity);
    }
}

pub(crate) fn register_softbody_component_2d(reg: &mut Registry<'_>) {
    let schema = [shape_schema(), crate::softbody::shared_softbody_schema()].join("\n");
    reg.register_component(
        c::SOFTBODY_2D,
        ComponentDef {
            doc: "A deformable 2D body: particles linked by elastic constraints, laid out by `kind` and made of what the material rows say. A `polygon` on the same node is drawn from the solver's positions when the two agree on the vertex count, which the `polygon`, `trimesh` and `volumetric` kinds give and a generator does not.",
            schema: ComponentDef::parse_schema(c::SOFTBODY_2D, &schema),
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &[],
            apply: Box::new(apply_softbody_2d),
            remove: Box::new(|eng, entity| {
                remove_softbody_2d(eng, entity);
                let _ = eng
                    .world_mut()
                    .remove_one::<balaur_core::mesh::SolvedPolygon>(entity);
                Ok(())
            }),
            get: Box::new(get_softbody_params_2d),
        },
    );
}

pub(crate) fn install_softbody_api_2d(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_softbody", &[c::SOFTBODY_2D], "", "Build the node's soft body from a `softbody2d` table: `kind`, the shape rows, and the material rows."),
        ("softbody_particles", &[c::SOFTBODY_2D], "", "How many particles the body ended up with, which a generator decides rather than the author."),
        ("softbody_position", &[c::SOFTBODY_2D], "", "Where one particle is, in world space."),
        ("softbody_area", &[c::SOFTBODY_2D], "", "How much area the body encloses right now, against `softbody_rest_area` for how far it is squeezed."),
        ("softbody_rest_area", &[c::SOFTBODY_2D], "", "How much it encloses at rest."),
        ("softbody_center", &[c::SOFTBODY_2D], "", "The body's centre of mass, which is where it is when a deformable body has no one position."),
        ("pin_particle", &[c::SOFTBODY_2D], "", "Hold one particle where it is, which is how a cloth hangs from a hook."),
    ]);
    m.function(
        "set_softbody",
        |eng: &Engine, (node, params): (NodeId, balaur_script::Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            apply_softbody_2d(eng, entity_of(node)?, &params)
        },
    );
    m.function("softbody_particles", |eng: &Engine, node: NodeId| {
        with_softbody_2d(eng, node, |body| {
            Ok(i64::try_from(body.num_particles()).unwrap_or(i64::MAX))
        })
    });
    m.function(
        "softbody_position",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_softbody_2d(eng, node, |body| {
                let index = usize::try_from(index)
                    .ok()
                    .filter(|i| *i < body.num_particles())
                    .ok_or_else(|| anyhow!("this body has no particle {index}"))?;
                Ok(balaur_script::Value::Vec2(scalar::a2(
                    body.particle_position(index),
                )))
            })
        },
    );
    m.function("softbody_area", |eng: &Engine, node: NodeId| {
        with_softbody_2d(eng, node, |body| Ok(scalar::f32_of(body.volume())))
    });
    m.function("softbody_rest_area", |eng: &Engine, node: NodeId| {
        with_softbody_2d(eng, node, |body| Ok(scalar::f32_of(body.rest_volume())))
    });
    m.function("softbody_center", |eng: &Engine, node: NodeId| {
        with_softbody_2d(eng, node, |body| {
            Ok(balaur_script::Value::Vec2(scalar::a2(
                body.center_of_mass(),
            )))
        })
    });
    m.function(
        "pin_particle",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            let entity = entity_of(node)?;
            let state = eng.resource::<PhysicsState2d>();
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

fn with_softbody_2d<T>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&crate::rapier2d::prelude::SoftBody) -> Result<T>,
) -> Result<T> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState2d>();
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

/// The handle type the 2D snapshot carries per node.
pub(crate) type SoftRef2d = SoftBodyHandle2;
