//! The `body3d` component and the script calls that drive one.
//!
//! Split from `lib.rs` under `MAX_FILE_LINES`: bodies, colliders and the
//! plugin itself are three subjects, and only the plugin needs all three.

use crate::rapier3d::prelude::{
    ColliderBuilder, LockedAxes, MassProperties, RigidBody, RigidBodyActivation, RigidBodyBuilder,
    RigidBodyHandle, RigidBodyType,
};
use crate::scalar::{self, Real, Vector};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::collider::{add_collider, apply_collider, get_collider_params};
use crate::query::overlaps_value;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};
use crate::{PhysicsState, node_pose};

/// The schema `body3d` and `body2d` share, minus the dimension-shaped
/// properties each adds for itself. One text, so a property cannot mean two
/// things in two dimensions.
pub(crate) fn shared_body_schema() -> String {
    v::schema(&[
        (k::LINEAR_DAMPING, r#"{ type = "float", default = 0.0, min = 0.0, description = "Drag on travel: how fast the body loses speed with nothing touching it" }"#),
        (k::ANGULAR_DAMPING, r#"{ type = "float", default = 0.0, min = 0.0, description = "Drag on spin, in the same terms as linear_damping" }"#),
        (k::GRAVITY_SCALE, r#"{ type = "float", default = 1.0, description = "Multiplier on world gravity for this body: 0 hangs in the air, negative floats up" }"#),
        (k::MASS, r#"{ type = "float", default = 0.0, min = 0.0, description = "Extra mass on top of what the colliders' density gives; 0 leaves the body at its collider mass" }"#),
        (k::DOMINANCE, r#"{ type = "float", default = 0.0, min = -127.0, max = 127.0, description = "A body in a higher group is unpushable by a lower one; every non-dynamic body outranks them all" }"#),
        (k::SOLVER_ITERATIONS, r#"{ type = "float", default = 0.0, min = 0.0, description = "Extra solver iterations for this body alone, for the one stack that jitters" }"#),
        (k::CCD, r#"{ type = "bool", default = false, description = "Sweep the body's whole path each step so a fast one cannot pass through a wall" }"#),
        (k::SOFT_CCD, r#"{ type = "float", default = 0.0, min = 0.0, description = "Distance ahead the body predicts contacts, in units; cheaper than ccd for merely fast bodies" }"#),
        (k::FAST_ROTATION, r#"{ type = "bool", default = false, description = "Allow a spin fast enough that rapier would otherwise clamp it" }"#),
        (k::CAN_SLEEP, r#"{ type = "bool", default = true, description = "Let the body stop being simulated once it has held still" }"#),
        (k::SLEEP_TIME, r#"{ type = "float", default = 0.5, min = 0.0, description = "Seconds of stillness before the body sleeps" }"#),
        (k::ENABLED, r#"{ type = "bool", default = true, description = "Simulate this body at all; a disabled body keeps its state and costs nothing" }"#),
    ])
}

crate::shared::body::functions!(
    state = PhysicsState,
    handle = RigidBodyHandle,
    builder = RigidBodyBuilder,
    node_pose = node_pose,
    missing = "node has no rigid body"
);

/// The axes a `flags` property locks, as rapier's flag set.
fn locked_axes(params: &toml::Value) -> LockedAxes {
    let mut axes = LockedAxes::empty();
    for (key, x, y, z) in [
        (
            k::LOCK_TRANSLATION,
            LockedAxes::TRANSLATION_LOCKED_X,
            LockedAxes::TRANSLATION_LOCKED_Y,
            LockedAxes::TRANSLATION_LOCKED_Z,
        ),
        (
            k::LOCK_ROTATION,
            LockedAxes::ROTATION_LOCKED_X,
            LockedAxes::ROTATION_LOCKED_Y,
            LockedAxes::ROTATION_LOCKED_Z,
        ),
    ] {
        for (name, flag) in [(w::X, x), (w::Y, y), (w::Z, z)] {
            if v::flag(params, key, name) {
                axes |= flag;
            }
        }
    }
    axes
}

/// Every schema property onto a body that already exists.
///
/// Writing rather than rebuilding is what lets a scene change a body's kind
/// mid-flight without dropping its velocity — and what keeps `patch` (one
/// property at a time, from an animation) honest.
pub(crate) fn write_body(body: &mut RigidBody, params: &toml::Value, world_may_sleep: bool) {
    if let Ok(kind) = body_type(v::text(params, k::KIND, w::DYNAMIC))
        && body.body_type() != kind {
            body.set_body_type(kind, true);
        }
    body.set_linear_damping(scalar::real(v::f(params, k::LINEAR_DAMPING, 0.0)));
    body.set_angular_damping(scalar::real(v::f(params, k::ANGULAR_DAMPING, 0.0)));
    body.set_gravity_scale(scalar::real(v::f(params, k::GRAVITY_SCALE, 1.0)), true);
    body.set_dominance_group(v::f(params, k::DOMINANCE, 0.0).clamp(-127.0, 127.0) as i8);
    body.set_additional_solver_iterations(v::f(params, k::SOLVER_ITERATIONS, 0.0).max(0.0) as usize);
    body.set_locked_axes(locked_axes(params), true);
    body.enable_ccd(v::boolean(params, k::CCD, false));
    body.set_soft_ccd_prediction(scalar::real(v::f(params, k::SOFT_CCD, 0.0)));
    body.set_allow_fast_rotation(v::boolean(params, k::FAST_ROTATION, false));
    body.enable_gyroscopic_forces(v::boolean(params, k::GYROSCOPIC, false));
    body.set_enabled(v::boolean(params, k::ENABLED, true));
    write_mass(body, params);
    // The world-wide toggle wins while it is off: `physics.set_sleeping_allowed`
    // is what an editor's "Sleep bodies" switch writes, and a per-body opinion
    // must not quietly re-enable sleeping under it.
    let may_sleep = world_may_sleep && v::boolean(params, k::CAN_SLEEP, true);
    *body.activation_mut() = if may_sleep {
        let mut activation = RigidBodyActivation::default();
        activation.time_until_sleep = scalar::real(v::f(params, k::SLEEP_TIME, 0.5).max(0.0));
        activation
    } else {
        body.wake_up(true);
        RigidBodyActivation::cannot_sleep()
    };
}

/// `mass` is *additional* mass, so 0 means "whatever the colliders weigh" —
/// which is what a body with no mass property has always meant here.
/// Whether the author left a property at its all-zero default. An exact test
/// on purpose: this asks what the file says, not how big a number is.
pub(crate) fn is_default(v: &[f32]) -> bool {
    v.iter().all(|n| *n == 0.0)
}

fn write_mass(body: &mut RigidBody, params: &toml::Value) {
    let mass = v::f(params, k::MASS, 0.0).max(0.0);
    let inertia = v::vec3(params, k::INERTIA, [0.0; 3]);
    let com = v::vec3(params, k::CENTER_OF_MASS, [0.0; 3]);
    if mass <= 0.0 {
        body.set_additional_mass(0.0, false);
    } else if is_default(&inertia) && is_default(&com) {
        // Rapier scales the angular inertia with the mass, which is what an
        // author who only wrote `mass = 5` means.
        body.set_additional_mass(scalar::real(mass), true);
    } else {
        let com = scalar::v3a(com);
        let inertia = scalar::v3a(inertia);
        body.set_additional_mass_properties(
            MassProperties::new(com, scalar::real(mass), inertia),
            true,
        );
    }
}

/// Every property `apply` writes, read back off the body.
pub(crate) fn get_body_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let body = &state.world.bodies[*state.bodies.get(&entity)?];
    let axes = body.locked_axes();
    let flags = |x: LockedAxes, y: LockedAxes, z: LockedAxes| {
        toml::Value::Array(
            [(w::X, x), (w::Y, y), (w::Z, z)]
                .into_iter()
                .filter(|(_, flag)| axes.contains(*flag))
                .map(|(name, _)| toml::Value::String(name.to_string()))
                .collect(),
        )
    };
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let mut map = toml::map::Map::new();
    map.insert(k::KIND.into(), kind_name(body).into());
    map.insert(k::LINEAR_DAMPING.into(), f(body.linear_damping()));
    map.insert(k::ANGULAR_DAMPING.into(), f(body.angular_damping()));
    map.insert(k::GRAVITY_SCALE.into(), f(body.gravity_scale()));
    map.insert(k::DOMINANCE.into(), f(Real::from(body.dominance_group())));
    map.insert(
        k::SOLVER_ITERATIONS.into(),
        f(body.additional_solver_iterations() as Real),
    );
    map.insert(
        k::LOCK_TRANSLATION.into(),
        flags(
            LockedAxes::TRANSLATION_LOCKED_X,
            LockedAxes::TRANSLATION_LOCKED_Y,
            LockedAxes::TRANSLATION_LOCKED_Z,
        ),
    );
    map.insert(
        k::LOCK_ROTATION.into(),
        flags(
            LockedAxes::ROTATION_LOCKED_X,
            LockedAxes::ROTATION_LOCKED_Y,
            LockedAxes::ROTATION_LOCKED_Z,
        ),
    );
    map.insert(k::CCD.into(), body.is_ccd_enabled().into());
    map.insert(k::SOFT_CCD.into(), f(body.soft_ccd_prediction()));
    map.insert(
        k::FAST_ROTATION.into(),
        body.is_fast_rotation_allowed().into(),
    );
    map.insert(k::GYROSCOPIC.into(), body.gyroscopic_forces_enabled().into());
    map.insert(k::ENABLED.into(), body.is_enabled().into());
    map.insert(
        k::CAN_SLEEP.into(),
        (body.activation().normalized_linear_threshold >= 0.0).into(),
    );
    map.insert(k::SLEEP_TIME.into(), f(body.activation().time_until_sleep));
    read_mass(body, &mut map);
    Some(toml::Value::Table(map))
}

/// The mass the author added, read back off the body.
///
/// `body.mass()` is the total, colliders included; writing that back as
/// `mass` would add the colliders' weight again on every save.
fn read_mass(body: &RigidBody, map: &mut toml::map::Map<String, toml::Value>) {
    use crate::rapier3d::dynamics::RigidBodyAdditionalMassProps as Extra;
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let vec3 = |v: Vector| toml::Value::Array(vec![f(v.x), f(v.y), f(v.z)]);
    let (mass, inertia, com) = match body.mass_properties().additional_local_mprops.as_deref() {
        Some(Extra::Mass(mass)) => (*mass, Vector::ZERO, Vector::ZERO),
        Some(Extra::MassProps(props)) => (props.mass(), props.principal_inertia(), props.local_com),
        None => (0.0, Vector::ZERO, Vector::ZERO),
    };
    map.insert(k::MASS.into(), f(mass));
    map.insert(k::INERTIA.into(), vec3(inertia));
    map.insert(k::CENTER_OF_MASS.into(), vec3(com));
}

/// Body creation, the forces that move one, and the overlap query.
///
/// Split from [`install_body_state_api`] under `MAX_FN_LINES`; the line is
/// between *doing something to* a body and *asking or tuning* one.
pub(crate) fn install_body_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("add_body", &[c::BODY_3D], "", "Give the node a rigid body of the given kind (`BODY_DYNAMIC`, `BODY_STATIC`, `BODY_KINEMATIC`, `BODY_KINEMATIC_VELOCITY`)."),
        ("add_ball_collider", &[c::COLLIDER_3D], "", "Attach a sphere collider of the given radius."),
        ("add_cuboid_collider", &[c::COLLIDER_3D], "", "Attach a box collider from its three half-extents."),
        ("apply_impulse", &[c::BODY_3D], "", "Add an instant change in momentum, as if the body were struck."),
        ("apply_impulse_at_point", &[c::BODY_3D], "", "Strike the body at a world point, which spins it as well as moves it."),
        ("apply_torque_impulse", &[c::BODY_3D], "", "Add an instant change in angular momentum, as if the body were spun."),
    ]);
    m.function(
        "add_body",
        |eng: &Engine, (node, kind): (NodeId, String)| add_body(eng, entity_of(node)?, &kind),
    );
    m.function(
        "add_ball_collider",
        |eng: &Engine, (node, radius): (NodeId, f32)| {
            add_collider(
                eng,
                entity_of(node)?,
                ColliderBuilder::ball(scalar::real(radius)),
            )
        },
    );
    m.function(
        "add_cuboid_collider",
        |eng: &Engine, (node, hx, hy, hz): (NodeId, f32, f32, f32)| {
            let (hx, hy, hz) = (scalar::real(hx), scalar::real(hy), scalar::real(hz));
            add_collider(eng, entity_of(node)?, ColliderBuilder::cuboid(hx, hy, hz))
        },
    );
    m.function(
        "apply_impulse",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function(
        "apply_impulse_at_point",
        |eng: &Engine, (node, x, y, z, px, py, pz): (NodeId, f32, f32, f32, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse_at_point(
                    scalar::v3(x, y, z),
                    scalar::v3(px, py, pz),
                    true,
                );
            })
        },
    );
    m.function(
        "apply_torque_impulse",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_torque_impulse(scalar::v3(x, y, z), true);
            })
        },
    );
    // Sensor pairs only: rapier's narrow phase reports an intersection only
    // when at least one of the two colliders is a sensor.
    m.function("overlaps", |eng: &Engine, node: NodeId| {
        overlaps_value(eng, node)
    });
}

/// The forces that move a body over time, and the world's own gravity.
///
/// Split from [`install_body_api`] under `MAX_FN_LINES`; the line is between
/// *making* a body and *pushing* one.
pub(crate) fn install_force_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "add_force",
            &[c::BODY_3D],
            "",
            "Push the body until the force is reset; unlike an impulse this is spread over time.",
        ),
        (
            "add_force_at_point",
            &[c::BODY_3D],
            "",
            "Push at a world point, which also turns the body.",
        ),
        (
            "add_torque",
            &[c::BODY_3D],
            "",
            "Turn the body until the torque is reset.",
        ),
        (
            "reset_forces",
            &[c::BODY_3D],
            "",
            "Drop every force added since the last step.",
        ),
        (
            "reset_torques",
            &[c::BODY_3D],
            "",
            "Drop every torque added since the last step.",
        ),
        (
            "user_force",
            &[c::BODY_3D],
            "",
            "The force the next step will integrate.",
        ),
        (
            "user_torque",
            &[c::BODY_3D],
            "",
            "The torque the next step will integrate.",
        ),
        (
            "set_gravity",
            &[],
            "",
            "Set the 3D world's gravity, in units per second squared.",
        ),
        (
            "wake_all",
            &[],
            "()",
            "Wake every sleeping body in the 3D world.",
        ),
    ]);
    m.function(
        "add_force",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_force(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function(
        "add_force_at_point",
        |eng: &Engine, (node, x, y, z, px, py, pz): (NodeId, f32, f32, f32, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_force_at_point(
                    scalar::v3(x, y, z),
                    scalar::v3(px, py, pz),
                    true,
                );
            })
        },
    );
    m.function(
        "add_torque",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_torque(scalar::v3(x, y, z), true);
            })
        },
    );
}

