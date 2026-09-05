//! `body2d`: the 2D half of `crate::body`, against rapier2d.
//!
//! The two dimensions share their vocabulary (`crate::vocabulary`) and not
//! their calls: rapier2d and rapier3d are separate crates whose types do not
//! meet, and a macro over both would cost every reader of this file more than
//! the duplication does.
//!
//! Where a property is shaped by the dimension it is spelled differently and
//! deliberately: 2D locks translation on two axes and rotation on one, and
//! its angular velocity and inertia are single numbers.

use crate::rapier2d::prelude::{
    LockedAxes, MassProperties, RigidBody, RigidBodyActivation,
    RigidBodyBuilder as RigidBodyBuilder2, RigidBodyHandle as RigidBodyHandle2, RigidBodyType,
};
use crate::scalar::{self, Real, Vector2};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::body::shared_body_schema;
use crate::dim2::collider::{apply_collider, get_collider_params};
use crate::dim2::{PhysicsState2d, node_pose_2d};
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

crate::shared::body::functions!(
    state = PhysicsState2d,
    handle = RigidBodyHandle2,
    builder = RigidBodyBuilder2,
    node_pose = node_pose_2d,
    missing = "node has no 2D rigid body"
);

/// 2D locks two translation axes and the one rotation there is, so the flag
/// set is built by hand rather than shared with 3D.
fn locked_axes(params: &toml::Value) -> LockedAxes {
    let mut axes = LockedAxes::empty();
    if v::flag(params, k::LOCK_TRANSLATION, w::X) {
        axes |= LockedAxes::TRANSLATION_LOCKED_X;
    }
    if v::flag(params, k::LOCK_TRANSLATION, w::Y) {
        axes |= LockedAxes::TRANSLATION_LOCKED_Y;
    }
    if v::boolean(params, k::LOCK_ROTATION, false) {
        axes |= LockedAxes::ROTATION_LOCKED;
    }
    axes
}

pub(crate) fn write_body(body: &mut RigidBody, params: &toml::Value, world_may_sleep: bool) {
    if let Ok(kind) = body_type(v::text(params, k::KIND, w::DYNAMIC))
        && body.body_type() != kind
    {
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
    body.set_enabled(v::boolean(params, k::ENABLED, true));
    write_mass(body, params);
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

/// In 2D the angular inertia is one number, so `inertia` is a float here and
/// a vec3 in 3D — the same property, shaped by the dimension.
fn write_mass(body: &mut RigidBody, params: &toml::Value) {
    let mass = v::f(params, k::MASS, 0.0).max(0.0);
    let inertia = v::f(params, k::INERTIA, 0.0);
    let com = v::vec2(params, k::CENTER_OF_MASS, [0.0; 2]);
    if mass <= 0.0 {
        body.set_additional_mass(0.0, false);
    } else if inertia == 0.0 && crate::body::is_default(&com) {
        body.set_additional_mass(scalar::real(mass), true);
    } else {
        let com = scalar::v2a(com);
        body.set_additional_mass_properties(
            MassProperties::new(com, scalar::real(mass), scalar::real(inertia)),
            true,
        );
    }
}

pub(crate) fn get_body_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let body = &state.world.bodies[*state.bodies.get(&entity)?];
    let axes = body.locked_axes();
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
        toml::Value::Array(
            [
                (w::X, LockedAxes::TRANSLATION_LOCKED_X),
                (w::Y, LockedAxes::TRANSLATION_LOCKED_Y),
            ]
            .into_iter()
            .filter(|(_, flag)| axes.contains(*flag))
            .map(|(name, _)| toml::Value::String(name.to_string()))
            .collect(),
        ),
    );
    map.insert(
        k::LOCK_ROTATION.into(),
        axes.contains(LockedAxes::ROTATION_LOCKED).into(),
    );
    map.insert(k::CCD.into(), body.is_ccd_enabled().into());
    map.insert(k::SOFT_CCD.into(), f(body.soft_ccd_prediction()));
    map.insert(
        k::FAST_ROTATION.into(),
        body.is_fast_rotation_allowed().into(),
    );
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
    use crate::rapier2d::dynamics::RigidBodyAdditionalMassProps as Extra;
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let (mass, inertia, com) = match body.mass_properties().additional_local_mprops.as_deref() {
        Some(Extra::Mass(mass)) => (*mass, 0.0, scalar::v2(0.0, 0.0)),
        Some(Extra::MassProps(props)) => (props.mass(), props.principal_inertia(), props.local_com),
        None => (0.0, 0.0, scalar::v2(0.0, 0.0)),
    };
    map.insert(k::MASS.into(), f(mass));
    map.insert(k::INERTIA.into(), f(inertia));
    map.insert(
        k::CENTER_OF_MASS.into(),
        toml::Value::Array(vec![f(com.x), f(com.y)]),
    );
}

