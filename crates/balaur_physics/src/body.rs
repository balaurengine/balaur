//! The `body3d` component and the script calls that drive one.
//!
//! Split from `lib.rs` under `MAX_FILE_LINES`: bodies, colliders and the
//! plugin itself are three subjects, and only the plugin needs all three.

use crate::rapier3d::prelude::{
    LockedAxes, MassProperties, RigidBody, RigidBodyBuilder, RigidBodyHandle, RigidBodyType,
};
use crate::scalar::{self, Real, Vector};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Written};
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId};

use crate::collider::{apply_collider, authored_collider};
use crate::properties::{Axis3d, BodyKind};
use crate::query::overlaps_value;
use crate::vocabulary::words;
use crate::vocabulary::{component as c, keys as k};
use crate::{PhysicsState3d, node_pose};

const TOTAL_INERTIA_ROTATION: &str = "total_inertia_rotation";

/// A 3D rigid body simulated by rapier. `kind` is `dynamic`, `static`, `kinematic` or `kinematic_velocity`; add a `collider3d` for its shape.
#[allow(
    clippy::struct_excessive_bools,
    reason = "a property a field: each switch the component offers is one"
)]
#[derive(Clone, Component)]
#[component(name = "body3d", requires = [balaur_core::transform::COMPONENT], after_write = after_write)]
pub struct Body3d {
    /// How physics drives the node: simulated, immovable, moved by script, or moved by a velocity you set
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
    pub center_of_mass: [f32; 3],
    /// Resistance to spin about each axis, read when mass is set; 0 derives it from the colliders' shapes scaled to the mass, about center_of_mass when that is set
    #[property(group = "mass")]
    pub inertia: [f32; 3],
    /// Turns the axes inertia is measured about, as `rotation_euler` does; read with a non-zero inertia. Euler radians in the file
    #[property(unit = "degrees", group = "mass")]
    pub inertia_rotation: [f32; 3],
    /// Drag on travel: how fast the body loses speed with nothing touching it
    #[property(min = 0.0, group = "damping")]
    pub linear_damping: f32,
    /// Drag on spin, in the same terms as linear_damping
    #[property(min = 0.0, group = "damping")]
    pub angular_damping: f32,
    /// World axes the body may not turn about; locking all three keeps a character upright
    #[property(flags, group = "locks")]
    pub lock_rotation: Vec<Axis3d>,
    /// World axes the body may not move along
    #[property(flags, group = "locks")]
    pub lock_translation: Vec<Axis3d>,
    /// How fast the body travels when it is made, in units per second; a later patch does not reapply it
    #[property(group = "start")]
    pub initial_linear_velocity: [f32; 3],
    /// How fast the body spins when it is made, about each axis; radians per second in the file, and a later patch does not reapply it
    #[property(unit = "degrees", group = "start")]
    pub initial_angular_velocity: [f32; 3],
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
    /// Model the wobble a spinning body's own inertia gives it, as a thrown American football has
    #[property(group = "solver")]
    pub gyroscopic_forces: bool,
}

crate::shared::body::functions!(
    state = PhysicsState3d,
    body = Body3d,
    handle = RigidBodyHandle,
    builder = RigidBodyBuilder,
    node_pose = node_pose,
    missing = "node has no rigid body"
);

/// The axes the body may not move or turn along, as rapier's flag set.
fn locked_axes(asked: &Body3d) -> LockedAxes {
    let mut axes = LockedAxes::empty();
    for (named, x, y, z) in [
        (
            &asked.lock_translation,
            LockedAxes::TRANSLATION_LOCKED_X,
            LockedAxes::TRANSLATION_LOCKED_Y,
            LockedAxes::TRANSLATION_LOCKED_Z,
        ),
        (
            &asked.lock_rotation,
            LockedAxes::ROTATION_LOCKED_X,
            LockedAxes::ROTATION_LOCKED_Y,
            LockedAxes::ROTATION_LOCKED_Z,
        ),
    ] {
        for (axis, flag) in [(Axis3d::X, x), (Axis3d::Y, y), (Axis3d::Z, z)] {
            if named.contains(&axis) {
                axes |= flag;
            }
        }
    }
    axes
}

