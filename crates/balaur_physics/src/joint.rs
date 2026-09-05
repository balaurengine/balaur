//! `joint3d`: two bodies held together, and everything rapier lets you say
//! about how.
//!
//! The joint lives on a node so it can be selected, gizmo-drawn and deleted
//! like anything else. The node it sits on is one end; the `body` property
//! names the other.
//!
//! Two solvers, because rapier has two. `impulse` is the general one: any
//! graph, loops included. `reduced` is a multibody joint in reduced
//! coordinates — no drift and no wasted solver work, at the price of no loops
//! — and it is what an articulated arm with inverse kinematics wants.

use crate::rapier3d::prelude::{
    FixedJointBuilder, GenericJoint, GenericJointBuilder, ImpulseJointHandle, JointAxesMask,
    JointAxis, MotorModel, MultibodyJointHandle, PrismaticJointBuilder, RevoluteJointBuilder,
    RigidBodyHandle, RopeJointBuilder, SphericalJointBuilder, SpringJointBuilder,
};
use crate::scalar::{self, Real, Vector};
use anyhow::{Result, anyhow};
use balaur_core::components::{ComponentDef, as_node};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::PhysicsState;
use crate::rapier3d::pipeline::PhysicsWorld;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

/// Which handle a node's joint has, because rapier keeps the two solvers in
/// two sets and a joint is one or the other for its whole life.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum JointHandle {
    Impulse(ImpulseJointHandle),
    Multibody(MultibodyJointHandle),
}

/// A node's joint, and the pull that snaps it.
///
/// The threshold lives here rather than being read back from the component
/// each step: the step checks every breakable joint every tick, and a
/// component read is a registry lookup and a `toml::Value` per joint.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct JointRef {
    pub handle: JointHandle,
    pub break_force: Real,
}

/// The axis a kind's limits and motor act on.
///
/// A revolute joint turns about one axis and a prismatic slides along one, so
/// "the limits" means that axis's. A spherical joint has three; its motor and
/// limits are applied to all of them, which is what a shoulder wants.
fn free_axes(kind: &str) -> &'static [JointAxis] {
    match kind {
        w::REVOLUTE => &[JointAxis::AngX],
        w::PRISMATIC | w::ROPE | w::SPRING => &[JointAxis::LinX],
        w::SPHERICAL => &[JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ],
        _ => &[],
    }
}

/// The axes a `generic` joint locks, from its `flags` property.
fn locked_axes(params: &toml::Value) -> JointAxesMask {
    let mut mask = JointAxesMask::empty();
    for (name, axis) in [
        (w::X, JointAxesMask::LIN_X),
        (w::Y, JointAxesMask::LIN_Y),
        (w::Z, JointAxesMask::LIN_Z),
        (w::ANG_X, JointAxesMask::ANG_X),
        (w::ANG_Y, JointAxesMask::ANG_Y),
        (w::ANG_Z, JointAxesMask::ANG_Z),
    ] {
        if v::flag(params, k::LOCKED_AXES, name) {
            mask |= axis;
        }
    }
    mask
}

/// The joint a `joint3d` table describes.
pub(crate) fn joint_of(params: &toml::Value) -> Result<GenericJoint> {
    let kind = v::text(params, k::KIND, w::FIXED);
    let axis = scalar::v3a(v::vec3(params, k::AXIS, [0.0, 0.0, 1.0]));
    let axis = if axis.length_squared() < 1.0e-12 {
        Vector::Z
    } else {
        axis.normalize()
    };
    let anchor1 = scalar::v3a(v::vec3(params, k::ANCHOR, [0.0; 3]));
    let anchor2 = scalar::v3a(v::vec3(params, k::OTHER_ANCHOR, [0.0; 3]));
    let length = scalar::real(v::f(params, k::LENGTH, 0.0));
    let stiffness = scalar::real(v::f(params, k::STIFFNESS, 0.0));
    let damping = scalar::real(v::f(params, k::DAMPING, 1.0));
    let mut joint: GenericJoint = match kind {
        w::FIXED => FixedJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::REVOLUTE => RevoluteJointBuilder::new(axis)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::PRISMATIC => PrismaticJointBuilder::new(axis)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::SPHERICAL => SphericalJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::ROPE => RopeJointBuilder::new(length.max(0.0))
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::SPRING => SpringJointBuilder::new(length.max(0.0), stiffness, damping)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::GENERIC => GenericJointBuilder::new(locked_axes(params))
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .local_axis1(axis)
            .local_axis2(axis)
            .build(),
        other => return Err(anyhow!("unknown joint kind '{other}'")),
    };
    joint.set_contacts_enabled(v::boolean(params, k::CONTACTS, false));
    write_limits_and_motor(&mut joint, params, kind);
    Ok(joint)
}

