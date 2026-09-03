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

use anyhow::{anyhow, Result};
use balaur_core::components::{as_node, ComponentDef};
use balaur_core::hecs::Entity;
use balaur_core::{entity_of, App, Engine};
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::Vec3;
use rapier3d::prelude::{
    FixedJointBuilder, GenericJoint, GenericJointBuilder, ImpulseJointHandle, JointAxesMask,
    JointAxis, MotorModel, MultibodyJointHandle, PrismaticJointBuilder, RevoluteJointBuilder,
    RigidBodyHandle, RopeJointBuilder, SphericalJointBuilder, SpringJointBuilder,
};

use crate::vocabulary as v;
use crate::PhysicsState;

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
    pub break_force: f32,
}

/// The axis a kind's limits and motor act on.
///
/// A revolute joint turns about one axis and a prismatic slides along one, so
/// "the limits" means that axis's. A spherical joint has three; its motor and
/// limits are applied to all of them, which is what a shoulder wants.
fn free_axes(kind: &str) -> &'static [JointAxis] {
    match kind {
        "revolute" => &[JointAxis::AngX],
        "prismatic" | "rope" | "spring" => &[JointAxis::LinX],
        "spherical" => &[JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ],
        _ => &[],
    }
}

/// The axes a `generic` joint locks, from its `flags` property.
fn locked_axes(params: &toml::Value) -> JointAxesMask {
    let mut mask = JointAxesMask::empty();
    for (name, axis) in [
        ("x", JointAxesMask::LIN_X),
        ("y", JointAxesMask::LIN_Y),
        ("z", JointAxesMask::LIN_Z),
        ("ang_x", JointAxesMask::ANG_X),
        ("ang_y", JointAxesMask::ANG_Y),
        ("ang_z", JointAxesMask::ANG_Z),
    ] {
        if v::flag(params, "locked_axes", name) {
            mask |= axis;
        }
    }
    mask
}

/// The joint a `joint3d` table describes.
pub(crate) fn joint_of(params: &toml::Value) -> Result<GenericJoint> {
    let kind = v::text(params, "kind", "fixed");
    let axis = Vec3::from(v::vec3(params, "axis", [0.0, 0.0, 1.0]));
    let axis = if axis.length_squared() < 1.0e-12 {
        Vec3::Z
    } else {
        axis.normalize()
    };
    let anchor1 = Vec3::from(v::vec3(params, "anchor", [0.0; 3]));
    let anchor2 = Vec3::from(v::vec3(params, "other_anchor", [0.0; 3]));
    let length = v::f(params, "length", 0.0);
    let stiffness = v::f(params, "stiffness", 0.0);
    let damping = v::f(params, "damping", 1.0);
    let mut joint: GenericJoint = match kind {
        "fixed" => FixedJointBuilder::new()
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "revolute" => RevoluteJointBuilder::new(axis)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "prismatic" => PrismaticJointBuilder::new(axis)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        "spherical" => SphericalJointBuilder::new()
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
        "generic" => GenericJointBuilder::new(locked_axes(params))
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .local_axis1(axis)
            .local_axis2(axis)
            .build(),
        other => return Err(anyhow!("unknown joint kind '{other}'")),
    };
    joint.set_contacts_enabled(v::boolean(params, "contacts", false));
    write_limits_and_motor(&mut joint, params, kind);
    Ok(joint)
}