/// Every property onto a body that already exists.
///
/// Writing rather than rebuilding is what lets a scene change a body's kind
/// mid-flight without dropping its velocity — and what keeps `patch` (one
/// property at a time, from an animation) honest.
pub(crate) fn write_body(body: &mut RigidBody, asked: &Body3d, may_sleep: bool) {
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
    body.enable_gyroscopic_forces(asked.gyroscopic_forces);
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

/// What the author wrote that the body cannot hold.
pub(crate) fn authored(asked: &Body3d) -> crate::shared::body::Authored {
    crate::shared::body::Authored {
        can_sleep: asked.can_sleep,
        fit_inertia: asked.mass > 0.0
            && is_default(&asked.inertia)
            && !is_default(&asked.center_of_mass),
        sleep_threshold: asked.sleep_threshold,
        sleep_angular_threshold: asked.sleep_angular_threshold,
        start_asleep: asked.start_asleep,
        initial_linear_velocity: asked.initial_linear_velocity,
        initial_angular_velocity: asked.initial_angular_velocity,
    }
}

/// What rapier takes only when a body is made: its first velocities, and
/// whether it starts asleep. A patch never reaches here.
fn start_body(body: &mut RigidBody, authored: &crate::shared::body::Authored, may_sleep: bool) {
    body.set_linvel(scalar::v3a(authored.initial_linear_velocity), true);
    body.set_angvel(scalar::v3a(authored.initial_angular_velocity), true);
    // As rapier's builder does: a body that may not sleep starts awake.
    if authored.start_asleep && may_sleep {
        body.sleep();
    }
}

/// A rotation from the Euler radians a schema property holds, read as a
/// transform's `rotation_euler` is.
pub(crate) fn rotation_from_euler(r: [f32; 3]) -> scalar::Rotation {
    balaur_geometry::skeleton::quat_from_euler(glamx::Vec3::from_array(r))
}

/// The inverse of [`rotation_from_euler`].
pub(crate) fn euler_of(rotation: scalar::Rotation) -> [f32; 3] {
    balaur_geometry::skeleton::euler_from_quat(rotation).to_array()
}

/// The inertia rapier's `Mass` variant would derive, about the stated centre of
/// mass instead of the shapes' own: unit-density shapes scaled to the mass,
/// moved by the parallel-axis theorem.
fn fit_inertia(world: &mut crate::rapier3d::pipeline::PhysicsWorld, handle: RigidBodyHandle) {
    use crate::rapier3d::dynamics::RigidBodyAdditionalMassProps as Extra;
    use crate::rapier3d::math::Matrix;
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
    let fitted = if shapes.mass() > 0.0 {
        shapes.set_mass(mass, true);
        let d = com - shapes.local_com;
        let outer = Matrix::from_cols(d * d.x, d * d.y, d * d.z);
        let shift = (Matrix::from_diagonal(Vector::splat(d.length_squared())) - outer) * mass;
        MassProperties::with_inertia_matrix(com, mass, shapes.reconstruct_inertia_matrix() + shift)
    } else {
        // No shape yet to derive it from; the first collider refits it.
        MassProperties::new(com, mass, Vector::ZERO)
    };
    world.bodies[handle].set_additional_mass_properties(fitted, false);
}

/// Whether the body states its own `mass`, which its colliders then do not add
/// to.
pub(crate) fn has_total_mass(body: &RigidBody) -> bool {
    use crate::rapier3d::dynamics::RigidBodyAdditionalMassProps as Extra;
    match body.mass_properties().additional_local_mprops.as_deref() {
        Some(Extra::Mass(mass)) => *mass > 0.0,
        Some(Extra::MassProps(props)) => props.mass() > 0.0,
        None => false,
    }
}

/// Whether the author left a property at its all-zero default. An exact test
/// on purpose: this asks what the file says, not how big a number is.
pub(crate) fn is_default(v: &[f32]) -> bool {
    v.iter().all(|n| *n == 0.0)
}

fn write_mass(body: &mut RigidBody, asked: &Body3d) {
    let mass = asked.mass.max(0.0);
    let (inertia, com) = (asked.inertia, asked.center_of_mass);
    if mass <= 0.0 {
        body.set_additional_mass(0.0, false);
    } else if is_default(&inertia) && is_default(&com) {
        // Rapier scales the angular inertia with the mass, which is what an
        // author who only wrote `mass = 5` means.
        body.set_additional_mass(mass, true);
    } else {
        let com = scalar::v3a(com);
        let inertia = scalar::v3a(inertia);
        let frame = rotation_from_euler(asked.inertia_rotation);
        body.set_additional_mass_properties(
            MassProperties::with_principal_inertia_frame(com, mass, inertia, frame),
            true,
        );
    }
}

/// Body creation, the forces that move one, and the overlap query.
///
/// Split from [`install_body_state_api`] under `MAX_FN_LINES`; the line is
/// between *doing something to* a body and *asking or tuning* one.
pub(crate) fn install_body_api(m: &mut Module<Engine>) {
    m.describe(&[
        (
            words::functions::APPLY_IMPULSE,
            &[c::BODY_3D],
            "",
            "Add an instant change in momentum, as if the body were struck.",
        ),
        (
            words::functions::APPLY_IMPULSE_AT_POINT,
            &[c::BODY_3D],
            "",
            "Strike the body at a world point, which spins it as well as moves it.",
        ),
        (
            words::functions::APPLY_TORQUE_IMPULSE,
            &[c::BODY_3D],
            "",
            "Add an instant change in angular momentum, as if the body were spun.",
        ),
    ]);
    // Takes the `collider3d` component's own table (`kind`, `radius`,
    // `size`, `restitution`, `friction`, `density`), so one
    // vocabulary covers scripts and scene files, as `physics2d` does.
    m.function(
        words::functions::APPLY_IMPULSE,
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function(
        words::functions::APPLY_IMPULSE_AT_POINT,
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
        words::functions::APPLY_TORQUE_IMPULSE,
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_torque_impulse(scalar::v3(x, y, z), true);
            })
        },
    );
    // Sensor pairs only: rapier's narrow phase reports an intersection only
    // when at least one of the two colliders is a sensor.
    m.function(words::functions::OVERLAPS, |eng: &Engine, node: NodeId| {
        overlaps_value(eng, node)
    });
}

