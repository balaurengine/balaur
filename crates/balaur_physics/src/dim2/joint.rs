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
use anyhow::{anyhow, Result};
use balaur_core::components::{as_node, ComponentDef};
use balaur_core::hecs::Entity;
use balaur_core::{entity_of, App, Engine};
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::joint::SHARED_JOINT_SCHEMA;
use crate::vocabulary as v;
use crate::PhysicsState2d;

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
        "revolute" => &[JointAxis::AngX],
        "prismatic" | "rope" | "spring" | "pin_slot" => &[JointAxis::LinX],
        _ => &[],
    }
}

fn locked_axes(params: &toml::Value) -> JointAxesMask {
    let mut mask = JointAxesMask::empty();
    for (name, axis) in [
        ("x", JointAxesMask::LIN_X),
        ("y", JointAxesMask::LIN_Y),
        ("ang_x", JointAxesMask::ANG_X),
    ] {
        if v::flag(params, "locked_axes", name) {
            mask |= axis;
        }
    }
    mask
}

pub(crate) fn joint_of(params: &toml::Value) -> Result<GenericJoint> {
    let kind = v::text(params, "kind", "fixed");
    let axis = scalar::v2a(v::vec2(params, "axis", [1.0, 0.0]));
    let axis = if axis.length_squared() < 1.0e-12 {
        Vector2::X
    } else {
        axis.normalize()
    };
    let anchor1 = scalar::v2a(v::vec2(params, "anchor", [0.0; 2]));
    let anchor2 = scalar::v2a(v::vec2(params, "other_anchor", [0.0; 2]));
    let length = scalar::real(v::f(params, "length", 0.0));
    let stiffness = scalar::real(v::f(params, "stiffness", 0.0));
    let damping = scalar::real(v::f(params, "damping", 1.0));
    let mut joint: GenericJoint = match kind {
        "fixed" => FixedJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "revolute" => RevoluteJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "prismatic" => PrismaticJointBuilder::new(axis)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "rope" => RopeJointBuilder::new(length.max(0.0))
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "spring" => SpringJointBuilder::new(length.max(0.0), stiffness, damping)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "pin_slot" => PinSlotJointBuilder::new(axis)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "generic" => GenericJointBuilder::new(locked_axes(params))
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .local_axis1(axis)
            .local_axis2(axis)
            .build(),
        other => return Err(anyhow!("unknown joint2d kind '{other}'")),
    };
    joint.set_contacts_enabled(v::boolean(params, "contacts", false));
    let limits = scalar::v2a(v::vec2(params, "limits", [0.0; 2]));
    let motor = v::text(params, "motor", "off");
    let target = scalar::real(v::f(params, "motor_target", 0.0));
    let max_force = scalar::real(v::f(params, "motor_max_force", 0.0));
    let model = match v::text(params, "motor_model", "acceleration") {
        "force" => MotorModel::ForceBased,
        _ => MotorModel::AccelerationBased,
    };
    for axis in free_axes(kind).iter().copied() {
        if limits.x < limits.y {
            joint.set_limits(axis, [limits.x, limits.y]);
        }
        match motor {
            "velocity" => {
                joint.set_motor_model(axis, model);
                joint.set_motor_velocity(axis, target, damping);
            }
            "position" => {
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

fn handles(
    state: &PhysicsState2d,
    a: Entity,
    b: Entity,
) -> Result<(RigidBodyHandle, RigidBodyHandle)> {
    let first = *state
        .bodies
        .get(&a)
        .ok_or_else(|| anyhow!("the node holding the joint has no body2d"))?;
    let second = *state
        .bodies
        .get(&b)
        .ok_or_else(|| anyhow!("the node at the joint's other end has no body2d"))?;
    Ok((first, second))
}

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
    if !v::boolean(params, "enabled", true) {
        return Ok(());
    }
    let Some(other) = as_node(eng, entity, params.get("body")) else {
        return Ok(());
    };
    let joint = joint_of(params)?;
    let reduced = v::text(params, "solver", "impulse") == "reduced";
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
            break_force: scalar::real(v::f(params, "break_force", 0.0)),
        },
    );
    Ok(())
}

pub(crate) fn remove_joint(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    state.joint_params.swap_remove(&entity);
    match state.joints.swap_remove(&entity).map(|j| j.handle) {
        Some(JointHandle2d::Impulse(handle)) => {
            state.world.remove_impulse_joint(handle);
        }
        Some(JointHandle2d::Multibody(handle)) => state.world.remove_multibody_joint(handle),
        None => {}
    }
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
    map.insert("anchor".into(), vec2(data.local_anchor1()));
    map.insert("other_anchor".into(), vec2(data.local_anchor2()));
    map.insert("contacts".into(), data.contacts_enabled().into());
    map.insert(
        "solver".into(),
        toml::Value::String(
            match reference.handle {
                JointHandle2d::Impulse(_) => "impulse",
                JointHandle2d::Multibody(_) => "reduced",
            }
            .into(),
        ),
    );
    map.insert(
        "break_force".into(),
        toml::Value::Float(f64::from(reference.break_force)),
    );
    Some(toml::Value::Table(map))
}

/// Joints authored but not yet made, because the node at the other end had not
/// been spawned when this one was.
pub(crate) fn pending(state: &PhysicsState2d) -> Vec<Entity> {
    let mut out: Vec<Entity> = state
        .joint_params
        .keys()
        .filter(|entity| !state.joints.contains_key(*entity))
        .copied()
        .collect();
    out.sort_unstable_by_key(|e| e.to_bits());
    out
}

/// Joints whose pull passed their `break_force` this step.
pub(crate) fn broken(state: &PhysicsState2d) -> Vec<Entity> {
    let mut out = Vec::new();
    for (entity, reference) in &state.joints {
        if reference.break_force <= 0.0 {
            continue;
        }
        let JointHandle2d::Impulse(handle) = reference.handle else {
            continue;
        };
        let Some(joint) = state.world.impulse_joints.get(handle) else {
            continue;
        };
        if crate::joint::impulse_magnitude_2d(&joint.impulses) > reference.break_force {
            out.push(*entity);
        }
    }
    out.sort_unstable_by_key(|e| e.to_bits());
    out
}

pub(crate) fn install_joint2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("add_joint", &["joint2d"], "", "Tie this node's body to another with a 2D joint, from a `joint2d` table."),
        ("remove_joint", &["joint2d"], "", "Undo the node's joint, leaving both bodies free."),
        ("set_motor_velocity", &["joint2d"], "", "Drive the joint towards a speed: how a wheel is powered."),
        ("set_motor_position", &["joint2d"], "", "Drive the joint towards an angle or a distance, with a spring's stiffness and damping."),
        ("set_joint_limits", &["joint2d"], "", "Set how far the joint may travel."),
        ("joint_impulse", &["joint2d"], "", "How hard the joint is pulling right now."),
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
                    .def("joint2d")
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

fn with_joint(eng: &Engine, node: NodeId, f: impl FnOnce(&mut GenericJoint, &str)) -> Result<()> {
    let entity = entity_of(node)?;
    let kind = balaur_core::components::get(eng, entity, "joint2d")
        .and_then(|params| {
            params
                .get("kind")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "revolute".to_string());
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    match state.joints.get(&entity).map(|j| j.handle) {
        Some(JointHandle2d::Impulse(handle)) => {
            let joint = state
                .world
                .impulse_joints
                .get_mut(handle, true)
                .ok_or_else(|| anyhow!("this node's joint is gone"))?;
            f(&mut joint.data, &kind);
            Ok(())
        }
        Some(JointHandle2d::Multibody(handle)) => {
            let (multibody, link) = state
                .world
                .multibody_joints
                .get_mut(handle)
                .ok_or_else(|| anyhow!("this node's joint is gone"))?;
            let link = multibody
                .link_mut(link)
                .ok_or_else(|| anyhow!("this node's joint is gone"))?;
            f(&mut link.joint.data, &kind);
            Ok(())
        }
        None => Err(anyhow!("node has no joint2d")),
    }
}

pub(crate) fn register_joint2d_component(app: &mut App) {
    let schema = format!(
        r#"kind = {{ type = "enum", default = "fixed", options = ["fixed", "revolute", "prismatic", "rope", "spring", "pin_slot", "generic"], shorthand = true, description = "How the two bodies may move relative to each other" }}
body = {{ type = "node", default = "", description = "The node at the joint's other end; this node is the first end" }}
anchor = {{ type = "vec2", default = [0.0, 0.0], description = "Where the joint attaches on this node, in its own space" }}
other_anchor = {{ type = "vec2", default = [0.0, 0.0], description = "Where it attaches on the other node, in that node's space" }}
axis = {{ type = "vec2", default = [1.0, 0.0], description = "The direction a prismatic joint slides along" }}
limits = {{ type = "vec2", default = [0.0, 0.0], description = "How far the joint may travel, as a low and a high; equal values mean no limit" }}
locked_axes = {{ type = "flags", default = [], options = ["x", "y", "ang_x"], description = "Which of the three freedoms a generic joint takes away" }}
{SHARED_JOINT_SCHEMA}"#
    );
    app.register_component(
        "joint2d",
        ComponentDef {
            doc: "Holds this node's body to another one in 2D: a hinge, a slider, a rope, a spring, or a generic joint you lock axis by axis. Both ends need a `body2d`.",
            schema: ComponentDef::parse_schema("joint2d", &schema),
            tags: &["2d", "physics"],
            expects: &["body2d"],
            apply: Box::new(apply_joint),
            remove: Box::new(|eng, entity| {
                remove_joint(eng, entity);
                Ok(())
            }),
            get: Box::new(get_joint_params),
        },
    );
}
