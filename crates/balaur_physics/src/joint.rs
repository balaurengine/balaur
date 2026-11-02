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
use crate::rapier3d::dynamics::{JointEnabled, MultibodyDofCoupling, RevoluteJoint};
use crate::rapier3d::prelude::{
    FixedJointBuilder, GenericJoint, GenericJointBuilder, ImpulseJoint, ImpulseJointHandle,
    JointAxesMask, JointAxis, MotorModel, MultibodyJointHandle, PrismaticJointBuilder,
    RevoluteJointBuilder, RigidBodyHandle, RopeJointBuilder, SphericalJointBuilder,
    SpringCoefficients, SpringJointBuilder,
};
use crate::scalar::{self, Real, Vector};
use crate::shared::joint::JointAsk as _;
use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{self, Component, Record, Written};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::PhysicsState3d;
use crate::properties::{Axis3d, JointAxis3d, JointKind3d, Motor, MotorStrength};
use crate::rapier3d::pipeline::PhysicsWorld;
use crate::shared::beside::{self, Beside};
use crate::shared::joint::{DEFAULT_DAMPING, DEFAULT_SOFTNESS_HZ};
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
fn free_axes(kind: &str, ask: &JointAsk3d) -> JointAxesMask {
    match kind {
        w::HINGE => JointAxesMask::FREE_REVOLUTE_AXES,
        w::SLIDER => JointAxesMask::FREE_PRISMATIC_AXES,
        w::BALL_SOCKET => JointAxesMask::FREE_SPHERICAL_AXES,
        w::ROPE | w::SPRING => JointAxesMask::LIN_X | JointAxesMask::ANG_AXES,
        w::GENERIC => {
            (JointAxesMask::LIN_AXES | JointAxesMask::ANG_AXES) - locked_axes(&ask.generic)
        }
        _ => JointAxesMask::empty(),
    }
}

/// The mask three axes make, translation first: `lin` the ones along, `ang`
/// the ones about.
fn mask_of(lin: &[Axis3d], ang: &[Axis3d]) -> JointAxesMask {
    let bit = |axis: &Axis3d, x, y, z| match axis {
        Axis3d::X => x,
        Axis3d::Y => y,
        Axis3d::Z => z,
    };
    let along = lin.iter().fold(JointAxesMask::empty(), |mask, axis| {
        mask | bit(
            axis,
            JointAxesMask::LIN_X,
            JointAxesMask::LIN_Y,
            JointAxesMask::LIN_Z,
        )
    });
    ang.iter().fold(along, |mask, axis| {
        mask | bit(
            axis,
            JointAxesMask::ANG_X,
            JointAxesMask::ANG_Y,
            JointAxesMask::ANG_Z,
        )
    })
}

/// The axes a `generic` joint locks.
fn locked_axes(generic: &GenericJoint3d) -> JointAxesMask {
    mask_of(&generic.lock_translation, &generic.lock_rotation)
}

/// The joint a node asks for.
pub(crate) fn joint_of(ask: &JointAsk3d) -> Result<GenericJoint> {
    let held = &ask.joint;
    let kind = held.kind.option();
    let axis = scalar::v3a(held.axis);
    let axis = if axis.length_squared() < 1.0e-12 {
        Vector::Z
    } else {
        axis.normalize()
    };
    let anchor1 = scalar::v3a(held.anchor);
    let anchor2 = scalar::v3a(held.connected_anchor);
    let max_length = ask.rope.max_length;
    let rest_length = ask.spring.rest_length;
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
        w::SPRING => SpringJointBuilder::new(rest_length.max(0.0), 0.0, DEFAULT_DAMPING)
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .build()
            .into(),
        w::GENERIC => GenericJointBuilder::new(locked_axes(&ask.generic))
            .local_anchor1(anchor1)
            .local_anchor2(anchor2)
            .local_axis1(axis)
            .local_axis2(axis)
            .build(),
        other => return Err(anyhow!("unknown joint kind '{other}'")),
    };
    // The other end's own axis when it names one; `axis` reaches both ends otherwise.
    let connected = scalar::v3a(held.connected_axis);
    if matches!(kind, w::HINGE | w::SLIDER | w::GENERIC) && connected.length_squared() >= 1.0e-12 {
        joint.set_local_axis2(connected.normalize());
    }
    let turn = |[x, y, z]: [f32; 3]| glamx::Quat::from_euler(glamx::EulerRot::XYZ, x, y, z);
    joint.local_frame1.rotation *= turn(held.anchor_rotation);
    joint.local_frame2.rotation *= turn(held.connected_anchor_rotation);
    let (hz, ratio) = crate::shared::joint::softness(held.softness_hz, held.softness_damping_ratio);
    joint.softness = SpringCoefficients::new(hz, ratio);
    joint.coupled_axes |= mask_of(
        &ask.generic.coupled_translation,
        &ask.generic.coupled_rotation,
    );
    joint.set_contacts_enabled(held.collide_connected);
    joint.set_enabled(held.enabled);
    write_axes(&mut joint, ask, kind)?;
    Ok(joint)
}

