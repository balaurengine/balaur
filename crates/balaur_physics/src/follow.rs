//! `follow3d`: rapier's PD or PID controller pulling a body towards a node.
//!
//! A body has no target of its own, and a joint motor drives one body
//! against another; this turns the gap between a body and a target node into
//! a velocity change every fixed step, before the solver runs.

use crate::properties::{Axis3d, FollowKind, node_at};
use crate::rapier3d::control::{PdController, PidController};
use crate::rapier3d::dynamics::{AxesMask, RigidBodyVelocity};
use crate::scalar::{self, Real};
use balaur_core::components::derived::Component;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, GlobalTransform, Stage, entity_of, fixed_dt};
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId};

use crate::PhysicsState3d;
use crate::vocabulary::component as c;
use crate::vocabulary::words;

/// Pulls the node's `body3d` towards the pose of `target` every fixed step, with rapier's PD or PID controller. `node.follow3d.reset()` forgets what a PID summed.
#[derive(Clone, Component)]
#[component(
    name = "follow3d",
    on_remove = follow3d_removed,
    tags = [ balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
    expects = [c::BODY_3D],
)]
pub struct Follow3d {
    /// The node whose pose the body is pulled to; empty pulls nothing
    #[property(node)]
    pub target: String,
    /// Rapier's proportional-derivative controller, or the same with an integral that keeps pulling against a steady push
    #[property(default = FollowKind::Pd)]
    pub kind: FollowKind,
    /// The axes the body is pulled along
    #[property(flags, default = vec![Axis3d::X, Axis3d::Y, Axis3d::Z])]
    pub translation_axes: Vec<Axis3d>,
    /// The axes the body is turned about
    #[property(flags, default = vec![Axis3d::X, Axis3d::Y, Axis3d::Z])]
    pub rotation_axes: Vec<Axis3d>,
    /// How hard a gap in position pulls, per axis: about one over the fixed step closes it in one step
    #[property(default = [60.0, 60.0, 60.0], group = "gains")]
    pub position_gain: [f32; 3],
    /// How much of a gap in velocity a step corrects, per axis: 0 none, 1 all of it
    #[property(default = [0.8, 0.8, 0.8], group = "gains")]
    pub velocity_gain: [f32; 3],
    /// How hard a gap in rotation pulls, per axis
    #[property(default = [60.0, 60.0, 60.0], group = "gains")]
    pub rotation_gain: [f32; 3],
    /// How much of a gap in spin a step corrects, per axis
    #[property(default = [0.8, 0.8, 0.8], group = "gains")]
    pub spin_gain: [f32; 3],
    /// How hard the gap in position summed over time pulls, per axis, with `pid`
    #[property(default = [1.0, 1.0, 1.0], group = "gains")]
    pub integral_gain: [f32; 3],
    /// How hard the gap in rotation summed over time pulls, per axis, with `pid`
    #[property(default = [1.0, 1.0, 1.0], group = "gains")]
    pub rotation_integral_gain: [f32; 3],
    /// The velocity the body is pulled to match, in world units per second
    #[property(group = "velocity")]
    pub target_linear_velocity: [f32; 3],
    /// The spin the body is pulled to match about each axis, radians a second in the file
    #[property(group = "velocity", unit = "degrees")]
    pub target_angular_velocity: [f32; 3],
}

/// One body's controller, kept across steps: a PID's integrals change every
/// step, so the snapshot carries it.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum FollowRef3d {
    Pd(PdController),
    Pid(PidController),
}

/// The axes a flags property names, as rapier's mask.
fn axes(named: &[Axis3d], [x, y, z]: [AxesMask; 3]) -> AxesMask {
    let mut mask = AxesMask::empty();
    for (axis, bit) in [(Axis3d::X, x), (Axis3d::Y, y), (Axis3d::Z, z)] {
        if named.contains(&axis) {
            mask |= bit;
        }
    }
    mask
}