/// Limits and the motor, on whichever axes the kind leaves free.
///
/// `limits = [0, 0]` is how a scene says *no limit*: a joint whose minimum and
/// maximum are the same is either locked or unlimited, and locked is what
/// `locked_axes` is for.
fn write_limits_and_motor(joint: &mut GenericJoint, params: &toml::Value, kind: &str) {
    let limits = scalar::v2a(v::vec2(params, k::LIMITS, [0.0; 2]));
    let motor = v::text(params, k::MOTOR, w::OFF);
    let target = scalar::real(v::f(params, k::MOTOR_TARGET, 0.0));
    let max_force = scalar::real(v::f(params, k::MOTOR_MAX_FORCE, 0.0));
    let stiffness = scalar::real(v::f(params, k::STIFFNESS, 0.0));
    let damping = scalar::real(v::f(params, k::DAMPING, 1.0));
    let model = match v::text(params, k::MOTOR_MODEL, w::ACCELERATION) {
        w::FORCE => MotorModel::ForceBased,
        _ => MotorModel::AccelerationBased,
    };
    for axis in free_axes(kind).iter().copied() {
        if limits.x < limits.y {
            joint.set_limits(axis, [limits.x, limits.y]);
        }
        match motor {
            w::VELOCITY => {
                joint.set_motor_model(axis, model);
                joint.set_motor_velocity(axis, target, damping);
            }
            w::POSITION => {
                joint.set_motor_model(axis, model);
                joint.set_motor_position(axis, target, stiffness, damping);
            }
            _ => {}
        }
        if max_force > 0.0 {
            joint.set_motor_max_force(axis, max_force);
        }
    }
}

