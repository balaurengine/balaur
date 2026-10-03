//! `joint2d`: the 2D half of `crate::joint`.
//!
//! Six kinds rather than seven — 2D has no spherical joint, because a ball
//! socket in a plane is a hinge — and one axis of rotation, so `axis` names
//! only the direction a prismatic joint slides along.

use crate::rapier2d::dynamics::InverseKinematicsOption;
use crate::rapier2d::dynamics::{JointEnabled, Multibody, MultibodyDofCoupling, RevoluteJoint};
use crate::rapier2d::prelude::{
    FixedJointBuilder, GenericJoint, GenericJointBuilder, ImpulseJoint, ImpulseJointHandle,
    JointAxesMask, JointAxis, MotorModel, MultibodyJointHandle, PinSlotJointBuilder,
    PrismaticJointBuilder, RevoluteJointBuilder, RigidBodyHandle, RopeJointBuilder,
    SpringCoefficients, SpringJointBuilder,
};
use crate::scalar::{self, Real, Vector2};
use anyhow::{Result, anyhow};
use balaur_core::components::{ComponentDef, as_node};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::PhysicsState2d;
use crate::rapier2d::pipeline::PhysicsWorld as PhysicsWorld2;
use crate::shared::joint::DEFAULT_DAMPING;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum JointHandle2d {
    Impulse(ImpulseJointHandle),
    Multibody(MultibodyJointHandle),
}

/// A node's 2D joint and the force and torque that snap it, as in 3D.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct JointRef2d {
    pub handle: JointHandle2d,
    pub break_force: Real,
    pub break_torque: Real,
}

/// Each `axes` word and the degree of freedom it names, in the joint's frame.
const AXIS_WORDS: &[(&str, JointAxis)] = &[
    (w::X, JointAxis::LinX),
    (w::Y, JointAxis::LinY),
    (w::ROTATION, JointAxis::AngX),
];

/// The degrees of freedom a kind leaves free, which an `axes` record may name.
/// A rope and a spring couple both linear axes into one distance, named `x`.
fn free_axes(kind: &str, params: &toml::Value) -> JointAxesMask {
    match kind {
        w::HINGE => JointAxesMask::FREE_REVOLUTE_AXES,
        w::SLIDER => JointAxesMask::FREE_PRISMATIC_AXES,
        w::ROPE | w::SPRING | w::GROOVE => JointAxesMask::LIN_X | JointAxesMask::ANG_X,
        w::GENERIC => (JointAxesMask::LIN_AXES | JointAxesMask::ANG_AXES) - locked_axes(params),
        _ => JointAxesMask::empty(),
    }
}

fn locked_axes(params: &toml::Value) -> JointAxesMask {
    let mut mask = JointAxesMask::empty();
    for (name, axis) in [(w::X, JointAxesMask::LIN_X), (w::Y, JointAxesMask::LIN_Y)] {
        if v::flag(params, k::LOCK_TRANSLATION, name) {
            mask |= axis;
        }
    }
    if v::boolean(params, k::LOCK_ROTATION, false) {
        mask |= JointAxesMask::ANG_X;
    }
    mask
}

pub(crate) fn joint_of(params: &toml::Value) -> Result<GenericJoint> {
    let kind = v::text(params, k::KIND, w::FIXED);
    let axis = scalar::v2a(v::vec2(params, k::AXIS, [1.0, 0.0]));
    let axis = if axis.length_squared() < 1.0e-12 {
        Vector2::X
    } else {
        axis.normalize()
    };
    let anchor1 = scalar::v2a(v::vec2(params, k::ANCHOR, [0.0; 2]));
    let anchor2 = scalar::v2a(v::vec2(params, k::CONNECTED_ANCHOR, [0.0; 2]));
    let max_length = scalar::real(v::f(params, k::MAX_LENGTH, 0.0));
    let rest_length = scalar::real(v::f(params, k::REST_LENGTH, 0.0));
    let mut joint: GenericJoint = match kind {
        w::FIXED => FixedJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::HINGE => RevoluteJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::SLIDER => PrismaticJointBuilder::new(axis)
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
        w::GROOVE => PinSlotJointBuilder::new(axis)
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
        other => return Err(anyhow!("unknown joint2d kind '{other}'")),
    };
    // The other end's own axis when it names one; `axis` reaches both ends otherwise.
    let connected = scalar::v2a(v::vec2(params, k::CONNECTED_AXIS, [0.0; 2]));
    if matches!(kind, w::SLIDER | w::GROOVE | w::GENERIC) && connected.length_squared() >= 1.0e-12 {
        joint.set_local_axis2(connected.normalize());
    }
    let turn = |key: &str| scalar::Rotation2::from_angle(scalar::real(v::f(params, key, 0.0)));
    joint.local_frame1.rotation *= turn(k::ANCHOR_ROTATION);
    joint.local_frame2.rotation *= turn(k::CONNECTED_ANCHOR_ROTATION);
    let (hz, ratio) = crate::shared::joint::softness(params);
    joint.softness = SpringCoefficients::new(hz, ratio);
    for (name, axis) in [(w::X, JointAxesMask::LIN_X), (w::Y, JointAxesMask::LIN_Y)] {
        if v::flag(params, k::COUPLED_TRANSLATION, name) {
            joint.coupled_axes |= axis;
        }
    }
    joint.set_contacts_enabled(v::boolean(params, k::COLLIDE_CONNECTED, false));
    joint.set_enabled(v::boolean(params, k::ENABLED, true));
    write_axes(&mut joint, params, kind)?;
    Ok(joint)
}

