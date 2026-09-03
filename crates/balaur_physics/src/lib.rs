//! Rapier physics as a Balaur plugin.
//!
//! This crate doubles as the reference for wrapping a Rust library for
//! scripting: insert a resource, add a system, declare script functions, and
//! optionally teach scene files new keys. Nothing else is required.
//!
//! v0 constraint: physics-driven nodes are treated as world-space (their
//! simulated pose is written to the local transform). Nest them under
//! non-moving parents only.

use anyhow::{anyhow, Result};

use balaur_core::collections::DetHashMap;

use balaur_core::hecs::Entity;

use balaur_core::{App, Engine, Plugin, Stage, Transform};

use balaur_script::{Bindings, BindingsExt};

use crate::rapier3d::pipeline::PhysicsWorld;

use crate::rapier3d::prelude::{ColliderHandle, RigidBodyHandle};

#[cfg(not(any(feature = "f32", feature = "f64")))]
compile_error!(
    "balaur_physics needs a scalar: build with the `f32` feature (the default) or with \
     `--no-default-features --features f64`"
);

// The `f64` build swaps rapier for its f64 twin under the same names, so
// every `use crate::rapier3d::..` below and in the submodules follows the scalar.
// `scalar.rs` is the seam where a number changes width.
#[cfg(not(feature = "f64"))]
pub use ::rapier2d;
#[cfg(feature = "f64")]
pub use ::rapier2d_f64 as rapier2d;
#[cfg(not(feature = "f64"))]
pub use ::rapier3d;
#[cfg(feature = "f64")]
pub use ::rapier3d_f64 as rapier3d;

pub mod body;
pub mod character;
pub mod collider;
pub mod debug;
pub mod dim2;
pub mod events;
pub mod geometry;
pub mod joint;
pub mod query;
pub(crate) mod scalar;
pub mod tuning;
pub mod vehicle;
mod vocabulary;

pub use dim2::PhysicsState2d;
pub use query::overlaps;

use balaur_core::digest::{node_label, Entry, Hasher};

use balaur_core::FIXED_DT;

pub struct PhysicsState {
    pub world: PhysicsWorld,
    /// Insertion-ordered with an unseeded hasher: iteration order must be
    /// deterministic (see `balaur_core::collections`).
    pub bodies: DetHashMap<Entity, RigidBodyHandle>,
    /// Colliders per entity (attached to the entity's body, or standalone).
    pub colliders: DetHashMap<Entity, Vec<ColliderHandle>>,
    /// Joints per entity. Which of rapier's two sets a joint lives in is
    /// decided when it is made and never changes.
    pub joints: DetHashMap<Entity, joint::JointRef>,
    /// What each collider and joint was authored from.
    ///
    /// Rapier keeps a shape, not the asset it was built from, and not the
    /// `fill` or `fit` that shaped it — so without this an editor round-trip
    /// would quietly lose them. Read back under whatever rapier does report,
    /// so a live edit still shows through.
    pub collider_params: DetHashMap<Entity, toml::Value>,
    pub joint_params: DetHashMap<Entity, toml::Value>,
    /// While paused the simulation does not step (editors pause by default
    /// and unpause on play).
    pub paused: bool,
    /// What a script set on each wheel, and what the last step left there.
    /// Beside the world because rapier's vehicle controller is rebuilt every
    /// step (see [`vehicle`]).
    pub wheel_inputs: DetHashMap<Entity, vehicle::WheelInput>,
    /// Whether the broad phase's tree matches the colliders.
    ///
    /// Rapier builds it during a step, so a world that has not stepped yet has
    /// an empty one — and a raycast in `init`, which is where a game places
    /// things on the ground, would find nothing at all. Queries refresh it
    /// when this is false (`query::ensure_queries`).
    pub queries_ready: bool,
    /// Bumped by every shape edit a script makes — digging a voxel, replacing
    /// a collider. Hashed into the digest: nothing else about the world says a
    /// hole was dug until something falls into it.
    pub shape_revision: u64,
    /// Whether bodies may fall asleep. Recorded here (rapier keeps it
    /// per-body) so `physics.sleeping_allowed` can read it back and bodies
    /// created later inherit it.
    pub sleeping_allowed: bool,
}

