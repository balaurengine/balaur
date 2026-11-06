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
use crate::scalar::{self, Vector2};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Written};
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId};

use crate::dim2::collider::{apply_collider, authored_collider};
use crate::dim2::{PhysicsState2d, node_pose_2d};
use crate::properties::{Axis2d, BodyKind};
use crate::vocabulary::{component as c, keys as k};

/// A 2D rigid body simulated by rapier in the xy plane. `kind` is `dynamic`, `static`, `kinematic` or `kinematic_velocity`; add a `collider2d` for its shape.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, Component)]
#[component(name = "body2d", requires = [balaur_core::transform::COMPONENT], after_write = after_write)]
pub struct Body2d {
    /// How 2D physics drives the node: simulated, immovable, moved by script, or moved by a velocity you set
    #[property(default = BodyKind::Dynamic)]
    pub kind: BodyKind,
    /// Simulate this body at all; a disabled body keeps its state and costs nothing
    #[property(default = true)]
    pub enabled: bool,
    /// Multiplier on world gravity for this body: 0 hangs in the air, negative floats up
    #[property(default = 1.0)]
    pub gravity_scale: f32,
    /// The body's total mass; 0 sums what its colliders weigh
    #[property(min = 0.0, group = "mass")]
    pub mass: f32,
    /// Where the extra mass sits, in the node's own space; only read when mass is set
    #[property(group = "mass")]
    pub center_of_mass: [f32; 2],
    /// Resistance to spin, read when mass is set; 0 derives it from the colliders' shapes scaled to the mass, about center_of_mass when that is set
    #[property(min = 0.0, group = "mass")]
    pub inertia: f32,
    /// Drag on travel: how fast the body loses speed with nothing touching it
    #[property(min = 0.0, group = "damping")]
    pub linear_damping: f32,
    /// Drag on spin, in the same terms as linear_damping
    #[property(min = 0.0, group = "damping")]
    pub angular_damping: f32,
    /// Stop the body turning; how a 2D character stays upright
    #[property(group = "locks")]
    pub lock_rotation: bool,
    /// Axes the body may not move along
    #[property(flags, group = "locks")]
    pub lock_translation: Vec<Axis2d>,
    /// How fast the body travels when it is made, in units per second; a later patch does not reapply it
    #[property(group = "start")]
    pub initial_linear_velocity: [f32; 2],
    /// How fast the body spins when it is made; radians per second in the file, and a later patch does not reapply it
    #[property(unit = "degrees", group = "start")]
    pub initial_angular_velocity: f32,
    /// Also sweep moving bodies: a fast dynamic body sweeps static colliders without this, and this adds kinematic and dynamic ones
    #[property(group = "fast motion")]
    pub continuous_collision: bool,
    /// Distance ahead the body predicts contacts, in units; cheaper than ccd for merely fast bodies
    #[property(min = 0.0, group = "fast motion")]
    pub speculative_distance: f32,
    /// Allow a spin fast enough that rapier would otherwise clamp it
    #[property(group = "fast motion")]
    pub allow_fast_rotation: bool,
    /// Let the body stop being simulated once it has held still
    #[property(default = true, group = "sleep")]
    pub can_sleep: bool,
    /// Make the body asleep, with no velocity, until something wakes it; read once, when the body is made
    #[property(group = "sleep")]
    pub start_asleep: bool,
    /// Seconds of stillness before the body sleeps
    #[property(default = 0.5, min = 0.0, group = "sleep")]
    pub time_to_sleep: f32,
    /// The speed of the body's farthest point below which it counts as still, in length units per second scaled by `[physics] length_unit`
    #[property(default = 0.05, min = 0.0, group = "sleep")]
    pub sleep_threshold: f32,
    /// The spin below which a body with no collider counts as still; radians per second in the file
    #[property(default = 0.5, min = 0.0, unit = "degrees", group = "sleep")]
    pub sleep_angular_threshold: f32,
    /// A body in a higher group is unpushable by a lower one; a kinematic body keeps its own group, and only a static body outranks every group
    #[property(min = -128, max = 127, group = "solver")]
    pub dominance: i64,
    /// Extra solver substeps for the whole island this body is in, joints and contacts together: everything it touches pays for them
    #[property(min = 0, group = "solver")]
    pub solver_iterations: i64,
    /// Extra Gauss-Seidel iterations inside each substep for the island this body is in; the island runs the largest any of its bodies asks
    #[property(min = 0, group = "solver")]
    pub internal_iterations: i64,
}

crate::shared::body::functions!(
    state = PhysicsState2d,
    body = Body2d,
    handle = RigidBodyHandle2,
    builder = RigidBodyBuilder2,
    node_pose = node_pose_2d,
    missing = "node has no 2D rigid body"
);

