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
    LockedAxes, MassProperties, RigidBody, RigidBodyBuilder as RigidBodyBuilder2,
    RigidBodyHandle as RigidBodyHandle2, RigidBodyType,
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

pub(crate) fn write_body(body: &mut RigidBody, params: &toml::Value, may_sleep: bool) {
    if let Ok(kind) = body_type(v::text(params, k::KIND, w::DYNAMIC))
        && body.body_type() != kind
    {
        body.set_body_type(kind, true);
    }
    body.set_linear_damping(scalar::real(v::f(params, k::LINEAR_DAMPING, 0.0)));
    body.set_angular_damping(scalar::real(v::f(params, k::ANGULAR_DAMPING, 0.0)));
    body.set_gravity_scale(scalar::real(v::f(params, k::GRAVITY_SCALE, 1.0)), true);
    body.set_dominance_group(v::f(params, k::DOMINANCE, 0.0).clamp(-128.0, 127.0) as i8);
    body.set_additional_solver_iterations(v::f(params, k::SOLVER_ITERATIONS, 0.0).max(0.0) as usize);
    body.set_additional_pgs_iterations(v::f(params, k::INTERNAL_ITERATIONS, 0.0).max(0.0) as usize);
    body.set_locked_axes(locked_axes(params), true);
    body.enable_ccd(v::boolean(params, k::CONTINUOUS_COLLISION, false));
    body.set_soft_ccd_prediction(scalar::real(v::f(params, k::SPECULATIVE_DISTANCE, 0.0)));
    body.set_allow_fast_rotation(v::boolean(params, k::ALLOW_FAST_ROTATION, false));
    body.set_enabled(v::boolean(params, k::ENABLED, true));
    write_mass(body, params);
    body.wake_up(true);
    allow_sleep(body, may_sleep, crate::body::sleep_thresholds(params));
    body.activation_mut().time_until_sleep =
        scalar::real(v::f(params, k::TIME_TO_SLEEP, 0.5).max(0.0));
}

/// What the author wrote that the body cannot hold, as in 3D.
pub(crate) fn authored(params: &toml::Value) -> crate::shared::body::Authored {
    let mass = v::f(params, k::MASS, 0.0);
    let (sleep_threshold, sleep_angular_threshold) = crate::body::sleep_thresholds(params);
    let [x, y] = v::vec2(params, k::INITIAL_LINEAR_VELOCITY, [0.0; 2]);
    crate::shared::body::Authored {
        can_sleep: v::boolean(params, k::CAN_SLEEP, true),
        fit_inertia: mass > 0.0
            && v::f(params, k::INERTIA, 0.0) == 0.0
            && !crate::body::is_default(&v::vec2(params, k::CENTER_OF_MASS, [0.0; 2])),
        sleep_threshold,
        sleep_angular_threshold,
        start_asleep: v::boolean(params, k::START_ASLEEP, false),
        initial_linear_velocity: [x, y, 0.0],
        initial_angular_velocity: [0.0, 0.0, v::f(params, k::INITIAL_ANGULAR_VELOCITY, 0.0)],
    }
}

/// What rapier takes only when a body is made, as in 3D.
fn start_body(body: &mut RigidBody, authored: &crate::shared::body::Authored, may_sleep: bool) {
    let [x, y, _] = authored.initial_linear_velocity;
    body.set_linvel(scalar::v2(x, y), true);
    body.set_angvel(scalar::real(authored.initial_angular_velocity[2]), true);
    if authored.start_asleep && may_sleep {
        body.sleep();
    }
}