impl PhysicsState {
    fn new() -> Self {
        Self {
            world: PhysicsWorld::default(),
            bodies: DetHashMap::default(),
            colliders: DetHashMap::default(),
            joints: DetHashMap::default(),
            collider_params: DetHashMap::default(),
            joint_params: DetHashMap::default(),
            paused: false,
            queries_ready: false,
            wheel_inputs: DetHashMap::default(),
            shape_revision: 0,
            sleeping_allowed: true,
        }
    }
}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn name(&self) -> &'static str {
        "physics"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        app.engine.insert_resource(PhysicsState::new());
        app.add_system(Stage::FixedUpdate, step_system);
        build_physics_digest(app);
        build_physics_snapshot(app);
        debug::build(app);
        tuning::build(app);
        vehicle::build(app);

        // `physics` holds what spans both worlds; each dimension has its own.
        {
            let mut m = app.script_module("physics")?;
            install_world_controls(&mut *m);
            debug::install_debug_api(&mut *m);
            tuning::install_tuning_api(&mut *m);
        }
        let mut m = app.script_module("physics3d")?;
        m.module_doc(
            "The 3D rigid-body world: bodies and colliders on nodes, their \
             velocities, and overlap queries. `physics` holds what spans both \
             worlds.",
        );
        install_constants(&mut *m, BODY_KINDS, SHAPE_KINDS);
        body::install_body_api(&mut *m);
        body::install_force_api(&mut *m);
        body::install_force_reader_api(&mut *m);
        body::install_body_state_api(&mut *m);
        body::install_body_tuning_api(&mut *m);
        body::install_body_ccd_api(&mut *m);
        body::install_body_lock_api(&mut *m);
        body::install_body_pose_api(&mut *m);
        body::install_body_sleep_api(&mut *m);
        collider::install_collider_api(&mut *m);
        collider::install_voxel_api(&mut *m);
        collider::install_collider_reader_api(&mut *m);
        query::install_query_api(&mut *m);
        query::install_shapecast_api(&mut *m);
        query::install_volume_query_api(&mut *m);
        query::install_pair_query_api(&mut *m);
        query::install_world_list_api(&mut *m);
        joint::install_joint_api(&mut *m);
        character::install_character_api(&mut *m);
        vehicle::install_vehicle_api(&mut *m);
        body::register_body_component(app);
        collider::register_collider_component(app);
        joint::register_joint_component(app);
        character::register_character_component(app);
        vehicle::register_vehicle_components(app);
        register_physics_presets(app)?;

        {
            let mut m = app.script_module("geometry3d")?;
            geometry::install_geometry_api(&mut *m);
            geometry::install_mesh_edit_api(&mut *m);
        }

        dim2::build(app)?;
        Ok(())
    }
}

/// Velocity and sleep state, which no component `get` reports: two peers
/// can agree on every position and still be about to diverge.
fn build_physics_digest(app: &mut App) {
    app.add_digest_source("physics", |eng, out| {
        let Some(state) = eng.try_resource::<PhysicsState>() else {
            return;
        };
        let state = state.borrow();
        let world = eng.world();
        // One row for the whole world's shape edits, before the per-body rows.
        {
            let mut h = Hasher::new();
            h.write(&state.shape_revision.to_le_bytes());
            out.push(Entry {
                label: "physics/shapes".to_string(),
                digest: h.finish(),
            });
        }
        for (&entity, &handle) in &state.bodies {
            let body = &state.world.bodies[handle];
            let (v, w) = (body.linvel(), body.angvel());
            let mut h = Hasher::new();
            for value in [v.x, v.y, v.z, w.x, w.y, w.z] {
                // Whatever width this build runs at: a digest compares two
                // runs of the same engine, not an f32 run against an f64 one.
                h.write_f64(f64::from(value));
            }
            h.write(&[u8::from(body.is_sleeping())]);
            out.push(Entry {
                label: node_label(&world, entity),
                digest: h.finish(),
            });
        }
    });
}

