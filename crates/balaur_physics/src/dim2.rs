//! The 2D half of the physics plugin: a rapier2d world living alongside the
//! 3D one. 2D nodes use the regular scene `Transform`: x/y translate, the
//! rotation is about z, and the z coordinate is left untouched.
//!
//! Determinism matches the 3D world: enhanced-determinism rapier, ordered
//! collections, fixed timestep.

use anyhow::{anyhow, Result};
use balaur_core::collections::DetHashMap;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::mlua::{self, UserDataRef};
use balaur_core::{App, Engine, NodeRef, Stage, Transform};
use glamx::{EulerRot, Pose2, Quat, Rot2, Vec2};
use rapier2d::pipeline::PhysicsWorld as PhysicsWorld2;
use rapier2d::prelude::{
    ColliderBuilder as ColliderBuilder2, ColliderHandle as ColliderHandle2,
    RigidBodyBuilder as RigidBodyBuilder2, RigidBodyHandle as RigidBodyHandle2,
};

const FIXED_DT: f32 = 1.0 / 60.0;
const MAX_SUBSTEPS: u32 = 4;

pub struct Physics2DState {
    pub world: PhysicsWorld2,
    pub bodies: DetHashMap<Entity, RigidBodyHandle2>,
    pub colliders: DetHashMap<Entity, Vec<ColliderHandle2>>,
    pub paused: bool,
    accumulator: f32,
}

impl Physics2DState {
    fn new() -> Self {
        let world = PhysicsWorld2 {
            gravity: Vec2::new(0.0, -9.81),
            ..Default::default()
        };
        Physics2DState {
            world,
            bodies: DetHashMap::default(),
            colliders: DetHashMap::default(),
            paused: false,
            accumulator: 0.0,
        }
    }
}

/// The node's global pose flattened to 2D (x, y, angle about z).
fn node_pose_2d(eng: &Engine, entity: Entity) -> Result<Pose2> {
    let root = eng.root();
    balaur_core::scene::propagate_transforms(&mut eng.world_mut(), root);
    let world = eng.world();
    let global = world
        .get::<&balaur_core::GlobalTransform>(entity)
        .map_err(|_| anyhow!("node is dead or not in the scene tree"))?;
    let (angle, _, _) = global.rotation.to_euler(EulerRot::ZYX);
    Ok(Pose2::from_parts(
        Vec2::new(global.position.x, global.position.y),
        Rot2::from_angle(angle),
    ))
}

fn add_body(eng: &Engine, entity: Entity, kind: &str) -> Result<()> {
    let builder = match kind {
        "dynamic" => RigidBodyBuilder2::dynamic(),
        "fixed" | "static" => RigidBodyBuilder2::fixed(),
        "kinematic" => RigidBodyBuilder2::kinematic_position_based(),
        other => return Err(anyhow!("unknown body kind '{other}'")),
    };
    let pose = node_pose_2d(eng, entity)?;
    let state = eng.resource::<Physics2DState>();
    let mut state = state.borrow_mut();
    let handle = state.world.insert_body(builder.pose(pose));
    state.bodies.insert(entity, handle);
    Ok(())
}

fn add_collider(eng: &Engine, entity: Entity, builder: ColliderBuilder2) -> Result<()> {
    let handle;
    {
        let state = eng.resource::<Physics2DState>();
        let mut state = state.borrow_mut();
        match state.bodies.get(&entity).copied() {
            Some(body) => {
                handle = state.world.insert_collider(builder, Some(body));
            }
            None => {
                drop(state);
                let pose = node_pose_2d(eng, entity)?;
                let state = eng.resource::<Physics2DState>();
                handle = state
                    .borrow_mut()
                    .world
                    .insert_collider(builder.position(pose), None);
            }
        }
    }
    let state = eng.resource::<Physics2DState>();
    state
        .borrow_mut()
        .colliders
        .entry(entity)
        .or_default()
        .push(handle);
    Ok(())
}

fn remove_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<Physics2DState>();
    let mut state = state.borrow_mut();
    if let Some(handles) = state.colliders.swap_remove(&entity) {
        for handle in handles {
            state.world.remove_collider(handle);
        }
    }
}

fn remove_body_and_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<Physics2DState>();
    let mut state = state.borrow_mut();
    state.colliders.swap_remove(&entity);
    if let Some(handle) = state.bodies.swap_remove(&entity) {
        state.world.remove_body(handle);
    }
}

/// Build and insert the collider described by `params`, replacing any
/// existing one.
fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let shape = params
        .get("shape")
        .and_then(|v| v.as_str())
        .unwrap_or("rect");
    let f = |key: &str, default: f64| {
        params
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    let he = |i: usize| {
        params
            .get("half_extents")
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(0.5) as f32
    };
    let builder = match shape {
        "circle" => ColliderBuilder2::ball(f("radius", 0.5).max(0.01)),
        "rect" => ColliderBuilder2::cuboid(he(0).max(0.01), he(1).max(0.01)),
        other => return Err(anyhow!("unknown collider2d shape '{other}'")),
    };
    let builder = builder
        .restitution(f("restitution", 0.0))
        .friction(f("friction", 0.5))
        .density(f("density", 1.0).max(0.001));
    remove_colliders(eng, entity);
    add_collider(eng, entity, builder)
}