/// What a body's forces currently are, and how to drop them.
///
/// Split from [`install_force_api`] under `MAX_FN_LINES`.
pub(crate) fn install_force_reader_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("overlaps", &[c::COLLIDER_3D], "", "The nodes this one currently intersects; rapier reports a pair only when one of the two colliders is a sensor."),
        ("gravity", &[], "", "The 3D world's gravity."),
    ]);
    m.function("reset_forces", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].reset_forces(true);
        })
    });
    m.function("reset_torques", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].reset_torques(true);
        })
    });
    m.function("user_force", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let f = body.user_force();
            (f.x, f.y, f.z)
        })
    });
    m.function("user_torque", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let t = body.user_torque();
            (t.x, t.y, t.z)
        })
    });
    m.function("set_gravity", |eng: &Engine, (x, y, z): (f32, f32, f32)| {
        let state = eng.resource::<PhysicsState>();
        state.borrow_mut().world.gravity = scalar::v3(x, y, z);
        Ok(())
    });
    m.function("gravity", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let g = state.borrow().world.gravity;
        Ok((g.x, g.y, g.z))
    });
    m.function("wake_all", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        state.borrow_mut().world.wake_up_all(true);
        Ok(())
    });
}

/// Velocity, sleep, mass and the per-body tuning knobs — everything that asks
/// a body about itself or changes how it is simulated.
pub(crate) fn install_body_state_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("potential_energy", &[c::BODY_3D], "", "The body's gravitational potential energy over one step."),
        ("is_moving", &[c::BODY_3D], "", "Whether the body is awake and actually going somewhere."),
        ("effective_dominance", &[c::BODY_3D], "", "The dominance rapier will use for this body: its own group, or the rank every non-dynamic body outranks with."),
        (
            "set_linear_velocity",
            &[c::BODY_3D],
            "",
            "Set how fast the body travels, in units per second.",
        ),
        (
            "linear_velocity",
            &[c::BODY_3D],
            "",
            "How fast the body is travelling, in units per second.",
        ),
        (
            "set_angular_velocity",
            &[c::BODY_3D],
            "",
            "Set how fast the body spins, in radians per second about each axis.",
        ),
        (
            "angular_velocity",
            &[c::BODY_3D],
            "",
            "How fast the body is spinning, in radians per second about each axis.",
        ),
        (
            "velocity_at_point",
            &[c::BODY_3D],
            "",
            "How fast a world point on the body is moving, spin included.",
        ),
        (
            k::MASS,
            &[c::BODY_3D],
            "",
            "The body's total mass, colliders included.",
        ),
        (
            "kinetic_energy",
            &[c::BODY_3D],
            "",
            "The body's kinetic energy, for a rest test the solver agrees with.",
        ),
    ]);
    m.function(
        "set_linear_velocity",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_linvel(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function("linear_velocity", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let v = body.linvel();
            (v.x, v.y, v.z)
        })
    });
    m.function(
        "set_angular_velocity",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_angvel(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function("angular_velocity", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let w = body.angvel();
            (w.x, w.y, w.z)
        })
    });
    m.function(
        "velocity_at_point",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            read_body(eng, entity_of(node)?, |body| {
                let v = body.velocity_at_point(scalar::v3(x, y, z));
                (v.x, v.y, v.z)
            })
        },
    );
    install_body_readers(m);
}

