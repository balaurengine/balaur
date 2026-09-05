//! `vehicle3d` and `wheel3d`: a car, in rapier's ray-cast vehicle model.
//!
//! Not a stack of joints. Each wheel is a ray from the chassis with a spring
//! along it, which is how driving games have modelled cars since before
//! physics engines had joints worth using: it never jams, never tunnels, and
//! tunes with numbers a designer can reason about.
//!
//! The chassis is the node with the `vehicle3d`; each child with a `wheel3d`
//! is a wheel, and its position on the chassis is where its ray starts.

use crate::rapier3d::control::{DynamicRayCastVehicleController, WheelTuning};
use crate::rapier3d::prelude::QueryFilter;
use crate::scalar::{self, Real, Vector};
use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::scene::{Children, Transform};
use balaur_core::{Engine, Stage, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::vocabulary::{self as v, component as c, keys as k, map};
use crate::{FIXED_DT, PhysicsState};

/// The chassis settings, held on the node like a character's.
pub struct Vehicle3d(pub toml::Value);

/// One wheel's settings, held on its own node.
pub struct Wheel3d(pub toml::Value);

pub(crate) fn build(reg: &mut Registry<'_>) {
    // After the physics step: a vehicle reads the world the step just wrote,
    // and writes forces the next step will integrate.
    reg.add_system(Stage::FixedUpdate, drive_system);
}

/// Rebuild the controller for a vehicle whose wheels changed, then step it.
///
/// Rebuilding rather than keeping one across frames is what makes a wheel
/// added or removed mid-game work at all, and rapier's controller is a
/// `Vec<Wheel>` and a handle — cheap enough that the alternative would be
/// caching for its own sake.
fn drive_system(eng: &Engine, _dt: f32) {
    let vehicles: Vec<Entity> = {
        let world = eng.world();
        let mut query = world.query::<(Entity, &Vehicle3d)>();
        query.iter().map(|(entity, _)| entity).collect()
    };
    for chassis in vehicles {
        if let Err(why) = drive_one(eng, chassis) {
            tracing::warn!("vehicle3d: {why:#}");
        }
    }
}

fn drive_one(eng: &Engine, chassis: Entity) -> Result<()> {
    let (params, wheels) = {
        let world = eng.world();
        let params = {
            let vehicle = world
                .get::<&Vehicle3d>(chassis)
                .map_err(|_| anyhow!("no vehicle3d"))?;
            vehicle.0.clone()
        };
        let mut wheels = Vec::new();
        if let Ok(children) = world.get::<&Children>(chassis) {
            for child in &children.0 {
                let Ok(wheel) = world.get::<&Wheel3d>(*child) else {
                    continue;
                };
                let at = world
                    .get::<&Transform>(*child)
                    .map_or(glamx::Vec3::ZERO, |t| t.position);
                wheels.push((*child, wheel.0.clone(), at));
            }
        }
        (params, wheels)
    };
    if wheels.is_empty() {
        return Ok(());
    }
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let handle = *state
        .bodies
        .get(&chassis)
        .ok_or_else(|| anyhow!("a vehicle3d needs a body3d on the same node"))?;
    let mut controller = DynamicRayCastVehicleController::new(handle);
    controller.index_up_axis = v::f(&params, k::UP_AXIS, 1.0).clamp(0.0, 2.0) as usize;
    controller.index_forward_axis = v::f(&params, k::FORWARD_AXIS, 2.0).clamp(0.0, 2.0) as usize;
    for (entity, wheel_params, at) in &wheels {
        let real = |key: &str, default: f32| scalar::real(v::f(wheel_params, key, default));
        let tuning = WheelTuning {
            suspension_stiffness: real(k::STIFFNESS, 30.0),
            suspension_compression: real(k::COMPRESSION, 0.82),
            suspension_damping: real(k::DAMPING, 0.88),
            max_suspension_travel: real(k::MAX_TRAVEL, 5.0),
            side_friction_stiffness: real(k::SIDE_FRICTION, 1.0),
            friction_slip: real(k::FRICTION_SLIP, 10.5),
            max_suspension_force: real(k::MAX_FORCE, 6000.0),
        };
        let direction = scalar::v3a(v::vec3(wheel_params, k::DIRECTION, [0.0, -1.0, 0.0]));
        let axle = scalar::v3a(v::vec3(wheel_params, k::AXLE, [-1.0, 0.0, 0.0]));
        let wheel = controller.add_wheel(
            scalar::v3(at.x, at.y, at.z),
            direction,
            axle,
            real(k::REST_LENGTH, 0.3),
            real(k::RADIUS, 0.4).max(0.01),
            &tuning,
        );
        // The inputs a script set since the last step, kept per wheel node so
        // they survive the rebuild.
        if let Some(input) = state.wheel_inputs.get(entity) {
            wheel.engine_force = input.engine_force;
            wheel.brake = input.brake;
            wheel.steering = input.steering;
            wheel.rotation = input.rotation;
        }
    }
    let dispatcher = state.world.narrow_phase.query_dispatcher();
    let queries = state.world.broad_phase.as_query_pipeline_mut(
        dispatcher,
        &mut state.world.bodies,
        &mut state.world.colliders,
        QueryFilter::default().exclude_rigid_body(handle),
    );
    controller.update_vehicle(scalar::real(FIXED_DT), queries);
    // Keep what the step worked out, so the wheel's rotation and its ground
    // contact are readable and survive into the next rebuild.
    for ((entity, _, _), wheel) in wheels.iter().zip(controller.wheels()) {
        let input = state.wheel_inputs.entry(*entity).or_default();
        input.rotation = wheel.rotation;
        input.suspension_force = wheel.wheel_suspension_force;
        input.grounded = wheel.raycast_info().is_in_contact;
    }
    Ok(())
}

/// What a script sets on a wheel, and what the last step left there.
///
/// Kept beside the world rather than in it: rapier's controller is rebuilt
/// every step, and these are the four numbers that must not be.
#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct WheelInput {
    pub engine_force: Real,
    pub brake: Real,
    pub steering: Real,
    pub rotation: Real,
    pub suspension_force: Real,
    pub grounded: bool,
}