/// The 2D twin of `crate::body::fit_inertia`, where an inertia is one number.
fn fit_inertia(world: &mut crate::rapier2d::pipeline::PhysicsWorld, handle: RigidBodyHandle2) {
    use crate::rapier2d::dynamics::RigidBodyAdditionalMassProps as Extra;
    let body = &world.bodies[handle];
    let Some(Extra::MassProps(stated)) = body.mass_properties().additional_local_mprops.as_deref()
    else {
        return;
    };
    let (mass, com) = (stated.mass(), stated.local_com);
    let mut shapes = MassProperties::default();
    for collider in body.colliders() {
        let Some(collider) = world.colliders.get(*collider).filter(|c| c.is_enabled()) else {
            continue;
        };
        let Some(at) = collider.position_wrt_parent() else {
            continue;
        };
        shapes += collider.shape().mass_properties(1.0).transform_by(at);
    }
    let inertia = if shapes.mass() > 0.0 {
        shapes.set_mass(mass, true);
        shapes.principal_inertia() + mass * (com - shapes.local_com).length_squared()
    } else {
        0.0
    };
    world.bodies[handle]
        .set_additional_mass_properties(MassProperties::new(com, mass, inertia), false);
}

/// Whether the body states its own `mass`, which its colliders then do not add
/// to.
pub(crate) fn has_total_mass(body: &RigidBody) -> bool {
    use crate::rapier2d::dynamics::RigidBodyAdditionalMassProps as Extra;
    match body.mass_properties().additional_local_mprops.as_deref() {
        Some(Extra::Mass(mass)) => *mass > 0.0,
        Some(Extra::MassProps(props)) => props.mass() > 0.0,
        None => false,
    }
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
    map.insert(
        k::DOMINANCE.into(),
        i64::from(body.dominance_group()).into(),
    );
    map.insert(
        k::SOLVER_ITERATIONS.into(),
        i64::try_from(body.additional_solver_iterations())
            .unwrap_or(i64::MAX)
            .into(),
    );
    map.insert(
        k::INTERNAL_ITERATIONS.into(),
        i64::try_from(body.additional_pgs_iterations())
            .unwrap_or(i64::MAX)
            .into(),
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
    map.insert(k::CONTINUOUS_COLLISION.into(), body.is_ccd_enabled().into());
    map.insert(
        k::SPECULATIVE_DISTANCE.into(),
        f(body.soft_ccd_prediction()),
    );
    map.insert(
        k::ALLOW_FAST_ROTATION.into(),
        body.is_fast_rotation_allowed().into(),
    );
    map.insert(k::ENABLED.into(), body.is_enabled().into());
    let authored = state.body_authored.get(&entity);
    map.insert(
        k::CAN_SLEEP.into(),
        authored.is_none_or(|a| a.can_sleep).into(),
    );
    map.insert(
        k::TIME_TO_SLEEP.into(),
        f(body.activation().time_until_sleep),
    );
    let activation = body.activation();
    let live = (
        activation.normalized_linear_threshold,
        activation.angular_threshold,
    );
    crate::body::read_authored(live, authored, &mut map);
    let start = authored.map_or(([0.0; 3], [0.0; 3]), |a| {
        (a.initial_linear_velocity, a.initial_angular_velocity)
    });
    map.insert(
        k::INITIAL_LINEAR_VELOCITY.into(),
        crate::body::floats(&start.0[..2]),
    );
    map.insert(
        k::INITIAL_ANGULAR_VELOCITY.into(),
        toml::Value::Float(f64::from(start.1[2])),
    );
    read_mass(body, authored.is_some_and(|a| a.fit_inertia), &mut map);
    Some(toml::Value::Table(map))
}

/// The mass the author stated, read back off the body: `body.mass()` is also
/// the total when none was, and writing that back would pin the colliders'
/// weight as the body's own. A fitted inertia reads back as its 0, as in 3D.
fn read_mass(body: &RigidBody, fitted: bool, map: &mut toml::map::Map<String, toml::Value>) {
    use crate::rapier2d::dynamics::RigidBodyAdditionalMassProps as Extra;
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let (mass, inertia, com) = match body.mass_properties().additional_local_mprops.as_deref() {
        Some(Extra::Mass(mass)) => (*mass, 0.0, scalar::v2(0.0, 0.0)),
        Some(Extra::MassProps(props)) if fitted => (props.mass(), 0.0, props.local_com),
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
        ("total_mass", &[c::BODY_2D], "", "The body's total mass: its `mass` when it states one, or what its colliders weigh."),
        ("kinetic_energy", &[c::BODY_2D], "", "The body's kinetic energy, for a rest test the solver agrees with."),
        ("potential_energy", &[c::BODY_2D], "", "The body's gravitational potential energy over one step."),
        ("is_moving", &[c::BODY_2D], "", "Whether the body is awake and actually going somewhere."),
        ("effective_dominance", &[c::BODY_2D], "", "The dominance rapier will use for this body: its own group, or the rank every non-dynamic body outranks with."),
        ("teleport", &[c::BODY_2D], "", "Move the body to a world position at once, clearing its velocity: what assigning the node's position cannot do, because the step writes that back every tick. `#{ rotation = angle }` turns it too, in radians."),
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
    m.function("total_mass", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::mass)
    });
    m.function("kinetic_energy", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::kinetic_energy)
    });
    m.function("potential_energy", |eng: &Engine, node: NodeId| {
        let gravity = eng.resource::<PhysicsState2d>().borrow().world.gravity;
        read_body(eng, entity_of(node)?, |body| {
            body.gravitational_potential_energy(scalar::real(balaur_core::fixed_dt()), gravity)
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
    m.function(
        "teleport",
        |eng: &Engine, (node, x, y, opts): (NodeId, f32, f32, Option<balaur_script::Value>)| {
            let entity = entity_of(node)?;
            let opts = crate::vocabulary::Opts(opts.as_ref());
            let turn = opts.get(k::ROTATION).map(|_| opts.f32(k::ROTATION, 0.0));
            with_body(eng, entity, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.set_translation(scalar::v2(x, y), true);
                if let Some(turn) = turn {
                    let turn = crate::rapier2d::math::Rotation::from_angle(scalar::real(turn));
                    body.set_rotation(turn, true);
                }
                body.set_linvel(Vector2::ZERO, true);
                body.set_angvel(0.0, true);
                state.queries_ready = false;
            })?;
            balaur_core::interpolate::reset(eng, entity);
            Ok(())
        },
    );
}

/// A 2D pose as a script reads one: `#{ position, rotation }`, the angle in
/// radians.
fn pose_value(pose: &scalar::Pose2) -> balaur_script::Value {
    crate::vocabulary::map([
        (
            k::POSITION,
            balaur_script::Value::Vec2(scalar::a2(pose.translation)),
        ),
        (
            k::ROTATION,
            balaur_script::Value::Num(f64::from(pose.rotation.angle())),
        ),
    ])
}

/// What a 2D body weighs and how that mass is spread, the twin of
/// `crate::body::install_body_mass_api`.
pub(crate) fn install_body2d_mass_api(m: &mut dyn Bindings<Engine>) {
    use balaur_script::Value;
    m.describe(&[
        ("world_center_of_mass", &[c::BODY_2D], "", "Where the body's whole mass sits, in world space."),
        ("local_center_of_mass", &[c::BODY_2D], "", "Where the body's whole mass sits, in the body's own space."),
        ("total_inertia", &[c::BODY_2D], "", "The body's resistance to spin, colliders included."),
        ("effective_mass", &[c::BODY_2D], "", "The mass the solver pushes against along each world axis: zero along a locked axis, and on any body that is not dynamic."),
        ("effective_angular_inertia", &[c::BODY_2D], "", "The inertia the solver turns: zero with rotation locked, and on any body that is not dynamic."),
    ]);
    m.function("world_center_of_mass", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            Value::Vec2(scalar::a2(body.center_of_mass()))
        })
    });
    m.function("local_center_of_mass", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            Value::Vec2(scalar::a2(body.local_center_of_mass()))
        })
    });
    m.function("total_inertia", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            scalar::f32_of(body.mass_properties().local_mprops.principal_inertia())
        })
    });
    m.function("effective_mass", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            Value::Vec2(scalar::a2(body.mass_properties().effective_mass()))
        })
    });
    m.function("effective_angular_inertia", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            scalar::f32_of(body.mass_properties().effective_angular_inertia())
        })
    });
}

