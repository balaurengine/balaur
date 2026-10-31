//! `follow2d`: the 2D twin of `follow3d`.

use crate::properties::{Axis2d, FollowKind, Turn, node_at};
use crate::rapier2d::control::{PdController, PidController};
use crate::rapier2d::dynamics::{AxesMask, RigidBodyVelocity};
use crate::scalar::{self, Real};
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::Component;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Stage, entity_of, fixed_dt};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use super::PhysicsState2d;
use crate::vocabulary::component as c;

/// Pulls the node's `body2d` towards the pose of `target` every fixed step, with rapier's PD or PID controller. `node.follow2d.reset_follow()` forgets what a PID summed.
#[derive(Clone, Component)]
#[component(name = "follow2d")]
pub struct Follow2d {
    /// The node whose pose the body is pulled to; empty pulls nothing
    #[property(node)]
    pub target: String,
    /// Rapier's proportional-derivative controller, or the same with an integral that keeps pulling against a steady push
    #[property(default = FollowKind::Pd)]
    pub kind: FollowKind,
    /// The axes the body is pulled along
    #[property(flags, default = vec![Axis2d::X, Axis2d::Y])]
    pub translation_axes: Vec<Axis2d>,
    /// Whether the body is turned to the target's rotation too: the one turn a 2D body has, as a 2D joint names it
    #[property(flags, default = vec![Turn::Rotation])]
    pub rotation_axes: Vec<Turn>,
    /// How hard a gap in position pulls, per axis: about one over the fixed step closes it in one step
    #[property(default = [60.0, 60.0], group = "gains")]
    pub position_gain: [f32; 2],
    /// How much of a gap in velocity a step corrects, per axis: 0 none, 1 all of it
    #[property(default = [0.8, 0.8], group = "gains")]
    pub velocity_gain: [f32; 2],
    /// How hard a gap in rotation pulls
    #[property(default = 60.0, group = "gains")]
    pub rotation_gain: f32,
    /// How much of a gap in spin a step corrects
    #[property(default = 0.8, group = "gains")]
    pub spin_gain: f32,
    /// How hard the gap in position summed over time pulls, per axis, with `pid`
    #[property(default = [1.0, 1.0], group = "gains")]
    pub integral_gain: [f32; 2],
    /// How hard the gap in rotation summed over time pulls, with `pid`
    #[property(default = 1.0, group = "gains")]
    pub rotation_integral_gain: f32,
    /// The velocity the body is pulled to match, in world units per second
    #[property(group = "velocity")]
    pub target_linear_velocity: [f32; 2],
    /// The spin the body is pulled to match, radians a second in the file
    #[property(group = "velocity", unit = "degrees")]
    pub target_angular_velocity: f32,
}

/// As [`crate::follow::FollowRef3d`], in the plane.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum FollowRef2d {
    Pd(PdController),
    Pid(PidController),
}

fn controller_of(follow: &Follow2d) -> FollowRef2d {
    let gain = scalar::v2a;
    let mut mask = AxesMask::empty();
    if follow.translation_axes.contains(&Axis2d::X) {
        mask |= AxesMask::LIN_X;
    }
    if follow.translation_axes.contains(&Axis2d::Y) {
        mask |= AxesMask::LIN_Y;
    }
    if follow.rotation_axes.contains(&Turn::Rotation) {
        mask |= AxesMask::ANG_Z;
    }
    let pd = PdController {
        lin_kp: gain(follow.position_gain),
        lin_kd: gain(follow.velocity_gain),
        ang_kp: scalar::real(follow.rotation_gain),
        ang_kd: scalar::real(follow.spin_gain),
        axes: mask,
    };
    match follow.kind {
        FollowKind::Pid => {
            let mut pid = PidController::new(0.0, 0.0, 0.0, mask);
            pid.pd = pd;
            pid.lin_ki = gain(follow.integral_gain);
            pid.ang_ki = scalar::real(follow.rotation_integral_gain);
            FollowRef2d::Pid(pid)
        }
        FollowKind::Pd => FollowRef2d::Pd(pd),
    }
}

impl FollowRef2d {
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

/// The system, before the step it feeds, and the component.
pub(crate) fn build(reg: &mut Registry<'_>) {
    reg.add_system(Stage::FixedUpdate, follow_system);
    register_follow2d_component(reg);
}

fn follow_system(eng: &Engine, _dt: f32) {
    if eng.paused() {
        return;
    }
    let followers: Vec<(Entity, Follow2d)> = {
        let world = eng.world();
        let mut query = world.query::<(Entity, &Follow2d)>();
        query.iter().map(|(e, f)| (e, f.clone())).collect()
    };
    let dt: Real = fixed_dt();
    for (entity, follow) in followers {
        let Some(target) = node_at(eng, entity, &follow.target) else {
            continue;
        };
        let Ok(goal) = super::node_pose_2d(eng, target) else {
            continue;
        };
        let wanted = RigidBodyVelocity {
            linvel: scalar::v2a(follow.target_linear_velocity),
            angvel: scalar::real(follow.target_angular_velocity),
        };
        let state = eng.resource::<PhysicsState2d>();
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
            FollowRef2d::Pd(pd) => pd.rigid_body_correction(body, goal, wanted),
            FollowRef2d::Pid(pid) => pid.rigid_body_correction(dt, body, goal, wanted),
        };
        state.follows.insert(entity, controller);
        if let Some(body) = state.world.bodies.get_mut(handle) {
            let (linvel, angvel) = (body.linvel() + change.linvel, body.angvel() + change.angvel);
            body.set_linvel(linvel, true);
            body.set_angvel(angvel, true);
        }
    }
}

/// `follow2d`: what was asked, held on the node; the controller a step keeps
/// is in [`PhysicsState2d::follows`].
fn register_follow2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Follow2d::NAME,
        ComponentDef {
            tags: &[
                balaur_core::components::tag::DIM_2D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &[c::BODY_2D],
            remove: Box::new(|eng, entity| {
                let _ = eng.remove_one::<Follow2d>(entity);
                let state = eng.resource::<PhysicsState2d>();
                state.borrow_mut().follows.swap_remove(&entity);
                Ok(())
            }),
            ..ComponentDef::of::<Follow2d>(None)
        },
    );
}

pub(crate) fn install_follow2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "reset_follow",
        &[c::FOLLOW_2D],
        "",
        "Forget what a `pid` follow summed, so it pulls from now as if it had just started.",
    )]);
    m.function("reset_follow", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let state = eng.resource::<PhysicsState2d>();
        if let Some(FollowRef2d::Pid(pid)) = state.borrow_mut().follows.get_mut(&entity) {
            pid.reset_integrals();
        }
        Ok(())
    });
}