/// The 2D twin of `crate::body::install_body_state_api`. Every function here
/// has a 3D sibling under the same name in `physics3d`; where the shapes
/// differ, the dimension is the reason.
pub(crate) fn install_body2d_state_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("velocity_at_point", &[c::BODY_2D], "", "How fast a world point on the body is moving, spin included."),
        ("mass", &[c::BODY_2D], "", "The body's total mass, colliders included."),
        ("kinetic_energy", &[c::BODY_2D], "", "The body's kinetic energy, for a rest test the solver agrees with."),
        ("teleport", &[c::BODY_2D], "", "Move the body to a world position at once, clearing its velocity: what assigning the node's position cannot do, because the step writes that back every tick."),
        ("set_body_kind", &[c::BODY_2D], "", "Change the body between dynamic, static and kinematic in place, keeping its velocity."),
        ("body_kind", &[c::BODY_2D], "", "Whether the body is dynamic, static, kinematic or kinematic_velocity."),
    ]);
    m.function(
        "velocity_at_point",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            read_body(eng, entity_of(node)?, |body| {
                let v = body.velocity_at_point(scalar::v2(x, y));
                (v.x, v.y)
            })
        },
    );
    m.function("mass", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::mass)
    });
    m.function("kinetic_energy", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::kinetic_energy)
    });
    m.function(
        "teleport",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.set_translation(scalar::v2(x, y), true);
                body.set_linvel(Vector2::ZERO, true);
                body.set_angvel(0.0, true);
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

/// How a 2D body is simulated rather than what it is doing.
///
/// Split from [`install_body2d_state_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body2d_tuning_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "set_gravity_scale",
            &[c::BODY_2D],
            "",
            "Scale world gravity for this body alone.",
        ),
        (
            k::GRAVITY_SCALE,
            &[c::BODY_2D],
            "",
            "This body's gravity multiplier.",
        ),
        (
            "set_damping",
            &[c::BODY_2D],
            "",
            "Set linear and angular damping together.",
        ),
        (
            k::DAMPING,
            &[c::BODY_2D],
            "",
            "This body's linear and angular damping.",
        ),
        (
            "set_lock_translation",
            &[c::BODY_2D],
            "",
            "Freeze the body's movement along x and y.",
        ),
        (
            "set_lock_rotation",
            &[c::BODY_2D],
            "",
            "Freeze the body's spin: how a 2D character stays upright.",
        ),
        (
            k::LOCKED_AXES,
            &[c::BODY_2D],
            "",
            "Whether x, y and rotation are frozen.",
        ),
        (
            "set_ccd",
            &[c::BODY_2D],
            "",
            "Sweep this body's whole path each step so it cannot pass through a wall.",
        ),
        (
            "is_ccd",
            &[c::BODY_2D],
            "",
            "Whether continuous collision detection is on for this body.",
        ),
        (
            "set_dominance",
            &[c::BODY_2D],
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

/// The 2D axis locks.
///
/// Split from [`install_body2d_tuning_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body2d_lock_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[]);
    m.function(
        "set_lock_translation",
        |eng: &Engine, (node, x, y): (NodeId, bool, bool)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                let mut axes = body.locked_axes();
                axes.set(LockedAxes::TRANSLATION_LOCKED_X, x);
                axes.set(LockedAxes::TRANSLATION_LOCKED_Y, y);
                body.set_locked_axes(axes, true);
            })
        },
    );
    m.function(
        "set_lock_rotation",
        |eng: &Engine, (node, locked): (NodeId, bool)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                let mut axes = body.locked_axes();
                axes.set(LockedAxes::ROTATION_LOCKED, locked);
                body.set_locked_axes(axes, true);
            })
        },
    );
    m.function("locked_axes", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let axes = body.locked_axes();
            (
                axes.contains(LockedAxes::TRANSLATION_LOCKED_X),
                axes.contains(LockedAxes::TRANSLATION_LOCKED_Y),
                axes.contains(LockedAxes::ROTATION_LOCKED),
            )
        })
    });
}

/// The 2D twin of `crate::body::install_body_ccd_api`.
///
/// Split from [`install_body2d_tuning_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body2d_ccd_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "dominance",
        &[c::BODY_2D],
        "",
        "This body's dominance group.",
    )]);
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

/// Whether a 2D body is simulated at all, and whether it is asleep.
///
/// Split from [`install_body2d_tuning_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body2d_sleep_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "set_enabled",
            &[c::BODY_2D],
            "",
            "Simulate this body or leave it out entirely, keeping its state.",
        ),
        (
            "is_enabled",
            &[c::BODY_2D],
            "",
            "Whether the body is being simulated.",
        ),
        ("sleep", &[c::BODY_2D], "", "Put the body to sleep now."),
        (
            "wake_up",
            &[c::BODY_2D],
            "",
            "Wake the body, so the next step moves it.",
        ),
        (
            "is_sleeping",
            &[c::BODY_2D],
            "",
            "Whether the body is asleep and being skipped.",
        ),
        (
            "predict_position",
            &[c::BODY_2D],
            "",
            "Where the body will be after `dt` seconds at its current velocity.",
        ),
        (
            "next_position",
            &[c::BODY_2D],
            "",
            "The position a kinematic body has been told to move to.",
        ),
        (
            "wake_all",
            &[],
            "()",
            "Wake every sleeping body in the 2D world.",
        ),
        ("gravity", &[], "", "The 2D world's gravity."),
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
                let p = body.predict_position_using_velocity(dt).translation;
                (p.x, p.y)
            })
        },
    );
    m.function("next_position", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let p = body.next_position().translation;
            (p.x, p.y)
        })
    });
    m.function("wake_all", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState2d>();
        state.borrow_mut().world.wake_up_all(true);
        Ok(())
    });
    m.function("gravity", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState2d>();
        let g = state.borrow().world.gravity;
        Ok((g.x, g.y))
    });
}

