//! The 2D half of the physics plugin: a rapier2d world living alongside the
//! 3D one. 2D nodes use the regular scene `Transform`: x/y translate, the
//! rotation is about z, and the z coordinate is left untouched.
//!
//! Determinism matches the 3D world: enhanced-determinism rapier, ordered
//! collections, fixed timestep.
use crate::rapier2d::pipeline::PhysicsWorld as PhysicsWorld2;
use crate::rapier2d::prelude::{
    ColliderHandle as ColliderHandle2, RigidBodyHandle as RigidBodyHandle2,
};
use crate::scalar::{self, Pose2, Rotation2};
use anyhow::{Result, anyhow};
use balaur_core::collections::DetHashMap;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Stage, Transform};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::{EulerRot, Quat};
use serde::Deserialize as _;

pub mod body;
pub mod character;
pub mod collider;
pub mod decompose;
pub mod events;
pub mod follow;
pub mod joint;
pub mod query;
mod readers;
mod shapes;
pub mod softbody;
pub mod tiles;

use body::with_body;
use collider::max_contact_impulse;
pub use query::overlaps;
use query::overlaps_value;

use balaur_core::digest::{Entry, Hasher, node_label};

use crate::vocabulary::{component as c, hook};
use balaur_core::fixed_dt;

pub struct PhysicsState2d {
    pub world: PhysicsWorld2,
    pub bodies: DetHashMap<Entity, RigidBodyHandle2>,
    pub colliders: DetHashMap<Entity, Vec<ColliderHandle2>>,
    /// What each body's author wrote that rapier keeps no copy of, as in 3D.
    pub(crate) body_authored: DetHashMap<Entity, crate::shared::body::Authored>,
    /// Removed colliders by owner until the next step, as in 3D.
    pub(crate) gone: DetHashMap<ColliderHandle2, crate::shared::events::Owner>,
    /// Asleep after the last step, as in 3D.
    pub(crate) asleep: balaur_core::collections::DetHashSet<Entity>,
    /// Whether the broad phase's tree matches the colliders, as in 3D.
    pub queries_ready: bool,
    /// Joints per entity, as in the 3D world.
    pub joints: DetHashMap<Entity, joint::JointRef2d>,
    /// Soft bodies per entity, and what each was built from, as in 3D.
    pub soft_bodies: DetHashMap<Entity, softbody::SoftRef2d>,
    pub soft_params: DetHashMap<Entity, toml::Value>,
    /// What each collider and joint was authored from, as in the 3D world:
    /// rapier keeps the shape, not the asset or the choices behind it.
    pub collider_params: DetHashMap<Entity, toml::Value>,
    /// What each `tile_collision` was authored from, what it was last built
    /// from, and the colliders it made — so a rebuild drops its own and
    /// leaves the ones the node authored.
    pub(crate) tile_params: DetHashMap<Entity, toml::Value>,
    pub(crate) tile_built: DetHashMap<Entity, tiles::Built>,
    pub(crate) tile_colliders: DetHashMap<Entity, Vec<ColliderHandle2>>,
    pub joint_params: DetHashMap<Entity, toml::Value>,
    /// What the contact hook reads per collider, as in 3D.
    pub(crate) surfaces: events::Surfaces,
    /// What the last `move_character` found under each character's feet, as
    /// in 3D, so `is_on_floor` reads rather than moves.
    pub grounded: DetHashMap<Entity, bool>,
    /// Each `follow2d`'s controller, kept across steps for a PID's integrals.
    pub follows: DetHashMap<Entity, follow::FollowRef2d>,
    pub paused: bool,
    /// Mirrors `PhysicsState3d::sleeping_allowed`; `physics.set_sleeping_allowed`
    /// writes both worlds.
    pub sleeping_allowed: bool,
    /// Bumped by every shape edit a script makes, as in 3D, so a dug voxel
    /// grid reaches the digest.
    pub shape_revision: u64,
}

