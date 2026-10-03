//! `joint3d`: two bodies held together, and everything rapier lets you say
//! about how.
//!
//! The joint lives on a node so it can be selected, gizmo-drawn and deleted
//! like anything else. The node it sits on is one end; the `connected_body` property
//! names the other.
//!
//! Two solvers, because rapier has two. `impulse` is the general one: any
//! graph, loops included. `reduced` is a multibody joint in reduced
//! coordinates — no drift and no wasted solver work, at the price of no loops
//! — and it is what an articulated arm with inverse kinematics wants.

use crate::rapier3d::dynamics::InverseKinematicsOption;
use crate::rapier3d::dynamics::{JointEnabled, Multibody, MultibodyDofCoupling, RevoluteJoint};
use crate::rapier3d::prelude::{
    FixedJointBuilder, GenericJoint, GenericJointBuilder, ImpulseJoint, ImpulseJointHandle,
    JointAxesMask, JointAxis, MotorModel, MultibodyJointHandle, PrismaticJointBuilder,
    RevoluteJointBuilder, RigidBodyHandle, RopeJointBuilder, SphericalJointBuilder,
    SpringCoefficients, SpringJointBuilder,
};
use crate::scalar::{self, Real, Vector};
use anyhow::{Result, anyhow};
use balaur_core::components::{ComponentDef, as_node};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::PhysicsState3d;
use crate::rapier3d::pipeline::PhysicsWorld;
use crate::shared::joint::DEFAULT_DAMPING;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

/// Which handle a node's joint has, because rapier keeps the two solvers in
/// two sets and a joint is one or the other for its whole life.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum JointHandle3d {
    Impulse(ImpulseJointHandle),
    Multibody(MultibodyJointHandle),
}

/// A node's joint, and the force and the torque that snap it.
///
/// The thresholds live here rather than being read back from the component
/// each step: the step checks every breakable joint every tick, and a
/// component read is a registry lookup and a `toml::Value` per joint.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct JointRef3d {
    pub handle: JointHandle3d,
    pub break_force: Real,
    pub break_torque: Real,
}

/// Each `axes` word and the degree of freedom it names, in the joint's frame.
const AXIS_WORDS: &[(&str, JointAxis)] = &[
    (w::X, JointAxis::LinX),
    (w::Y, JointAxis::LinY),
    (w::Z, JointAxis::LinZ),
    (w::ROTATION_X, JointAxis::AngX),
    (w::ROTATION_Y, JointAxis::AngY),
    (w::ROTATION_Z, JointAxis::AngZ),
];

/// The degrees of freedom a kind leaves free, which an `axes` record may name.
///
/// A rope and a spring couple the three linear axes into one distance, which
/// `x` names, and leave the turn free.
fn free_axes(kind: &str, params: &toml::Value) -> JointAxesMask {
    match kind {
        w::HINGE => JointAxesMask::FREE_REVOLUTE_AXES,
        w::SLIDER => JointAxesMask::FREE_PRISMATIC_AXES,
        w::BALL_SOCKET => JointAxesMask::FREE_SPHERICAL_AXES,
        w::ROPE | w::SPRING => JointAxesMask::LIN_X | JointAxesMask::ANG_AXES,
        w::GENERIC => (JointAxesMask::LIN_AXES | JointAxesMask::ANG_AXES) - locked_axes(params),
        _ => JointAxesMask::empty(),
    }
}