/// The forces that move a body over time, and the world's own gravity.
///
/// Split from [`install_body_api`] under `MAX_FN_LINES`; the line is between
/// *making* a body and *pushing* one.
/// A force for one step: the impulse it would deliver over one fixed step, so
/// nothing is left on the body for the step after.
fn install_step_force_api(m: &mut Module<Engine>) {
    m.describe(&[
        (
            words::functions::APPLY_FORCE,
            &[c::BODY_3D],
            "",
            "Push the body for the next step only; `add_constant_force` keeps pushing.",
        ),
        (
            words::functions::APPLY_FORCE_AT_POINT,
            &[c::BODY_3D],
            "",
            "Push at a world point for the next step only, which also turns the body.",
        ),
        (
            words::functions::APPLY_TORQUE,
            &[c::BODY_3D],
            "",
            "Turn the body for the next step only; `add_constant_torque` keeps turning it.",
        ),
    ]);
    let dt = || balaur_core::fixed_dt();
    m.function(
        words::functions::APPLY_FORCE,
        move |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse(scalar::v3(x, y, z) * dt(), true);
            })
        },
    );
    m.function(
        words::functions::APPLY_FORCE_AT_POINT,
        move |eng: &Engine, (node, x, y, z, px, py, pz): (NodeId, f32, f32, f32, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse_at_point(
                    scalar::v3(x, y, z) * dt(),
                    scalar::v3(px, py, pz),
                    true,
                );
            })
        },
    );
    m.function(
        words::functions::APPLY_TORQUE,
        move |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_torque_impulse(scalar::v3(x, y, z) * dt(), true);
            })
        },
    );
}