impl PhysicsState2d {
    fn new() -> Self {
        let world = PhysicsWorld2 {
            gravity: scalar::v2(0.0, -9.81),
            ..Default::default()
        };
        Self {
            world,
            bodies: DetHashMap::default(),
            colliders: DetHashMap::default(),
            body_authored: DetHashMap::default(),
            gone: DetHashMap::default(),
            asleep: balaur_core::collections::DetHashSet::default(),
            queries_ready: false,
            joints: DetHashMap::default(),
            soft_bodies: DetHashMap::default(),
            soft_params: DetHashMap::default(),
            collider_params: DetHashMap::default(),
            tile_params: DetHashMap::default(),
            tile_built: DetHashMap::default(),
            tile_colliders: DetHashMap::default(),
            joint_params: DetHashMap::default(),
            surfaces: events::Surfaces::default(),
            grounded: DetHashMap::default(),
            follows: DetHashMap::default(),
            paused: false,
            sleeping_allowed: true,
            shape_revision: 0,
        }
    }
}

/// The node's global pose flattened to 2D (x, y, angle about z).
pub(crate) fn node_pose_2d(eng: &Engine, entity: Entity) -> Result<Pose2> {
    // Composed from the ancestors, as in 3D: see `crate::node_pose`.
    let world = eng.world();
    if !world.contains(entity) {
        return Err(anyhow!("node is dead or not in the scene tree"));
    }
    let global = balaur_core::scene::composed_global(&world, entity);
    let (angle, _, _) = global.rotation.to_euler(EulerRot::ZYX);
    Ok(Pose2::from_parts(
        scalar::v2(global.position.x, global.position.y),
        Rotation2::from_angle(scalar::real(angle)),
    ))
}

crate::shared::world::functions!(
    state = PhysicsState2d,
    component = c::JOINT_2D,
    prune = prune_freed_nodes_except_tiles
);

pub(crate) fn prune_freed_nodes(eng: &Engine, state: &mut PhysicsState2d) {
    prune_freed_nodes_except_tiles(eng, state);
    let world = eng.world();
    state.tile_params.retain(|e, _| world.contains(*e));
    state.tile_built.retain(|e, _| world.contains(*e));
    state.tile_colliders.retain(|e, _| world.contains(*e));
    state.follows.retain(|e, _| world.contains(*e));
    let colliders = &state.world.colliders;
    state
        .surfaces
        .retain(|handle, _| colliders.contains(*handle));
}

fn step_system(eng: &Engine, _dt: f32) {
    // One world, so a paused game holds every body — an `always` subtree
    // included, as `crate::step_system` holds the 3D one.
    if eng.paused() {
        return;
    }
    // A map whose cells moved rebuilds once, before the step that has to
    // collide with them.
    tiles::sync_tile_colliders(eng);
    {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        prune_freed_nodes(eng, &mut state);
    }
    resolve_pending_joints(eng);
    let events = {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        if state.paused {
            return;
        }

        // Feed the kinematic bodies their targets before the step reads them.
        {
            let world = eng.world();
            for (&entity, &handle) in &state.bodies {
                let body = &mut state.world.bodies[handle];
                if body.is_kinematic()
                    && let Ok(t) = world.get::<&Transform>(entity)
                {
                    let (angle, _, _) = t.rotation.to_euler(EulerRot::ZYX);
                    body.set_next_kinematic_position(Pose2::from_parts(
                        scalar::v2(t.position.x, t.position.y),
                        Rotation2::from_angle(scalar::real(angle)),
                    ));
                }
            }
        }

        // Exactly one step: Stage::FixedUpdate already repeats at the fixed
        // step, and a second accumulator here would drift out of step with
        // the scripts.
        state.world.integration_parameters.dt = scalar::real(fixed_dt());
        // The step rebuilds the broad phase itself, as in 3D; without this a
        // query after a collider was added rebuilds it a second time.
        state.queries_ready = true;
        let collector = events::Collector::after(std::mem::take(&mut state.gone));
        let hooks = events::Hooks {
            surfaces: &state.surfaces,
        };
        balaur_core::timings::measure(eng, "physics2d/step", || {
            state.world.step_with_events(&hooks, &collector);
        });

        // Write simulated poses back (x, y and the rotation about z).
        let world = eng.world();
        for (&entity, &handle) in &state.bodies {
            let body = &state.world.bodies[handle];
            if body.is_fixed() || body.is_kinematic() {
                continue;
            }
            if let Ok(mut t) = world.get::<&mut Transform>(entity) {
                let pos = body.translation();
                t.position.x = scalar::f32_of(pos.x);
                t.position.y = scalar::f32_of(pos.y);
                t.rotation = Quat::from_rotation_z(scalar::f32_of(body.rotation().angle()));
            }
        }
        (
            collector.take(),
            joint::broken(state, &world),
            sleep_changes(state),
        )
    };
    // Before the events, as in 3D: a tear handler reads the torn body's own
    // geometry, not the one it had before the tear.
    softbody::write_every_solved_polygon(eng);
    events::deliver(eng, &events.0);
    for entity in &events.1 {
        let payload = joint::break_payload(&eng.resource::<PhysicsState2d>().borrow(), *entity);
        joint::remove_joint(eng, *entity);
        balaur_core::events::announce(eng, *entity, hook::JOINT_BREAK, payload);
    }
    announce_sleep(eng, &events.2);
    crate::tuning::warn_about_quarantine_2d(eng);
}