/// The axes a `generic` joint locks, from its `flags` property.
fn locked_axes(params: &toml::Value) -> JointAxesMask {
    let mut mask = JointAxesMask::empty();
    for (key, name, axis) in [
        (k::LOCK_TRANSLATION, w::X, JointAxesMask::LIN_X),
        (k::LOCK_TRANSLATION, w::Y, JointAxesMask::LIN_Y),
        (k::LOCK_TRANSLATION, w::Z, JointAxesMask::LIN_Z),
        (k::LOCK_ROTATION, w::X, JointAxesMask::ANG_X),
        (k::LOCK_ROTATION, w::Y, JointAxesMask::ANG_Y),
        (k::LOCK_ROTATION, w::Z, JointAxesMask::ANG_Z),
    ] {
        if v::flag(params, key, name) {
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
    let anchor2 = scalar::v3a(v::vec3(params, k::CONNECTED_ANCHOR, [0.0; 3]));
    let max_length = scalar::real(v::f(params, k::MAX_LENGTH, 0.0));
    let rest_length = scalar::real(v::f(params, k::REST_LENGTH, 0.0));
    let mut joint: GenericJoint = match kind {
        w::FIXED => FixedJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::HINGE => RevoluteJointBuilder::new(axis)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::SLIDER => PrismaticJointBuilder::new(axis)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::BALL_SOCKET => SphericalJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::ROPE => RopeJointBuilder::new(max_length.max(0.0))
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        // The pull itself is the `x` record's; without one the spring is slack.
        w::SPRING => {
            SpringJointBuilder::new(rest_length.max(0.0), 0.0, scalar::real(DEFAULT_DAMPING))
                .local_anchor1(anchor1)
                .local_anchor2(anchor2)
                .build()
                .into()
        }
        w::GENERIC => GenericJointBuilder::new(locked_axes(params))
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .local_axis1(axis)
            .local_axis2(axis)
            .build(),
        other => return Err(anyhow!("unknown joint kind '{other}'")),
    };
    // The other end's own axis when it names one; `axis` reaches both ends otherwise.
    let connected = scalar::v3a(v::vec3(params, k::CONNECTED_AXIS, [0.0; 3]));
    if matches!(kind, w::HINGE | w::SLIDER | w::GENERIC) && connected.length_squared() >= 1.0e-12 {
        joint.set_local_axis2(connected.normalize());
    }
    let turn = |key: &str| {
        let [x, y, z] = v::vec3(params, key, [0.0; 3]);
        scalar::rotation_of(glamx::Quat::from_euler(glamx::EulerRot::XYZ, x, y, z))
    };
    joint.local_frame1.rotation *= turn(k::ANCHOR_ROTATION);
    joint.local_frame2.rotation *= turn(k::CONNECTED_ANCHOR_ROTATION);
    let (hz, ratio) = crate::shared::joint::softness(params);
    joint.softness = SpringCoefficients::new(hz, ratio);
    joint.coupled_axes |= coupled_axes(params);
    joint.set_contacts_enabled(v::boolean(params, k::COLLIDE_CONNECTED, false));
    joint.set_enabled(v::boolean(params, k::ENABLED, true));
    write_axes(&mut joint, params, kind)?;
    Ok(joint)
}

/// The axes `coupled_translation` and `coupled_rotation` tie into one.
fn coupled_axes(params: &toml::Value) -> JointAxesMask {
    let mut mask = JointAxesMask::empty();
    for (key, name, axis) in [
        (k::COUPLED_TRANSLATION, w::X, JointAxesMask::LIN_X),
        (k::COUPLED_TRANSLATION, w::Y, JointAxesMask::LIN_Y),
        (k::COUPLED_TRANSLATION, w::Z, JointAxesMask::LIN_Z),
        (k::COUPLED_ROTATION, w::X, JointAxesMask::ANG_X),
        (k::COUPLED_ROTATION, w::Y, JointAxesMask::ANG_Y),
        (k::COUPLED_ROTATION, w::Z, JointAxesMask::ANG_Z),
    ] {
        if v::flag(params, key, name) {
            mask |= axis;
        }
    }
    mask
}

/// The two bodies a joint ties: the node it sits on, and the one `connected_body` names.
///
/// Either end may be a bodiless child, which stands for the nearest body
/// above it — as a collider on a child does. A joint is one per node, so
/// this is how one body carries several.
fn ends(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<(Entity, Entity)> {
    let other = as_node(eng, entity, params.get(k::CONNECTED_BODY)).ok_or_else(|| {
        anyhow!("a joint needs a `connected_body` naming the node at its other end")
    })?;
    Ok((body_above(eng, entity), body_above(eng, other)))
}

/// The node itself when it has a body, else the nearest ancestor that does;
/// the node again when none does, so `handles` reports the missing body.
fn body_above(eng: &Engine, entity: Entity) -> Entity {
    crate::collider::nearest_body(eng, entity).map_or(entity, |(node, _)| node)
}

crate::shared::joint::functions!(
    state = PhysicsState3d,
    world = PhysicsWorld,
    reference = JointRef3d,
    handle = JointHandle3d,
    component = c::JOINT_3D,
    body = c::BODY_3D,
    impulse_parts = impulse_parts
);

pub(crate) fn apply_joint(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    if toggled_in_place(eng, entity, params) {
        return Ok(());
    }
    remove_joint(eng, entity);
    {
        let state = eng.resource::<PhysicsState3d>();
        state
            .borrow_mut()
            .joint_params
            .insert(entity, params.clone());
    }
    let reduced = v::boolean(params, k::ARTICULATION, false);
    // rapier's chains ignore a joint's switch, so an articulation that is off
    // stays out of its chain.
    if reduced && !v::boolean(params, k::ENABLED, true) {
        return Ok(());
    }
    let (break_force, break_torque) = crate::shared::joint::thresholds(params, reduced)?;
    // A joint whose other end is not in the scene yet is inert rather than an
    // error: a scene file names nodes in whatever order it likes.
    let Ok((a, b)) = ends(eng, entity, params) else {
        return Ok(());
    };
    let joint = joint_of(params)?;
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let (first, second) = handles(&state, a, b)?;
    let handle = if reduced {
        // The connected body is the parent link, so a chain's root is the end
        // its joints point towards; rapier wants the frames the other way round.
        let mut joint = joint;
        joint.flip();
        let set = &mut state.world.multibody_joints;
        let made = if v::boolean(params, k::KINEMATIC_LINK, false) {
            set.insert_kinematic(second, first, joint, true)
        } else {
            set.insert(second, first, joint, true)
        };
        made.map(JointHandle3d::Multibody).ok_or_else(|| {
            anyhow!("an articulation cannot close a loop; set articulation = false")
        })?
    } else {
        JointHandle3d::Impulse(state.world.insert_impulse_joint(first, second, joint))
    };
    state.joints.insert(
        entity,
        JointRef3d {
            handle,
            break_force,
            break_torque,
        },
    );
    drop(state);
    if reduced {
        rewrite_articulations(eng);
    }
    Ok(())
}

/// What `apply` wrote, read back off the joint.
pub(crate) fn get_joint_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    // Authored values first, so a joint waiting for its other end still
    // reports what it is waiting to be. A script's limit or motor call
    // rewrites them, so they hold what the joint runs.
    let params = state.joint_params.get(&entity)?;
    let mut map = params.as_table().cloned()?;
    let spring = v::text(params, k::KIND, w::FIXED) == w::SPRING;
    map.insert(
        k::AXES.into(),
        crate::shared::joint::resolved_axes(params, |axis| spring && axis == w::X),
    );
    let Some(reference) = state.joints.get(&entity) else {
        return Some(toml::Value::Table(map));
    };
    let data = match &reference.handle {
        JointHandle3d::Impulse(handle) => state.world.impulse_joints.get(*handle)?.data,
        JointHandle3d::Multibody(handle) => {
            let (multibody, link) = state.world.multibody_joints.get(*handle)?;
            read_chain(&mut map, multibody, link);
            let mut data = multibody.link(link)?.joint.data;
            data.flip();
            data
        }
    };
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let vec3 = |v: Vector| toml::Value::Array(vec![f(v.x), f(v.y), f(v.z)]);
    map.insert(k::SOFTNESS_HZ.into(), f(data.softness.natural_frequency));
    map.insert(
        k::SOFTNESS_DAMPING_RATIO.into(),
        f(data.softness.damping_ratio),
    );
    map.insert(k::ANCHOR.into(), vec3(data.local_anchor1()));
    map.insert(k::CONNECTED_ANCHOR.into(), vec3(data.local_anchor2()));
    map.insert(k::COLLIDE_CONNECTED.into(), data.contacts_enabled().into());
    map.insert(
        k::ARTICULATION.into(),
        matches!(reference.handle, JointHandle3d::Multibody(_)).into(),
    );
    map.insert(k::BREAK_FORCE.into(), f(reference.break_force));
    map.insert(k::BREAK_TORQUE.into(), f(reference.break_torque));
    Some(toml::Value::Table(map))
}

/// The impulse an impulse joint held through the last substep: its linear
/// part and its torque about the anchor, each as one magnitude.
///
/// Rebuilt from the rows rapier's solver makes (`JointConstraint::update`),
/// in its two groups: the motors, then the locks and the limits.
fn impulse_parts(world: &PhysicsWorld, joint: &ImpulseJoint) -> Option<(Real, Real)> {
    use crate::rapier3d::utils::RotationOps;
    use crate::shared::joint::{Metric, Row};
    let body1 = world.bodies.get(joint.body1())?;
    let body2 = world.bodies.get(joint.body2())?;
    let data = &joint.data;
    let frame1 = *body1.position() * data.local_frame1;
    let frame2 = *body2.position() * data.local_frame2;
    let basis = frame1.rotation.to_mat();
    let lin_err = frame2.translation - frame1.translation;
    let locked = data.locked_axes.bits();
    let (motors, limits) = (
        data.motor_axes.bits() & !locked,
        data.limit_axes.bits() & !locked,
    );
    let coupled = data.coupled_axes.bits();
    let has = |mask: u8, i: usize| mask & (1 << i) != 0;
    // The first body's force lands where its locked axes pin it to the second.
    let mut center1 = frame2.translation;
    for i in (0..3).filter(|i| has(locked, *i)) {
        center1 -= basis.col(i) * lin_err.dot(basis.col(i));
    }
    let r1 = center1 - body1.center_of_mass();
    let r2 = frame2.translation - body2.center_of_mass();
    let sign = if frame1.rotation.dot(frame2.rotation) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let ang_basis = frame1.rotation.diff_conj1_2_tr(&frame2.rotation) * sign;
    let row = |lin: Vector, ang1: Vector, ang2: Vector, impulse: Real, locked: bool| Row {
        lin: lin.to_array(),
        ang1: ang1.to_array(),
        ang2: ang2.to_array(),
        impulse,
        locked,
    };
    let linear = |i: usize, impulse: Real, locked: bool| {
        let axis = basis.col(i);
        row(axis, r1.cross(axis), r2.cross(axis), impulse, locked)
    };
    let turn =
        |axis: Vector, impulse: Real, locked: bool| row(Vector::ZERO, axis, axis, impulse, locked);
    let distance = |impulse: Real| {
        let along: Vector = (0..3)
            .filter(|i| has(coupled, *i))
            .map(|i| basis.col(i) * basis.col(i).dot(lin_err))
            .sum();
        let n = along.normalize_or_zero();
        row(n, r1.cross(n), r2.cross(n), impulse, false)
    };
    let first = (coupled & JointAxesMask::LIN_AXES.bits()).trailing_zeros() as usize;
    let coupled_lin = coupled & JointAxesMask::LIN_AXES.bits() != 0;
    let mut driven = Vec::new();
    for i in (3..6).filter(|i| has(motors & !coupled, *i)) {
        driven.push(turn(basis.col(i - 3), data.motors[i].impulse, false));
    }
    for i in (0..3).filter(|i| has(motors & !coupled, *i)) {
        driven.push(linear(i, data.motors[i].impulse, false));
    }
    if coupled_lin && has(motors, first) {
        driven.push(distance(data.motors[first].impulse));
    }
    let mut held = Vec::new();
    for i in (3..6).filter(|i| has(locked, *i)) {
        held.push(turn(ang_basis.col(i - 3), joint.impulses[i], true));
    }
    for i in (0..3).filter(|i| has(locked, *i)) {
        held.push(linear(i, joint.impulses[i], true));
    }
    for i in (3..6).filter(|i| has(limits & !coupled, *i)) {
        held.push(turn(basis.col(i - 3), data.limits[i].impulse, false));
    }
    for i in (0..3).filter(|i| has(limits & !coupled, *i)) {
        held.push(linear(i, data.limits[i].impulse, false));
    }
    if coupled_lin && has(limits, first) {
        held.push(distance(data.limits[first].impulse));
    }
    let solver = |body: &crate::rapier3d::prelude::RigidBody| {
        let props = body.mass_properties();
        let i = props.effective_world_inv_inertia;
        let inertia = [
            [i.m11, i.m12, i.m13],
            [i.m12, i.m22, i.m23],
            [i.m13, i.m23, i.m33],
        ];
        if body.is_dynamic() {
            (props.effective_inv_mass, inertia)
        } else {
            (Vector::ZERO, [[0.0; 3]; 3])
        }
    };
    let ((mass1, inertia1), (mass2, inertia2)) = (solver(body1), solver(body2));
    let metric = Metric {
        inv_mass: (mass1 + mass2).to_array(),
        inv_inertia1: inertia1,
        inv_inertia2: inertia2,
    };
    let (lin, ang) = crate::shared::joint::applied(&mut [driven, held], &metric);
    let (force, about_com) = (Vector::from_array(lin), Vector::from_array(ang));
    Some((force.length(), (about_com - r1.cross(force)).length()))
}

pub(crate) fn install_joint_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("remove_joint", &[c::JOINT_3D], "", "Undo the node's joint, leaving both bodies free."),
        ("set_motor_velocity", &[c::JOINT_3D], "(node: node, axis: string, velocity: float, damping: float)", "Drive one of the joint's free axes towards a speed: how a wheel is powered or a door swings itself shut. Rewrites that axis's `axes` record."),
        ("set_motor_position", &[c::JOINT_3D], "(node: node, axis: string, target: float, stiffness: float, damping: float)", "Drive one of the joint's free axes towards an angle or a distance, with a spring's stiffness and damping. Rewrites that axis's `axes` record."),
        ("set_joint_limits", &[c::JOINT_3D], "(node: node, axis: string, min: float, max: float)", "Set how far one of the joint's free axes may travel, in radians about a rotation axis and units along a linear one; equal values lift the limit. Rewrites that axis's `axes` record."),
        ("joint_state", &[c::JOINT_3D], "(node: node) -> map", "What the joint is doing: `#{ status, angle, limit_impulses, motor_impulses, coordinates, velocities }`. `status` is `enabled`, `disabled`, `body_disabled` (a body at either end is disabled) or `waiting` (its other end is not in the scene yet); `angle` is a hinge's turn in radians; the impulse maps hold each free axis's last limit and motor impulse; an articulation adds each free axis's coordinate and velocity."),
        ("joint_force", &[c::JOINT_3D], "(node: node) -> map", "The force and the torque the joint held through the last step, `#{ force, torque }`: what `break_force` and `break_torque` are measured against. An articulation has none to read, and says so."),
        ("solve_ik", &[c::JOINT_3D], "(node: node, x: float, y: float, z: float, opts: table)", "Move a reduced-coordinates chain so its last link reaches a world position, leaving every joint inside its limits. Options: `rotation` (euler radians to turn the link to), `constrain` (the world axes to match: `x`, `y`, `z`, `rotation_x`, `rotation_y`, `rotation_z`; the translation by default, every axis when `rotation` is given), `damping` (1.0), `iterations` (10), `tolerance` (0.001)."),
    ]);
    m.function("remove_joint", |eng: &Engine, node: NodeId| {
        remove_joint(eng, entity_of(node)?);
        Ok(())
    });
    m.function(
        "set_motor_velocity",
        |eng: &Engine, (node, axis, velocity, damping): (NodeId, String, f32, f32)| {
            set_axis(eng, node, &axis, |record| {
                crate::shared::joint::velocity_motor(record, velocity, damping);
            })
        },
    );
    m.function(
        "set_motor_position",
        |eng: &Engine,
         (node, axis, target, stiffness, damping): (NodeId, String, f32, f32, f32)| {
            set_axis(eng, node, &axis, |record| {
                crate::shared::joint::position_motor(record, target, stiffness, damping);
            })
        },
    );
    m.function(
        "set_joint_limits",
        |eng: &Engine, (node, axis, min, max): (NodeId, String, f32, f32)| {
            set_axis(eng, node, &axis, |record| {
                crate::shared::joint::limits(record, min, max);
            })
        },
    );
    m.function("joint_force", |eng: &Engine, node: NodeId| {
        joint_force(eng, node)
    });
    m.function("joint_state", |eng: &Engine, node: NodeId| {
        joint_state(eng, node)
    });
    m.function(
        "solve_ik",
        |eng: &Engine, (node, x, y, z, opts): (NodeId, f32, f32, f32, Option<Value>)| {
            solve_ik(
                eng,
                entity_of(node)?,
                scalar::v3(x, y, z),
                &v::Opts(opts.as_ref()),
            )
        },
    );
}