pub(crate) fn install_force_api(m: &mut Module<Engine>) {
    install_step_force_api(m);
    m.describe(&[
        (
            words::functions::ADD_CONSTANT_FORCE,
            &[c::BODY_3D],
            "",
            "Push the body every step until the constant force is set back to zero; unlike an impulse this is spread over time.",
        ),
        (
            words::functions::ADD_CONSTANT_FORCE_AT_POINT,
            &[c::BODY_3D],
            "",
            "Push at a world point every step, which also turns the body.",
        ),
        (
            words::functions::ADD_CONSTANT_TORQUE,
            &[c::BODY_3D],
            "",
            "Turn the body every step until the constant torque is set back to zero.",
        ),
        (
            words::functions::SET_CONSTANT_FORCE,
            &[c::BODY_3D],
            "",
            "Replace the constant force with this one; zero stops the push.",
        ),
        (
            words::functions::SET_CONSTANT_TORQUE,
            &[c::BODY_3D],
            "",
            "Replace the constant torque with this one; zero stops the turn.",
        ),
        (
            words::functions::CONSTANT_FORCE,
            &[c::BODY_3D],
            "",
            "The force every step integrates until it is set back to zero.",
        ),
        (
            words::functions::CONSTANT_TORQUE,
            &[c::BODY_3D],
            "",
            "The torque every step integrates until it is set back to zero.",
        ),
        (
            words::functions::SET_GRAVITY,
            &[],
            "",
            "Set the 3D world's gravity, in units per second squared.",
        ),
        (
            words::functions::WAKE_ALL,
            &[],
            "()",
            "Wake every sleeping body in the 3D world.",
        ),
    ]);
    m.function(
        words::functions::ADD_CONSTANT_FORCE,
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].add_force(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function(
        words::functions::ADD_CONSTANT_FORCE_AT_POINT,
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
        words::functions::ADD_CONSTANT_TORQUE,
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
pub(crate) fn install_force_reader_api(m: &mut Module<Engine>) {
    m.describe(&[
        (words::functions::OVERLAPS, &[c::COLLIDER_3D], "", "The nodes this one currently intersects; rapier reports a pair only when one of the two colliders is a sensor."),
        (words::functions::GRAVITY, &[], "", "The 3D world's gravity."),
    ]);
    m.function(
        words::functions::SET_CONSTANT_FORCE,
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.reset_forces(true);
                body.add_force(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function(
        words::functions::SET_CONSTANT_TORQUE,
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.reset_torques(true);
                body.add_torque(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function(
        words::functions::CONSTANT_FORCE,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                let f = body.user_force();
                (f.x, f.y, f.z)
            })
        },
    );
    m.function(
        words::functions::CONSTANT_TORQUE,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                let t = body.user_torque();
                (t.x, t.y, t.z)
            })
        },
    );
    m.function(
        words::functions::SET_GRAVITY,
        |eng: &Engine, (x, y, z): (f32, f32, f32)| {
            let state = eng.resource::<PhysicsState3d>();
            state.borrow_mut().world.gravity = scalar::v3(x, y, z);
            Ok(())
        },
    );
    m.function(words::functions::GRAVITY, |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState3d>();
        let g = state.borrow().world.gravity;
        Ok((g.x, g.y, g.z))
    });
    m.function(words::functions::WAKE_ALL, |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState3d>();
        state.borrow_mut().world.wake_up_all(true);
        Ok(())
    });
}

