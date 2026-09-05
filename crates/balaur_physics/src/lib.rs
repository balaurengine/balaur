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
use balaur_plugin::Registry;

use balaur_core::collections::DetHashMap;

use balaur_core::hecs::Entity;

use balaur_core::{Engine, Stage, Transform};

use balaur_script::{Bindings, BindingsExt};

use crate::rapier3d::pipeline::PhysicsWorld;

use crate::rapier3d::prelude::{ColliderHandle, RigidBodyHandle};

// The `f64` build swaps rapier for its f64 twin under the same names, so
// every `use crate::rapier3d::..` below and in the submodules follows the scalar.
// `scalar.rs` is the seam where a number changes width.
pub use ::rapier2d;
pub use ::rapier3d;

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
mod shared;
pub mod tuning;
pub mod vehicle;
mod vocabulary;
use crate::vocabulary::words;

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
    /// What the last `move_character` found under each character's feet, so
    /// `is_grounded` can answer without sweeping the shape again.
    pub grounded: DetHashMap<Entity, bool>,
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
            grounded: DetHashMap::default(),
            shape_revision: 0,
            sleeping_allowed: true,
        }
    }
}

pub struct PhysicsPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for PhysicsPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("physics", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for PhysicsPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        balaur_core::settings::define_group(
            reg.engine(),
            "physics",
            balaur_core::settings::Scope::Project,
            &balaur_core::ComponentDef::parse_schema(
                "settings.physics",
                r#"
solver_iterations = { type = "float", default = 4.0, min = 1.0, max = 64.0, help = "Solver iterations per step. More is stabler and slower; every value here changes results, so a recording only replays against the same numbers." }
ccd_substeps = { type = "float", default = 1.0, min = 0.0, max = 16.0, help = "Substeps for continuous collision detection, which stops a fast body tunnelling through a thin one." }
length_unit = { type = "float", default = 1.0, min = 0.000001, max = 1000.0, help = "How many of the game's units make a metre. A pixel game sets this rather than scaling every body." }
contact_clustering = { type = "bool", default = true, help = "Group contacts before solving them." }
contact_recycling = { type = "bool", default = true, help = "Reuse contact state between steps." }
allowed_linear_error = { type = "float", default = 0.001, min = 0.0, max = 1.0, help = "How far a body may sink into another before the solver pushes back." }
max_corrective_velocity = { type = "float", default = 10.0, min = 0.0, max = 1000.0, help = "A cap on how fast the solver may push overlapping bodies apart." }
prediction_distance = { type = "float", default = 0.002, min = 0.0, max = 1.0, help = "How far ahead contacts are predicted." }
"#,
            ),
        );
        reg.insert_resource(PhysicsState::new());
        reg.add_system(Stage::FixedUpdate, step_system);
        build_physics_digest(reg);
        build_physics_snapshot(reg);
        debug::build(reg);
        tuning::build(reg);
        vehicle::build(reg);

        // `physics` holds what spans both worlds; each dimension has its own.
        {
            let mut m = reg.script_module("physics")?;
            install_world_controls(&mut *m);
            debug::install_debug_api(&mut *m);
            tuning::install_tuning_api(&mut *m);
        }
        let mut m = reg.script_module("physics3d")?;
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
        body::register_body_component(reg);
        collider::register_collider_component(reg);
        joint::register_joint_component(reg);
        character::register_character_component(reg);
        vehicle::register_vehicle_components(reg);
        register_physics_presets(reg)?;

        {
            let mut m = reg.script_module("geometry3d")?;
            geometry::install_geometry_api(&mut *m);
            geometry::install_mesh_edit_api(&mut *m);
        }

        dim2::build(reg)?;
        Ok(())
    }
}