pub(crate) fn install_vehicle_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_engine_force", &[c::WHEEL_3D], "", "How hard this wheel drives, in newtons; negative reverses."),
        ("set_brake", &[c::WHEEL_3D], "", "How hard this wheel brakes."),
        ("set_steering", &[c::WHEEL_3D], "", "Turn this wheel, in radians."),
        ("wheel_state", &[c::WHEEL_3D], "", "What the last step did with this wheel: `#{ rotation, suspension_force, grounded, engine_force, brake, steering }`."),
        ("vehicle_speed", &[c::VEHICLE_3D], "", "How fast the chassis is going along its forward axis, in units per second."),
    ]);
    m.function(
        "set_engine_force",
        |eng: &Engine, (node, force): (NodeId, f32)| {
            with_wheel(eng, node, |input| input.engine_force = scalar::real(force))
        },
    );
    m.function("set_brake", |eng: &Engine, (node, brake): (NodeId, f32)| {
        with_wheel(eng, node, |input| {
            input.brake = scalar::real(brake.max(0.0));
        })
    });
    m.function(
        "set_steering",
        |eng: &Engine, (node, angle): (NodeId, f32)| {
            with_wheel(eng, node, |input| input.steering = scalar::real(angle))
        },
    );
    m.function("wheel_state", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let input = state.wheel_inputs.get(&entity).copied().unwrap_or_default();
        Ok(map([
            (k::ROTATION, Value::Num(f64::from(input.rotation))),
            (
                k::SUSPENSION_FORCE,
                Value::Num(f64::from(input.suspension_force)),
            ),
            (k::GROUNDED, Value::Bool(input.grounded)),
            (k::ENGINE_FORCE, Value::Num(f64::from(input.engine_force))),
            (k::BRAKE, Value::Num(f64::from(input.brake))),
            (k::STEERING, Value::Num(f64::from(input.steering))),
        ]))
    });
    m.function("vehicle_speed", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        // The chassis's own `forward_axis`, not z: a car built along x would
        // otherwise read the speed it is sliding sideways at.
        let axis = {
            let world = eng.world();
            let vehicle = world
                .get::<&Vehicle3d>(entity)
                .map_err(|_| anyhow!("node has no vehicle3d"))?;
            forward_axis(&vehicle.0)
        };
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let handle = *state
            .bodies
            .get(&entity)
            .ok_or_else(|| anyhow!("a vehicle3d needs a body3d on the same node"))?;
        let body = state
            .world
            .bodies
            .get(handle)
            .ok_or_else(|| anyhow!("this node's body is gone: the node was freed"))?;
        Ok(body.linvel().dot(body.rotation() * axis))
    });
}