/// Forces and impulses on a 2D body. Torque is a single number here, which is
/// the whole difference from the 3D file.
pub(crate) fn install_body2d_force_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "apply_impulse_at_point",
            &[c::BODY_2D],
            "",
            "Strike the body at a world point, which spins it as well as moves it.",
        ),
        (
            "apply_torque_impulse",
            &[c::BODY_2D],
            "",
            "Add an instant change in angular momentum, as if the body were spun.",
        ),
        (
            "add_force",
            &[c::BODY_2D],
            "",
            "Push the body until the force is reset; unlike an impulse this is spread over time.",
        ),
        (
            "add_force_at_point",
            &[c::BODY_2D],
            "",
            "Push at a world point, which also turns the body.",
        ),
        (
            "add_torque",
            &[c::BODY_2D],
            "",
            "Turn the body until the torque is reset.",
        ),
        (
            "reset_forces",
            &[c::BODY_2D],
            "",
            "Drop every force added since the last step.",
        ),
        (
            "reset_torques",
            &[c::BODY_2D],
            "",
            "Drop every torque added since the last step.",
        ),
        (
            "user_force",
            &[c::BODY_2D],
            "",
            "The force the next step will integrate.",
        ),
        (
            "user_torque",
            &[c::BODY_2D],
            "",
            "The torque the next step will integrate.",
        ),
    ]);
    m.function(
        "apply_impulse_at_point",
        |eng: &Engine, (node, x, y, px, py): (NodeId, f32, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse_at_point(
                    scalar::v2(x, y),
                    scalar::v2(px, py),
                    true,
                );
            })
        },
    );
    m.function(
        "apply_torque_impulse",
        |eng: &Engine, (node, torque): (NodeId, f32)| {
            let torque = scalar::real(torque);
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_torque_impulse(torque, true);
            })
        },
    );
    install_body_forces(m);
}

/// The forces and impulses a script applies to a 2D body.
fn install_body_forces(m: &mut dyn Bindings<Engine>) {
    m.function(
        "add_force",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_force(scalar::v2(x, y), true);
            })
        },
    );
    m.function(
        "add_force_at_point",
        |eng: &Engine, (node, x, y, px, py): (NodeId, f32, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_force_at_point(
                    scalar::v2(x, y),
                    scalar::v2(px, py),
                    true,
                );
            })
        },
    );
    m.function(
        "add_torque",
        |eng: &Engine, (node, torque): (NodeId, f32)| {
            let torque = scalar::real(torque);
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_torque(torque, true);
            })
        },
    );
}

/// What a 2D body's forces currently are, and how to drop them.
///
/// Split from [`install_body2d_force_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body2d_force_reader_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[]);
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
            (f.x, f.y)
        })
    });
    m.function("user_torque", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::user_torque)
    });
}

/// The `body2d` key. Like `body3d`, backed by no component type: it writes
/// into [`crate::PhysicsState2d`].
pub(crate) fn register_body2d_component(reg: &mut Registry<'_>) {
    let kinds = v::options(w::BODY_KINDS);
    let axes = v::options(w::LOCK_AXES_2D);
    let default = w::DYNAMIC;
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{default}", options = [{kinds}], shorthand = true, description = "How 2D physics drives the node: simulated, immovable, moved by script, or moved by a velocity you set" }}"#)),
            (k::LOCK_TRANSLATION, &format!(r#"{{ type = "flags", default = [], options = [{axes}], description = "Axes the body may not move along" }}"#)),
            (k::LOCK_ROTATION, r#"{ type = "bool", default = false, description = "Stop the body turning; how a 2D character stays upright" }"#),
            (k::CENTER_OF_MASS, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where the extra mass sits, in the node's own space; only read when mass is set" }"#),
            (k::INERTIA, r#"{ type = "float", default = 0.0, min = 0.0, description = "Resistance to spin; 0 lets rapier derive it from the mass" }"#),
        ]),
        shared_body_schema(),
    ]
    .join("\n");
    reg.register_component(
        c::BODY_2D,
        ComponentDef {
            doc: "Makes the node a 2D rigid body rapier simulates, in the xy plane: `dynamic` falls and responds to forces, `static` never moves, `kinematic` is moved by script or animation and pushes what it meets. Add a `collider2d` for it to collide with anything.",
            schema: ComponentDef::parse_schema(c::BODY_2D, &schema),
            tags: &[balaur_core::components::tag::DIM_2D, balaur_core::components::tag::PHYSICS],
            expects: &[],
            apply: Box::new(apply_body),
            remove: Box::new(|eng, entity| {
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