/// What a body weighs and how it is moving, read-only: the numbers a script
/// asks about rather than the ones it sets.
fn install_body_readers(m: &mut dyn Bindings<Engine>) {
    m.function("mass", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::mass)
    });
    m.function("kinetic_energy", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::kinetic_energy)
    });
    m.function("potential_energy", |eng: &Engine, node: NodeId| {
        let gravity = {
            let state = eng.resource::<PhysicsState>();
            
            state.borrow().world.gravity
        };
        read_body(eng, entity_of(node)?, |body| {
            body.gravitational_potential_energy(scalar::real(crate::FIXED_DT), gravity)
        })
    });
    m.function("is_moving", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| -> bool { body.is_moving() })
    });
    m.function("effective_dominance", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            f32::from(body.effective_dominance_group())
        })
    });
    // The step writes a dynamic body's pose into `Transform` every tick, so
    // assigning `node.position` on one is overwritten before it is seen. This
    // is the way to move one, and it says what it costs: the velocity goes.
}

/// Where a body is and where it is about to be, and the one way to move a
/// dynamic one.
///
/// Split from [`install_body_state_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body_pose_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("teleport", &[c::BODY_3D], "", "Move the body to a world position at once, clearing its velocity: what assigning the node's position cannot do, because the step writes that back every tick."),
        ("set_body_kind", &[c::BODY_3D], "", "Change the body between dynamic, static and kinematic in place, keeping its velocity."),
        ("body_kind", &[c::BODY_3D], "", "Whether the body is dynamic, static, kinematic or kinematic_velocity."),
        ("predict_position", &[c::BODY_3D], "", "Where the body will be after `dt` seconds at its current velocity."),
        ("predict_position_with_forces", &[c::BODY_3D], "", "The same, with the forces already applied taken into account: where a thrust or a spring will have put it."),
        ("next_position", &[c::BODY_3D], "", "The pose a kinematic body has been told to move to."),
    ]);
    m.function(
        "teleport",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.set_translation(scalar::v3(x, y, z), true);
                body.set_linvel(scalar::Vector::ZERO, true);
                body.set_angvel(scalar::Vector::ZERO, true);
                // A query before the next step must see the new place.
                state.queries_ready = false;
            })
        },
    );
    m.function(
        "set_body_kind",
        |eng: &Engine, (node, kind): (NodeId, String)| {
            let kind = body_type(&kind)?;
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_body_type(kind, true);
            })
        },
    );
    m.function("body_kind", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| kind_name(body).to_string())
    });
}