/// 2D locks two translation axes and the one rotation there is, so the flag
/// set is built by hand rather than shared with 3D.
fn locked_axes(asked: &Body2d) -> LockedAxes {
    let mut axes = LockedAxes::empty();
    if asked.lock_translation.contains(&Axis2d::X) {
        axes |= LockedAxes::TRANSLATION_LOCKED_X;
    }
    if asked.lock_translation.contains(&Axis2d::Y) {
        axes |= LockedAxes::TRANSLATION_LOCKED_Y;
    }
    if asked.lock_rotation {
        axes |= LockedAxes::ROTATION_LOCKED;
    }
    axes
}

pub(crate) fn write_body(body: &mut RigidBody, asked: &Body2d, may_sleep: bool) {
    let kind = body_type(asked.kind);
    if body.body_type() != kind {
        body.set_body_type(kind, true);
    }
    body.set_linear_damping(asked.linear_damping);
    body.set_angular_damping(asked.angular_damping);
    body.set_gravity_scale(asked.gravity_scale, true);
    body.set_dominance_group(asked.dominance.clamp(-128, 127) as i8);
    body.set_additional_solver_iterations(asked.solver_iterations.max(0) as usize);
    body.set_additional_pgs_iterations(asked.internal_iterations.max(0) as usize);
    body.set_locked_axes(locked_axes(asked), true);
    body.enable_ccd(asked.continuous_collision);
    body.set_soft_ccd_prediction(asked.speculative_distance);
    body.set_allow_fast_rotation(asked.allow_fast_rotation);
    body.set_enabled(asked.enabled);
    write_mass(body, asked);
    body.wake_up(true);
    allow_sleep(
        body,
        may_sleep,
        (asked.sleep_threshold, asked.sleep_angular_threshold),
    );
    body.activation_mut().time_until_sleep = asked.time_to_sleep.max(0.0);
}

/// What the author wrote that the body cannot hold, as in 3D.
pub(crate) fn authored(asked: &Body2d) -> crate::shared::body::Authored {
    let [x, y] = asked.initial_linear_velocity;
    crate::shared::body::Authored {
        can_sleep: asked.can_sleep,
        fit_inertia: asked.mass > 0.0
            && asked.inertia == 0.0
            && !crate::body::is_default(&asked.center_of_mass),
        sleep_threshold: asked.sleep_threshold,
        sleep_angular_threshold: asked.sleep_angular_threshold,
        start_asleep: asked.start_asleep,
        initial_linear_velocity: [x, y, 0.0],
        initial_angular_velocity: [0.0, 0.0, asked.initial_angular_velocity],
    }
}

/// What rapier takes only when a body is made, as in 3D.
fn start_body(body: &mut RigidBody, authored: &crate::shared::body::Authored, may_sleep: bool) {
    let [x, y, _] = authored.initial_linear_velocity;
    body.set_linvel(scalar::v2(x, y), true);
    body.set_angvel(authored.initial_angular_velocity[2], true);
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
fn write_mass(body: &mut RigidBody, asked: &Body2d) {
    let mass = asked.mass.max(0.0);
    let (inertia, com) = (asked.inertia, asked.center_of_mass);
    if mass <= 0.0 {
        body.set_additional_mass(0.0, false);
    } else if inertia == 0.0 && crate::body::is_default(&com) {
        body.set_additional_mass(mass, true);
    } else {
        let com = scalar::v2a(com);
        body.set_additional_mass_properties(MassProperties::new(com, mass, inertia), true);
    }
}

/// The 2D twin of `crate::body::install_body_state_api`. Every function here
/// has a 3D sibling under the same name in `physics3d`; where the shapes
/// differ, the dimension is the reason.
pub(crate) fn install_body2d_state_api(m: &mut Module<Engine>) {
    m.describe(&[
        ("velocity_at_point", &[c::BODY_2D], "", "How fast a world point on the body is moving, spin included."),
        ("total_mass", &[c::BODY_2D], "", "The body's total mass: its `mass` when it states one, or what its colliders weigh."),
        ("kinetic_energy", &[c::BODY_2D], "", "The body's kinetic energy, for a rest test the solver agrees with."),
        ("potential_energy", &[c::BODY_2D], "", "The body's gravitational potential energy over one step."),
        ("is_moving", &[c::BODY_2D], "", "Whether the body is awake and actually going somewhere."),
        ("effective_dominance", &[c::BODY_2D], "", "The dominance rapier will use for this body: its own group, or the rank every non-dynamic body outranks with."),
        ("teleport", &[c::BODY_2D], "(node: node, x: float, y: float, opts: TeleportOptions?)", "Move the body to a world position at once, clearing its velocity: what assigning the node's position cannot do, because the step writes that back every tick. A `rotation` turns it too, in radians."),
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
            body.gravitational_potential_energy(balaur_core::fixed_dt(), gravity)
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
    m.records(&[
        (
            "Pose",
            "{ position: Vec2, rotation: float }",
            "Where something stands and how it is turned, in radians.",
        ),
        (
            "TeleportOptions",
            "{ rotation: float? }",
            "How `teleport` turns the body as it moves it, in radians.",
        ),
    ]);
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
                    let turn = crate::rapier2d::math::Rotation::from_angle(turn);
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

/// A 2D pose as a script reads one: `{ position, rotation }`, the angle in
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
pub(crate) fn install_body2d_mass_api(m: &mut Module<Engine>) {
    use balaur_script::Value;
    m.describe(&[
        ("world_center_of_mass", &[c::BODY_2D], "(node: node) -> Vec2", "Where the body's whole mass sits, in world space."),
        ("local_center_of_mass", &[c::BODY_2D], "(node: node) -> Vec2", "Where the body's whole mass sits, in the body's own space."),
        ("total_inertia", &[c::BODY_2D], "", "The body's resistance to spin, colliders included."),
        ("effective_mass", &[c::BODY_2D], "(node: node) -> Vec2", "The mass the solver pushes against along each world axis: zero along a locked axis, and on any body that is not dynamic."),
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
            body.mass_properties().local_mprops.principal_inertia()
        })
    });
    m.function("effective_mass", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            Value::Vec2(scalar::a2(body.mass_properties().effective_mass()))
        })
    });
    m.function("effective_angular_inertia", |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            body.mass_properties().effective_angular_inertia()
        })
    });
}