/// The two bodies a joint ties: the node it sits on, and the one `connected_body` names.
///
/// Either end may be a bodiless child, which stands for the nearest body
/// above it — as a collider on a child does. A joint is one per node, so
/// this is how one body carries several.
fn ends(eng: &Engine, entity: Entity, connected_body: &str) -> Result<(Entity, Entity)> {
    let other = crate::shared::joint::node_named(eng, entity, connected_body).ok_or_else(|| {
        anyhow!("a joint needs a `connected_body` naming the node at its other end")
    })?;
    Ok((body_above(eng, entity), body_above(eng, other)))
}

/// The node itself when it has a body, else the nearest ancestor that does;
/// the node again when none does, so `handles` reports the missing body.
fn body_above(eng: &Engine, entity: Entity) -> Entity {
    crate::collider::nearest_body(eng, entity).map_or(entity, |(node, _)| node)
}

/// Joins this node's body to `connected_body`. `kind` is `fixed`, `hinge`, `slider`, `ball_socket`, `rope`, `spring` or `generic`; both ends need a `body3d` on or above the node. `axes` limits and drives each free axis.
#[derive(Clone, Component)]
#[component(name = "joint3d", on_remove = joint3d_removed)]
pub struct Joint3d {
    /// How the two bodies may move relative to each other
    #[property(default = JointKind3d::Fixed)]
    pub kind: JointKind3d,
    /// The node at the joint's other end; this node is the first end
    #[property(node)]
    pub connected_body: String,
    /// Hold the two bodies together at all. Off keeps the joint and stops solving it; an articulation leaves its chain instead, as rapier's chains ignore the switch
    #[property(default = true)]
    pub enabled: bool,
    /// Let the two joined bodies collide with each other
    pub collide_connected: bool,
    /// One record per free axis to limit or drive. A rope's x takes no limits (max_length is its limit); a spring's x is its spring, whose motor stays off
    pub axes: Vec<FreeAxis3d>,
    /// Where the joint attaches on this node, in its own space
    #[property(group = "anchors")]
    pub anchor: [f32; 3],
    /// Where it attaches on the other node, in that node's space
    #[property(group = "anchors")]
    pub connected_anchor: [f32; 3],
    /// The joint frame's x axis: what a hinge turns about, a slider slides along, and a generic joint's x and rotation_x name
    #[property(default = [0.0, 0.0, 1.0], group = "anchors")]
    pub axis: [f32; 3],
    /// The same axis in the other node's space, for a hinge, a slider or a generic joint; zero takes axis
    #[property(group = "anchors")]
    pub connected_axis: [f32; 3],
    /// Turns this end's joint frame about its own axes, x first: what a hinge's angle and its limits measure from, and the relative turn a fixed joint holds. Euler radians in the file
    #[property(group = "anchors", unit = "degrees")]
    pub anchor_rotation: [f32; 3],
    /// The same for the other end's frame
    #[property(group = "anchors", unit = "degrees")]
    pub connected_anchor_rotation: [f32; 3],
    /// How stiffly the joint's locked axes and limits are held, as a spring frequency in hertz; lower lets them stretch and spring back
    #[property(default = DEFAULT_SOFTNESS_HZ, group = "softness", min = 0.0)]
    pub softness_hz: f32,
    /// The damping ratio of that spring; 1 settles without overshooting
    #[property(default = 1.0, group = "softness", min = 0.0)]
    pub softness_damping_ratio: f32,
}