/// The rapier world plus the maps tying it to entities.
///
/// The whole world rather than a per-body summary: rapier's own
/// `serde-serialize` skips exactly the workspace fields a snapshot must not
/// carry (the pipeline, the CCD solver), and reconstructing islands and
/// contact state by hand would be a second physics engine.
#[derive(serde::Deserialize)]
struct PhysicsFrame {
    world: PhysicsWorld,
    bodies: Vec<(u64, RigidBodyHandle)>,
    colliders: Vec<(u64, Vec<ColliderHandle>)>,
    joints: Vec<(u64, joint::JointRef)>,
    shape_revision: u64,
    paused: bool,
    sleeping_allowed: bool,
}

/// The save side borrows: a rollback ring holds many of these, and
/// `PhysicsWorld` is not `Clone` precisely because copying one is expensive.
#[derive(serde::Serialize)]
struct PhysicsFrameRef<'a> {
    world: &'a PhysicsWorld,
    bodies: Vec<(u64, RigidBodyHandle)>,
    colliders: Vec<(u64, Vec<ColliderHandle>)>,
    joints: Vec<(u64, joint::JointRef)>,
    shape_revision: u64,
    paused: bool,
    sleeping_allowed: bool,
}

fn build_physics_snapshot(app: &mut App) {
    app.add_snapshot_source(
        "physics",
        |eng| {
            let state = eng.resource::<PhysicsState>();
            let state = state.borrow();
            let frame = PhysicsFrameRef {
                world: &state.world,
                bodies: state
                    .bodies
                    .iter()
                    .map(|(e, h)| (e.to_bits().get(), *h))
                    .collect(),
                colliders: state
                    .colliders
                    .iter()
                    .map(|(e, h)| (e.to_bits().get(), h.clone()))
                    .collect(),
                joints: state
                    .joints
                    .iter()
                    .map(|(e, j)| (e.to_bits().get(), *j))
                    .collect(),
                shape_revision: state.shape_revision,
                paused: state.paused,
                sleeping_allowed: state.sleeping_allowed,
            };
            serde_json::to_value(frame).unwrap_or(serde_json::Value::Null)
        },
        |eng, value| {
            let frame: PhysicsFrame = match serde_json::from_value(value.clone()) {
                Ok(frame) => frame,
                Err(e) => {
                    tracing::error!(error = %e, "restoring the physics world");
                    return;
                }
            };
            let state = eng.resource::<PhysicsState>();
            let mut state = state.borrow_mut();
            state.world = frame.world;
            state.shape_revision = frame.shape_revision;
            state.paused = frame.paused;
            state.sleeping_allowed = frame.sleeping_allowed;
            state.bodies = frame
                .bodies
                .into_iter()
                .filter_map(|(bits, h)| Some((Entity::from_bits(bits)?, h)))
                .collect();
            state.colliders = frame
                .colliders
                .into_iter()
                .filter_map(|(bits, h)| Some((Entity::from_bits(bits)?, h)))
                .collect();
            state.joints = frame
                .joints
                .into_iter()
                .filter_map(|(bits, j)| Some((Entity::from_bits(bits)?, j)))
                .collect();
        },
    );
}

/// The 3D body presets. Both dimensions carry their marker (D5).
fn register_physics_presets(app: &mut App) -> Result<()> {
    app.register_preset(
        "rigid_body3d",
        balaur_core::presets::preset(
            "A body physics simulates, with a box collider",
            &["3d", "physics"],
            &[("body3d", Some("kind = \"dynamic\"")), ("collider3d", None)],
        )?,
    );
    app.register_preset(
        "static_body3d",
        balaur_core::presets::preset(
            "An immovable body with a box collider: ground, walls",
            &["3d", "physics"],
            &[("body3d", Some("kind = \"static\"")), ("collider3d", None)],
        )?,
    );
    Ok(())
}