/// The two bodies a joint ties: the node it sits on, and the one `body` names.
///
/// Either end may be a bodiless child, which stands for the nearest body
/// above it — as a collider on a child does. A joint is one per node, so
/// this is how one body carries several.
fn ends(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<(Entity, Entity)> {
    let other = as_node(eng, entity, params.get(k::BODY))
        .ok_or_else(|| anyhow!("a joint needs a `body` naming the node at its other end"))?;
    Ok((body_above(eng, entity), body_above(eng, other)))
}

/// The node itself when it has a body, else the nearest ancestor that does;
/// the node again when none does, so `handles` reports the missing body.
fn body_above(eng: &Engine, entity: Entity) -> Entity {
    crate::collider::nearest_body(eng, entity).map_or(entity, |(node, _)| node)
}

crate::shared::joint::functions!(
    state = PhysicsState,
    world = PhysicsWorld,
    reference = JointRef,
    handle = JointHandle,
    component = c::JOINT_3D,
    body = c::BODY_3D,
    impulse_magnitude = impulse_magnitude
);

pub(crate) fn apply_joint(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    remove_joint(eng, entity);
    {
        let state = eng.resource::<PhysicsState>();
        state
            .borrow_mut()
            .joint_params
            .insert(entity, params.clone());
    }
    if !v::boolean(params, k::ENABLED, true) {
        return Ok(());
    }
    // A joint whose other end is not in the scene yet is inert rather than an
    // error: a scene file names nodes in whatever order it likes.
    let Ok((a, b)) = ends(eng, entity, params) else {
        return Ok(());
    };
    let joint = joint_of(params)?;
    let reduced = v::text(params, k::SOLVER, w::IMPULSE) == w::REDUCED;
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let (first, second) = handles(&state, a, b)?;
    let handle = if reduced {
        state
            .world
            .insert_multibody_joint(first, second, joint)
            .map(JointHandle::Multibody)
            .ok_or_else(|| {
                anyhow!("a reduced-coordinates joint cannot close a loop; use solver = \"impulse\"")
            })?
    } else {
        JointHandle::Impulse(state.world.insert_impulse_joint(first, second, joint))
    };
    state.joints.insert(
        entity,
        JointRef {
            handle,
            break_force: scalar::real(v::f(params, k::BREAK_FORCE, 0.0)),
        },
    );
    Ok(())
}

/// What `apply` wrote, read back off the joint.
pub(crate) fn get_joint_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    // Authored values first, so a joint waiting for its other end still
    // reports what it is waiting to be.
    let authored = state
        .joint_params
        .get(&entity)
        .and_then(|params| params.as_table().cloned())?;
    let Some(reference) = state.joints.get(&entity) else {
        return Some(toml::Value::Table(authored));
    };
    let data = match &reference.handle {
        JointHandle::Impulse(handle) => state.world.impulse_joints.get(*handle)?.data,
        JointHandle::Multibody(handle) => {
            let (multibody, link) = state.world.multibody_joints.get(*handle)?;
            multibody.link(link)?.joint.data
        }
    };
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let vec3 = |v: Vector| toml::Value::Array(vec![f(v.x), f(v.y), f(v.z)]);
    let mut map = authored;
    map.insert(k::ANCHOR.into(), vec3(data.local_anchor1()));
    map.insert(k::OTHER_ANCHOR.into(), vec3(data.local_anchor2()));
    map.insert(k::CONTACTS.into(), data.contacts_enabled().into());
    map.insert(
        k::SOLVER.into(),
        toml::Value::String(
            match reference.handle {
                JointHandle::Impulse(_) => w::IMPULSE,
                JointHandle::Multibody(_) => w::REDUCED,
            }
            .into(),
        ),
    );
    map.insert(
        k::BREAK_FORCE.into(),
        toml::Value::Float(f64::from(reference.break_force)),
    );
    Some(toml::Value::Table(map))
}

/// How hard a joint is pulling, as one number.
///
/// Rapier keeps the reaction as six components — three linear, three angular —
/// and a breakable joint wants their magnitude.
fn impulse_magnitude(impulses: &[Real; 6]) -> Real {
    impulses.iter().map(|i| i * i).sum::<Real>().sqrt()
}

/// The same, for the 2D world's three-component reaction (two linear, one
/// angular), which rapier hands back as a vector.
pub(crate) fn impulse_magnitude_2d(impulses: &crate::rapier2d::math::SpatialVector) -> Real {
    impulses.length()
}