/// How a body is simulated rather than what it is doing: gravity scale,
/// damping, axis locks, CCD, dominance, sleep.
///
/// Split from [`install_body_state_api`] under `MAX_FN_LINES`; that one asks a
/// body about itself, this one changes how it behaves.
pub(crate) fn install_body_tuning_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "set_gravity_scale",
            &[c::BODY_3D],
            "",
            "Scale world gravity for this body alone.",
        ),
        (
            k::GRAVITY_SCALE,
            &[c::BODY_3D],
            "",
            "This body's gravity multiplier.",
        ),
        (
            "set_damping",
            &[c::BODY_3D],
            "",
            "Set linear and angular damping together.",
        ),
        (
            k::DAMPING,
            &[c::BODY_3D],
            "",
            "This body's linear and angular damping.",
        ),
        (
            "set_lock_translation",
            &[c::BODY_3D],
            "",
            "Freeze the body's movement along each world axis.",
        ),
        (
            "set_lock_rotation",
            &[c::BODY_3D],
            "",
            "Freeze the body's spin about each world axis: how an upright character stays upright.",
        ),
        (
            k::LOCKED_AXES,
            &[c::BODY_3D],
            "",
            "Which translation and rotation axes are frozen.",
        ),
        (
            "set_ccd",
            &[c::BODY_3D],
            "",
            "Sweep this body's whole path each step so it cannot pass through a wall.",
        ),
        (
            "is_ccd",
            &[c::BODY_3D],
            "",
            "Whether continuous collision detection is on for this body.",
        ),
        (
            "set_dominance",
            &[c::BODY_3D],
            "",
            "Set the group that decides which of two bodies can push the other.",
        ),
    ]);
    m.function(
        "set_gravity_scale",
        |eng: &Engine, (node, scale): (NodeId, f32)| {
            let scale = scalar::real(scale);
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_gravity_scale(scale, true);
            })
        },
    );
    m.function("gravity_scale", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::gravity_scale)
    });
    m.function(
        "set_damping",
        |eng: &Engine, (node, linear, angular): (NodeId, f32, f32)| {
            let (linear, angular) = (scalar::real(linear), scalar::real(angular));
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.set_linear_damping(linear);
                body.set_angular_damping(angular);
            })
        },
    );
    m.function("damping", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            (body.linear_damping(), body.angular_damping())
        })
    });
}

