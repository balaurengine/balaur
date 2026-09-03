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
use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::scene::{Children, Transform};
use balaur_core::{entity_of, App, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::vocabulary::{self as v, map};
use crate::{PhysicsState, FIXED_DT};

/// The chassis settings, held on the node like a character's.
pub struct Vehicle3d(pub toml::Value);

/// One wheel's settings, held on its own node.
pub struct Wheel3d(pub toml::Value);

pub(crate) fn build(app: &mut App) {
    // After the physics step: a vehicle reads the world the step just wrote,
    // and writes forces the next step will integrate.
    app.add_system(Stage::FixedUpdate, drive_system);
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
    controller.index_up_axis = v::f(&params, "up_axis", 1.0).clamp(0.0, 2.0) as usize;
    controller.index_forward_axis = v::f(&params, "forward_axis", 2.0).clamp(0.0, 2.0) as usize;
    for (entity, wheel_params, at) in &wheels {
        let real = |key: &str, default: f32| scalar::real(v::f(wheel_params, key, default));
        let tuning = WheelTuning {
            suspension_stiffness: real("stiffness", 30.0),
            suspension_compression: real("compression", 0.82),
            suspension_damping: real("damping", 0.88),
            max_suspension_travel: real("max_travel", 5.0),
            side_friction_stiffness: real("side_friction", 1.0),
            friction_slip: real("friction_slip", 10.5),
            max_suspension_force: real("max_force", 6000.0),
        };
        let direction = scalar::v3a(v::vec3(wheel_params, "direction", [0.0, -1.0, 0.0]));
        let axle = scalar::v3a(v::vec3(wheel_params, "axle", [-1.0, 0.0, 0.0]));
        let wheel = controller.add_wheel(
            scalar::v3(at.x, at.y, at.z),
            direction,
            axle,
            real("rest_length", 0.3),
            real("radius", 0.4).max(0.01),
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
#[derive(Clone, Copy, Default)]
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
        ("set_engine_force", &["wheel3d"], "", "How hard this wheel drives, in newtons; negative reverses."),
        ("set_brake", &["wheel3d"], "", "How hard this wheel brakes."),
        ("set_steering", &["wheel3d"], "", "Turn this wheel, in radians."),
        ("wheel_state", &["wheel3d"], "", "What the last step did with this wheel: `#{ rotation, suspension_force, grounded, engine_force, brake, steering }`."),
        ("vehicle_speed", &["vehicle3d"], "", "How fast the chassis is going along its forward axis, in units per second."),
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
            ("rotation", Value::Num(f64::from(input.rotation))),
            (
                "suspension_force",
                Value::Num(f64::from(input.suspension_force)),
            ),
            ("grounded", Value::Bool(input.grounded)),
            ("engine_force", Value::Num(f64::from(input.engine_force))),
            ("brake", Value::Num(f64::from(input.brake))),
            ("steering", Value::Num(f64::from(input.steering))),
        ]))
    });
    m.function("vehicle_speed", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let handle = *state
            .bodies
            .get(&entity)
            .ok_or_else(|| anyhow!("a vehicle3d needs a body3d on the same node"))?;
        let body = &state.world.bodies[handle];
        let forward = body.rotation() * Vector::Z;
        Ok(body.linvel().dot(forward))
    });
}

fn with_wheel(eng: &Engine, node: NodeId, f: impl FnOnce(&mut WheelInput)) -> Result<()> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    f(state.wheel_inputs.entry(entity).or_default());
    Ok(())
}

pub(crate) fn register_vehicle_components(app: &mut App) {
    app.register_component(
        "vehicle3d",
        ComponentDef {
            doc: "Makes this node's body a car chassis, driven by the `wheel3d` children under it. Rapier casts a ray down from each wheel and pushes the chassis along a spring, which is how driving games model cars: it never jams and never tunnels.",
            schema: ComponentDef::parse_schema(
                "vehicle3d",
                r#"up_axis = { type = "float", default = 1.0, min = 0.0, max = 2.0, description = "Which of the chassis's own axes points up: 0 for x, 1 for y, 2 for z" }
forward_axis = { type = "float", default = 2.0, min = 0.0, max = 2.0, description = "Which of the chassis's own axes points forward" }"#,
            ),
            tags: &["3d", "physics"],
            expects: &["body3d"],
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
    app.register_component(
        "wheel3d",
        ComponentDef {
            doc: "One wheel of the `vehicle3d` above it. Where the node sits on the chassis is where the wheel's ray starts; the rest is suspension tuning. Drive it with `physics3d.set_engine_force`, `set_brake` and `set_steering`.",
            schema: ComponentDef::parse_schema(
                "wheel3d",
                r#"radius = { type = "float", default = 0.4, min = 0.01, description = "The wheel's radius, which is how far off the ground it holds the ray's end" }
rest_length = { type = "float", default = 0.3, min = 0.0, description = "How long the suspension is with no weight on it" }
direction = { type = "vec3", default = [0.0, -1.0, 0.0], description = "Which way the suspension pushes, in the chassis's own space: down" }
axle = { type = "vec3", default = [-1.0, 0.0, 0.0], description = "The axle the wheel turns about, in the chassis's own space" }
stiffness = { type = "float", default = 30.0, min = 0.0, description = "Spring stiffness: higher is a stiffer, twitchier car" }
compression = { type = "float", default = 0.82, min = 0.0, description = "Damping while the suspension is being squashed" }
damping = { type = "float", default = 0.88, min = 0.0, description = "Damping while the suspension is coming back" }
max_travel = { type = "float", default = 5.0, min = 0.0, description = "How far the suspension may move in total" }
friction_slip = { type = "float", default = 10.5, min = 0.0, description = "Grip along the wheel's rolling direction; lower slides more" }
side_friction = { type = "float", default = 1.0, min = 0.0, description = "Grip sideways: what stops the car sliding out of a corner" }
max_force = { type = "float", default = 6000.0, min = 0.0, description = "The most force this suspension may push the chassis with" }"#,
            ),
            tags: &["3d", "physics"],
            expects: &["vehicle3d"],
            apply: Box::new(|eng, entity, params| {
                let _ = eng.world_mut().insert_one(entity, Wheel3d(params.clone()));
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Wheel3d>(entity);
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