crate::shared::joint::functions!(
    state = PhysicsState2d,
    world = PhysicsWorld2,
    reference = JointRef2d,
    handle = JointHandle2d,
    component = c::JOINT_2D,
    body = c::BODY_2D,
    impulse_parts = impulse_parts
);

/// The impulse a 2D impulse joint held through the last substep, rebuilt
/// from its solver rows as `crate::joint` does in 3D; an angular row's
/// Jacobian is 1 here.
fn impulse_parts(world: &PhysicsWorld2, joint: &ImpulseJoint) -> Option<(Real, Real)> {
    use crate::rapier2d::utils::RotationOps;
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
    let mut center1 = frame2.translation;
    for i in (0..2).filter(|i| has(locked, *i)) {
        center1 -= basis.col(i) * lin_err.dot(basis.col(i));
    }
    let r1 = center1 - body1.center_of_mass();
    let r2 = frame2.translation - body2.center_of_mass();
    let row = |lin: Vector2, ang1: Real, ang2: Real, impulse: Real, locked: bool| Row {
        lin: lin.to_array(),
        ang1: [ang1],
        ang2: [ang2],
        impulse,
        locked,
    };
    let linear = |i: usize, impulse: Real, locked: bool| {
        let axis = basis.col(i);
        row(axis, r1.perp_dot(axis), r2.perp_dot(axis), impulse, locked)
    };
    let turn = |impulse: Real, locked: bool| row(Vector2::ZERO, 1.0, 1.0, impulse, locked);
    let distance = |impulse: Real| {
        let along: Vector2 = (0..2)
            .filter(|i| has(coupled, *i))
            .map(|i| basis.col(i) * basis.col(i).dot(lin_err))
            .sum();
        let n = along.normalize_or_zero();
        row(n, r1.perp_dot(n), r2.perp_dot(n), impulse, false)
    };
    let first = (coupled & JointAxesMask::LIN_AXES.bits()).trailing_zeros() as usize;
    let coupled_lin = coupled & JointAxesMask::LIN_AXES.bits() != 0;
    let mut driven = Vec::new();
    if has(motors & !coupled, 2) {
        driven.push(turn(data.motors[2].impulse, false));
    }
    for i in (0..2).filter(|i| has(motors & !coupled, *i)) {
        driven.push(linear(i, data.motors[i].impulse, false));
    }
    if coupled_lin && has(motors, first) {
        driven.push(distance(data.motors[first].impulse));
    }
    let mut held = Vec::new();
    if has(locked, 2) {
        held.push(turn(joint.impulses.z, true));
    }
    for i in (0..2).filter(|i| has(locked, *i)) {
        held.push(linear(i, joint.impulses[i], true));
    }
    if has(limits & !coupled, 2) {
        held.push(turn(data.limits[2].impulse, false));
    }
    for i in (0..2).filter(|i| has(limits & !coupled, *i)) {
        held.push(linear(i, data.limits[i].impulse, false));
    }
    if coupled_lin && has(limits, first) {
        held.push(distance(data.limits[first].impulse));
    }
    let solver = |body: &crate::rapier2d::prelude::RigidBody| {
        let props = body.mass_properties();
        if body.is_dynamic() {
            (props.effective_inv_mass, props.effective_world_inv_inertia)
        } else {
            (Vector2::ZERO, 0.0)
        }
    };
    let ((mass1, inertia1), (mass2, inertia2)) = (solver(body1), solver(body2));
    let metric = Metric {
        inv_mass: (mass1 + mass2).to_array(),
        inv_inertia1: [[inertia1]],
        inv_inertia2: [[inertia2]],
    };
    let (lin, ang) = crate::shared::joint::applied(&mut [driven, held], &metric);
    let force = Vector2::from_array(lin);
    Some((force.length(), (ang[0] - r1.perp_dot(force)).abs()))
}