pub fn clear(eng: &Engine) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    // A fresh world, not a drained one: rapier reuses a freed handle's slot
    // with the generation bumped and the solver works in handle order, so a
    // scene rebuilt in place would not simulate as a fresh process does.
    let gravity = state.world.gravity;
    let params = state.world.integration_parameters;
    state.world = PhysicsWorld2::default();
    state.world.gravity = gravity;
    state.world.integration_parameters = params;
    state.bodies.clear();
    state.colliders.clear();
    state.body_authored.clear();
    state.joints.clear();
    // As in 3D: a handle into the old world's arena would alias the new one's.
    state.soft_bodies.clear();
    state.soft_params.clear();
    state.collider_params.clear();
    state.tile_params.clear();
    state.tile_built.clear();
    state.tile_colliders.clear();
    state.joint_params.clear();
    state.surfaces.clear();
    state.grounded.clear();
    state.follows.clear();
    state.asleep.clear();
}

pub fn set_paused(eng: &Engine, paused: bool) {
    let state = eng.resource::<PhysicsState2d>();
    state.borrow_mut().paused = paused;
}

pub fn set_sleeping_allowed(eng: &Engine, allowed: bool) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    state.sleeping_allowed = allowed;
    for (entity, &handle) in &state.bodies {
        let authored = state.body_authored.get(entity);
        let can_sleep = authored.is_none_or(|a| a.can_sleep);
        let thresholds = crate::shared::body::Authored::sleep_thresholds(authored);
        if let Some(body) = state.world.bodies.get_mut(handle) {
            body::allow_sleep(body, allowed && can_sleep, thresholds);
        }
    }
}

pub fn build(reg: &mut Registry<'_>) -> Result<()> {
    build_physics2d(reg);

    {
        let mut m = reg.script_module("physics2d")?;
        crate::install_constants(&mut *m, crate::CONSTANTS_2D);
        install_physics2d_api(&mut *m);
        body::install_body2d_force_api(&mut *m);
        body::install_body2d_state_api(&mut *m);
        body::install_body2d_mass_api(&mut *m);
        body::install_body2d_ccd_api(&mut *m);
        body::install_body2d_lock_api(&mut *m);
        body::install_body2d_sleep_api(&mut *m);
        body::install_body2d_force_reader_api(&mut *m);
        query::install_physics2d_query_api(&mut *m);
        query::install_physics2d_raycast_all_api(&mut *m);
        query::install_physics2d_shapecast_api(&mut *m);
        query::install_physics2d_volume_query_api(&mut *m);
        query::install_physics2d_shape_query_api(&mut *m);
        query::install_physics2d_pair_query_api(&mut *m);
        query::install_physics2d_world_list_api(&mut *m);
        joint::install_joint2d_api(&mut *m);
        softbody::install_softbody_api_2d(&mut *m);
        character::install_character2d_api(&mut *m);
        follow::install_follow2d_api(&mut *m);
        collider::install_voxel_2d_api(&mut *m);
        readers::install_collider2d_api(&mut *m);
        readers::install_collider2d_reader_api(&mut *m);
        readers::install_heightfield_2d_api(&mut *m);
    }
    body::register_body2d_component(reg);
    collider::register_collider2d_component(reg);
    joint::register_joint2d_component(reg);
    softbody::register_softbody_component_2d(reg);
    character::register_character2d_component(reg);

    reg.register_preset(
        "rigid_body2d",
        balaur_core::presets::preset(
            "A 2D body physics simulates, with a rect collider",
            &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            &[
                (c::BODY_2D, Some("kind = \"dynamic\"")),
                (c::COLLIDER_2D, None),
            ],
        )?,
    );
    reg.register_preset(
        "static_body2d",
        balaur_core::presets::preset(
            "An immovable 2D body with a rect collider: ground, walls",
            &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            &[
                (c::BODY_2D, Some("kind = \"static\"")),
                (c::COLLIDER_2D, None),
            ],
        )?,
    );
    reg.register_preset(
        "soft_body2d",
        balaur_core::presets::preset(
            "A deformable 2D block that squashes and springs back",
            &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            &[(
                c::SOFTBODY_2D,
                Some("kind = \"grid\"\ncell_model = \"corotational\"\nshape_matching = \"on\""),
            )],
        )?,
    );
    reg.register_preset(
        "rope2d",
        balaur_core::presets::preset(
            "A 2D rope of linked particles, pinned at one end",
            &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            &[(
                c::SOFTBODY_2D,
                Some("kind = \"rope\"\nparticle_count = 24\npinned_particles = [0]"),
            )],
        )?,
    );

    Ok(())
}