/// What snaps the node's `joint3d`: past either, it breaks and calls on_joint_break. An articulation never breaks, so this sits beside a joint that is not one.
#[derive(Clone, Component)]
#[component(name = "breakable_joint3d")]
pub struct BreakableJoint3d {
    /// The force, in newtons, that snaps the joint; 0 never breaks it by force
    #[property(min = 0.0)]
    pub break_force: f32,
    /// The torque about the anchor, in newton metres, that snaps the joint; 0 never breaks it by torque
    #[property(min = 0.0)]
    pub break_torque: f32,
}

impl Beside for BreakableJoint3d {
    type Main = Joint3d;

    fn reads(_: &Joint3d) -> bool {
        true
    }
}

/// How long a `joint3d` of kind rope may stretch.
#[derive(Clone, Component)]
#[component(name = "rope_joint3d")]
pub struct RopeJoint3d {
    /// The rope's greatest length
    #[property(min = 0.0)]
    pub max_length: f32,
}

impl Beside for RopeJoint3d {
    type Main = Joint3d;

    fn reads(main: &Joint3d) -> bool {
        main.kind == JointKind3d::Rope
    }
}

/// The length a `joint3d` of kind spring pulls back to.
#[derive(Clone, Component)]
#[component(name = "spring_joint3d")]
pub struct SpringJoint3d {
    /// The length a spring pulls back to
    #[property(min = 0.0)]
    pub rest_length: f32,
}

impl Beside for SpringJoint3d {
    type Main = Joint3d;

    fn reads(main: &Joint3d) -> bool {
        main.kind == JointKind3d::Spring
    }
}

/// Which axes a `joint3d` of kind generic locks, and which free ones it ties into one.
#[derive(Clone, Component)]
#[component(name = "generic_joint3d")]
pub struct GenericJoint3d {
    /// The axes a generic joint may not slide along
    #[property(flags)]
    pub lock_translation: Vec<Axis3d>,
    /// The axes a generic joint may not turn about
    #[property(flags)]
    pub lock_rotation: Vec<Axis3d>,
    /// Free linear axes tied into one distance, whose limit and motor come from the first of them; a rope and a spring couple all three already
    #[property(flags)]
    pub coupled_translation: Vec<Axis3d>,
    /// Free turning axes tied into one angle, whose limit and motor come from the first of them
    #[property(flags)]
    pub coupled_rotation: Vec<Axis3d>,
}

impl Beside for GenericJoint3d {
    type Main = Joint3d;

    fn reads(main: &Joint3d) -> bool {
        main.kind == JointKind3d::Generic
    }
}

/// Solves the node's `joint3d` in reduced coordinates: the chain never drifts and can be solved for inverse kinematics, but cannot close a loop or break.
#[derive(Clone, Component)]
#[component(name = "articulation3d")]
pub struct Articulation3d {
    /// On an articulation: let the chain's links collide with each other. One setting for the whole chain, off when any of its joints says off
    #[property(default = true)]
    pub self_collision: bool,
    /// On an articulation: the solver never changes this joint's velocity, so the link holds its pose against its parent and moves only by solve_ik
    pub kinematic_link: bool,
    /// On an articulation: a spring on each free axis pulling it towards passive_rest; 0 is none
    #[property(min = 0.0)]
    pub passive_stiffness: f32,
    /// On an articulation: where that spring rests, in radians about a rotation axis and units along a linear one
    pub passive_rest: f32,
    /// On an articulation: another joint node of the same chain this joint follows, its first free axis tied to that joint's first free axis
    #[property(node)]
    pub gear_with: String,
    /// How far this joint turns or slides per unit of the gear_with joint's
    #[property(default = 1.0)]
    pub gear_ratio: f32,
    /// Where this joint stands when the gear_with joint is at 0
    pub gear_offset: f32,
    /// One record per free axis whose link moves differently from rapier's own: its damping, the inertia a rotor adds, and dry friction
    pub link_axes: Vec<LinkAxis3d>,
}

/// How one free axis of an articulated link moves, as `link_axes` holds it.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct LinkAxis3d {
    /// The free axis, in the joint's frame
    #[property(default = JointAxis3d::RotationX)]
    pub axis: JointAxis3d,
    /// Damping on this axis; below zero takes rapier's, 0.1 about a rotation axis and 0 along a linear one
    #[property(default = -1.0, min = -1.0)]
    pub link_damping: f32,
    /// Inertia added to this axis, as a motor's own rotor adds it
    #[property(min = 0.0)]
    pub armature: f32,
    /// The most force or torque dry friction resists this axis with
    #[property(min = 0.0)]
    pub friction: f32,
}