pub(crate) fn apply_joint(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    if toggled_in_place(eng, entity, params) {
        return Ok(());
    }
    remove_joint(eng, entity);
    {
        // After the removal, which clears it: what the joint was authored
        // from is what a `get` reports and what the retry re-reads.
        let state = eng.resource::<PhysicsState2d>();
        state
            .borrow_mut()
            .joint_params
            .insert(entity, params.clone());
    }
    let reduced = v::boolean(params, k::ARTICULATION, false);
    // rapier's chains ignore a joint's switch, as in 3D.
    if reduced && !v::boolean(params, k::ENABLED, true) {
        return Ok(());
    }
    let (break_force, break_torque) = crate::shared::joint::thresholds(params, reduced)?;
    let Some(other) = as_node(eng, entity, params.get(k::CONNECTED_BODY)) else {
        return Ok(());
    };
    // A bodiless child stands for the nearest body above it, as in 3D.
    let (a, b) = (body_above(eng, entity), body_above(eng, other));
    let joint = joint_of(params)?;
    let state = eng.resource::<PhysicsState2d>();
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
        made.map(JointHandle2d::Multibody).ok_or_else(|| {
            anyhow!("an articulation cannot close a loop; set articulation = false")
        })?
    } else {
        JointHandle2d::Impulse(state.world.insert_impulse_joint(first, second, joint))
    };
    state.joints.insert(
        entity,
        JointRef2d {
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

fn body_above(eng: &Engine, entity: Entity) -> Entity {
    super::collider::nearest_body(eng, entity).map_or(entity, |(node, _)| node)
}

pub(crate) fn get_joint_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    // Authored values first, so a joint waiting for its other end still
    // reports what it is waiting to be; a script's limit or motor call
    // rewrites them.
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
        JointHandle2d::Impulse(handle) => state.world.impulse_joints.get(*handle)?.data,
        JointHandle2d::Multibody(handle) => {
            let (multibody, link) = state.world.multibody_joints.get(*handle)?;
            read_chain(&mut map, multibody, link);
            let mut data = multibody.link(link)?.joint.data;
            data.flip();
            data
        }
    };
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let vec2 = |v: Vector2| toml::Value::Array(vec![f(v.x), f(v.y)]);
    map.insert(k::SOFTNESS_HZ.into(), f(data.softness.natural_frequency));
    map.insert(
        k::SOFTNESS_DAMPING_RATIO.into(),
        f(data.softness.damping_ratio),
    );
    map.insert(k::ANCHOR.into(), vec2(data.local_anchor1()));
    map.insert(k::CONNECTED_ANCHOR.into(), vec2(data.local_anchor2()));
    map.insert(k::COLLIDE_CONNECTED.into(), data.contacts_enabled().into());
    map.insert(
        k::ARTICULATION.into(),
        matches!(reference.handle, JointHandle2d::Multibody(_)).into(),
    );
    map.insert(k::BREAK_FORCE.into(), f(reference.break_force));
    map.insert(k::BREAK_TORQUE.into(), f(reference.break_torque));
    Some(toml::Value::Table(map))
}

pub(crate) fn install_joint2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("remove_joint", &[c::JOINT_2D], "", "Undo the node's joint, leaving both bodies free."),
        ("set_motor_velocity", &[c::JOINT_2D], "(node: node, axis: string, velocity: float, damping: float)", "Drive one of the joint's free axes towards a speed: how a wheel is powered. Rewrites that axis's `axes` record."),
        ("set_motor_position", &[c::JOINT_2D], "(node: node, axis: string, target: float, stiffness: float, damping: float)", "Drive one of the joint's free axes towards an angle or a distance, with a spring's stiffness and damping. Rewrites that axis's `axes` record."),
        ("set_joint_limits", &[c::JOINT_2D], "(node: node, axis: string, min: float, max: float)", "Set how far one of the joint's free axes may travel; equal values lift the limit. Rewrites that axis's `axes` record."),
        ("joint_state", &[c::JOINT_2D], "(node: node) -> map", "What the joint is doing: `#{ status, angle, limit_impulses, motor_impulses, coordinates, velocities }`, as in 3D: `status` is `enabled`, `disabled`, `body_disabled` or `waiting`; `angle` is a hinge's turn in radians; an articulation adds each free axis's coordinate and velocity."),
        ("joint_force", &[c::JOINT_2D], "(node: node) -> map", "The force and the torque the joint held through the last step, `#{ force, torque }`. An articulation has none to read, and says so."),
        ("solve_ik", &[c::JOINT_2D], "(node: node, x: float, y: float, opts: table)", "Move a reduced-coordinates chain so its last link reaches a world position, leaving every joint inside its limits. Options: `rotation` (radians to turn the link to), `constrain` (the world axes to match: `x`, `y`, `rotation`; the translation by default, every axis when `rotation` is given), `damping` (1.0), `iterations` (10), `tolerance` (0.001)."),
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
        |eng: &Engine, (node, x, y, opts): (NodeId, f32, f32, Option<Value>)| {
            solve_ik(
                eng,
                entity_of(node)?,
                scalar::v2(x, y),
                &v::Opts(opts.as_ref()),
            )
        },
    );
}