fn get_collider_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<Physics2DState>();
    let state = state.borrow();
    let handle = state.colliders.get(&entity)?.first()?;
    let collider = state.world.colliders.get(*handle)?;
    let mut map = toml::map::Map::new();
    if let Some(ball) = collider.shape().as_ball() {
        map.insert("shape".into(), toml::Value::String("circle".into()));
        map.insert("radius".into(), toml::Value::Float(ball.radius as f64));
    } else {
        let cuboid = collider.shape().as_cuboid()?;
        map.insert("shape".into(), toml::Value::String("rect".into()));
        map.insert(
            "half_extents".into(),
            toml::Value::Array(vec![
                toml::Value::Float(cuboid.half_extents.x as f64),
                toml::Value::Float(cuboid.half_extents.y as f64),
            ]),
        );
    }
    map.insert(
        "restitution".into(),
        toml::Value::Float(collider.restitution() as f64),
    );
    map.insert(
        "friction".into(),
        toml::Value::Float(collider.friction() as f64),
    );
    map.insert(
        "density".into(),
        toml::Value::Float(collider.density() as f64),
    );
    Some(toml::Value::Table(map))
}

fn with_body<R>(
    eng: &Engine,
    entity: Entity,
    f: impl FnOnce(&mut Physics2DState, RigidBodyHandle2) -> R,
) -> Result<R> {
    let state = eng.resource::<Physics2DState>();
    let mut state = state.borrow_mut();
    let handle = state
        .bodies
        .get(&entity)
        .copied()
        .ok_or_else(|| anyhow!("node has no 2D rigid body"))?;
    Ok(f(&mut state, handle))
}

/// Largest contact normal impulse currently applied to the node's colliders
/// (0 when untouched). Gameplay uses this for impact damage.
fn max_contact_impulse(eng: &Engine, entity: Entity) -> f32 {
    let state = eng.resource::<Physics2DState>();
    let state = state.borrow();
    let Some(handles) = state.colliders.get(&entity) else {
        return 0.0;
    };
    let mut max = 0.0f32;
    for &handle in handles {
        for pair in state.world.narrow_phase.contact_pairs_with(handle) {
            for manifold in &pair.manifolds {
                for point in &manifold.points {
                    max = max.max(point.data.impulse.abs());
                }
            }
        }
    }
    max
}

fn step_system(eng: &Engine, dt: f32) {
    let state = eng.resource::<Physics2DState>();
    let mut state = state.borrow_mut();
    let state = &mut *state;
    if state.paused {
        return;
    }

    // Prune bodies whose node died, and feed kinematic targets in.
    {
        let world = eng.world();
        state.bodies.retain(|&entity, handle| {
            if !world.contains(entity) {
                state.world.remove_body(*handle);
                return false;
            }
            true
        });
        for (&entity, &handle) in &state.bodies {
            let body = &mut state.world.bodies[handle];
            if body.is_kinematic() {
                if let Ok(t) = world.get::<&Transform>(entity) {
                    let (angle, _, _) = t.rotation.to_euler(EulerRot::ZYX);
                    body.set_next_kinematic_position(Pose2::from_parts(
                        Vec2::new(t.position.x, t.position.y),
                        Rot2::from_angle(angle),
                    ));
                }
            }
        }
    }

    state.accumulator = (state.accumulator + dt).min(FIXED_DT * MAX_SUBSTEPS as f32);
    state.world.integration_parameters.dt = FIXED_DT;
    while state.accumulator >= FIXED_DT {
        state.world.step();
        state.accumulator -= FIXED_DT;
    }

    // Write simulated poses back (x, y and the rotation about z).
    let world = eng.world();
    for (&entity, &handle) in &state.bodies {
        let body = &state.world.bodies[handle];
        if body.is_fixed() || body.is_kinematic() {
            continue;
        }
        if let Ok(mut t) = world.get::<&mut Transform>(entity) {
            let pos = body.translation();
            t.position.x = pos.x;
            t.position.y = pos.y;
            t.rotation = Quat::from_rotation_z(body.rotation().angle());
        }
    }
}

pub fn clear(eng: &Engine) {
    let state = eng.resource::<Physics2DState>();
    let mut state = state.borrow_mut();
    let handles: Vec<_> = state.bodies.values().copied().collect();
    for handle in handles {
        state.world.remove_body(handle);
    }
    let standalone: Vec<_> = state.colliders.values().flatten().copied().collect();
    for handle in standalone {
        state.world.remove_collider(handle);
    }
    state.bodies.clear();
    state.colliders.clear();
}