impl Beside for Articulation3d {
    type Main = Joint3d;

    fn reads(_: &Joint3d) -> bool {
        true
    }
}

/// The components whose rows a `joint3d`'s build reads: its own, then
/// those beside it.
pub(crate) const PARAMS: &[&str] = &[
    c::JOINT_3D,
    <RopeJoint3d as Component>::NAME,
    <SpringJoint3d as Component>::NAME,
    <GenericJoint3d as Component>::NAME,
    <Articulation3d as Component>::NAME,
    <BreakableJoint3d as Component>::NAME,
];

/// What a node's joint is built from: the `joint3d` and the components
/// beside it as its kind reads them.
#[derive(Clone)]
pub struct JointAsk3d {
    pub joint: Joint3d,
    pub rope: RopeJoint3d,
    pub spring: SpringJoint3d,
    pub generic: GenericJoint3d,
    pub breakable: BreakableJoint3d,
    /// The `articulation3d` beside it, when one stands there.
    pub articulation: Option<Articulation3d>,
}

impl JointAsk3d {
    /// What `entity` asks for, `None` without a `joint3d`.
    pub fn of(world: &balaur_core::hecs::World, entity: Entity) -> Option<Self> {
        let joint = (*world.get::<&Joint3d>(entity).ok()?).clone();
        Some(Self {
            rope: beside::held(world, entity, &joint),
            spring: beside::held(world, entity, &joint),
            generic: beside::held(world, entity, &joint),
            breakable: beside::held(world, entity, &joint),
            articulation: world
                .get::<&Articulation3d>(entity)
                .ok()
                .map(|held| (*held).clone()),
            joint,
        })
    }

    /// The ask as one table, the way a snapshot keeps it: every row of the
    /// components beside the joint, and whether an articulation stands there.
    pub fn to_table(&self) -> toml::Value {
        let mut table = derived::to_table(&self.joint);
        let articulation = self.articulation.clone().unwrap_or_default();
        for beside in [
            derived::to_table(&self.rope),
            derived::to_table(&self.spring),
            derived::to_table(&self.generic),
            derived::to_table(&articulation),
            derived::to_table(&self.breakable),
        ] {
            if let (Some(rows), toml::Value::Table(more)) = (table.as_table_mut(), beside) {
                rows.extend(more);
            }
        }
        if let Some(rows) = table.as_table_mut() {
            rows.insert(k::ARTICULATION.into(), self.articulation.is_some().into());
        }
        table
    }

    /// The ask out of one table, as [`Self::to_table`]
    /// wrote it.
    pub fn from_table(params: &toml::Value) -> Result<Self> {
        Ok(Self {
            joint: derived::from_table(params)?,
            rope: derived::from_table(params)?,
            spring: derived::from_table(params)?,
            generic: derived::from_table(params)?,
            breakable: derived::from_table(params)?,
            articulation: v::boolean(params, k::ARTICULATION, false)
                .then(|| derived::from_table(params))
                .transpose()?,
        })
    }
}