/// Velocity, sleep, mass and the per-body tuning knobs — everything that asks
/// a body about itself or changes how it is simulated.
pub(crate) fn install_body_state_api(m: &mut Module<Engine>) {
    m.describe(&[
        (words::functions::POTENTIAL_ENERGY, &[c::BODY_3D], "", "The body's gravitational potential energy over one step."),
        (words::functions::IS_MOVING, &[c::BODY_3D], "", "Whether the body is awake and actually going somewhere."),
        (words::functions::EFFECTIVE_DOMINANCE, &[c::BODY_3D], "", "The dominance rapier will use for this body: its own group, or the rank every non-dynamic body outranks with."),
        (
            words::functions::SET_LINEAR_VELOCITY,
            &[c::BODY_3D],
            "",
            "Set how fast the body travels, in units per second.",
        ),
        (
            words::functions::LINEAR_VELOCITY,
            &[c::BODY_3D],
            "",
            "How fast the body is travelling, in units per second.",
        ),
        (
            words::functions::SET_ANGULAR_VELOCITY,
            &[c::BODY_3D],
            "",
            "Set how fast the body spins, in radians per second about each axis.",
        ),
        (
            words::functions::ANGULAR_VELOCITY,
            &[c::BODY_3D],
            "",
            "How fast the body is spinning, in radians per second about each axis.",
        ),
        (
            words::functions::VELOCITY_AT_POINT,
            &[c::BODY_3D],
            "",
            "How fast a world point on the body is moving, spin included.",
        ),
        (
            words::functions::TOTAL_MASS,
            &[c::BODY_3D],
            "",
            "The body's total mass: its `mass` when it states one, or what its colliders weigh.",
        ),
        (
            words::functions::KINETIC_ENERGY,
            &[c::BODY_3D],
            "",
            "The body's kinetic energy, for a rest test the solver agrees with.",
        ),
    ]);
    m.function(
        words::functions::SET_LINEAR_VELOCITY,
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_linvel(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function(
        words::functions::LINEAR_VELOCITY,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                let v = body.linvel();
                (v.x, v.y, v.z)
            })
        },
    );
    m.function(
        words::functions::SET_ANGULAR_VELOCITY,
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_angvel(scalar::v3(x, y, z), true);
            })
        },
    );
    m.function(
        words::functions::ANGULAR_VELOCITY,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                let w = body.angvel();
                (w.x, w.y, w.z)
            })
        },
    );
    m.function(
        words::functions::VELOCITY_AT_POINT,
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
fn install_body_readers(m: &mut Module<Engine>) {
    m.function(
        words::functions::TOTAL_MASS,
        |eng: &Engine, node: NodeId| read_body(eng, entity_of(node)?, RigidBody::mass),
    );
    m.function(
        words::functions::KINETIC_ENERGY,
        |eng: &Engine, node: NodeId| read_body(eng, entity_of(node)?, RigidBody::kinetic_energy),
    );
    m.function(
        words::functions::POTENTIAL_ENERGY,
        |eng: &Engine, node: NodeId| {
            let gravity = {
                let state = eng.resource::<PhysicsState3d>();

                state.borrow().world.gravity
            };
            read_body(eng, entity_of(node)?, |body| {
                body.gravitational_potential_energy(balaur_core::fixed_dt(), gravity)
            })
        },
    );
    m.function(words::functions::IS_MOVING, |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| -> bool { body.is_moving() })
    });
    m.function(
        words::functions::EFFECTIVE_DOMINANCE,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                f32::from(body.effective_dominance_group())
            })
        },
    );
    // The step writes a dynamic body's pose into `Transform` every tick, so
    // assigning `node.position` on one is overwritten before it is seen. This
    // is the way to move one, and it says what it costs: the velocity goes.
}

/// A pose as a script reads one: `{ position, rotation }`, the rotation in
/// Euler radians, as `rotation_euler` spells them.
fn pose_value(pose: &scalar::Pose) -> balaur_script::Value {
    crate::vocabulary::map([
        (
            k::POSITION,
            balaur_script::Value::Vec3(scalar::a3(pose.translation)),
        ),
        (
            k::ROTATION,
            balaur_script::Value::Vec3(euler_of(pose.rotation)),
        ),
    ])
}

