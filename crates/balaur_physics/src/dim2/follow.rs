//! `follow2d`: the 2D twin of `follow3d`.

use crate::rapier2d::control::{PdController, PidController};
use crate::rapier2d::dynamics::{AxesMask, RigidBodyVelocity};
use crate::scalar::{self, Real};
use balaur_core::components::{ComponentDef, as_node, prop_bool, prop_f32, prop_str, prop_vec2};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Stage, entity_of, fixed_dt};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use super::PhysicsState2d;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

/// The settings, held on the node.
pub struct Follow2d(pub toml::Value);

/// As [`crate::follow::FollowRef3d`], in the plane.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum FollowRef2d {
    Pd(PdController),
    Pid(PidController),
}

fn controller_of(params: &toml::Value) -> FollowRef2d {
    let gain = |key: &str| scalar::v2a(prop_vec2(params, key));
    let named = |word: &str| {
        params
            .get(k::TRANSLATION_AXES)
            .and_then(toml::Value::as_array)
            .is_some_and(|words| words.iter().any(|named| named.as_str() == Some(word)))
    };
    let mut mask = AxesMask::empty();
    if named(w::X) {
        mask |= AxesMask::LIN_X;
    }
    if named(w::Y) {
        mask |= AxesMask::LIN_Y;
    }
    if prop_bool(params, k::FOLLOW_ROTATION) {
        mask |= AxesMask::ANG_Z;
    }
    let pd = PdController {
        lin_kp: gain(k::POSITION_GAIN),
        lin_kd: gain(k::VELOCITY_GAIN),
        ang_kp: scalar::real(prop_f32(params, k::ROTATION_GAIN)),
        ang_kd: scalar::real(prop_f32(params, k::SPIN_GAIN)),
        axes: mask,
    };
    if prop_str(params, k::KIND) == w::PID {
        let mut pid = PidController::new(0.0, 0.0, 0.0, mask);
        pid.pd = pd;
        pid.lin_ki = gain(k::INTEGRAL_GAIN);
        pid.ang_ki = scalar::real(prop_f32(params, k::ROTATION_INTEGRAL_GAIN));
        FollowRef2d::Pid(pid)
    } else {
        FollowRef2d::Pd(pd)
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
    let followers: Vec<(Entity, toml::Value)> = {
        let world = eng.world();
        let mut query = world.query::<(Entity, &Follow2d)>();
        query.iter().map(|(e, f)| (e, f.0.clone())).collect()
    };
    let dt: Real = fixed_dt();
    for (entity, params) in followers {
        let Some(target) = as_node(eng, entity, params.get(k::TARGET)) else {
            continue;
        };
        let Ok(goal) = super::node_pose_2d(eng, target) else {
            continue;
        };
        let wanted = RigidBodyVelocity {
            linvel: scalar::v2a(prop_vec2(&params, k::TARGET_LINEAR_VELOCITY)),
            angvel: scalar::real(prop_f32(&params, k::TARGET_ANGULAR_VELOCITY)),
        };
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let Some(&handle) = state.bodies.get(&entity) else {
            continue;
        };
        let fresh = controller_of(&params);
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

fn schema() -> String {
    let kinds = v::options(w::FOLLOW_KINDS);
    let axes = v::options(w::LOCK_AXES_2D);
    v::schema(&[
        (
            k::KIND,
            &format!(
                r#"{{ type = "enum", default = "{}", options = [{kinds}], description = "Rapier's proportional-derivative controller, or the same with an integral that keeps pulling against a steady push" }}"#,
                w::PD
            ),
        ),
        (
            k::TARGET,
            r#"{ type = "node", default = "", description = "The node whose pose the body is pulled to; empty pulls nothing" }"#,
        ),
        (
            k::TARGET_LINEAR_VELOCITY,
            r#"{ type = "vec2", default = [0.0, 0.0], description = "The velocity the body is pulled to match, in world units per second" }"#,
        ),
        (
            k::TARGET_ANGULAR_VELOCITY,
            r#"{ type = "float", default = 0.0, unit = "degrees", description = "The spin the body is pulled to match, radians a second in the file" }"#,
        ),
        (
            k::POSITION_GAIN,
            r#"{ type = "vec2", default = [60.0, 60.0], description = "How hard a gap in position pulls, per axis: about one over the fixed step closes it in one step" }"#,
        ),
        (
            k::VELOCITY_GAIN,
            r#"{ type = "vec2", default = [0.8, 0.8], description = "How much of a gap in velocity a step corrects, per axis: 0 none, 1 all of it" }"#,
        ),
        (
            k::INTEGRAL_GAIN,
            r#"{ type = "vec2", default = [1.0, 1.0], description = "How hard the gap in position summed over time pulls, per axis, with `pid`" }"#,
        ),
        (
            k::ROTATION_GAIN,
            r#"{ type = "float", default = 60.0, description = "How hard a gap in rotation pulls" }"#,
        ),
        (
            k::SPIN_GAIN,
            r#"{ type = "float", default = 0.8, description = "How much of a gap in spin a step corrects" }"#,
        ),
        (
            k::ROTATION_INTEGRAL_GAIN,
            r#"{ type = "float", default = 1.0, description = "How hard the gap in rotation summed over time pulls, with `pid`" }"#,
        ),
        (
            k::TRANSLATION_AXES,
            &format!(
                r#"{{ type = "flags", default = ["{}", "{}"], options = [{axes}], description = "The axes the body is pulled along" }}"#,
                w::X,
                w::Y
            ),
        ),
        (
            k::FOLLOW_ROTATION,
            r#"{ type = "bool", default = true, description = "Whether the body is turned to the target's rotation too" }"#,
        ),
    ])
}

fn register_follow2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        c::FOLLOW_2D,
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "Pulls the node's `body2d` towards the pose of `target` every fixed step, with rapier's PD or PID controller. `node.follow2d.reset_follow()` forgets what a PID summed.",
            schema: ComponentDef::parse_schema(c::FOLLOW_2D, &schema()),
            tags: &[balaur_core::components::tag::DIM_2D, balaur_core::components::tag::PHYSICS],
            expects: &[c::BODY_2D],
            apply: Box::new(|eng, entity, params| {
                let _ = eng.world_mut().insert_one(entity, Follow2d(params.clone()));
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Follow2d>(entity);
                let state = eng.resource::<PhysicsState2d>();
                state.borrow_mut().follows.swap_remove(&entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let follow = world.get::<&Follow2d>(entity).ok()?;
                Some(follow.0.clone())
            }),
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