/// The 2D axis locks.
///
/// Split from [`install_body2d_mass_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body2d_lock_api(m: &mut Module<Engine>) {
    m.describe(&[]);
}

/// The 2D twin of `crate::body::install_body_ccd_api`.
pub(crate) fn install_body2d_ccd_api(m: &mut Module<Engine>) {
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
pub(crate) fn install_body2d_sleep_api(m: &mut Module<Engine>) {
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
            "(node: node, dt: float) -> Pose",
            "Where the body will be after `dt` seconds at its current velocity.",
        ),
        (
            "predict_position_with_forces",
            &[c::BODY_2D],
            "(node: node, dt: float) -> Pose",
            "The same, with the forces already applied taken into account: where a thrust or a spring will have put it.",
        ),
        (
            "next_position",
            &[c::BODY_2D],
            "(node: node) -> Pose",
            "The pose a kinematic body has been told to move to.",
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
            read_body(eng, entity_of(node)?, |body| {
                pose_value(&body.predict_position_using_velocity(dt))
            })
        },
    );
    m.function(
        "predict_position_with_forces",
        |eng: &Engine, (node, dt): (NodeId, f32)| {
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
            body.activation().time_since_can_sleep
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
pub(crate) fn install_body2d_force_api(m: &mut Module<Engine>) {
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
fn install_step_force_api(m: &mut Module<Engine>) {
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
    let dt = || balaur_core::fixed_dt();
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
            let torque = torque * dt();
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_torque_impulse(torque, true);
            })
        },
    );
}

fn install_body_forces(m: &mut Module<Engine>) {
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
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_torque(torque, true);
            })
        },
    );
}

/// What a 2D body's forces currently are, and how to drop them.
///
/// Split from [`install_body2d_force_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body2d_force_reader_api(m: &mut Module<Engine>) {
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

/// The body the component asks for, made or written again.
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let asked = eng
        .world()
        .get::<&Body2d>(entity)
        .map(|held| Body2d::clone(&held));
    match asked {
        Ok(asked) => apply_body(eng, entity, &asked),
        Err(_) => Ok(()),
    }
}

/// `body2d`: as `body3d`, the body itself in [`crate::PhysicsState2d`].
pub(crate) fn register_body2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Body2d::NAME,
        ComponentDef {
            events: crate::vocabulary::hook::BODY,
            warnings: Some(Box::new(body_warnings_2d)),
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &[balaur_core::transform::COMPONENT],
            remove: Box::new(|eng, entity| {
                let _ = eng.remove_one::<Body2d>(entity);
                let collider = authored_collider(eng, entity);
                remove_body_and_colliders(eng, entity);
                if let Some(params) = collider {
                    apply_collider(eng, entity, &params)?;
                }
                Ok(())
            }),
            ..ComponentDef::of::<Body2d>()
        },
    );
}