/// Move a reduced-coordinates chain so the node's own body reaches `target`.
///
/// Rapier's own solver, damped least squares, every joint's limits respected.
/// Impulse joints have no such thing — there are no generalised coordinates to
/// solve for — so this is `articulation = true` only, and says so.
fn solve_ik(eng: &Engine, entity: Entity, target: Vector, opts: &v::Opts<'_>) -> Result<()> {
    let rotation = opts.get(k::ROTATION).map(|_| {
        let [x, y, z] = opts.vec3(k::ROTATION, [0.0; 3]);
        scalar::rotation_of(glamx::Quat::from_euler(glamx::EulerRot::ZYX, z, y, x))
    });
    let options = ik_options(opts, rotation.is_some())?;
    let pose = scalar::Pose::from_parts(target, rotation.unwrap_or(scalar::Rotation::IDENTITY));
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let Some(JointHandle3d::Multibody(handle)) = state.joints.get(&entity).map(|j| j.handle) else {
        return Err(anyhow!(
            "inverse kinematics needs a chain of reduced-coordinates joints (articulation = true)"
        ));
    };
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

pub(crate) fn register_joint_component(reg: &mut Registry<'_>) {
    let kinds = v::options(w::JOINT_KINDS);
    let axes = v::options(w::LOCK_AXES);
    let default = w::FIXED;
    let shared = crate::shared::joint::schema(w::JOINT_AXES, w::ROTATION_X);
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{default}", options = [{kinds}], description = "How the two bodies may move relative to each other" }}"#)),
            (k::CONNECTED_BODY, r#"{ type = "node", default = "", description = "The node at the joint's other end; this node is the first end" }"#),
            (k::ANCHOR, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where the joint attaches on this node, in its own space" }"#),
            (k::CONNECTED_ANCHOR, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where it attaches on the other node, in that node's space" }"#),
            (k::AXIS, r#"{ type = "vec3", default = [0.0, 0.0, 1.0], description = "The joint frame's x axis: what a hinge turns about, a slider slides along, and a generic joint's x and rotation_x name" }"#),
            (k::CONNECTED_AXIS, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "The same axis in the other node's space, for a hinge, a slider or a generic joint; zero takes axis" }"#),
            (k::ANCHOR_ROTATION, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], unit = "degrees", description = "Turns this end's joint frame about its own axes, x first: what a hinge's angle and its limits measure from, and the relative turn a fixed joint holds. Euler radians in the file" }"#),
            (k::CONNECTED_ANCHOR_ROTATION, r#"{ type = "vec3", default = [0.0, 0.0, 0.0], unit = "degrees", description = "The same for the other end's frame" }"#),
            (k::LOCK_TRANSLATION, &format!(r#"{{ type = "flags", default = [], options = [{axes}], description = "The axes a generic joint may not slide along" }}"#)),
            (k::LOCK_ROTATION, &format!(r#"{{ type = "flags", default = [], options = [{axes}], description = "The axes a generic joint may not turn about" }}"#)),
            (k::COUPLED_TRANSLATION, &format!(r#"{{ type = "flags", default = [], options = [{axes}], description = "Free linear axes tied into one distance, whose limit and motor come from the first of them; a rope and a spring couple all three already" }}"#)),
            (k::COUPLED_ROTATION, &format!(r#"{{ type = "flags", default = [], options = [{axes}], description = "Free turning axes tied into one angle, whose limit and motor come from the first of them" }}"#)),
        ]),
        shared,
    ]
    .join("\n");
    reg.register_component(
        c::JOINT_3D,
        ComponentDef {
            events: crate::vocabulary::hook::JOINT,
            warnings: None,
            doc: "Joins this node's body to `connected_body`. `kind` is `fixed`, `hinge`, `slider`, `ball_socket`, `rope`, `spring` or `generic`; both ends need a `body3d` on or above the node. `axes` limits and drives each free axis.",
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