/// The 2D world and the system that steps it, mirroring `PhysicsPlugin::build`.
fn build_physics2d(reg: &mut Registry<'_>) {
    reg.insert_resource(PhysicsState2d::new());
    follow::build(reg);
    reg.add_system(Stage::FixedUpdate, step_system);
    tiles::register_tile_collision_component(reg);
    build_physics2d_digest(reg);
    build_physics2d_snapshot(reg);
}

/// The 2D twin of the 3D snapshot source.
#[derive(serde::Deserialize)]
struct PhysicsFrame2d {
    #[serde(with = "crate::world_bytes")]
    world: PhysicsWorld2,
    bodies: Vec<(crate::NodeKey, RigidBodyHandle2)>,
    colliders: Vec<(crate::NodeKey, Vec<ColliderHandle2>)>,
    body_authored: Vec<(crate::NodeKey, crate::shared::body::Authored)>,
    joints: Vec<(crate::NodeKey, joint::JointRef2d)>,
    soft_bodies: Vec<(crate::NodeKey, softbody::SoftRef2d)>,
    soft_params: Vec<(crate::NodeKey, toml::Value)>,
    collider_params: Vec<(crate::NodeKey, toml::Value)>,
    tile_params: Vec<(crate::NodeKey, toml::Value)>,
    tile_built: Vec<(crate::NodeKey, tiles::Built)>,
    tile_colliders: Vec<(crate::NodeKey, Vec<ColliderHandle2>)>,
    joint_params: Vec<(crate::NodeKey, toml::Value)>,
    surfaces: Vec<(ColliderHandle2, events::Surface)>,
    grounded: Vec<(crate::NodeKey, bool)>,
    follows: Vec<(crate::NodeKey, follow::FollowRef2d)>,
    shape_revision: u64,
    paused: bool,
    sleeping_allowed: bool,
}

#[derive(serde::Serialize)]
struct PhysicsFrameRef2d<'a> {
    #[serde(with = "crate::world_bytes")]
    world: &'a PhysicsWorld2,
    bodies: Vec<(crate::NodeKey, RigidBodyHandle2)>,
    colliders: Vec<(crate::NodeKey, Vec<ColliderHandle2>)>,
    body_authored: Vec<(crate::NodeKey, crate::shared::body::Authored)>,
    joints: Vec<(crate::NodeKey, joint::JointRef2d)>,
    soft_bodies: Vec<(crate::NodeKey, softbody::SoftRef2d)>,
    soft_params: Vec<(crate::NodeKey, toml::Value)>,
    collider_params: Vec<(crate::NodeKey, toml::Value)>,
    tile_params: Vec<(crate::NodeKey, toml::Value)>,
    tile_built: Vec<(crate::NodeKey, tiles::Built)>,
    tile_colliders: Vec<(crate::NodeKey, Vec<ColliderHandle2>)>,
    joint_params: Vec<(crate::NodeKey, toml::Value)>,
    surfaces: Vec<(ColliderHandle2, events::Surface)>,
    grounded: Vec<(crate::NodeKey, bool)>,
    follows: Vec<(crate::NodeKey, follow::FollowRef2d)>,
    shape_revision: u64,
    paused: bool,
    sleeping_allowed: bool,
}

