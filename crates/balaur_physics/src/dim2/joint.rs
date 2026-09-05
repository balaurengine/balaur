//! `joint2d`: the 2D half of `crate::joint`.
//!
//! Six kinds rather than seven — 2D has no spherical joint, because a ball
//! socket in a plane is a hinge — and one axis of rotation, so `axis` names
//! only the direction a prismatic joint slides along.

use crate::rapier2d::prelude::{
    FixedJointBuilder, GenericJoint, GenericJointBuilder, ImpulseJointHandle, JointAxesMask,
    JointAxis, MotorModel, MultibodyJointHandle, PinSlotJointBuilder, PrismaticJointBuilder,
    RevoluteJointBuilder, RigidBodyHandle, RopeJointBuilder, SpringJointBuilder,
};
use crate::scalar::{self, Real, Vector2};
use anyhow::{Result, anyhow};
use balaur_core::components::{ComponentDef, as_node};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::PhysicsState2d;
use crate::joint::impulse_magnitude_2d;
use crate::rapier2d::pipeline::PhysicsWorld as PhysicsWorld2;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum JointHandle2d {
    Impulse(ImpulseJointHandle),
    Multibody(MultibodyJointHandle),
}

/// A node's 2D joint and the pull that snaps it, as in 3D.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct JointRef2d {
    pub handle: JointHandle2d,
    pub break_force: Real,
}

fn free_axes(kind: &str) -> &'static [JointAxis] {
    match kind {
        w::REVOLUTE => &[JointAxis::AngX],
        w::PRISMATIC | w::ROPE | w::SPRING | w::PIN_SLOT => &[JointAxis::LinX],
        _ => &[],
    }
}