pub(crate) fn install_joint_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("add_joint", &[c::JOINT_3D], "", "Tie this node's body to another with a joint, from a `joint3d` table: `kind`, `body`, `anchor`, `axis`, `limits`, and the rest of the component's own vocabulary."),
        ("remove_joint", &[c::JOINT_3D], "", "Undo the node's joint, leaving both bodies free."),
        ("set_motor_velocity", &[c::JOINT_3D], "", "Drive the joint towards a speed: how a wheel is powered or a door swings itself shut."),
        ("set_motor_position", &[c::JOINT_3D], "", "Drive the joint towards an angle or a distance, with a spring's stiffness and damping."),
        ("set_joint_limits", &[c::JOINT_3D], "", "Set how far the joint may travel, in radians for a revolute one and units for a prismatic one."),
        ("joint_impulse", &[c::JOINT_3D], "", "How hard the joint is pulling right now: what a breakable one is measured against."),
        ("solve_ik", &[c::JOINT_3D], "", "Move a reduced-coordinates chain so its last link reaches a world position, leaving every joint inside its limits."),
    ]);
    m.function(
        "add_joint",
        |eng: &Engine, (node, params): (NodeId, balaur_script::Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            let entity = entity_of(node)?;
            let full =
                balaur_core::components::properties(eng, &joint_schema_value(eng)?, Some(&params))?;
            apply_joint(eng, entity, &full)
        },
    );
    m.function("remove_joint", |eng: &Engine, node: NodeId| {
        remove_joint(eng, entity_of(node)?);
        Ok(())
    });
    m.function(
        "set_motor_velocity",
        |eng: &Engine, (node, target, factor): (NodeId, f32, f32)| {
            let (target, factor) = (scalar::real(target), scalar::real(factor));
            with_joint(eng, node, |joint, kind| {
                for axis in free_axes(kind).iter().copied() {
                    joint.set_motor_velocity(axis, target, factor);
                }
            })
        },
    );
    m.function(
        "set_motor_position",
        |eng: &Engine, (node, target, stiffness, damping): (NodeId, f32, f32, f32)| {
            let (target, stiffness, damping) = (
                scalar::real(target),
                scalar::real(stiffness),
                scalar::real(damping),
            );
            with_joint(eng, node, |joint, kind| {
                for axis in free_axes(kind).iter().copied() {
                    joint.set_motor_position(axis, target, stiffness, damping);
                }
            })
        },
    );
    m.function(
        "set_joint_limits",
        |eng: &Engine, (node, min, max): (NodeId, f32, f32)| {
            let (min, max) = (scalar::real(min), scalar::real(max));
            with_joint(eng, node, |joint, kind| {
                for axis in free_axes(kind).iter().copied() {
                    joint.set_limits(axis, [min, max]);
                }
            })
        },
    );
    m.function("joint_impulse", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let Some(JointHandle::Impulse(handle)) = state.joints.get(&entity).map(|j| j.handle) else {
            return Ok(0.0);
        };
        Ok(state
            .world
            .impulse_joints
            .get(handle)
            .map_or(0.0, |joint| impulse_magnitude(&joint.impulses)))
    });
    m.function(
        "solve_ik",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            solve_ik(eng, entity_of(node)?, scalar::v3(x, y, z))
        },
    );
}

/// Move a reduced-coordinates chain so the node's own body reaches `target`.
///
/// Rapier's own solver, damped least squares, every joint's limits respected.
/// Impulse joints have no such thing — there are no generalised coordinates to
/// solve for — so this is `solver = "reduced"` only, and says so.
fn solve_ik(eng: &Engine, entity: Entity, target: Vector) -> Result<()> {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let Some(JointHandle::Multibody(handle)) = state.joints.get(&entity).map(|j| j.handle) else {
        return Err(anyhow!(
            "inverse kinematics needs a chain of reduced-coordinates joints (solver = \"reduced\")"
        ));
    };
    let options = crate::rapier3d::dynamics::InverseKinematicsOption::default();
    let pose = scalar::Pose::from_translation(target);
    // The solver reads the bodies and writes the chain, so the two borrows are
    // taken one after the other rather than together.
    let displacements = {
        let (multibody, link) = state
            .world
            .multibody_joints
            .get(handle)
            .ok_or_else(|| anyhow!("this node's joint is gone"))?;
        let mut displacements = crate::rapier3d::na::DVector::zeros(multibody.ndofs());
        multibody.inverse_kinematics(
            &state.world.bodies,
            link,
            &options,
            &pose,
            |_| true,
            &mut displacements,
        );
        displacements
    };
    let (multibody, _) = state
        .world
        .multibody_joints
        .get_mut(handle)
        .ok_or_else(|| anyhow!("this node's joint is gone"))?;
    multibody.apply_displacements(displacements.as_slice());
    let bodies = &mut state.world.bodies;
    multibody.forward_kinematics(bodies, true);
    multibody.update_rigid_bodies(bodies, true);
    Ok(())
}

/// The registered schema, for `add_joint`'s table to be merged over.
fn joint_schema_value(eng: &Engine) -> Result<toml::Value> {
    let registry = eng.resource::<balaur_core::components::ComponentRegistry>();
    let registry = registry.borrow();
    Ok(registry
        .def(c::JOINT_3D)
        .ok_or_else(|| anyhow!("joint3d is not registered"))?
        .schema
        .clone())
}