pub fn set_paused(eng: &Engine, paused: bool) {
    let state = eng.resource::<Physics2DState>();
    state.borrow_mut().paused = paused;
}

pub fn set_sleeping_allowed(eng: &Engine, allowed: bool) {
    use rapier2d::prelude::RigidBodyActivation;
    let state = eng.resource::<Physics2DState>();
    let mut state = state.borrow_mut();
    let handles: Vec<_> = state.bodies.values().copied().collect();
    for handle in handles {
        let body = &mut state.world.bodies[handle];
        *body.activation_mut() = if allowed {
            RigidBodyActivation::default()
        } else {
            body.wake_up(true);
            RigidBodyActivation::cannot_sleep()
        };
    }
}

pub fn build(app: &mut App) -> Result<()> {
    app.engine.insert_resource(Physics2DState::new());
    app.add_system(Stage::PostUpdate, step_system);

    let m = app.lua_module("physics2d")?;
    m.function("set_gravity", |eng, (x, y): (f32, f32)| {
        let state = eng.resource::<Physics2DState>();
        state.borrow_mut().world.gravity = Vec2::new(x, y);
        Ok(())
    })?;
    m.function(
        "apply_impulse",
        |eng, (node, x, y): (UserDataRef<NodeRef>, f32, f32)| {
            with_body(eng, node.entity, |state, handle| {
                state.world.bodies[handle].apply_impulse(Vec2::new(x, y), true);
            })
            .map_err(mlua::Error::external)
        },
    )?;
    m.function(
        "set_linear_velocity",
        |eng, (node, x, y): (UserDataRef<NodeRef>, f32, f32)| {
            with_body(eng, node.entity, |state, handle| {
                state.world.bodies[handle].set_linvel(Vec2::new(x, y), true);
            })
            .map_err(mlua::Error::external)
        },
    )?;
    m.function("linear_velocity", |eng, node: UserDataRef<NodeRef>| {
        with_body(eng, node.entity, |state, handle| {
            let v = state.world.bodies[handle].linvel();
            (v.x, v.y)
        })
        .map_err(mlua::Error::external)
    })?;
    m.function(
        "set_angular_velocity",
        |eng, (node, w): (UserDataRef<NodeRef>, f32)| {
            with_body(eng, node.entity, |state, handle| {
                state.world.bodies[handle].set_angvel(w, true);
            })
            .map_err(mlua::Error::external)
        },
    )?;
    m.function("angular_velocity", |eng, node: UserDataRef<NodeRef>| {
        with_body(eng, node.entity, |state, handle| {
            state.world.bodies[handle].angvel()
        })
        .map_err(mlua::Error::external)
    })?;
    m.function("max_contact_impulse", |eng, node: UserDataRef<NodeRef>| {
        Ok(max_contact_impulse(eng, node.entity))
    })?;

    // Components (schema-driven; also usable as scene keys).
    app.register_component(
        "body2d",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                r#"kind = { kind = "enum", default = "dynamic", options = ["dynamic", "fixed", "kinematic"], shorthand = true }"#,
            ),
            apply: Box::new(|eng, entity, params| {
                let kind = params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("dynamic")
                    .to_string();
                // Recreate the body, preserving any collider.
                let collider = get_collider_params(eng, entity);
                remove_body_and_colliders(eng, entity);
                add_body(eng, entity, &kind)?;
                if let Some(params) = collider {
                    apply_collider(eng, entity, &params)?;
                }
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                // Removing the body keeps the collider, as static geometry.
                let collider = get_collider_params(eng, entity);
                remove_body_and_colliders(eng, entity);
                if let Some(params) = collider {
                    apply_collider(eng, entity, &params)?;
                }
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let state = eng.resource::<Physics2DState>();
                let state = state.borrow();
                let handle = state.bodies.get(&entity)?;
                let kind = match state.world.bodies[*handle].body_type() {
                    rapier2d::prelude::RigidBodyType::Dynamic => "dynamic",
                    rapier2d::prelude::RigidBodyType::Fixed => "fixed",
                    _ => "kinematic",
                };
                Some(toml::Value::Table(toml::map::Map::from_iter([(
                    "kind".to_string(),
                    toml::Value::String(kind.to_string()),
                )])))
            }),
        },
    );
    app.register_component(
        "collider2d",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                r#"shape = { kind = "enum", default = "rect", options = ["circle", "rect"] }
radius = { kind = "float", default = 0.5, min = 0.01 }
half_extents = { kind = "vec2", default = [0.5, 0.5] }
restitution = { kind = "float", default = 0.0, min = 0.0, max = 1.0 }
friction = { kind = "float", default = 0.5, min = 0.0 }
density = { kind = "float", default = 1.0, min = 0.001 }"#,
            ),
            apply: Box::new(apply_collider),
            remove: Box::new(|eng, entity| {
                remove_colliders(eng, entity);
                Ok(())
            }),
            get: Box::new(get_collider_params),
        },
    );
    Ok(())
}