/// The 2D chain solved for a world position, as `crate::joint`'s is.
fn solve_ik(eng: &Engine, entity: Entity, target: Vector2, opts: &v::Opts<'_>) -> Result<()> {
    let rotation = opts.get(k::ROTATION).map(|_| opts.f32(k::ROTATION, 0.0));
    let options = ik_options(opts, rotation.is_some())?;
    let angle = scalar::Rotation2::from_angle(scalar::real(rotation.unwrap_or(0.0)));
    let pose = scalar::Pose2::from_parts(target, angle);
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let Some(JointHandle2d::Multibody(handle)) = state.joints.get(&entity).map(|j| j.handle) else {
        return Err(anyhow!(
            "inverse kinematics needs a chain of reduced-coordinates joints (articulation = true)"
        ));
    };
    // The solver reads the bodies and writes the chain: one borrow, then the other.
    let displacements = {
        let (multibody, link) = state
            .world
            .multibody_joints
            .get(handle)
            .ok_or_else(|| anyhow!("this node's joint is gone"))?;
        let mut displacements = crate::rapier2d::na::DVector::zeros(multibody.ndofs());
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

pub(crate) fn register_joint2d_component(reg: &mut Registry<'_>) {
    let kinds = v::options(w::JOINT_KINDS_2D);
    let axes = v::options(w::LOCK_AXES_2D);
    let default = w::FIXED;
    let shared = crate::shared::joint::schema(w::JOINT_AXES_2D, w::ROTATION);
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{default}", options = [{kinds}], description = "How the two bodies may move relative to each other" }}"#)),
            (k::CONNECTED_BODY, r#"{ type = "node", default = "", description = "The node at the joint's other end; this node is the first end" }"#),
            (k::ANCHOR, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where the joint attaches on this node, in its own space" }"#),
            (k::CONNECTED_ANCHOR, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where it attaches on the other node, in that node's space" }"#),
            (k::AXIS, r#"{ type = "vec2", default = [1.0, 0.0], description = "The joint frame's x axis: what a slider or a groove slides along, and a generic joint's x names" }"#),
            (k::CONNECTED_AXIS, r#"{ type = "vec2", default = [0.0, 0.0], description = "The same axis in the other node's space, for a slider, a groove or a generic joint; zero takes axis" }"#),
            (k::ANCHOR_ROTATION, r#"{ type = "float", default = 0.0, unit = "degrees", description = "Turns this end's joint frame: what a hinge's angle and its limits measure from, and the relative turn a fixed joint holds. Radians in the file" }"#),
            (k::CONNECTED_ANCHOR_ROTATION, r#"{ type = "float", default = 0.0, unit = "degrees", description = "The same for the other end's frame" }"#),
            (k::LOCK_TRANSLATION, &format!(r#"{{ type = "flags", default = [], options = [{axes}], description = "The axes a generic joint may not slide along" }}"#)),
            (k::LOCK_ROTATION, r#"{ type = "bool", default = false, description = "Stop a generic joint turning" }"#),
            (k::COUPLED_TRANSLATION, &format!(r#"{{ type = "flags", default = [], options = [{axes}], description = "Free linear axes tied into one distance, whose limit and motor come from the first of them; a rope and a spring couple both already" }}"#)),
        ]),
        shared,
    ]
    .join("\n");
    reg.register_component(
        c::JOINT_2D,
        ComponentDef {
            events: crate::vocabulary::hook::JOINT,
            warnings: None,
            doc: "Joins this node's body to `connected_body`. `kind` is `fixed`, `hinge`, `slider`, `rope`, `spring`, `groove` or `generic`; both ends need a `body2d` on or above the node. `axes` limits and drives each free axis.",
            schema: ComponentDef::parse_schema(c::JOINT_2D, &schema),
            tags: &[balaur_core::components::tag::DIM_2D, balaur_core::components::tag::PHYSICS],
            expects: &[c::BODY_2D],
            apply: Box::new(apply_joint),
            remove: Box::new(|eng, entity| {
                remove_joint(eng, entity);
                Ok(())
            }),
            get: Box::new(get_joint_params),
        },
    );
}