impl crate::shared::joint::JointAsk for JointAsk3d {
    fn kind_word(&self) -> &'static str {
        self.joint.kind.option()
    }
    fn enabled(&self) -> bool {
        self.joint.enabled
    }
    fn articulated(&self) -> bool {
        self.articulation.is_some()
    }
    fn kinematic_link(&self) -> bool {
        self.articulation
            .as_ref()
            .is_some_and(|held| held.kinematic_link)
    }
    fn connected_body(&self) -> &str {
        &self.joint.connected_body
    }
    fn gear_with(&self) -> &str {
        self.articulation
            .as_ref()
            .map_or("", |held| &held.gear_with)
    }
    fn gear(&self) -> (f32, f32) {
        self.articulation
            .as_ref()
            .map_or((1.0, 0.0), |held| (held.gear_ratio, held.gear_offset))
    }
    fn self_collision(&self) -> bool {
        self.articulation
            .as_ref()
            .is_none_or(|held| held.self_collision)
    }
    fn passive(&self) -> (f32, f32) {
        self.articulation.as_ref().map_or((0.0, 0.0), |held| {
            (held.passive_stiffness, held.passive_rest)
        })
    }
    fn link_record(&self, word: &str) -> crate::shared::joint::LinkRecord {
        self.articulation
            .as_ref()
            .and_then(|held| held.link_axes.iter().find(|row| row.axis.option() == word))
            .map_or(crate::shared::joint::LinkRecord::NONE, |row| {
                crate::shared::joint::LinkRecord {
                    link_damping: row.link_damping,
                    armature: row.armature.max(0.0),
                    friction: row.friction.max(0.0),
                }
            })
    }
    fn breaks(&self) -> (f32, f32) {
        (self.breakable.break_force, self.breakable.break_torque)
    }
    fn rest_length(&self) -> f32 {
        self.spring.rest_length
    }
    fn axis_records(&self) -> Vec<crate::shared::joint::AxisRecord> {
        self.joint
            .axes
            .iter()
            .map(|row| crate::shared::joint::AxisRecord {
                axis: row.axis.option(),
                limits: row.limits,
                motor: row.motor.option(),
                target: row.motor_target,
                target_velocity: row.motor_target_velocity,
                max_force: row.motor_max_force,
                model: row.motor_model.option(),
                stiffness: row.stiffness,
                damping: row.damping,
            })
            .collect()
    }
}

/// One free axis of a `joint3d` to limit or drive, as its `axes` list
/// holds it.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct FreeAxis3d {
    /// The free axis this record limits and drives, in the joint's frame
    #[property(default = JointAxis3d::RotationX)]
    pub axis: JointAxis3d,
    /// How far the axis may travel, low then high, in radians about a rotation axis; equal values mean no limit
    pub limits: [f32; 2],
    /// Drive the axis towards a speed, towards a position, or not at all
    #[property(default = Motor::Off)]
    pub motor: Motor,
    /// The speed or the position the motor drives towards
    pub motor_target: f32,
    /// The speed a position motor wants at its target
    pub motor_target_velocity: f32,
    /// The most force or torque the motor may use; 0 means as much as it takes
    #[property(min = 0.0)]
    pub motor_max_force: f32,
    /// Whether the motor's strength is an acceleration, ignoring mass, or a force; auto is force for a spring's x and acceleration elsewhere
    #[property(default = MotorStrength::Auto)]
    pub motor_model: MotorStrength,
    /// Spring stiffness, for a position motor or a spring's x
    #[property(min = 0.0)]
    pub stiffness: f32,
    /// How quickly the motion settles, for a motor or a spring's x
    #[property(default = DEFAULT_DAMPING, min = 0.0)]
    pub damping: f32,
}

crate::shared::joint::functions!(
    joint = Joint3d,
    ask = JointAsk3d,
    state = PhysicsState3d,
    world = PhysicsWorld,
    reference = JointRef3d,
    handle = JointHandle3d,
    component = c::JOINT_3D,
    body = c::BODY_3D,
    impulse_parts = impulse_parts
);