/// The 2D axis locks.
///
/// Split from [`install_body2d_mass_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body2d_lock_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[]);
}

/// The 2D twin of `crate::body::install_body_ccd_api`.
pub(crate) fn install_body2d_ccd_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "is_ccd_active",
        &[c::BODY_2D],
        "",
        "Whether the body moved fast enough last step for rapier to sweep it, with or without continuous_collision.",
    )]);
    m.function("is_ccd_active", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| -> bool {
            body.is_ccd_active()
        })
    });
}

/// Whether a 2D body is asleep, where it is going, and the world's gravity.
pub(crate) fn install_body2d_sleep_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
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
            "Where the body will be after `dt` seconds at its current velocity, as `#{ position, rotation }`.",
        ),
        (
            "predict_position_with_forces",
            &[c::BODY_2D],
            "",
            "The same, with the forces already applied taken into account: where a thrust or a spring will have put it.",
        ),
        (
            "next_position",
            &[c::BODY_2D],
            "",
            "The pose a kinematic body has been told to move to, as `#{ position, rotation }`.",
        ),
        (
            "time_since_can_sleep",
            &[c::BODY_2D],
            "",
            "Seconds the body has spent under its sleep thresholds; it sleeps once this reaches time_to_sleep.",
        ),
        (
            "wake_all",
            &[],
            "()",
            "Wake every sleeping body in the 2D world.",
        ),
        ("gravity", &[], "", "The 2D world's gravity."),
    ]);
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
                pose_value(&body.predict_position_using_velocity(dt))
            })
        },
    );
    m.function(
        "predict_position_with_forces",
        |eng: &Engine, (node, dt): (NodeId, f32)| {
            let dt = scalar::real(dt);
            read_body(eng, entity_of(node)?, |body| {
                pose_value(&body.predict_position_using_velocity_and_forces(dt))
            })
        },
    );
    m.function("next_position", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            pose_value(body.next_position())
        })
    });
    m.function("time_since_can_sleep", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            scalar::f32_of(body.activation().time_since_can_sleep)
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
            "add_constant_force",
            &[c::BODY_2D],
            "",
            "Push the body every step until the constant force is set back to zero; unlike an impulse this is spread over time.",
        ),
        (
            "add_constant_force_at_point",
            &[c::BODY_2D],
            "",
            "Push at a world point every step, which also turns the body.",
        ),
        (
            "add_constant_torque",
            &[c::BODY_2D],
            "",
            "Turn the body every step until the constant torque is set back to zero.",
        ),
        (
            "set_constant_force",
            &[c::BODY_2D],
            "",
            "Replace the constant force with this one; zero stops the push.",
        ),
        (
            "set_constant_torque",
            &[c::BODY_2D],
            "",
            "Replace the constant torque with this one; zero stops the turn.",
        ),
        (
            "constant_force",
            &[c::BODY_2D],
            "",
            "The force every step integrates until it is set back to zero.",
        ),
        (
            "constant_torque",
            &[c::BODY_2D],
            "",
            "The torque every step integrates until it is set back to zero.",
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
/// A force for one step: the impulse it would deliver over one fixed step, so
/// nothing is left on the body for the step after.
fn install_step_force_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "apply_force",
            &[c::BODY_2D],
            "",
            "Push the body for the next step only; `add_constant_force` keeps pushing.",
        ),
        (
            "apply_force_at_point",
            &[c::BODY_2D],
            "",
            "Push at a world point for the next step only, which also turns the body.",
        ),
        (
            "apply_torque",
            &[c::BODY_2D],
            "",
            "Turn the body for the next step only; `add_constant_torque` keeps turning it.",
        ),
    ]);
    let dt = || scalar::real(balaur_core::fixed_dt());
    m.function(
        "apply_force",
        move |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse(scalar::v2(x, y) * dt(), true);
            })
        },
    );
    m.function(
        "apply_force_at_point",
        move |eng: &Engine, (node, x, y, px, py): (NodeId, f32, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse_at_point(
                    scalar::v2(x, y) * dt(),
                    scalar::v2(px, py),
                    true,
                );
            })
        },
    );
    m.function(
        "apply_torque",
        move |eng: &Engine, (node, torque): (NodeId, f32)| {
            let torque = scalar::real(torque) * dt();
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_torque_impulse(torque, true);
            })
        },
    );
}