/// The axis locks: what keeps a character upright and a top-down game flat.
///
/// Split from [`install_body_tuning_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body_lock_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[]);
    m.function(
        "set_lock_translation",
        |eng: &Engine, (node, x, y, z): (NodeId, bool, bool, bool)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_enabled_translations(!x, !y, !z, true);
            })
        },
    );
    m.function(
        "set_lock_rotation",
        |eng: &Engine, (node, x, y, z): (NodeId, bool, bool, bool)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_enabled_rotations(!x, !y, !z, true);
            })
        },
    );
    m.function("locked_axes", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let axes = body.locked_axes();
            (
                axes.contains(LockedAxes::TRANSLATION_LOCKED_X),
                axes.contains(LockedAxes::TRANSLATION_LOCKED_Y),
                axes.contains(LockedAxes::TRANSLATION_LOCKED_Z),
                axes.contains(LockedAxes::ROTATION_LOCKED_X),
                axes.contains(LockedAxes::ROTATION_LOCKED_Y),
                axes.contains(LockedAxes::ROTATION_LOCKED_Z),
            )
        })
    });
}

/// Continuous collision detection and dominance: the two knobs that decide
/// what a body may pass through and what it may push.
///
/// Split from [`install_body_tuning_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body_ccd_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[("dominance", &[c::BODY_3D], "", "This body's dominance group.")]);
    m.function("set_ccd", |eng: &Engine, (node, on): (NodeId, bool)| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].enable_ccd(on);
        })
    });
    m.function("is_ccd", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| -> bool {
            body.is_ccd_enabled()
        })
    });
    m.function(
        "set_dominance",
        |eng: &Engine, (node, group): (NodeId, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_dominance_group(group.clamp(-127.0, 127.0) as i8);
            })
        },
    );
    m.function("dominance", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            f32::from(body.dominance_group())
        })
    });
}