/// The schema both dimensions share; each adds its own axis-shaped half.
pub(crate) fn shared_joint_schema() -> String {
    let motors = v::options(w::MOTOR_MODES);
    let models = v::options(w::MOTOR_MODELS);
    let solvers = v::options(w::JOINT_SOLVERS);
    let (off, acceleration, impulse) = (w::OFF, w::ACCELERATION, w::IMPULSE);
    v::schema(&[
        (
            k::MOTOR,
            &format!(
                r#"{{ type = "enum", default = "{off}", options = [{motors}], description = "Drive the joint towards a speed, towards a position, or not at all" }}"#
            ),
        ),
        (
            k::MOTOR_TARGET,
            r#"{ type = "float", default = 0.0, description = "The speed or the position the motor drives towards" }"#,
        ),
        (
            k::MOTOR_MAX_FORCE,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The most force the motor may use; 0 means as much as it takes" }"#,
        ),
        (
            k::MOTOR_MODEL,
            &format!(
                r#"{{ type = "enum", default = "{acceleration}", options = [{models}], description = "Whether the motor's strength is felt as an acceleration, ignoring mass, or as a force" }}"#
            ),
        ),
        (
            k::STIFFNESS,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Spring stiffness, for a spring joint or a position motor" }"#,
        ),
        (
            k::DAMPING,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "How quickly the motion settles, for a spring joint or a motor" }"#,
        ),
        (
            k::LENGTH,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The rope's greatest length, or the spring's rest length" }"#,
        ),
        (
            k::CONTACTS,
            r#"{ type = "bool", default = false, description = "Let the two joined bodies collide with each other" }"#,
        ),
        (
            k::BREAK_FORCE,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The pull that snaps the joint and calls on_joint_break; 0 never breaks" }"#,
        ),
        (
            k::SOLVER,
            &format!(
                r#"{{ type = "enum", default = "{impulse}", options = [{solvers}], description = "impulse holds any arrangement, loops included; reduced never drifts and can be solved for inverse kinematics, but cannot close a loop" }}"#
            ),
        ),
        (
            k::ENABLED,
            r#"{ type = "bool", default = true, description = "Hold the two bodies together at all" }"#,
        ),
    ])
}

pub(crate) fn register_joint_component(reg: &mut Registry<'_>) {
    let kinds = v::options(w::JOINT_KINDS);
    let axes = v::options(w::JOINT_AXES);
    let default = w::FIXED;
    let shared = shared_joint_schema();
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{default}", options = [{kinds}], shorthand = true, description = "How the two bodies may move relative to each other" }}"#)),
            (k::BODY, r#"{ type = "node", default = "", description = "The node at the joint's other end; this node is the first end" }"#),
            (k::ANCHOR, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where the joint attaches on this node, in its own space" }"#),
            (k::OTHER_ANCHOR, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where it attaches on the other node, in that node's space" }"#),
            (k::AXIS, r#"{ type = "vec3", default = [0.0, 0.0, 1.0], description = "The axis a revolute joint turns about or a prismatic one slides along" }"#),
            (k::LIMITS, r#"{ type = "vec2", default = [0.0, 0.0], description = "How far the joint may travel, as a low and a high; equal values mean no limit" }"#),
            (k::LOCKED_AXES, &format!(r#"{{ type = "flags", default = [], options = [{axes}], description = "Which of the six freedoms a generic joint takes away" }}"#)),
        ]),
        shared,
    ]
    .join("\n");
    reg.register_component(
        c::JOINT_3D,
        ComponentDef {
            doc: "Holds this node's body to another one: a hinge, a slider, a rope, a spring, a ball socket, or a generic joint you lock axis by axis. Both ends need a `body3d`; a node without one stands for the nearest body above it, which is how one body carries several joints on child nodes.",
            schema: ComponentDef::parse_schema(c::JOINT_3D, &schema),
            tags: &[balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
            expects: &[c::BODY_3D],
            apply: Box::new(apply_joint),
            remove: Box::new(|eng, entity| {
                remove_joint(eng, entity);
                Ok(())
            }),
            get: Box::new(get_joint_params),
        },
    );
}