fn install_body_forces(m: &mut dyn Bindings<Engine>) {
    install_step_force_api(m);
    m.function(
        "add_constant_force",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_force(scalar::v2(x, y), true);
            })
        },
    );
    m.function(
        "add_constant_force_at_point",
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
        "add_constant_torque",
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
    m.function(
        "set_constant_force",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.reset_forces(true);
                body.add_force(scalar::v2(x, y), true);
            })
        },
    );
    m.function(
        "set_constant_torque",
        |eng: &Engine, (node, torque): (NodeId, f32)| {
            let torque = scalar::real(torque);
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.reset_torques(true);
                body.add_torque(torque, true);
            })
        },
    );
    m.function("constant_force", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let f = body.user_force();
            (f.x, f.y)
        })
    });
    m.function("constant_torque", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, RigidBody::user_torque)
    });
}

/// A dynamic body that nothing collides with: no collider on the node or
/// under it, so it falls through everything.
fn body_warnings_2d(eng: &Engine, entity: Entity) -> Vec<balaur_core::warnings::Warning> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let body = state
        .bodies
        .get(&entity)
        .and_then(|&handle| state.world.bodies.get(handle));
    match body {
        Some(body) if body.is_dynamic() && body.colliders().is_empty() => {
            vec![balaur_core::warnings::Warning::whole(format!(
                "nothing collides with it: add a {} to the node or a child, or it falls through everything",
                c::COLLIDER_2D
            ))]
        }
        _ => Vec::new(),
    }
}