fn locked_axes(params: &toml::Value) -> JointAxesMask {
    let mut mask = JointAxesMask::empty();
    for (name, axis) in [
        (w::X, JointAxesMask::LIN_X),
        (w::Y, JointAxesMask::LIN_Y),
        (w::ANG_X, JointAxesMask::ANG_X),
    ] {
        if v::flag(params, k::LOCKED_AXES, name) {
            mask |= axis;
        }
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
    let anchor2 = scalar::v2a(v::vec2(params, k::OTHER_ANCHOR, [0.0; 2]));
    let length = scalar::real(v::f(params, k::LENGTH, 0.0));
    let stiffness = scalar::real(v::f(params, k::STIFFNESS, 0.0));
    let damping = scalar::real(v::f(params, k::DAMPING, 1.0));
    let mut joint: GenericJoint = match kind {
        w::FIXED => FixedJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::REVOLUTE => RevoluteJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::PRISMATIC => PrismaticJointBuilder::new(axis)
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
        w::PIN_SLOT => PinSlotJointBuilder::new(axis)
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
    joint.set_contacts_enabled(v::boolean(params, k::CONTACTS, false));
    let limits = scalar::v2a(v::vec2(params, k::LIMITS, [0.0; 2]));
    let motor = v::text(params, k::MOTOR, w::OFF);
    let target = scalar::real(v::f(params, k::MOTOR_TARGET, 0.0));
    let max_force = scalar::real(v::f(params, k::MOTOR_MAX_FORCE, 0.0));
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
    Ok(joint)
}

crate::shared::joint::functions!(
    state = PhysicsState2d,
    world = PhysicsWorld2,
    reference = JointRef2d,
    handle = JointHandle2d,
    component = c::JOINT_2D,
    body = c::BODY_2D,
    impulse_magnitude = impulse_magnitude_2d
);

pub(crate) fn apply_joint(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
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
    if !v::boolean(params, k::ENABLED, true) {
        return Ok(());
    }
    let Some(other) = as_node(eng, entity, params.get(k::BODY)) else {
        return Ok(());
    };
    let joint = joint_of(params)?;
    let reduced = v::text(params, k::SOLVER, w::IMPULSE) == w::REDUCED;
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let (first, second) = handles(&state, entity, other)?;
    let handle = if reduced {
        state
            .world
            .insert_multibody_joint(first, second, joint)
            .map(JointHandle2d::Multibody)
            .ok_or_else(|| {
                anyhow!("a reduced-coordinates joint cannot close a loop; use solver = \"impulse\"")
            })?
    } else {
        JointHandle2d::Impulse(state.world.insert_impulse_joint(first, second, joint))
    };
    state.joints.insert(
        entity,
        JointRef2d {
            handle,
            break_force: scalar::real(v::f(params, k::BREAK_FORCE, 0.0)),
        },
    );
    Ok(())
}

pub(crate) fn get_joint_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState2d>();
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
        JointHandle2d::Impulse(handle) => state.world.impulse_joints.get(*handle)?.data,
        JointHandle2d::Multibody(handle) => {
            let (multibody, link) = state.world.multibody_joints.get(*handle)?;
            multibody.link(link)?.joint.data
        }
    };
    let f = |value: Real| toml::Value::Float(f64::from(value));
    let vec2 = |v: Vector2| toml::Value::Array(vec![f(v.x), f(v.y)]);
    let mut map = authored;
    map.insert(k::ANCHOR.into(), vec2(data.local_anchor1()));
    map.insert(k::OTHER_ANCHOR.into(), vec2(data.local_anchor2()));
    map.insert(k::CONTACTS.into(), data.contacts_enabled().into());
    map.insert(
        k::SOLVER.into(),
        toml::Value::String(
            match reference.handle {
                JointHandle2d::Impulse(_) => w::IMPULSE,
                JointHandle2d::Multibody(_) => w::REDUCED,
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

pub(crate) fn install_joint2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("add_joint", &[c::JOINT_2D], "", "Tie this node's body to another with a 2D joint, from a `joint2d` table."),
        ("remove_joint", &[c::JOINT_2D], "", "Undo the node's joint, leaving both bodies free."),
        ("set_motor_velocity", &[c::JOINT_2D], "", "Drive the joint towards a speed: how a wheel is powered."),
        ("set_motor_position", &[c::JOINT_2D], "", "Drive the joint towards an angle or a distance, with a spring's stiffness and damping."),
        ("set_joint_limits", &[c::JOINT_2D], "", "Set how far the joint may travel."),
        ("joint_impulse", &[c::JOINT_2D], "", "How hard the joint is pulling right now."),
    ]);
    m.function(
        "add_joint",
        |eng: &Engine, (node, params): (NodeId, balaur_script::Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            let entity = entity_of(node)?;
            let registry = eng.resource::<balaur_core::components::ComponentRegistry>();
            let schema = {
                let registry = registry.borrow();
                registry
                    .def(c::JOINT_2D)
                    .ok_or_else(|| anyhow!("joint2d is not registered"))?
                    .schema
                    .clone()
            };
            let full = balaur_core::components::properties(eng, &schema, Some(&params))?;
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
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        let Some(JointHandle2d::Impulse(handle)) = state.joints.get(&entity).map(|j| j.handle)
        else {
            return Ok(0.0);
        };
        Ok(state.world.impulse_joints.get(handle).map_or(0.0, |joint| {
            crate::joint::impulse_magnitude_2d(&joint.impulses)
        }))
    });
}

pub(crate) fn register_joint2d_component(reg: &mut Registry<'_>) {
    let kinds = v::options(w::JOINT_KINDS_2D);
    let axes = v::options(w::JOINT_AXES_2D);
    let default = w::FIXED;
    let shared = crate::joint::shared_joint_schema();
    let schema = [
        v::schema(&[
            (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], shorthand = true, description = "How the two bodies may move relative to each other" }}"#, default, kinds)),
            (k::BODY, r#"{ type = "node", default = "", description = "The node at the joint's other end; this node is the first end" }"#),
            (k::ANCHOR, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where the joint attaches on this node, in its own space" }"#),
            (k::OTHER_ANCHOR, r#"{ type = "vec2", default = [0.0, 0.0], description = "Where it attaches on the other node, in that node's space" }"#),
            (k::AXIS, r#"{ type = "vec2", default = [1.0, 0.0], description = "The direction a prismatic joint slides along" }"#),
            (k::LIMITS, r#"{ type = "vec2", default = [0.0, 0.0], description = "How far the joint may travel, as a low and a high; equal values mean no limit" }"#),
            (k::LOCKED_AXES, &format!(r#"{{ type = "flags", default = [], options = [{}], description = "Which of the three freedoms a generic joint takes away" }}"#, axes)),
        ]),
        shared,
    ]
    .join("\n");
    reg.register_component(
        c::JOINT_2D,
        ComponentDef {
            doc: "Holds this node's body to another one in 2D: a hinge, a slider, a rope, a spring, or a generic joint you lock axis by axis. Both ends need a `body2d`.",
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