pub(crate) fn node_pose(eng: &Engine, entity: Entity) -> Result<scalar::Pose> {
    // Make sure globals are up to date even before the first frame ran.
    let root = eng.root();
    balaur_core::scene::propagate_transforms(&mut eng.world_mut(), root);
    let world = eng.world();
    let global = world
        .get::<&balaur_core::GlobalTransform>(entity)
        .map_err(|_| anyhow!("node is dead or not in the scene tree"))?;
    Ok(scalar::pose_of(global.position, global.rotation))
}

fn step_system(eng: &Engine, _dt: f32) {
    resolve_pending_joints(eng);
    let events = {
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        if state.paused {
            return;
        }

        // Prune bodies whose node died, and feed kinematic targets in.
        {
            let world = eng.world();
            state.bodies.retain(|&entity, handle| {
                if !world.contains(entity) {
                    state.world.remove_body(*handle);
                    return false;
                }
                true
            });
            for (&entity, &handle) in &state.bodies {
                let body = &mut state.world.bodies[handle];
                if body.is_kinematic() {
                    if let Ok(t) = world.get::<&Transform>(entity) {
                        body.set_next_kinematic_position(scalar::pose_of(t.position, t.rotation));
                    }
                }
            }
        }

        // Exactly one step: Stage::FixedUpdate already repeats at FIXED_DT, and a
        // second accumulator here would drift out of step with the scripts.
        state.world.integration_parameters.dt = scalar::real(FIXED_DT);
        // The step rebuilds the broad phase itself.
        state.queries_ready = true;
        let collector = events::Collector::default();
        state
            .world
            .step_with_events(&events::Hooks { eng }, &collector);

        // Write simulated poses back to the scene tree.
        let world = eng.world();
        for (&entity, &handle) in &state.bodies {
            let body = &state.world.bodies[handle];
            if body.is_fixed() || body.is_kinematic() {
                continue;
            }
            if let Ok(mut t) = world.get::<&mut Transform>(entity) {
                t.position = scalar::position_of(body.translation());
                t.rotation = scalar::quat_of(*body.rotation());
            }
        }
        (collector.take(), joint::broken(state))
    };
    // Delivered with the world no longer borrowed: a handler is ordinary
    // script code and may move the body it was just told about.
    events::deliver(eng, &events.0);
    break_joints(eng, &events.1);
    // Rapier disables a body whose pose went non-finite rather than letting
    // the world become NaN. A game that never asks still deserves to be told.
    tuning::warn_about_quarantine(eng);
}

/// Make the joints whose other end had not been spawned when they were
/// applied.
///
/// A scene file names nodes in whatever order it likes, so a joint that points
/// forwards is normal rather than an error; it is made on the first step after
/// its partner exists.
fn resolve_pending_joints(eng: &Engine) {
    let pending = {
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        joint::pending(&state)
    };
    for entity in pending {
        let params = {
            let state = eng.resource::<PhysicsState>();
            let state = state.borrow();
            state.joint_params.get(&entity).cloned()
        };
        let Some(params) = params else { continue };
        if let Err(why) = joint::apply_joint(eng, entity, &params) {
            tracing::debug!("joint3d is still waiting: {why:#}");
        }
    }
}

/// Remove the joints that gave way this step and tell both ends.
///
/// A break is an event in every way that matters, so it travels the same
/// path: after the step, in entity order, through the node's own script.
fn break_joints(eng: &Engine, broken: &[balaur_core::hecs::Entity]) {
    for entity in broken {
        joint::remove_joint(eng, *entity);
        if let Some(host) = eng.script_host() {
            host.call_on(balaur_core::node_id_of(*entity), "on_joint_break", &[]);
        }
    }
}