pub(crate) fn apply_joint(eng: &Engine, entity: Entity, ask: &JointAsk3d) -> Result<()> {
    if toggled_in_place(eng, entity, ask) {
        return Ok(());
    }
    remove_joint(eng, entity);
    {
        let state = eng.resource::<PhysicsState3d>();
        state.borrow_mut().joint_params.insert(entity, ask.clone());
    }
    let reduced = ask.articulated();
    // rapier's chains ignore a joint's switch, so an articulation that is off
    // stays out of its chain.
    if reduced && !ask.enabled() {
        return Ok(());
    }
    let (break_force, break_torque) = crate::shared::joint::thresholds(ask.breaks(), reduced)?;
    // A joint whose other end is not in the scene yet is inert rather than an
    // error: a scene file names nodes in whatever order it likes.
    let Ok((a, b)) = ends(eng, entity, ask.connected_body()) else {
        return Ok(());
    };
    let joint = joint_of(ask)?;
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let (first, second) = handles(&state, a, b)?;
    let handle = if reduced {
        // The connected body is the parent link, so a chain's root is the end
        // its joints point towards; rapier wants the frames the other way round.
        let mut joint = joint;
        joint.flip();
        let set = &mut state.world.multibody_joints;
        let made = if ask.kinematic_link() {
            set.insert_kinematic(second, first, joint, true)
        } else {
            set.insert(second, first, joint, true)
        };
        made.map(JointHandle3d::Multibody).ok_or_else(|| {
            anyhow!("an articulation cannot close a loop; take the articulation3d off this joint")
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
        ("joint_state", &[c::JOINT_3D], "(node: node) -> map", "What the joint is doing: `{ status, angle, limit_impulses, motor_impulses, coordinates, velocities }`. `status` is `enabled`, `disabled`, `body_disabled` (a body at either end is disabled) or `waiting` (its other end is not in the scene yet); `angle` is a hinge's turn in radians; the impulse maps hold each free axis's last limit and motor impulse; an articulation adds each free axis's coordinate and velocity."),
        ("joint_force", &[c::JOINT_3D], "(node: node) -> map", "The force and the torque the joint held through the last step, `{ force, torque }`: what `break_force` and `break_torque` are measured against. An articulation has none to read, and says so."),
        ("solve_ik", &[c::JOINT_3D], "(node: node, x: float, y: float, z: float, opts: IkOptions?)", "Move a reduced-coordinates chain so its last link reaches a world position, leaving every joint inside its limits. Options: `rotation` (euler radians to turn the link to), `constrain` (the world axes to match: `x`, `y`, `z`, `rotation_x`, `rotation_y`, `rotation_z`; the translation by default, every axis when `rotation` is given), `damping` (1.0), `iterations` (10), `tolerance` (0.001)."),
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
    m.records(&[(
        "IkOptions",
        "{ rotation: any, constrain: [string]?, damping: float?, iterations: int?, tolerance: float? }",
        "How `solve_ik` reaches: the `rotation` to turn the last link to, the world axes it matches, and the solver's damping, iterations and tolerance.",
    )]);
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
/// solve for — so this is a joint with an `articulation3d` only, and says so.
fn solve_ik(eng: &Engine, entity: Entity, target: Vector, opts: &v::Opts<'_>) -> Result<()> {
    let rotation = opts.get(k::ROTATION).map(|_| {
        let [x, y, z] = opts.vec3(k::ROTATION, [0.0; 3]);
        glamx::Quat::from_euler(glamx::EulerRot::ZYX, z, y, x)
    });
    let options = ik_options(opts, rotation.is_some())?;
    let pose = scalar::Pose::from_parts(target, rotation.unwrap_or(scalar::Rotation::IDENTITY));
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let Some(JointHandle3d::Multibody(handle)) = state.joints.get(&entity).map(|j| j.handle) else {
        return Err(anyhow!(
            "inverse kinematics needs a chain of reduced-coordinates joints: an articulation3d beside each joint3d"
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

/// The joint the component asks for, made again.
fn after_write(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let Some(ask) = JointAsk3d::of(&eng.world(), entity) else {
        return Ok(());
    };
    apply_joint(eng, entity, &ask)
}

/// What a removed `Joint3d` leaves behind, taken with it.
fn joint3d_removed(eng: &Engine, entity: Entity) {
    remove_joint(eng, entity);
}

/// `joint3d`: what was asked, held on the node; the joint and the table it was
/// made from are in [`crate::PhysicsState3d`].
pub(crate) fn register_joint_component(reg: &mut Registry<'_>) {
    let tags: &'static [&'static str] = &[
        balaur_core::components::tag::DIM_3D,
        balaur_core::components::tag::PHYSICS,
    ];
    let main: &'static [&'static str] = &[c::JOINT_3D];
    beside::register::<RopeJoint3d>(reg, main, tags, after_write);
    beside::register::<SpringJoint3d>(reg, main, tags, after_write);
    beside::register::<GenericJoint3d>(reg, main, tags, after_write);
    beside::register::<Articulation3d>(reg, main, tags, after_write);
    beside::register::<BreakableJoint3d>(reg, main, tags, after_write);
    // After the components beside it, so a scene gives the node those first.
    reg.register_component(
        Joint3d::NAME,
        ComponentDef {
            events: crate::vocabulary::hook::JOINT,
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &[c::BODY_3D],
            ..ComponentDef::of::<Joint3d>(Some(after_write))
        },
    );
}