/// Where a body is and where it is about to be, and the one way to move a
/// dynamic one.
///
/// Split from [`install_body_state_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body_pose_api(m: &mut Module<Engine>) {
    m.describe(&[
        (words::functions::TELEPORT, &[c::BODY_3D], "(node: node, x: float, y: float, z: float, opts: TeleportOptions?)", "Move the body to a world position at once, clearing its velocity: what assigning the node's position cannot do, because the step writes that back every tick. A `rotation` turns it too, in Euler radians."),
        (words::functions::PREDICT_POSITION, &[c::BODY_3D], "(node: node, dt: float) -> Pose", "Where the body will be after `dt` seconds at its current velocity."),
        (words::functions::PREDICT_POSITION_WITH_FORCES, &[c::BODY_3D], "(node: node, dt: float) -> Pose", "The same, with the forces already applied taken into account: where a thrust or a spring will have put it."),
        (words::functions::NEXT_POSITION, &[c::BODY_3D], "(node: node) -> Pose", "The pose a kinematic body has been told to move to."),
    ]);
    m.records(&[
        (
            "Pose",
            "{ position: Vec3, rotation: Vec3 }",
            "Where something stands and how it is turned, in Euler radians as `rotation_euler` reads them.",
        ),
        (
            "TeleportOptions",
            "{ rotation: Vec3? }",
            "How `teleport` turns the body as it moves it: Euler radians as `rotation_euler` reads them.",
        ),
    ]);
    m.function(
        words::functions::TELEPORT,
        |eng: &Engine,
         (node, x, y, z, opts): (NodeId, f32, f32, f32, Option<balaur_script::Value>)| {
            let entity = entity_of(node)?;
            let opts = crate::vocabulary::Opts(opts.as_ref());
            let turn = opts
                .get(k::ROTATION)
                .map(|_| opts.vec3(k::ROTATION, [0.0; 3]));
            with_body(eng, entity, |state, handle| {
                let body = &mut state.world.bodies[handle];
                body.set_translation(scalar::v3(x, y, z), true);
                if let Some(turn) = turn {
                    body.set_rotation(rotation_from_euler(turn), true);
                }
                body.set_linvel(scalar::Vector::ZERO, true);
                body.set_angvel(scalar::Vector::ZERO, true);
                // A query before the next step must see the new place.
                state.queries_ready = false;
            })?;
            // Otherwise the frames after a respawn draw the character
            // streaking from where it was to where it now is.
            balaur_core::interpolate::reset(eng, entity);
            Ok(())
        },
    );
    m.function(
        words::functions::PREDICT_POSITION,
        |eng: &Engine, (node, dt): (NodeId, f32)| {
            read_body(eng, entity_of(node)?, |body| {
                pose_value(&body.predict_position_using_velocity(dt))
            })
        },
    );
    m.function(
        words::functions::PREDICT_POSITION_WITH_FORCES,
        |eng: &Engine, (node, dt): (NodeId, f32)| {
            read_body(eng, entity_of(node)?, |body| {
                pose_value(&body.predict_position_using_velocity_and_forces(dt))
            })
        },
    );
    m.function(
        words::functions::NEXT_POSITION,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                pose_value(body.next_position())
            })
        },
    );
}

/// What a body weighs and how that mass is spread, as rapier totals it from
/// the body's own `mass` and its colliders.
///
/// Split from [`install_body_state_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body_mass_api(m: &mut Module<Engine>) {
    use balaur_script::Value;
    m.describe(&[
        (words::functions::WORLD_CENTER_OF_MASS, &[c::BODY_3D], "(node: node) -> Vec3", "Where the body's whole mass sits, in world space."),
        (words::functions::LOCAL_CENTER_OF_MASS, &[c::BODY_3D], "(node: node) -> Vec3", "Where the body's whole mass sits, in the body's own space."),
        (words::functions::TOTAL_INERTIA, &[c::BODY_3D], "(node: node) -> Vec3", "The body's resistance to spin about each of its principal axes, colliders included."),
        (TOTAL_INERTIA_ROTATION, &[c::BODY_3D], "(node: node) -> Vec3", "How those principal axes are turned in the body's own space, as Euler radians as `rotation_euler` reads them."),
        (words::functions::EFFECTIVE_MASS, &[c::BODY_3D], "(node: node) -> Vec3", "The mass the solver pushes against along each world axis: zero along a locked axis, and on any body that is not dynamic."),
        (words::functions::EFFECTIVE_ANGULAR_INERTIA, &[c::BODY_3D], "(node: node) -> [Vec3]", "The world-space inertia the solver turns, rotation locks applied, as its three rows."),
    ]);
    m.function(
        words::functions::WORLD_CENTER_OF_MASS,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                Value::Vec3(scalar::a3(body.center_of_mass()))
            })
        },
    );
    m.function(
        words::functions::LOCAL_CENTER_OF_MASS,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                Value::Vec3(scalar::a3(body.local_center_of_mass()))
            })
        },
    );
    m.function(
        words::functions::TOTAL_INERTIA,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                Value::Vec3(scalar::a3(
                    body.mass_properties().local_mprops.principal_inertia(),
                ))
            })
        },
    );
    m.function(TOTAL_INERTIA_ROTATION, |eng: &Engine, node: NodeId| {
        read_body(eng, entity_of(node)?, |body| {
            let frame = body
                .mass_properties()
                .local_mprops
                .principal_inertia_local_frame;
            Value::Vec3(euler_of(frame))
        })
    });
    m.function(
        words::functions::EFFECTIVE_MASS,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                Value::Vec3(scalar::a3(body.mass_properties().effective_mass()))
            })
        },
    );
    m.function(
        words::functions::EFFECTIVE_ANGULAR_INERTIA,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                let i = body.mass_properties().effective_angular_inertia();
                let row = |a: Real, b: Real, c: Real| Value::Vec3(scalar::a3(Vector::new(a, b, c)));
                Value::List(vec![
                    row(i.m11, i.m12, i.m13),
                    row(i.m12, i.m22, i.m23),
                    row(i.m13, i.m23, i.m33),
                ])
            })
        },
    );
}

