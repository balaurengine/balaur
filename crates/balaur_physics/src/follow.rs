//! `follow3d`: rapier's PD or PID controller pulling a body towards a node.
//!
//! A body has no target of its own, and a joint motor drives one body
//! against another; this turns the gap between a body and a target node into
//! a velocity change every fixed step, before the solver runs.

use crate::rapier3d::control::{PdController, PidController};
use crate::rapier3d::dynamics::{AxesMask, RigidBodyVelocity};
use crate::scalar::{self, Real};
use balaur_core::components::{ComponentDef, as_node, prop_str, prop_vec3};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, GlobalTransform, Stage, entity_of, fixed_dt};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::PhysicsState3d;
use crate::vocabulary::{self as v, component as c, keys as k, words as w};

/// The settings, held on the node.
pub struct Follow3d(pub toml::Value);

/// One body's controller, kept across steps: a PID's integrals change every
/// step, so the snapshot carries it.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum FollowRef3d {
    Pd(PdController),
    Pid(PidController),
}

/// The axes a flags property names, as rapier's mask.
fn axes(params: &toml::Value, key: &str, [x, y, z]: [AxesMask; 3]) -> AxesMask {
    let named = |word: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_array)
            .is_some_and(|words| words.iter().any(|named| named.as_str() == Some(word)))
    };
    let mut mask = AxesMask::empty();
    for (word, bit) in [(w::X, x), (w::Y, y), (w::Z, z)] {
        if named(word) {
            mask |= bit;
        }
    }
    mask
}

/// The controller the params describe, with a PID's integrals at zero.
fn controller_of(params: &toml::Value) -> FollowRef3d {
    let gain = |key: &str| scalar::v3a(prop_vec3(params, key));
    let mask = axes(
        params,
        k::TRANSLATION_AXES,
        [AxesMask::LIN_X, AxesMask::LIN_Y, AxesMask::LIN_Z],
    ) | axes(
        params,
        k::ROTATION_AXES,
        [AxesMask::ANG_X, AxesMask::ANG_Y, AxesMask::ANG_Z],
    );
    let pd = PdController {
        lin_kp: gain(k::POSITION_GAIN),
        lin_kd: gain(k::VELOCITY_GAIN),
        ang_kp: gain(k::ROTATION_GAIN),
        ang_kd: gain(k::SPIN_GAIN),
        axes: mask,
    };
    if prop_str(params, k::KIND) == w::PID {
        let mut pid = PidController::new(0.0, 0.0, 0.0, mask);
        pid.pd = pd;
        pid.lin_ki = gain(k::INTEGRAL_GAIN);
        pid.ang_ki = gain(k::ROTATION_INTEGRAL_GAIN);
        FollowRef3d::Pid(pid)
    } else {
        FollowRef3d::Pd(pd)
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
    let followers: Vec<(Entity, toml::Value)> = {
        let world = eng.world();
        let mut query = world.query::<(Entity, &Follow3d)>();
        query.iter().map(|(e, f)| (e, f.0.clone())).collect()
    };
    let dt: Real = fixed_dt();
    for (entity, params) in followers {
        let Some(target) = as_node(eng, entity, params.get(k::TARGET)) else {
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
            linvel: scalar::v3a(prop_vec3(&params, k::TARGET_LINEAR_VELOCITY)),
            angvel: scalar::v3a(prop_vec3(&params, k::TARGET_ANGULAR_VELOCITY)),
        };
        let state = eng.resource::<PhysicsState3d>();
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

/// `follow3d`'s schema, shared in shape with `follow2d`.
fn schema() -> String {
    let kinds = v::options(w::FOLLOW_KINDS);
    let axes = v::options(w::LOCK_AXES);
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
            r#"{ type = "vec3", default = [0.0, 0.0, 0.0], description = "The velocity the body is pulled to match, in world units per second" }"#,
        ),
        (
            k::TARGET_ANGULAR_VELOCITY,
            r#"{ type = "vec3", default = [0.0, 0.0, 0.0], unit = "degrees", description = "The spin the body is pulled to match about each axis, radians a second in the file" }"#,
        ),
        (
            k::POSITION_GAIN,
            r#"{ type = "vec3", default = [60.0, 60.0, 60.0], description = "How hard a gap in position pulls, per axis: about one over the fixed step closes it in one step" }"#,
        ),
        (
            k::VELOCITY_GAIN,
            r#"{ type = "vec3", default = [0.8, 0.8, 0.8], description = "How much of a gap in velocity a step corrects, per axis: 0 none, 1 all of it" }"#,
        ),
        (
            k::INTEGRAL_GAIN,
            r#"{ type = "vec3", default = [1.0, 1.0, 1.0], description = "How hard the gap in position summed over time pulls, per axis, with `pid`" }"#,
        ),
        (
            k::ROTATION_GAIN,
            r#"{ type = "vec3", default = [60.0, 60.0, 60.0], description = "How hard a gap in rotation pulls, per axis" }"#,
        ),
        (
            k::SPIN_GAIN,
            r#"{ type = "vec3", default = [0.8, 0.8, 0.8], description = "How much of a gap in spin a step corrects, per axis" }"#,
        ),
        (
            k::ROTATION_INTEGRAL_GAIN,
            r#"{ type = "vec3", default = [1.0, 1.0, 1.0], description = "How hard the gap in rotation summed over time pulls, per axis, with `pid`" }"#,
        ),
        (
            k::TRANSLATION_AXES,
            &format!(
                r#"{{ type = "flags", default = ["{}", "{}", "{}"], options = [{axes}], description = "The axes the body is pulled along" }}"#,
                w::X,
                w::Y,
                w::Z
            ),
        ),
        (
            k::ROTATION_AXES,
            &format!(
                r#"{{ type = "flags", default = ["{}", "{}", "{}"], options = [{axes}], description = "The axes the body is turned about" }}"#,
                w::X,
                w::Y,
                w::Z
            ),
        ),
    ])
}

fn register_follow_component(reg: &mut Registry<'_>) {
    reg.register_component(
        c::FOLLOW_3D,
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "Pulls the node's `body3d` towards the pose of `target` every fixed step, with rapier's PD or PID controller. `node.follow3d.reset_follow()` forgets what a PID summed.",
            schema: ComponentDef::parse_schema(c::FOLLOW_3D, &schema()),
            tags: &[balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
            expects: &[c::BODY_3D],
            apply: Box::new(|eng, entity, params| {
                let _ = eng.world_mut().insert_one(entity, Follow3d(params.clone()));
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Follow3d>(entity);
                let state = eng.resource::<PhysicsState3d>();
                state.borrow_mut().follows.swap_remove(&entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let follow = world.get::<&Follow3d>(entity).ok()?;
                Some(follow.0.clone())
            }),
        },
    );
}

pub(crate) fn install_follow_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "reset_follow",
        &[c::FOLLOW_3D],
        "",
        "Forget what a `pid` follow summed, so it pulls from now as if it had just started.",
    )]);
    m.function("reset_follow", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let state = eng.resource::<PhysicsState3d>();
        if let Some(FollowRef3d::Pid(pid)) = state.borrow_mut().follows.get_mut(&entity) {
            pid.reset_integrals();
        }
        Ok(())
    });
}