fn save_physics2d(eng: &Engine) -> serde_json::Value {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let world = eng.world();
    let frame = PhysicsFrameRef2d {
        world: &state.world,
        bodies: crate::keyed(&world, &state.bodies),
        colliders: crate::keyed(&world, &state.colliders),
        body_authored: crate::keyed(&world, &state.body_authored),
        joints: crate::keyed(&world, &state.joints),
        soft_bodies: crate::keyed(&world, &state.soft_bodies),
        soft_params: crate::authored(eng, &world, &state.soft_params, c::SOFTBODY_2D),
        collider_params: crate::authored(eng, &world, &state.collider_params, c::COLLIDER_2D),
        tile_params: crate::authored(eng, &world, &state.tile_params, c::TILE_COLLISION),
        tile_built: crate::keyed(&world, &state.tile_built),
        tile_colliders: crate::keyed(&world, &state.tile_colliders),
        joint_params: crate::authored(eng, &world, &state.joint_params, c::JOINT_2D),
        surfaces: state.surfaces.iter().map(|(h, s)| (*h, *s)).collect(),
        grounded: crate::keyed(&world, &state.grounded),
        follows: crate::keyed(&world, &state.follows),
        shape_revision: state.shape_revision,
        paused: state.paused,
        sleeping_allowed: state.sleeping_allowed,
    };
    crate::frame_value(frame, "2D")
}

fn load_physics2d(eng: &Engine, value: &serde_json::Value) {
    let frame: PhysicsFrame2d = match PhysicsFrame2d::deserialize(value) {
        Ok(frame) => frame,
        Err(e) => {
            tracing::error!(error = %e, "restoring the 2D physics world");
            return;
        }
    };
    let bodies = crate::resolved(eng, frame.bodies);
    let colliders = crate::resolved(eng, frame.colliders);
    let body_authored = crate::resolved(eng, frame.body_authored);
    let joints = crate::resolved(eng, frame.joints);
    let soft_bodies = crate::resolved(eng, frame.soft_bodies);
    let soft_params = crate::resolved(eng, crate::filled(eng, frame.soft_params, c::SOFTBODY_2D));
    let collider_params = crate::resolved(
        eng,
        crate::filled(eng, frame.collider_params, c::COLLIDER_2D),
    );
    let tile_params = crate::resolved(
        eng,
        crate::filled(eng, frame.tile_params, c::TILE_COLLISION),
    );
    let tile_built = crate::resolved(eng, frame.tile_built);
    let tile_colliders = crate::resolved(eng, frame.tile_colliders);
    let joint_params = crate::resolved(eng, crate::filled(eng, frame.joint_params, c::JOINT_2D));
    let grounded = crate::resolved(eng, frame.grounded);
    let follows = crate::resolved(eng, frame.follows);
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    state.world = frame.world;
    state.shape_revision = frame.shape_revision;
    state.paused = frame.paused;
    state.sleeping_allowed = frame.sleeping_allowed;
    state.bodies = bodies;
    state.colliders = colliders;
    state.body_authored = body_authored;
    state.joints = joints;
    state.soft_bodies = soft_bodies;
    state.soft_params = soft_params;
    state.collider_params = collider_params;
    state.tile_params = tile_params;
    state.tile_built = tile_built;
    state.tile_colliders = tile_colliders;
    state.joint_params = joint_params;
    state.surfaces = frame.surfaces.into_iter().collect();
    state.grounded = grounded;
    state.follows = follows;
    restamp_collider_owners(&mut state);
}

fn build_physics2d_snapshot(reg: &mut Registry<'_>) {
    reg.add_snapshot_source("physics2d", save_physics2d, load_physics2d);
}