/// The axis locks: what keeps a character upright and a top-down game flat.
///
/// Split from [`install_body_mass_api`] under `MAX_FN_LINES`.
pub(crate) fn install_body_lock_api(m: &mut Module<Engine>) {
    m.describe(&[]);
}

/// Continuous collision detection: whether a body's sweep ran this step.
pub(crate) fn install_body_ccd_api(m: &mut Module<Engine>) {
    m.describe(&[(
        words::functions::IS_CCD_ACTIVE,
        &[c::BODY_3D],
        "",
        "Whether the body moved fast enough last step for rapier to sweep it, with or without continuous_collision.",
    )]);
    m.function(
        words::functions::IS_CCD_ACTIVE,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| -> bool {
                body.is_ccd_active()
            })
        },
    );
}

/// Whether a body is asleep, and how close it is to falling asleep.
pub(crate) fn install_body_sleep_api(m: &mut Module<Engine>) {
    m.describe(&[
        (words::functions::SLEEP, &[c::BODY_3D], "", "Put the body to sleep now."),
        (
            words::functions::WAKE_UP,
            &[c::BODY_3D],
            "",
            "Wake the body, so the next step moves it.",
        ),
        (
            words::functions::IS_SLEEPING,
            &[c::BODY_3D],
            "",
            "Whether the body is asleep and being skipped.",
        ),
        (
            words::functions::TIME_SINCE_CAN_SLEEP,
            &[c::BODY_3D],
            "",
            "Seconds the body has spent under its sleep thresholds; it sleeps once this reaches time_to_sleep.",
        ),
    ]);
    m.function(words::functions::SLEEP, |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].sleep();
        })
    });
    m.function(words::functions::WAKE_UP, |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].wake_up(true);
        })
    });
    m.function(
        words::functions::IS_SLEEPING,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| -> bool { body.is_sleeping() })
        },
    );
    m.function(
        words::functions::TIME_SINCE_CAN_SLEEP,
        |eng: &Engine, node: NodeId| {
            read_body(eng, entity_of(node)?, |body| {
                body.activation().time_since_can_sleep
            })
        },
    );
}

/// A dynamic body that nothing collides with: no collider on the node or
/// under it, so it falls through everything.
fn body_warnings(eng: &Engine, entity: Entity) -> Vec<balaur_core::warnings::Warning> {
    let state = eng.resource::<crate::PhysicsState3d>();
    let state = state.borrow();
    let body = state
        .bodies
        .get(&entity)
        .and_then(|&handle| state.world.bodies.get(handle));
    match body {
        Some(body) if body.is_dynamic() && body.colliders().is_empty() => {
            vec![balaur_core::warnings::Warning::whole(format!(
                "nothing collides with it: add a {} to the node or a child, or it falls through everything",
                c::COLLIDER_3D
            ))]
        }
        _ => Vec::new(),
    }
}

/// The body the component asks for, made or written again.
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let asked = eng
        .world()
        .get::<&Body3d>(entity)
        .map(|held| Body3d::clone(&held));
    match asked {
        Ok(asked) => apply_body(eng, entity, &asked),
        Err(_) => Ok(()),
    }
}

/// `body3d`: the asked-for struct on the node, the body itself in
/// [`crate::PhysicsState3d`].
pub(crate) fn register_body_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Body3d::NAME,
        ComponentDef {
            events: crate::vocabulary::hook::BODY,
            warnings: Some(Box::new(body_warnings)),
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &[balaur_core::transform::COMPONENT],
            remove: Box::new(|eng, entity| {
                let _ = eng.remove_one::<Body3d>(entity);
                // Removing the body keeps the collider, as static geometry.
                let collider = authored_collider(eng, entity);
                remove_body_and_colliders(eng, entity);
                if let Some(params) = collider {
                    apply_collider(eng, entity, &params)?;
                }
                Ok(())
            }),
            ..ComponentDef::of::<Body3d>()
        },
    );
}