/// Whether a body is simulated at all, and whether it is asleep.
///
/// Split from [`install_body_tuning_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body_sleep_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "set_enabled",
            &[c::BODY_3D],
            "",
            "Simulate this body or leave it out entirely, keeping its state.",
        ),
        (
            "is_enabled",
            &[c::BODY_3D],
            "",
            "Whether the body is being simulated.",
        ),
        ("sleep", &[c::BODY_3D], "", "Put the body to sleep now."),
        (
            "wake_up",
            &[c::BODY_3D],
            "",
            "Wake the body, so the next step moves it.",
        ),
        (
            "is_sleeping",
            &[c::BODY_3D],
            "",
            "Whether the body is asleep and being skipped.",
        ),
    ]);
    m.function("set_enabled", |eng: &Engine, (node, on): (NodeId, bool)| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].set_enabled(on);
        })
    });
    m.function("is_enabled", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| -> bool { body.is_enabled() })
    });
    m.function("sleep", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].sleep();
        })
    });
    m.function("wake_up", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].wake_up(true);
        })
    });
    m.function("is_sleeping", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| -> bool { body.is_sleeping() })
    });
    m.function(
        "predict_position",
        |eng: &Engine, (node, dt): (NodeId, f32)| {
            let dt = scalar::real(dt);
            read_body(eng, entity_of(node)?, |body| {
                let pose = body.predict_position_using_velocity(dt).translation;
                (pose.x, pose.y, pose.z)
            })
        },
    );
    m.function(
        "predict_position_with_forces",
        |eng: &Engine, (node, dt): (NodeId, f32)| {
            let dt = scalar::real(dt);
            read_body(eng, entity_of(node)?, |body| {
                let p = body
                    .predict_position_using_velocity_and_forces(dt)
                    .translation;
                (
                    scalar::f32_of(p.x),
                    scalar::f32_of(p.y),
                    scalar::f32_of(p.z),
                )
            })
        },
    );
    m.function("next_position", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let p = body.next_position().translation;
            (p.x, p.y, p.z)
        })
    });
}

