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
use anyhow::{anyhow, Result};
use balaur_core::collections::DetHashMap;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine, Stage, Transform};
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::{EulerRot, Quat};

pub mod body;
pub mod character;
pub mod collider;
pub mod events;
pub mod joint;
pub mod query;

use body::{add_body, with_body};
use collider::{add_collider, collider_builder, max_contact_impulse};
pub use query::overlaps;
use query::overlaps_value;

use balaur_core::digest::{node_label, Entry, Hasher};

use balaur_core::FIXED_DT;

pub struct PhysicsState2d {
    pub world: PhysicsWorld2,
    pub bodies: DetHashMap<Entity, RigidBodyHandle2>,
    pub colliders: DetHashMap<Entity, Vec<ColliderHandle2>>,
    /// Whether the broad phase's tree matches the colliders, as in 3D.
    pub queries_ready: bool,
    /// Joints per entity, as in the 3D world.
    pub joints: DetHashMap<Entity, joint::JointRef2d>,
    /// What each collider and joint was authored from, as in the 3D world:
    /// rapier keeps the shape, not the asset or the choices behind it.
    pub collider_params: DetHashMap<Entity, toml::Value>,
    pub joint_params: DetHashMap<Entity, toml::Value>,
    pub paused: bool,
    /// Mirrors `PhysicsState::sleeping_allowed`; `physics.set_sleeping_allowed`
    /// writes both worlds.
    pub sleeping_allowed: bool,
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
            queries_ready: false,
            joints: DetHashMap::default(),
            collider_params: DetHashMap::default(),
            joint_params: DetHashMap::default(),
            paused: false,
            sleeping_allowed: true,
        }
    }
}

/// The node's global pose flattened to 2D (x, y, angle about z).
pub(crate) fn node_pose_2d(eng: &Engine, entity: Entity) -> Result<Pose2> {
    let root = eng.root();
    balaur_core::scene::propagate_transforms(&mut eng.world_mut(), root);
    let world = eng.world();
    let global = world
        .get::<&balaur_core::GlobalTransform>(entity)
        .map_err(|_| anyhow!("node is dead or not in the scene tree"))?;
    let (angle, _, _) = global.rotation.to_euler(EulerRot::ZYX);
    Ok(Pose2::from_parts(
        scalar::v2(global.position.x, global.position.y),
        Rotation2::from_angle(scalar::real(angle)),
    ))
}

/// Make the 2D joints whose other end had not been spawned when they were
/// applied — a scene file names nodes in whatever order it likes.
fn resolve_pending_joints(eng: &Engine) {
    let pending = {
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        joint::pending(&state)
    };
    for entity in pending {
        let params = {
            let state = eng.resource::<PhysicsState2d>();
            let state = state.borrow();
            state.joint_params.get(&entity).cloned()
        };
        let Some(params) = params else { continue };
        if let Err(why) = joint::apply_joint(eng, entity, &params) {
            tracing::debug!("joint2d is still waiting: {why:#}");
        }
    }
}

fn step_system(eng: &Engine, _dt: f32) {
    resolve_pending_joints(eng);
    let events = {
        let state = eng.resource::<PhysicsState2d>();
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
                        let (angle, _, _) = t.rotation.to_euler(EulerRot::ZYX);
                        body.set_next_kinematic_position(Pose2::from_parts(
                            scalar::v2(t.position.x, t.position.y),
                            Rotation2::from_angle(scalar::real(angle)),
                        ));
                    }
                }
            }
        }

        // Exactly one step: Stage::FixedUpdate already repeats at FIXED_DT, and a
        // second accumulator here would drift out of step with the scripts.
        state.world.integration_parameters.dt = scalar::real(FIXED_DT);
        let collector = events::Collector::default();
        state
            .world
            .step_with_events(&events::Hooks { eng }, &collector);

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
        (collector.take(), joint::broken(state))
    };
    events::deliver(eng, events.0);
    for entity in &events.1 {
        joint::remove_joint(eng, *entity);
        if let Some(host) = eng.script_host() {
            host.call_on(balaur_core::node_id_of(*entity), "on_joint_break", &[]);
        }
    }
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
    state.joints.clear();
}

pub fn set_paused(eng: &Engine, paused: bool) {
    let state = eng.resource::<PhysicsState2d>();
    state.borrow_mut().paused = paused;
}

pub fn set_sleeping_allowed(eng: &Engine, allowed: bool) {
    use crate::rapier2d::prelude::RigidBodyActivation;
    let state = eng.resource::<PhysicsState2d>();
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
}

pub fn build(app: &mut App) -> Result<()> {
    build_physics2d(app);

    {
        let mut m = app.script_module("physics2d")?;
        crate::install_constants(&mut *m, crate::BODY_KINDS, crate::SHAPE_KINDS_2D);
        install_physics2d_api(&mut *m);
        body::install_body2d_force_api(&mut *m);
        body::install_body2d_state_api(&mut *m);
        body::install_body2d_tuning_api(&mut *m);
        body::install_body2d_ccd_api(&mut *m);
        body::install_body2d_lock_api(&mut *m);
        body::install_body2d_sleep_api(&mut *m);
        body::install_body2d_force_reader_api(&mut *m);
        query::install_physics2d_query_api(&mut *m);
        query::install_physics2d_shapecast_api(&mut *m);
        query::install_physics2d_volume_query_api(&mut *m);
        query::install_physics2d_shape_query_api(&mut *m);
        query::install_physics2d_pair_query_api(&mut *m);
        joint::install_joint2d_api(&mut *m);
        character::install_character2d_api(&mut *m);
    }
    body::register_body2d_component(app);
    collider::register_collider2d_component(app);
    joint::register_joint2d_component(app);
    character::register_character2d_component(app);

    app.register_preset(
        "rigid_body2d",
        balaur_core::presets::preset(
            "A 2D body physics simulates, with a rect collider",
            &["2d", "physics"],
            &[("body2d", Some("kind = \"dynamic\"")), ("collider2d", None)],
        )?,
    );
    app.register_preset(
        "static_body2d",
        balaur_core::presets::preset(
            "An immovable 2D body with a rect collider: ground, walls",
            &["2d", "physics"],
            &[("body2d", Some("kind = \"static\"")), ("collider2d", None)],
        )?,
    );

    Ok(())
}