/// The controller the component describes, with a PID's integrals at zero.
fn controller_of(follow: &Follow3d) -> FollowRef3d {
    let gain = scalar::v3a;
    let mask = axes(
        &follow.translation_axes,
        [AxesMask::LIN_X, AxesMask::LIN_Y, AxesMask::LIN_Z],
    ) | axes(
        &follow.rotation_axes,
        [AxesMask::ANG_X, AxesMask::ANG_Y, AxesMask::ANG_Z],
    );
    let pd = PdController {
        lin_kp: gain(follow.position_gain),
        lin_kd: gain(follow.velocity_gain),
        ang_kp: gain(follow.rotation_gain),
        ang_kd: gain(follow.spin_gain),
        axes: mask,
    };
    match follow.kind {
        FollowKind::Pid => {
            let mut pid = PidController::new(0.0, 0.0, 0.0, mask);
            pid.pd = pd;
            pid.lin_ki = gain(follow.integral_gain);
            pid.ang_ki = gain(follow.rotation_integral_gain);
            FollowRef3d::Pid(pid)
        }
        FollowKind::Pd => FollowRef3d::Pd(pd),
    }
}

impl FollowRef3d {
    /// The gains and axes of `fresh`, keeping a PID's integrals when the
    /// kind stays the same: a patch to a gain must not drop what it held.
    fn retune(&mut self, fresh: Self) {
        match (self, fresh) {
            (Self::Pid(held), Self::Pid(new)) => {
                held.pd = new.pd;
                held.lin_ki = new.lin_ki;
                held.ang_ki = new.ang_ki;
            }
            (held, new) => *held = new,
        }
    }
}

/// What a removed `Follow3d` leaves behind, taken with it.
fn follow3d_removed(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState3d>();
    state.borrow_mut().follows.swap_remove(&entity);
}

/// The system, before the step it feeds, and the component.
pub(crate) fn build(reg: &mut Registry<'_>) {
    reg.add_system(Stage::FixedUpdate, follow_system);
    register_follow_component(reg);
}

/// Before the step: the change this writes is what the solver integrates.
fn follow_system(eng: &Engine, _dt: f32) {
    if eng.paused() {
        return;
    }
    let followers: Vec<(Entity, Follow3d)> = {
        let world = eng.world();
        let mut query = world.query::<(Entity, &Follow3d)>();
        query
            .iter()
            .filter(|(e, _)| !balaur_core::parked::is_parked(eng, *e))
            .map(|(e, f)| (e, f.clone()))
            .collect()
    };
    let dt: Real = fixed_dt();
    for (entity, follow) in followers {
        let Some(target) = node_at(eng, entity, &follow.target) else {
            continue;
        };
        let Some(goal) = eng
            .world()
            .get::<&GlobalTransform>(target)
            .ok()
            .map(|g| scalar::pose_of(g.position, g.rotation))
        else {
            continue;
        };
        let wanted = RigidBodyVelocity {
            linvel: scalar::v3a(follow.target_linear_velocity),
            angvel: scalar::v3a(follow.target_angular_velocity),
        };
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let Some(&handle) = state.bodies.get(&entity) else {
            continue;
        };
        let fresh = controller_of(&follow);
        let mut controller = *state.follows.entry(entity).or_insert(fresh);
        controller.retune(fresh);
        let Some(body) = state.world.bodies.get(handle) else {
            continue;
        };
        let change = match &mut controller {
            FollowRef3d::Pd(pd) => pd.rigid_body_correction(body, goal, wanted),
            FollowRef3d::Pid(pid) => pid.rigid_body_correction(dt, body, goal, wanted),
        };
        state.follows.insert(entity, controller);
        if let Some(body) = state.world.bodies.get_mut(handle) {
            let (linvel, angvel) = (body.linvel() + change.linvel, body.angvel() + change.angvel);
            body.set_linvel(linvel, true);
            body.set_angvel(angvel, true);
        }
    }
}

/// `follow3d`: what was asked, held on the node; the controller a step keeps
/// is in [`PhysicsState3d::follows`].
fn register_follow_component(reg: &mut Registry<'_>) {
    reg.add_component::<Follow3d>();
}

pub(crate) fn install_follow_api(m: &mut Module<Engine>) {
    m.describe(&[(
        words::functions::RESET_FOLLOW,
        &[c::FOLLOW_3D],
        "",
        "Forget what a `pid` follow summed, so it pulls from now as if it had just started.",
    )]);
    m.function(
        words::functions::RESET_FOLLOW,
        |eng: &Engine, node: NodeId| {
            let entity = entity_of(node)?;
            let state = eng.resource::<PhysicsState3d>();
            if let Some(FollowRef3d::Pid(pid)) = state.borrow_mut().follows.get_mut(&entity) {
                pid.reset_integrals();
            }
            Ok(())
        },
    );
}