/// The `body3d` key. Not backed by a component type: it writes into
/// [`crate::PhysicsState`].
///
/// The `body = "dynamic"` shorthand keeps working via the schema's
/// `shorthand` marker.
pub(crate) fn register_body_component(reg: &mut Registry<'_>) {
    let kinds = v::options(w::BODY_KINDS);
    let axes = v::options(w::LOCK_AXES);
    let default = w::DYNAMIC;
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], shorthand = true, description = "How physics drives the node: simulated, immovable, moved by script, or moved by a velocity you set" }}"#, default, kinds)),
            (k::LOCK_TRANSLATION, &format!(r#"{{ type = "flags", default = [], options = [{}], description = "World axes the body may not move along" }}"#, axes)),
            (k::LOCK_ROTATION, &format!(r#"{{ type = "flags", default = [], options = [{}], description = "World axes the body may not turn about; locking all three keeps a character upright" }}"#, axes)),
            (k::CENTER_OF_MASS, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where the extra mass sits, in the node's own space; only read when mass is set" }"#),
            (k::INERTIA, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Resistance to spin about each axis; 0 lets rapier derive it from the mass" }"#),
            (k::GYROSCOPIC, r#"{ type = "bool", default = false, description = "Model the wobble a spinning body's own inertia gives it, as a thrown American football has" }"#),
        ]),
        shared_body_schema(),
    ]
    .join("\n");
    reg.register_component(
        c::BODY_3D,
        ComponentDef {
            doc: "Makes the node a 3D rigid body rapier simulates: `dynamic` falls and responds to forces, `static` never moves, `kinematic` is moved by script or animation and pushes what it meets. On its own a body has no shape; add a `collider3d` for it to collide with anything.",
            schema: ComponentDef::parse_schema(c::BODY_3D, &schema),
            tags: &[balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
            expects: &[],
            apply: Box::new(apply_body),
            remove: Box::new(|eng, entity| {
                // Removing the body keeps the collider, as static geometry.
                let collider = get_collider_params(eng, entity);
                remove_body_and_colliders(eng, entity);
                if let Some(params) = collider {
                    apply_collider(eng, entity, &params)?;
                }
                Ok(())
            }),
            get: Box::new(get_body_params),
        },
    );
}