/// The 2D twin of the 3D source: velocity and sleep state, which no
/// component `get` reports.
fn build_physics2d_digest(reg: &mut Registry<'_>) {
    reg.add_digest_source("physics2d", |eng, out| {
        let Some(state) = eng.try_resource::<PhysicsState2d>() else {
            return;
        };
        let state = state.borrow();
        let world = eng.world();
        // One row for the whole world's shape edits, as in 3D.
        {
            let mut h = Hasher::new();
            h.write(&state.shape_revision.to_le_bytes());
            out.push(Entry {
                label: "physics2d/shapes".to_string(),
                digest: h.finish(),
            });
        }
        for (&entity, &handle) in &state.bodies {
            let body = &state.world.bodies[handle];
            let v = body.linvel();
            let mut h = Hasher::new();
            for value in [v.x, v.y, body.angvel()] {
                // Whatever width this build runs at (see `crate::scalar`).
                h.write_f64(f64::from(value));
            }
            h.write(&[u8::from(body.is_sleeping())]);
            out.push(Entry {
                label: node_label(&world, entity),
                digest: h.finish(),
            });
        }
        // Every particle's velocity and the body's topology, as in 3D: a
        // deformable body has no one velocity, and a tear is a divergence
        // nothing else would report.
        for (&entity, &handle) in &state.soft_bodies {
            // What tore off is the node's body as much as what it kept.
            let mut h = Hasher::new();
            for piece in crate::shared::softbody::Family::family(&state.world.soft_bodies, handle) {
                let Some(body) = state.world.soft_bodies.get(piece) else {
                    continue;
                };
                h.write(&body.topology_version().to_le_bytes());
                for v in body.particle_velocities() {
                    for value in [v.x, v.y] {
                        h.write_f64(f64::from(value));
                    }
                }
                h.write(&[u8::from(body.is_sleeping())]);
            }
            out.push(Entry {
                label: format!("{}/soft", node_label(&world, entity)),
                digest: h.finish(),
            });
        }
    });
}

/// `physics2d`: bodies, colliders, gravity, velocities, contact impulse and
/// overlap queries.
fn install_physics2d_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "The 2D rigid-body world: bodies and colliders on nodes, their velocities, raycasts and overlap queries. `physics` holds what spans both worlds.",
    );
    m.describe(&[
        ("set_gravity", &[], "", "Set the 2D world's gravity, in units per second squared."),
        ("apply_impulse", &[c::BODY_2D], "", "Add an instant change in momentum, as if the body were struck."),
        ("set_linear_velocity", &[c::BODY_2D], "", "Set how fast the body travels, in units per second."),
        ("linear_velocity", &[c::BODY_2D], "", "How fast the body is travelling, in units per second."),
        ("set_angular_velocity", &[c::BODY_2D], "", "Set how fast the body spins, in radians per second."),
        ("angular_velocity", &[c::BODY_2D], "", "How fast the body is spinning, in radians per second."),
        ("max_contact_impulse", &[c::BODY_2D], "", "The hardest contact this body took in the last step, zero when nothing touched it."),
        ("overlaps", &[c::COLLIDER_2D], "", "The nodes this one currently intersects; rapier reports a pair only when one of the two colliders is a sensor."),
    ]);
    crate::ragdoll::install_ragdoll_api(m, false);
    // No reader by design (N8): the rapier world holds the gravity vector;
    // add `physics2d.gravity` when a caller needs to read it back.
    m.function("set_gravity", |eng: &Engine, (x, y): (f32, f32)| {
        let state = eng.resource::<PhysicsState2d>();
        state.borrow_mut().world.gravity = scalar::v2(x, y);
        Ok(())
    });
    m.function(
        "apply_impulse",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse(scalar::v2(x, y), true);
            })
        },
    );
    m.function(
        "set_linear_velocity",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_linvel(scalar::v2(x, y), true);
            })
        },
    );
    m.function("linear_velocity", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            let v = state.world.bodies[handle].linvel();
            (v.x, v.y)
        })
    });
    m.function(
        "set_angular_velocity",
        |eng: &Engine, (node, w): (NodeId, f32)| {
            let w = scalar::real(w);
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_angvel(w, true);
            })
        },
    );
    m.function("angular_velocity", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].angvel()
        })
    });
    m.function("max_contact_impulse", |eng: &Engine, node: NodeId| {
        Ok(max_contact_impulse(eng, entity_of(node)?))
    });
    // Sensor pairs only: rapier's narrow phase reports an intersection only
    // when at least one of the two colliders is a sensor.
    m.function("overlaps", |eng: &Engine, node: NodeId| {
        overlaps_value(eng, node)
    });
}