/// The 2D world and the system that steps it, mirroring `PhysicsPlugin::build`.
fn build_physics2d(app: &mut App) {
    app.engine.insert_resource(PhysicsState2d::new());
    app.add_system(Stage::FixedUpdate, step_system);
    build_physics2d_digest(app);
    build_physics2d_snapshot(app);
}

/// The 2D twin of the 3D snapshot source.
#[derive(serde::Deserialize)]
struct PhysicsFrame2d {
    world: PhysicsWorld2,
    bodies: Vec<(u64, RigidBodyHandle2)>,
    colliders: Vec<(u64, Vec<ColliderHandle2>)>,
    joints: Vec<(u64, joint::JointRef2d)>,
    paused: bool,
    sleeping_allowed: bool,
}

#[derive(serde::Serialize)]
struct PhysicsFrameRef2d<'a> {
    world: &'a PhysicsWorld2,
    bodies: Vec<(u64, RigidBodyHandle2)>,
    colliders: Vec<(u64, Vec<ColliderHandle2>)>,
    joints: Vec<(u64, joint::JointRef2d)>,
    paused: bool,
    sleeping_allowed: bool,
}

fn build_physics2d_snapshot(app: &mut App) {
    app.add_snapshot_source(
        "physics2d",
        |eng| {
            let state = eng.resource::<PhysicsState2d>();
            let state = state.borrow();
            let frame = PhysicsFrameRef2d {
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
                paused: state.paused,
                sleeping_allowed: state.sleeping_allowed,
            };
            serde_json::to_value(frame).unwrap_or(serde_json::Value::Null)
        },
        |eng, value| {
            let frame: PhysicsFrame2d = match serde_json::from_value(value.clone()) {
                Ok(frame) => frame,
                Err(e) => {
                    tracing::error!(error = %e, "restoring the 2D physics world");
                    return;
                }
            };
            let state = eng.resource::<PhysicsState2d>();
            let mut state = state.borrow_mut();
            state.world = frame.world;
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

/// The 2D twin of the 3D source: velocity and sleep state, which no
/// component `get` reports.
fn build_physics2d_digest(app: &mut App) {
    app.add_digest_source("physics2d", |eng, out| {
        let Some(state) = eng.try_resource::<PhysicsState2d>() else {
            return;
        };
        let state = state.borrow();
        let world = eng.world();
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
    });
}

/// `physics2d`: bodies, colliders, gravity, velocities, contact impulse and
/// overlap queries.
fn install_physics2d_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "The 2D rigid-body world: bodies and colliders on nodes, their \
         velocities, and overlap queries. `physics` holds what spans both \
         worlds.",
    );
    m.describe(&[
        ("add_body", &["body2d"], "", "Give the node a 2D rigid body of the given kind (`BODY_DYNAMIC`, `BODY_STATIC`, `BODY_KINEMATIC`)."),
        ("add_collider", &["collider2d"], "", "Attach a 2D collider from a `collider2d` table: `kind`, `radius`, `half_extents`, `friction`, and the rest of the component's own vocabulary."),
        ("set_gravity", &[], "", "Set the 2D world's gravity, in units per second squared."),
        ("apply_impulse", &["body2d"], "", "Add an instant change in momentum, as if the body were struck."),
        ("set_linear_velocity", &["body2d"], "", "Set how fast the body travels, in units per second."),
        ("linear_velocity", &["body2d"], "", "How fast the body is travelling, in units per second."),
        ("set_angular_velocity", &["body2d"], "", "Set how fast the body spins, in radians per second."),
        ("angular_velocity", &["body2d"], "", "How fast the body is spinning, in radians per second."),
        ("max_contact_impulse", &["body2d"], "", "The hardest contact this body took in the last step, zero when nothing touched it."),
        ("overlaps", &["collider2d"], "", "The nodes this one currently intersects; rapier reports a pair only when one of the two colliders is a sensor."),
    ]);
    // Constructors, so a 2D body can be built from script rather than only
    // declared in a scene file.
    m.function(
        "add_body",
        |eng: &Engine, (node, kind): (NodeId, String)| add_body(eng, entity_of(node)?, &kind),
    );
    // Takes the `collider2d` component's own table (`kind`, `radius`,
    // `half_extents`, `restitution`, `friction`, `density`), so one
    // vocabulary covers scripts and scene files.
    m.function(
        "add_collider",
        |eng: &Engine, (node, params): (NodeId, balaur_script::Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            add_collider(eng, entity_of(node)?, collider_builder(eng, &params)?)
        },
    );
    // No reader by design (N8): `PhysicsState2d`'s rapier world already
    // holds the gravity vector; add `physics2d.gravity` when a caller needs
    // to read it back.
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