/// Limits and the motor, on whichever axes the kind leaves free.
///
/// `limits = [0, 0]` is how a scene says *no limit*: a joint whose minimum and
/// maximum are the same is either locked or unlimited, and locked is what
/// `locked_axes` is for.
fn write_limits_and_motor(joint: &mut GenericJoint, params: &toml::Value, kind: &str) {
    let limits = v::vec2(params, "limits", [0.0; 2]);
    let motor = v::text(params, "motor", "off");
    let target = v::f(params, "motor_target", 0.0);
    let max_force = v::f(params, "motor_max_force", 0.0);
    let stiffness = v::f(params, "stiffness", 0.0);
    let damping = v::f(params, "damping", 1.0);
    let model = match v::text(params, "motor_model", "acceleration") {
        "force" => MotorModel::ForceBased,
        _ => MotorModel::AccelerationBased,
    };
    for axis in free_axes(kind).iter().copied() {
        if limits[0] < limits[1] {
            joint.set_limits(axis, limits);
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
}

/// The two bodies a joint ties: the node it sits on, and the one `body` names.
fn ends(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<(Entity, Entity)> {
    let other = as_node(eng, entity, params.get("body"))
        .ok_or_else(|| anyhow!("a joint needs a `body` naming the node at its other end"))?;
    Ok((entity, other))
}

fn handles(
    state: &PhysicsState,
    a: Entity,
    b: Entity,
) -> Result<(RigidBodyHandle, RigidBodyHandle)> {
    let first = *state
        .bodies
        .get(&a)
        .ok_or_else(|| anyhow!("the node holding the joint has no body3d"))?;
    let second = *state
        .bodies
        .get(&b)
        .ok_or_else(|| anyhow!("the node at the joint's other end has no body3d"))?;
    Ok((first, second))
}

pub(crate) fn apply_joint(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    remove_joint(eng, entity);
    {
        let state = eng.resource::<PhysicsState>();
        state
            .borrow_mut()
            .joint_params
            .insert(entity, params.clone());
    }
    if !v::boolean(params, "enabled", true) {
        return Ok(());
    }
    // A joint whose other end is not in the scene yet is inert rather than an
    // error: a scene file names nodes in whatever order it likes.
    let Ok((a, b)) = ends(eng, entity, params) else {
        return Ok(());
    };
    let joint = joint_of(params)?;
    let reduced = v::text(params, "solver", "impulse") == "reduced";
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
            break_force: v::f(params, "break_force", 0.0),
        },
    );
    Ok(())
}

pub(crate) fn remove_joint(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    state.joint_params.swap_remove(&entity);
    match state.joints.swap_remove(&entity).map(|j| j.handle) {
        Some(JointHandle::Impulse(handle)) => {
            state.world.remove_impulse_joint(handle);
        }
        Some(JointHandle::Multibody(handle)) => state.world.remove_multibody_joint(handle),
        None => {}
    }
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
    let f = |value: f32| toml::Value::Float(f64::from(value));
    let vec3 = |v: Vec3| toml::Value::Array(vec![f(v.x), f(v.y), f(v.z)]);
    let mut map = authored;
    map.insert("anchor".into(), vec3(data.local_anchor1()));
    map.insert("other_anchor".into(), vec3(data.local_anchor2()));
    map.insert("contacts".into(), data.contacts_enabled().into());
    map.insert(
        "solver".into(),
        toml::Value::String(
            match reference.handle {
                JointHandle::Impulse(_) => "impulse",
                JointHandle::Multibody(_) => "reduced",
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

/// How hard a joint is pulling, as one number.
///
/// Rapier keeps the reaction as six components — three linear, three angular —
/// and a breakable joint wants their magnitude.
fn impulse_magnitude(impulses: &[f32; 6]) -> f32 {
    impulses.iter().map(|i| i * i).sum::<f32>().sqrt()
}

/// The same, for the 2D world's three-component reaction (two linear, one
/// angular), which rapier hands back as a vector.
pub(crate) fn impulse_magnitude_2d(impulses: &glamx::Vec3) -> f32 {
    impulses.length()
}

/// Joints authored but not yet made, because the node at the other end had
/// not been spawned when this one was.
///
/// A scene file names nodes in whatever order it likes, and a joint that
/// pointed forwards used to be silently inert. Retried once per step, over
/// the few that are unresolved rather than over every joint.
pub(crate) fn pending(state: &PhysicsState) -> Vec<Entity> {
    let mut out: Vec<Entity> = state
        .joint_params
        .keys()
        .filter(|entity| !state.joints.contains_key(*entity))
        .copied()
        .collect();
    out.sort_unstable_by_key(|e| e.to_bits());
    out
}

/// Joints whose reaction force passed their `break_force` this step.
///
/// Checked after the step with the world still borrowed, and acted on after it
/// is released — the same shape as an event, because a break *is* one.
pub(crate) fn broken(state: &PhysicsState) -> Vec<Entity> {
    let mut out = Vec::new();
    for (entity, reference) in &state.joints {
        if reference.break_force <= 0.0 {
            continue;
        }
        let JointHandle::Impulse(handle) = reference.handle else {
            // A reduced-coordinates joint has no reaction impulse to read:
            // its constraint is built into the coordinates.
            continue;
        };
        let Some(joint) = state.world.impulse_joints.get(handle) else {
            continue;
        };
        if impulse_magnitude(&joint.impulses) > reference.break_force {
            out.push(*entity);
        }
    }
    out.sort_unstable_by_key(|e| e.to_bits());
    out
}

pub(crate) fn install_joint_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("add_joint", &["joint3d"], "", "Tie this node's body to another with a joint, from a `joint3d` table: `kind`, `body`, `anchor`, `axis`, `limits`, and the rest of the component's own vocabulary."),
        ("remove_joint", &["joint3d"], "", "Undo the node's joint, leaving both bodies free."),
        ("set_motor_velocity", &["joint3d"], "", "Drive the joint towards a speed: how a wheel is powered or a door swings itself shut."),
        ("set_motor_position", &["joint3d"], "", "Drive the joint towards an angle or a distance, with a spring's stiffness and damping."),
        ("set_joint_limits", &["joint3d"], "", "Set how far the joint may travel, in radians for a revolute one and units for a prismatic one."),
        ("joint_impulse", &["joint3d"], "", "How hard the joint is pulling right now: what a breakable one is measured against."),
        ("solve_ik", &["joint3d"], "", "Move a reduced-coordinates chain so its last link reaches a world position, leaving every joint inside its limits."),
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
            solve_ik(eng, entity_of(node)?, Vec3::new(x, y, z))
        },
    );
}

/// A joint's own data, whichever set it lives in.
fn with_joint(eng: &Engine, node: NodeId, f: impl FnOnce(&mut GenericJoint, &str)) -> Result<()> {
    let entity = entity_of(node)?;
    let kind = balaur_core::components::get(eng, entity, "joint3d")
        .and_then(|params| {
            params
                .get("kind")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "revolute".to_string());
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    match state.joints.get(&entity).map(|j| j.handle) {
        Some(JointHandle::Impulse(handle)) => {
            let joint = state
                .world
                .impulse_joints
                .get_mut(handle, true)
                .ok_or_else(|| anyhow!("this node's joint is gone"))?;
            f(&mut joint.data, &kind);
            Ok(())
        }
        Some(JointHandle::Multibody(handle)) => {
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
        None => Err(anyhow!("node has no joint3d")),
    }
}

/// Move a reduced-coordinates chain so the node's own body reaches `target`.
///
/// Rapier's own solver, damped least squares, every joint's limits respected.
/// Impulse joints have no such thing — there are no generalised coordinates to
/// solve for — so this is `solver = "reduced"` only, and says so.
fn solve_ik(eng: &Engine, entity: Entity, target: Vec3) -> Result<()> {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let Some(JointHandle::Multibody(handle)) = state.joints.get(&entity).map(|j| j.handle) else {
        return Err(anyhow!(
            "inverse kinematics needs a chain of reduced-coordinates joints (solver = \"reduced\")"
        ));
    };
    let options = rapier3d::dynamics::InverseKinematicsOption::default();
    let pose = glamx::Pose3::from_translation(target);
    // The solver reads the bodies and writes the chain, so the two borrows are
    // taken one after the other rather than together.
    let displacements = {
        let (multibody, link) = state
            .world
            .multibody_joints
            .get(handle)
            .ok_or_else(|| anyhow!("this node's joint is gone"))?;
        let mut displacements = rapier3d::na::DVector::zeros(multibody.ndofs());
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
        .def("joint3d")
        .ok_or_else(|| anyhow!("joint3d is not registered"))?
        .schema
        .clone())
}

/// The schema both dimensions share; each adds its own axis-shaped half.
pub(crate) const SHARED_JOINT_SCHEMA: &str = r#"
motor = { type = "enum", default = "off", options = ["off", "velocity", "position"], description = "Drive the joint towards a speed, towards a position, or not at all" }
motor_target = { type = "float", default = 0.0, description = "The speed or the position the motor drives towards" }
motor_max_force = { type = "float", default = 0.0, min = 0.0, description = "The most force the motor may use; 0 means as much as it takes" }
motor_model = { type = "enum", default = "acceleration", options = ["acceleration", "force"], description = "Whether the motor's strength is felt as an acceleration, ignoring mass, or as a force" }
stiffness = { type = "float", default = 0.0, min = 0.0, description = "Spring stiffness, for a spring joint or a position motor" }
damping = { type = "float", default = 1.0, min = 0.0, description = "How quickly the motion settles, for a spring joint or a motor" }
length = { type = "float", default = 0.0, min = 0.0, description = "The rope's greatest length, or the spring's rest length" }
contacts = { type = "bool", default = false, description = "Let the two joined bodies collide with each other" }
break_force = { type = "float", default = 0.0, min = 0.0, description = "The pull that snaps the joint and calls on_joint_break; 0 never breaks" }
solver = { type = "enum", default = "impulse", options = ["impulse", "reduced"], description = "impulse holds any arrangement, loops included; reduced never drifts and can be solved for inverse kinematics, but cannot close a loop" }
enabled = { type = "bool", default = true, description = "Hold the two bodies together at all" }
"#;

pub(crate) fn register_joint_component(app: &mut App) {
    let schema = format!(
        r#"kind = {{ type = "enum", default = "fixed", options = ["fixed", "revolute", "prismatic", "spherical", "rope", "spring", "generic"], shorthand = true, description = "How the two bodies may move relative to each other" }}
body = {{ type = "node", default = "", description = "The node at the joint's other end; this node is the first end" }}
anchor = {{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where the joint attaches on this node, in its own space" }}
other_anchor = {{ type = "vec3", default = [0.0, 0.0, 0.0], description = "Where it attaches on the other node, in that node's space" }}
axis = {{ type = "vec3", default = [0.0, 0.0, 1.0], description = "The axis a revolute joint turns about or a prismatic one slides along" }}
limits = {{ type = "vec2", default = [0.0, 0.0], description = "How far the joint may travel, as a low and a high; equal values mean no limit" }}
locked_axes = {{ type = "flags", default = [], options = ["x", "y", "z", "ang_x", "ang_y", "ang_z"], description = "Which of the six freedoms a generic joint takes away" }}
{SHARED_JOINT_SCHEMA}"#
    );
    app.register_component(
        "joint3d",
        ComponentDef {
            doc: "Holds this node's body to another one: a hinge, a slider, a rope, a spring, a ball socket, or a generic joint you lock axis by axis. Both ends need a `body3d`.",
            schema: ComponentDef::parse_schema("joint3d", &schema),
            tags: &["3d", "physics"],
            expects: &["body3d"],
            apply: Box::new(apply_joint),
            remove: Box::new(|eng, entity| {
                remove_joint(eng, entity);
                Ok(())
            }),
            get: Box::new(get_joint_params),
        },
    );
}