/// The `body2d` key. Like `body3d`, backed by no component type: it writes
/// into [`crate::PhysicsState2d`].
pub(crate) fn register_body2d_component(reg: &mut Registry<'_>) {
    let kinds = v::options(w::BODY_KINDS);
    let axes = v::options(w::LOCK_AXES_2D);
    let default = w::DYNAMIC;
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{default}", options = [{kinds}], description = "How 2D physics drives the node: simulated, immovable, moved by script, or moved by a velocity you set" }}"#)),
            (k::LOCK_TRANSLATION, &format!(r#"{{ type = "flags", default = [], options = [{axes}], group = "locks", description = "Axes the body may not move along" }}"#)),
            (k::LOCK_ROTATION, r#"{ type = "bool", default = false, group = "locks", description = "Stop the body turning; how a 2D character stays upright" }"#),
            (k::CENTER_OF_MASS, r#"{ type = "vec2", default = [0.0, 0.0], group = "mass", description = "Where the extra mass sits, in the node's own space; only read when mass is set" }"#),
            (k::INERTIA, r#"{ type = "float", default = 0.0, min = 0.0, group = "mass", description = "Resistance to spin, read when mass is set; 0 derives it from the colliders' shapes scaled to the mass, about center_of_mass when that is set" }"#),
            (k::INITIAL_LINEAR_VELOCITY, r#"{ type = "vec2", default = [0.0, 0.0], group = "start", description = "How fast the body travels when it is made, in units per second; a later patch does not reapply it" }"#),
            (k::INITIAL_ANGULAR_VELOCITY, r#"{ type = "float", default = 0.0, unit = "degrees", group = "start", description = "How fast the body spins when it is made; radians per second in the file, and a later patch does not reapply it" }"#),
        ]),
        shared_body_schema(),
    ]
    .join("\n");
    reg.register_component(
        c::BODY_2D,
        ComponentDef {
            events: crate::vocabulary::hook::BODY,
            warnings: Some(Box::new(body_warnings_2d)),
            doc: "A 2D rigid body simulated by rapier in the xy plane. `kind` is `dynamic`, `static`, `kinematic` or `kinematic_velocity`; add a `collider2d` for its shape.",
            schema: ComponentDef::parse_schema(c::BODY_2D, &schema),
            tags: &[balaur_core::components::tag::DIM_2D, balaur_core::components::tag::PHYSICS],
            expects: &[balaur_core::transform::COMPONENT],
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