/// Which of the chassis's own axes points forward, as the index rapier's
/// controller takes and `vehicle_speed` measures along.
fn forward_axis(params: &toml::Value) -> Vector {
    match v::f(params, k::FORWARD_AXIS, 2.0).clamp(0.0, 2.0) as usize {
        0 => Vector::X,
        1 => Vector::Y,
        _ => Vector::Z,
    }
}

fn with_wheel(eng: &Engine, node: NodeId, f: impl FnOnce(&mut WheelInput)) -> Result<()> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    f(state.wheel_inputs.entry(entity).or_default());
    Ok(())
}

pub(crate) fn register_vehicle_components(reg: &mut Registry<'_>) {
    reg.register_component(
        c::VEHICLE_3D,
        ComponentDef {
            doc: "Makes this node's body a car chassis, driven by the `wheel3d` children under it. Rapier casts a ray down from each wheel and pushes the chassis along a spring, which is how driving games model cars: it never jams and never tunnels.",
            schema: ComponentDef::parse_schema(
                c::VEHICLE_3D,
                &v::schema(&[
                    (k::UP_AXIS, r#"{ type = "float", default = 1.0, min = 0.0, max = 2.0, description = "Which of the chassis's own axes points up: 0 for x, 1 for y, 2 for z" }"#),
                    (k::FORWARD_AXIS, r#"{ type = "float", default = 2.0, min = 0.0, max = 2.0, description = "Which of the chassis's own axes points forward" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
            expects: &[c::BODY_3D],
            apply: Box::new(|eng, entity, params| {
                let _ = eng.world_mut().insert_one(entity, Vehicle3d(params.clone()));
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Vehicle3d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let vehicle = world.get::<&Vehicle3d>(entity).ok()?;
                Some(vehicle.0.clone())
            }),
        },
    );
    reg.register_component(
        c::WHEEL_3D,
        ComponentDef {
            doc: "One wheel of the `vehicle3d` above it. Where the node sits on the chassis is where the wheel's ray starts; the rest is suspension tuning. Drive it with `physics3d.set_engine_force`, `set_brake` and `set_steering`.",
            schema: ComponentDef::parse_schema(
                c::WHEEL_3D,
                &v::schema(&[
                    (k::RADIUS, r#"{ type = "float", default = 0.4, min = 0.01, description = "The wheel's radius, which is how far off the ground it holds the ray's end" }"#),
                    (k::REST_LENGTH, r#"{ type = "float", default = 0.3, min = 0.0, description = "How long the suspension is with no weight on it" }"#),
                    (k::DIRECTION, r#"{ type = "vec3", default = [0.0, -1.0, 0.0], description = "Which way the suspension pushes, in the chassis's own space: down" }"#),
                    (k::AXLE, r#"{ type = "vec3", default = [-1.0, 0.0, 0.0], description = "The axle the wheel turns about, in the chassis's own space" }"#),
                    (k::STIFFNESS, r#"{ type = "float", default = 30.0, min = 0.0, description = "Spring stiffness: higher is a stiffer, twitchier car" }"#),
                    (k::COMPRESSION, r#"{ type = "float", default = 0.82, min = 0.0, description = "Damping while the suspension is being squashed" }"#),
                    (k::DAMPING, r#"{ type = "float", default = 0.88, min = 0.0, description = "Damping while the suspension is coming back" }"#),
                    (k::MAX_TRAVEL, r#"{ type = "float", default = 5.0, min = 0.0, description = "How far the suspension may move in total" }"#),
                    (k::FRICTION_SLIP, r#"{ type = "float", default = 10.5, min = 0.0, description = "Grip along the wheel's rolling direction; lower slides more" }"#),
                    (k::SIDE_FRICTION, r#"{ type = "float", default = 1.0, min = 0.0, description = "Grip sideways: what stops the car sliding out of a corner" }"#),
                    (k::MAX_FORCE, r#"{ type = "float", default = 6000.0, min = 0.0, description = "The most force this suspension may push the chassis with" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::DIM_3D, balaur_core::components::tag::PHYSICS],
            expects: &[c::VEHICLE_3D],
            apply: Box::new(|eng, entity, params| {
                let _ = eng.world_mut().insert_one(entity, Wheel3d(params.clone()));
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Wheel3d>(entity);
                let state = eng.resource::<PhysicsState>();
                state.borrow_mut().wheel_inputs.swap_remove(&entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let wheel = world.get::<&Wheel3d>(entity).ok()?;
                Some(wheel.0.clone())
            }),
        },
    );
}
