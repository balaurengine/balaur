//! `vehicle3d` and `wheel3d`: a car, in rapier's ray-cast vehicle model.
//!
//! Not a stack of joints. Each wheel is a ray from the chassis with a spring
//! along it, which is how driving games have modelled cars since before
//! physics engines had joints worth using: it never jams, never tunnels, and
//! tunes with numbers a designer can reason about.
//!
//! The chassis is the node with the `vehicle3d`; each child with a `wheel3d`
//! is a wheel, and its position on the chassis is where its ray starts.

use crate::rapier3d::control::{DynamicRayCastVehicleController, Wheel, WheelTuning};
use crate::rapier3d::prelude::{
    Group, InteractionGroups, InteractionTestMode, QueryFilter, QueryFilterFlags, RigidBodyHandle,
};
use crate::scalar::{self, Real, Vector};
use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::Component;
use balaur_core::hecs::Entity;
use balaur_core::scene::{Children, Transform};
use balaur_core::{Engine, Stage, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

use crate::PhysicsState3d;
use crate::properties::{ChassisAxis, SweepIgnore};
use crate::vocabulary::{component as c, keys as k};
use balaur_core::fixed_dt;
use balaur_core::layers::Layer;

/// Makes the node's `body3d` a raycast vehicle chassis, driven by the `wheel3d` children under it. `forward_axis` and `up_axis` orient it.
#[derive(Clone, Component)]
#[component(name = "vehicle3d", on_remove = vehicle3d_removed)]
pub struct Vehicle3d {
    /// Which of the chassis's own axes points forward, either way along it: a negative axis drives and steers the other way round, and reads speed the other way
    #[property(default = ChassisAxis::Z)]
    pub forward_axis: ChassisAxis,
    /// Which of the chassis's own axes points up, either way along it
    #[property(default = ChassisAxis::Y)]
    pub up_axis: ChassisAxis,
    /// The collision layers the wheels' rays hit; empty hits every layer
    #[property(flags, group = "filtering")]
    pub collision_mask: Vec<Layer>,
    /// What the wheels' rays pass through: static takes colliders with no body too. The chassis's own body is never hit
    #[property(flags, default = vec![SweepIgnore::Sensors], group = "filtering")]
    pub ignore: Vec<SweepIgnore>,
}

/// One wheel of the `vehicle3d` above it; the node's position on the chassis is where its ray starts. `physics3d.set_engine_force`, `set_brake` and `set_steering` drive it.
#[derive(Clone, Component)]
#[component(name = "wheel3d", on_remove = wheel3d_removed)]
pub struct Wheel3d {
    /// The wheel's radius, which is how far off the ground it holds the ray's end
    #[property(default = 0.4, min = 0.01)]
    pub radius: f32,
    /// The axle the wheel turns about, in the chassis's own space
    #[property(default = [-1.0, 0.0, 0.0])]
    pub axle: [f32; 3],
    /// Grip along the wheel's rolling direction; lower slides more
    #[property(default = 10.5, min = 0.0)]
    pub friction_slip: f32,
    /// Grip sideways: what stops the car sliding out of a corner
    #[property(default = 1.0, min = 0.0)]
    pub side_friction: f32,
    /// How long the suspension is with no weight on it
    #[property(default = 0.3, min = 0.0, group = "suspension")]
    pub rest_length: f32,
    /// Spring stiffness, scaled by the chassis's mass: higher is a stiffer, twitchier car
    #[property(default = 30.0, min = 0.0, group = "suspension")]
    pub suspension_stiffness: f32,
    /// Damping while the suspension is being squashed
    #[property(default = 0.82, min = 0.0, group = "suspension")]
    pub damping_compression: f32,
    /// Damping while the suspension is coming back
    #[property(default = 0.88, min = 0.0, group = "suspension")]
    pub damping_relaxation: f32,
    /// How far the suspension may move either side of its rest length
    #[property(default = 5.0, min = 0.0, group = "suspension")]
    pub suspension_travel: f32,
    /// The most force this suspension may push the chassis with
    #[property(default = 6000.0, min = 0.0, group = "suspension")]
    pub suspension_max_force: f32,
    /// Which way the suspension pushes, in the chassis's own space: down
    #[property(default = [0.0, -1.0, 0.0], group = "suspension")]
    pub suspension_direction: [f32; 3],
}

/// A chassis's rapier controller, kept across steps, and the wheel nodes in
/// the order its wheels are.
///
/// Kept because a wheel carries state from one step to the next: an airborne
/// wheel keeps spinning and slows by rapier's own factor.
#[derive(Clone)]
pub struct VehicleRef3d {
    pub controller: DynamicRayCastVehicleController,
    pub wheels: Vec<Entity>,
}

pub(crate) fn build(reg: &mut Registry<'_>) {
    // After the physics step: a vehicle reads the world the step just wrote,
    // and writes forces the next step will integrate.
    reg.add_system(Stage::FixedUpdate, drive_system);
}

/// Step every vehicle's controller.
fn drive_system(eng: &Engine, _dt: f32) {
    // Held with the step it feeds: forces applied into a world that is not
    // stepping would all land on the frame the pause lifts.
    if eng.paused() {
        return;
    }
    let vehicles: Vec<Entity> = {
        let world = eng.world();
        let mut query = world.query::<(Entity, &Vehicle3d)>();
        query
            .iter()
            .map(|(entity, _)| entity)
            .filter(|entity| !balaur_core::parked::is_parked(eng, *entity))
            .collect()
    };
    for chassis in vehicles {
        if let Err(why) = drive_one(eng, chassis) {
            tracing::warn!("vehicle3d: {why:#}");
        }
    }
}

/// One wheel node: the node, its `wheel3d` settings, and where it sits on
/// the chassis.
type WheelNode = (Entity, Wheel3d, glamx::Vec3);

fn drive_one(eng: &Engine, chassis: Entity) -> Result<()> {
    let (chassis_settings, wheels) = {
        let world = eng.world();
        let chassis_settings = world
            .get::<&Vehicle3d>(chassis)
            .map(|held| Vehicle3d::clone(&held))
            .map_err(|_| anyhow!("no vehicle3d"))?;
        let mut wheels: Vec<WheelNode> = Vec::new();
        if let Ok(children) = world.get::<&Children>(chassis) {
            for child in &children.0 {
                let Ok(wheel) = world.get::<&Wheel3d>(*child) else {
                    continue;
                };
                let at = world
                    .get::<&Transform>(*child)
                    .map_or(glamx::Vec3::ZERO, |t| t.position);
                wheels.push((*child, Wheel3d::clone(&wheel), at));
            }
        }
        (chassis_settings, wheels)
    };
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let kept = state.vehicles.swap_remove(&chassis);
    if wheels.is_empty() {
        return Ok(());
    }
    let handle = *state
        .bodies
        .get(&chassis)
        .ok_or_else(|| anyhow!("a vehicle3d needs a body3d on the same node"))?;
    let inputs = &state.wheel_inputs;
    let mut vehicle = kept_or_rebuilt(kept, handle, &wheels, |entity| {
        inputs.get(&entity).map_or(0.0, |input| input.rotation)
    });
    let controller = &mut vehicle.controller;
    let (up, _) = axis_of(chassis_settings.up_axis);
    let (forward, sign) = axis_of(chassis_settings.forward_axis);
    // rapier only reads the up axis squared, so its sign needs nothing.
    controller.index_up_axis = up;
    controller.index_forward_axis = forward;
    let mut wake = false;
    for ((entity, settings, at), wheel) in wheels.iter().zip(controller.wheels_mut()) {
        tune(wheel, settings, *at);
        let input = state.wheel_inputs.get(entity).copied().unwrap_or_default();
        // rapier's indices name a positive axis; a car facing down one is
        // driven and steered the other way round.
        let (engine_force, steering) = (input.engine_force * sign, input.steering * sign);
        // Rapier wakes the chassis for a forward drive alone; reversing,
        // steering and braking a parked car have to wake it too.
        wake |= engine_force != 0.0
            || steering.to_bits() != wheel.steering.to_bits()
            || input.brake.to_bits() != wheel.brake.to_bits();
        wheel.engine_force = engine_force;
        wheel.brake = input.brake;
        wheel.steering = steering;
    }
    if wake && let Some(body) = state.world.bodies.get_mut(handle) {
        body.wake_up(true);
    }
    let mask = Layer::bits(&chassis_settings.collision_mask, true);
    let ignore = crate::vocabulary::named_bits(
        chassis_settings.ignore.iter().map(SweepIgnore::option),
        &crate::vocabulary::flags::query_ignores(),
    );
    let filter = QueryFilter::from(QueryFilterFlags::from_bits_truncate(ignore))
        .groups(InteractionGroups::new(
            Group::ALL,
            Group::from_bits_truncate(mask),
            InteractionTestMode::And,
        ))
        .exclude_rigid_body(handle);
    let dispatcher = state.world.narrow_phase.query_dispatcher();
    let queries = state.world.broad_phase.as_query_pipeline_mut(
        dispatcher,
        &mut state.world.bodies,
        &mut state.world.colliders,
        filter,
    );
    controller.update_vehicle(scalar::real(fixed_dt()), queries);
    for ((entity, _, _), wheel) in wheels.iter().zip(controller.wheels()) {
        let input = state.wheel_inputs.entry(*entity).or_default();
        input.rotation = wheel.rotation;
        // What rapier pushed with, after the cap; the field holds the force before it.
        input.suspension_force = wheel.wheel_suspension_force.min(wheel.max_suspension_force);
        input.grounded = wheel.raycast_info().is_in_contact;
    }
    state.vehicles.insert(chassis, vehicle);
    Ok(())
}

/// The kept controller while its chassis and its wheel nodes are the same,
/// and otherwise a new one that keeps each surviving wheel's state; a new
/// wheel starts at the turn `rotation` answers for its node.
fn kept_or_rebuilt(
    kept: Option<VehicleRef3d>,
    handle: RigidBodyHandle,
    wheels: &[WheelNode],
    rotation: impl Fn(Entity) -> Real,
) -> VehicleRef3d {
    let order: Vec<Entity> = wheels.iter().map(|(entity, _, _)| *entity).collect();
    let kept = match kept.filter(|vehicle| vehicle.controller.chassis == handle) {
        Some(vehicle) if vehicle.wheels == order => return vehicle,
        other => other,
    };
    let mut controller = DynamicRayCastVehicleController::new(handle);
    for entity in &order {
        let wheel = controller.add_wheel(
            Vector::ZERO,
            -Vector::Y,
            -Vector::X,
            0.0,
            0.0,
            &WheelTuning::default(),
        );
        let previous = kept.as_ref().and_then(|vehicle| {
            let at = vehicle.wheels.iter().position(|e| e == entity)?;
            vehicle.controller.wheels().get(at).copied()
        });
        match previous {
            Some(previous) => *wheel = previous,
            None => wheel.rotation = rotation(*entity),
        }
    }
    VehicleRef3d {
        controller,
        wheels: order,
    }
}

/// A wheel's settings onto rapier's wheel, every step: they are plain fields,
/// so an edit lands without losing the wheel's spin.
fn tune(wheel: &mut Wheel, settings: &Wheel3d, at: glamx::Vec3) {
    let real = scalar::real;
    wheel.chassis_connection_point_cs = scalar::v3(at.x, at.y, at.z);
    wheel.direction_cs = scalar::v3a(settings.suspension_direction);
    wheel.axle_cs = scalar::v3a(settings.axle);
    wheel.suspension_rest_length = real(settings.rest_length);
    wheel.radius = real(settings.radius).max(0.01);
    wheel.suspension_stiffness = real(settings.suspension_stiffness);
    wheel.damping_compression = real(settings.damping_compression);
    wheel.damping_relaxation = real(settings.damping_relaxation);
    wheel.max_suspension_travel = real(settings.suspension_travel);
    wheel.side_friction_stiffness = real(settings.side_friction);
    wheel.friction_slip = real(settings.friction_slip);
    wheel.max_suspension_force = real(settings.suspension_max_force);
}

/// What a script sets on a wheel, and what the last step left there.
///
/// The inputs are kept beside the world so a script can set them before the
/// first step makes the controller.
#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct WheelInput3d {
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
        ("set_brake", &[c::WHEEL_3D], "", "How hard this wheel brakes, as an impulse; ignored while its engine force is not 0."),
        ("set_steering", &[c::WHEEL_3D], "", "Turn this wheel, in radians."),
        ("wheel_state", &[c::WHEEL_3D], "", "What the last step did with this wheel: `#{ rotation, suspension_force, in_contact, engine_force, brake, steering, forward_impulse, side_impulse, contact_normal, contact_point, suspension_length, ray_origin, ground, center, suspension, axle }`. `suspension_force` is what the suspension pushed with, after `suspension_max_force`; the two impulses are the friction rapier applied along and across the wheel; the contact, the ray's start, the wheel's centre and its suspension and axle directions are world space, after steering; `ground` is the node the ray hit, or nil."),
        ("set_wheel_rotation", &[c::WHEEL_3D], "(node: node, angle: float)", "Set how far the wheel has turned about its axle, in radians: the angle `wheel_state` reads as `rotation`."),
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
        Ok(wheel_state(eng, entity_of(node)?))
    });
    m.function(
        "set_wheel_rotation",
        |eng: &Engine, (node, angle): (NodeId, f32)| {
            let entity = entity_of(node)?;
            let state = eng.resource::<PhysicsState3d>();
            let mut state = state.borrow_mut();
            let state = &mut *state;
            state.wheel_inputs.entry(entity).or_default().rotation = scalar::real(angle);
            if let Some(wheel) = kept_wheel(&mut state.vehicles, entity) {
                wheel.rotation = scalar::real(angle);
            }
            Ok(())
        },
    );
    m.function("vehicle_speed", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        // The chassis's own `forward_axis`, not z: a car built along x would
        // otherwise read the speed it is sliding sideways at.
        let axis = {
            let world = eng.world();
            let vehicle = world
                .get::<&Vehicle3d>(entity)
                .map_err(|_| anyhow!("node has no vehicle3d"))?;
            forward_axis(vehicle.forward_axis)
        };
        let state = eng.resource::<PhysicsState3d>();
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

/// Which way along the chassis's own axes is forward, which `vehicle_speed`
/// measures along.
fn forward_axis(forward: ChassisAxis) -> Vector {
    let (index, sign) = axis_of(forward);
    let axis = match index {
        0 => Vector::X,
        1 => Vector::Y,
        _ => Vector::Z,
    };
    axis * sign
}

/// The wheel of a kept controller that `entity` is.
fn kept_wheel(
    vehicles: &mut balaur_core::collections::DetHashMap<Entity, VehicleRef3d>,
    entity: Entity,
) -> Option<&mut Wheel> {
    vehicles.values_mut().find_map(|vehicle| {
        let at = vehicle.wheels.iter().position(|e| *e == entity)?;
        vehicle.controller.wheels_mut().get_mut(at)
    })
}

/// What `wheel_state` answers: the script's inputs and what the last step
/// left, with the kept controller's own readings beside them.
fn wheel_state(eng: &Engine, entity: Entity) -> Value {
    let state = eng.resource::<PhysicsState3d>();
    let state = state.borrow();
    let input = state.wheel_inputs.get(&entity).copied().unwrap_or_default();
    let wheel = state.vehicles.values().find_map(|vehicle| {
        let at = vehicle.wheels.iter().position(|e| *e == entity)?;
        vehicle.controller.wheels().get(at).copied()
    });
    let number = |x: Real| Value::Num(f64::from(scalar::f32_of(x)));
    let vector = |x: Vector| Value::Vec3(scalar::a3(x));
    let wheel = wheel.unwrap_or_else(|| {
        let mut idle = DynamicRayCastVehicleController::new(RigidBodyHandle::invalid());
        *idle.add_wheel(
            Vector::ZERO,
            -Vector::Y,
            -Vector::X,
            0.0,
            0.0,
            &WheelTuning::default(),
        )
    });
    let ray = wheel.raycast_info();
    let ground = ray
        .ground_object
        .and_then(|handle| state.world.colliders.get(handle))
        .and_then(|collider| Entity::from_bits(collider.user_data as u64))
        .map_or(Value::Nil, |node| Value::Node(node.to_bits().get()));
    Value::Map(
        [
            (k::ROTATION, number(input.rotation)),
            (k::SUSPENSION_FORCE, number(input.suspension_force)),
            (k::IN_CONTACT, Value::Bool(input.grounded)),
            (k::ENGINE_FORCE, number(input.engine_force)),
            (k::BRAKE, number(input.brake)),
            (k::STEERING, number(input.steering)),
            (k::FORWARD_IMPULSE, number(wheel.forward_impulse)),
            (k::SIDE_IMPULSE, number(wheel.side_impulse)),
            (k::CONTACT_NORMAL, vector(ray.contact_normal_ws)),
            (k::CONTACT_POINT, vector(ray.contact_point_ws)),
            (k::SUSPENSION_LENGTH, number(ray.suspension_length)),
            (k::RAY_ORIGIN, vector(ray.hard_point_ws)),
            (k::GROUND, ground),
            (k::CENTER, vector(wheel.center())),
            (k::SUSPENSION, vector(wheel.suspension())),
            (k::AXLE, vector(wheel.axle())),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect(),
    )
}

fn with_wheel(eng: &Engine, node: NodeId, f: impl FnOnce(&mut WheelInput3d)) -> Result<()> {
    let entity = entity_of(node)?;
    let state = eng.resource::<PhysicsState3d>();
    let mut state = state.borrow_mut();
    f(state.wheel_inputs.entry(entity).or_default());
    Ok(())
}

/// Which of a chassis's own axes `axis` is, and which way along it.
fn axis_of(axis: ChassisAxis) -> (usize, Real) {
    match axis {
        ChassisAxis::X => (0, 1.0),
        ChassisAxis::Y => (1, 1.0),
        ChassisAxis::Z => (2, 1.0),
        ChassisAxis::NegativeX => (0, -1.0),
        ChassisAxis::NegativeY => (1, -1.0),
        ChassisAxis::NegativeZ => (2, -1.0),
    }
}

/// What a removed `Vehicle3d` leaves behind, taken with it.
fn vehicle3d_removed(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState3d>();
    state.borrow_mut().vehicles.swap_remove(&entity);
}

/// What a removed `Wheel3d` leaves behind, taken with it.
fn wheel3d_removed(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState3d>();
    state.borrow_mut().wheel_inputs.swap_remove(&entity);
}

pub(crate) fn register_vehicle_components(reg: &mut Registry<'_>) {
    // What was asked, held on the chassis; the controller a step keeps is in
    // `PhysicsState3d::vehicles`.
    reg.register_component(
        Vehicle3d::NAME,
        ComponentDef {
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &[c::BODY_3D],
            ..ComponentDef::of::<Vehicle3d>(None)
        },
    );
    // What was asked, held on the wheel; what a script sets on it and what
    // the last step left there is in `PhysicsState3d::wheel_inputs`.
    reg.register_component(
        Wheel3d::NAME,
        ComponentDef {
            tags: &[
                balaur_core::components::tag::DIM_3D,
                balaur_core::components::tag::PHYSICS,
            ],
            expects: &[c::VEHICLE_3D],
            ..ComponentDef::of::<Wheel3d>(None)
        },
    );
}

/// A kept controller as a snapshot holds it: its wheel nodes by key, which a
/// respawn cannot invalidate.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct VehicleFrame3d {
    controller: DynamicRayCastVehicleController,
    wheels: Vec<crate::NodeKey>,
}

pub(crate) fn frame_rows(
    world: &balaur_core::hecs::World,
    vehicles: &balaur_core::collections::DetHashMap<Entity, VehicleRef3d>,
) -> Vec<(crate::NodeKey, VehicleFrame3d)> {
    vehicles
        .iter()
        .map(|(chassis, vehicle)| {
            let frame = VehicleFrame3d {
                controller: vehicle.controller.clone(),
                wheels: vehicle
                    .wheels
                    .iter()
                    .map(|wheel| crate::key_of(world, *wheel))
                    .collect(),
            };
            (crate::key_of(world, *chassis), frame)
        })
        .collect()
}

/// The kept controllers a snapshot restores. A vehicle with a wheel that no
/// longer resolves is left out, and the next step builds it again.
pub(crate) fn resolved_rows(
    eng: &Engine,
    rows: Vec<(crate::NodeKey, VehicleFrame3d)>,
) -> balaur_core::collections::DetHashMap<Entity, VehicleRef3d> {
    rows.into_iter()
        .filter_map(|(key, frame)| {
            let chassis = crate::resolve_key(eng, &key)?;
            let wheels = frame
                .wheels
                .iter()
                .map(|wheel| crate::resolve_key(eng, wheel))
                .collect::<Option<Vec<_>>>()?;
            let vehicle = VehicleRef3d {
                controller: frame.controller,
                wheels,
            };
            Some((chassis, vehicle))
        })
        .collect()
}