/// Velocity and sleep state, which no component `get` reports: two peers
/// can agree on every position and still be about to diverge.
fn build_physics_digest(reg: &mut Registry<'_>) {
    reg.add_digest_source("physics", |eng, out| {
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
    bodies: Vec<(NodeKey, RigidBodyHandle)>,
    colliders: Vec<(NodeKey, Vec<ColliderHandle>)>,
    joints: Vec<(NodeKey, joint::JointRef)>,
    collider_params: Vec<(NodeKey, toml::Value)>,
    joint_params: Vec<(NodeKey, toml::Value)>,
    wheel_inputs: Vec<(NodeKey, vehicle::WheelInput)>,
    grounded: Vec<(NodeKey, bool)>,
    shape_revision: u64,
    paused: bool,
    sleeping_allowed: bool,
}

/// The save side borrows: a rollback ring holds many of these, and
/// `PhysicsWorld` is not `Clone` precisely because copying one is expensive.
#[derive(serde::Serialize)]
struct PhysicsFrameRef<'a> {
    world: &'a PhysicsWorld,
    bodies: Vec<(NodeKey, RigidBodyHandle)>,
    colliders: Vec<(NodeKey, Vec<ColliderHandle>)>,
    joints: Vec<(NodeKey, joint::JointRef)>,
    collider_params: Vec<(NodeKey, toml::Value)>,
    joint_params: Vec<(NodeKey, toml::Value)>,
    wheel_inputs: Vec<(NodeKey, vehicle::WheelInput)>,
    grounded: Vec<(NodeKey, bool)>,
    shape_revision: u64,
    paused: bool,
    sleeping_allowed: bool,
}

/// How a snapshot names a node: its [`balaur_core::ids`] id, and its entity
/// bits for a tree built by hand. Entity bits alone would not survive a
/// respawn, which mints a new entity for the same node.
pub(crate) type NodeKey = (String, u64);

pub(crate) fn key_of(world: &balaur_core::hecs::World, entity: Entity) -> NodeKey {
    (
        balaur_core::ids::of(world, entity).unwrap_or_default(),
        entity.to_bits().get(),
    )
}

/// The map a snapshot row belongs to, as keys a respawn cannot invalidate.
pub(crate) fn keyed<V: Clone>(
    world: &balaur_core::hecs::World,
    map: &DetHashMap<Entity, V>,
) -> Vec<(NodeKey, V)> {
    map.iter()
        .map(|(entity, value)| (key_of(world, *entity), value.clone()))
        .collect()
}

/// The node a key names now, which is a different entity after a respawn.
pub(crate) fn resolve_key(eng: &Engine, key: &NodeKey) -> Option<Entity> {
    let root = eng.root();
    let world = eng.world();
    if !key.0.is_empty() {
        if let Some(entity) = balaur_core::ids::find(&world, root, &key.0) {
            return Some(entity);
        }
    }
    let entity = Entity::from_bits(key.1)?;
    world.contains(entity).then_some(entity)
}

pub(crate) fn resolved<V>(eng: &Engine, rows: Vec<(NodeKey, V)>) -> DetHashMap<Entity, V> {
    rows.into_iter()
        .filter_map(|(key, value)| Some((resolve_key(eng, &key)?, value)))
        .collect()
}

fn save_physics(eng: &Engine) -> serde_json::Value {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let world = eng.world();
    let frame = PhysicsFrameRef {
        world: &state.world,
        bodies: keyed(&world, &state.bodies),
        colliders: keyed(&world, &state.colliders),
        joints: keyed(&world, &state.joints),
        collider_params: keyed(&world, &state.collider_params),
        joint_params: keyed(&world, &state.joint_params),
        wheel_inputs: keyed(&world, &state.wheel_inputs),
        grounded: keyed(&world, &state.grounded),
        shape_revision: state.shape_revision,
        paused: state.paused,
        sleeping_allowed: state.sleeping_allowed,
    };
    serde_json::to_value(frame).unwrap_or(serde_json::Value::Null)
}

fn load_physics(eng: &Engine, value: &serde_json::Value) {
    let frame: PhysicsFrame = match serde_json::from_value(value.clone()) {
        Ok(frame) => frame,
        Err(e) => {
            tracing::error!(error = %e, "restoring the physics world");
            return;
        }
    };
    let bodies = resolved(eng, frame.bodies);
    let colliders = resolved(eng, frame.colliders);
    let joints = resolved(eng, frame.joints);
    let collider_params = resolved(eng, frame.collider_params);
    let joint_params = resolved(eng, frame.joint_params);
    let wheel_inputs = resolved(eng, frame.wheel_inputs);
    let grounded = resolved(eng, frame.grounded);
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    state.world = frame.world;
    state.shape_revision = frame.shape_revision;
    state.paused = frame.paused;
    state.sleeping_allowed = frame.sleeping_allowed;
    state.bodies = bodies;
    state.colliders = colliders;
    state.joints = joints;
    state.collider_params = collider_params;
    state.joint_params = joint_params;
    state.wheel_inputs = wheel_inputs;
    state.grounded = grounded;
    restamp_collider_owners(&mut state);
}

crate::shared::world::functions!(
    state = PhysicsState,
    component = "joint3d",
    prune = prune_freed_nodes_except_wheels
);

pub(crate) fn prune_freed_nodes(eng: &Engine, state: &mut PhysicsState) {
    prune_freed_nodes_except_wheels(eng, state);
    let world = eng.world();
    state.wheel_inputs.retain(|e, _| world.contains(*e));
}

fn build_physics_snapshot(reg: &mut Registry<'_>) {
    reg.add_snapshot_source("physics", save_physics, load_physics);
}

/// The 3D body presets. Both dimensions carry their marker (D5).
fn register_physics_presets(reg: &mut Registry<'_>) -> Result<()> {
    reg.register_preset(
        "rigid_body3d",
        balaur_core::presets::preset(
            "A body physics simulates, with a box collider",
            &["3d", "physics"],
            &[("body3d", Some("kind = \"dynamic\"")), ("collider3d", None)],
        )?,
    );
    reg.register_preset(
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
    {
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        prune_freed_nodes(eng, &mut state);
    }
    resolve_pending_joints(eng);
    let events = {
        let state = eng.resource::<PhysicsState>();
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
        state.world.step_with_events(&events::Hooks, &collector);

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
        state.wheel_inputs.clear();
        state.grounded.clear();
        drop(state);
        dim2::clear(eng);
        Ok(())
    });
}

/// Body kinds the 3D and 2D worlds both accept, so a script writes
/// `physics3d.BODY_DYNAMIC` rather than spelling "dynamic" and finding out at
/// runtime that "Dynamic" silently fell through to the default.
pub const BODY_KINDS: &[(&str, &str)] = &[
    ("BODY_DYNAMIC", words::DYNAMIC),
    ("BODY_STATIC", words::STATIC),
    ("BODY_KINEMATIC", words::KINEMATIC),
    ("BODY_KINEMATIC_VELOCITY", words::KINEMATIC_VELOCITY),
];

/// Collider shapes for the 3D world.
pub const SHAPE_KINDS: &[(&str, &str)] =
    &[("SHAPE_BALL", words::BALL), ("SHAPE_CUBOID", words::CUBOID)];

/// Collider shapes for the 2D world.
pub const SHAPE_KINDS_2D: &[(&str, &str)] =
    &[("SHAPE_CIRCLE", words::CIRCLE), ("SHAPE_RECT", words::RECT)];

pub(crate) fn install_constants(
    m: &mut dyn Bindings<Engine>,
    bodies: &[(&str, &str)],
    shapes: &[(&str, &str)],
) {
    for (name, value) in bodies.iter().chain(shapes) {
        m.constant(name, balaur_script::Value::Str((*value).to_string()));
    }
}