/// Pause, sleeping and gravity.
fn install_world_controls(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "What spans both physics worlds at once: pausing, sleeping and \
         clearing. Bodies and colliders live in `physics2d` and `physics3d`.",
    );
    m.describe(&[
        ("set_paused", &[], "", "Stop or resume stepping both worlds; nodes keep their poses."),
        ("is_paused", &[], "", "Whether stepping is stopped."),
        ("set_sleeping_allowed", &[], "", "Allow or forbid resting bodies falling asleep, in both worlds and for bodies added later."),
        ("sleeping_allowed", &[], "", "Whether resting bodies are allowed to fall asleep."),
        ("clear", &[], "", "Remove every body and collider from both worlds, as a play-in-editor session does on stop."),
    ]);
    // Everything here spans BOTH worlds: editors and games treat
    // "physics" as one simulation. Per-dimension calls live in physics3d/2d.
    m.function("set_paused", |eng: &Engine, paused: bool| {
        let state = eng.resource::<PhysicsState>();
        state.borrow_mut().paused = paused;
        dim2::set_paused(eng, paused);
        Ok(())
    });
    // Spans BOTH worlds: `set_paused` pauses them together, so one answer
    // is the truth for both.
    m.function("is_paused", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let v = state.borrow().paused;
        Ok(v)
    });
    // Allow or forbid bodies falling asleep (editors expose this as the
    // "Sleep bodies" toggle). Spans BOTH worlds, and applies to bodies
    // added later as well as to the ones alive now.
    m.function("set_sleeping_allowed", |eng: &Engine, allowed: bool| {
        use crate::rapier3d::prelude::RigidBodyActivation;
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        state.sleeping_allowed = allowed;
        let handles: Vec<_> = state.bodies.values().copied().collect();
        for handle in handles {
            let body = &mut state.world.bodies[handle];
            *body.activation_mut() = if allowed {
                RigidBodyActivation::default()
            } else {
                body.wake_up(true);
                RigidBodyActivation::cannot_sleep()
            };
        }
        drop(state);
        dim2::set_sleeping_allowed(eng, allowed);
        Ok(())
    });
    // Reads back what `set_sleeping_allowed` last wrote (true by default).
    // Spans BOTH worlds, because the setter writes both.
    m.function("sleeping_allowed", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let v = state.borrow().sleeping_allowed;
        Ok(v)
    });
    // Remove every body and collider (editors use this to reset a
    // play-in-editor session). Spans BOTH worlds.
    m.function("clear", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        // A fresh world, not a drained one: see `dim2::clear` for why a
        // rebuilt scene would otherwise not simulate the way a fresh process
        // does. Gravity and the step are settings, and carry over.
        let gravity = state.world.gravity;
        let params = state.world.integration_parameters;
        state.world = PhysicsWorld::default();
        state.world.gravity = gravity;
        state.world.integration_parameters = params;
        state.bodies.clear();
        state.colliders.clear();
        // Rapier drops a body's joints with the body, so the map is all that
        // is left to clear.
        state.joints.clear();
        state.collider_params.clear();
        state.joint_params.clear();
        drop(state);
        dim2::clear(eng);
        Ok(())
    });
}

/// Body kinds the 3D and 2D worlds both accept, so a script writes
/// `physics3d.BODY_DYNAMIC` rather than spelling "dynamic" and finding out at
/// runtime that "Dynamic" silently fell through to the default.
pub const BODY_KINDS: &[(&str, &str)] = &[
    ("BODY_DYNAMIC", "dynamic"),
    ("BODY_STATIC", "static"),
    ("BODY_KINEMATIC", "kinematic"),
    ("BODY_KINEMATIC_VELOCITY", "kinematic_velocity"),
];

/// Collider shapes for the 3D world.
pub const SHAPE_KINDS: &[(&str, &str)] = &[("SHAPE_BALL", "ball"), ("SHAPE_CUBOID", "cuboid")];

/// Collider shapes for the 2D world.
pub const SHAPE_KINDS_2D: &[(&str, &str)] = &[("SHAPE_CIRCLE", "circle"), ("SHAPE_RECT", "rect")];

pub(crate) fn install_constants(
    m: &mut dyn Bindings<Engine>,
    bodies: &[(&str, &str)],
    shapes: &[(&str, &str)],
) {
    for (name, value) in bodies.iter().chain(shapes) {
        m.constant(name, balaur_script::Value::Str((*value).to_string()));
    }
}
