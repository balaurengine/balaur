//! Rapier physics as a Balaur plugin.
//!
//! This crate doubles as the reference for wrapping a Rust library for
//! scripting: insert a resource, add a system, declare Lua functions, and
//! optionally teach scene files new keys. Nothing else is required.
//!
//! v0 constraint: physics-driven nodes are treated as world-space (their
//! simulated pose is written to the local transform). Nest them under
//! non-moving parents only.

use anyhow::{anyhow, Result};
use balaur_core::collections::DetHashMap;
use balaur_core::components::ComponentDef;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine, Plugin, Stage, Transform};
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::{Pose3, Vec3};
use rapier3d::pipeline::PhysicsWorld;
use rapier3d::prelude::{ColliderBuilder, ColliderHandle, RigidBodyBuilder, RigidBodyHandle};

pub mod dim2;
pub use dim2::Physics2DState;

const FIXED_DT: f32 = 1.0 / 60.0;
const MAX_SUBSTEPS: u32 = 4;

pub struct PhysicsState {
    pub world: PhysicsWorld,
    /// Insertion-ordered with an unseeded hasher: iteration order must be
    /// deterministic (see `balaur_core::collections`).
    pub bodies: DetHashMap<Entity, RigidBodyHandle>,
    /// Colliders per entity (attached to the entity's body, or standalone).
    pub colliders: DetHashMap<Entity, Vec<ColliderHandle>>,
    /// While paused the simulation does not step (editors pause by default
    /// and unpause on play).
    pub paused: bool,
    accumulator: f32,
}

impl PhysicsState {
    fn new() -> Self {
        Self {
            world: PhysicsWorld::default(),
            bodies: DetHashMap::default(),
            colliders: DetHashMap::default(),
            paused: false,
            accumulator: 0.0,
        }
    }
}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn name(&self) -> &'static str {
        "physics"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        app.engine.insert_resource(PhysicsState::new());
        app.add_system(Stage::PostUpdate, step_system);

        let mut m = app.lua_module("physics")?;
        install_world_controls(&mut m);
        install_body_api(&mut m);
        register_physics_components(app);

        dim2::build(app)?;
        Ok(())
    }
}

/// Build and insert the collider described by `params`, replacing any
/// existing one (attached to the entity's body when it has one).
fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let shape = params
        .get("shape")
        .and_then(|v| v.as_str())
        .unwrap_or("cuboid");
    let radius = params
        .get("radius")
        .and_then(balaur_core::components::as_f64)
        .unwrap_or(0.5) as f32;
    let he = |i: usize| {
        params
            .get("half_extents")
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(0.5) as f32
    };
    let builder = match shape {
        "ball" => ColliderBuilder::ball(radius.max(0.01)),
        "cuboid" => ColliderBuilder::cuboid(he(0).max(0.01), he(1).max(0.01), he(2).max(0.01)),
        other => return Err(anyhow!("unknown collider shape '{other}'")),
    };
    remove_colliders(eng, entity);
    add_collider(eng, entity, builder)
}

fn remove_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    if let Some(handles) = state.colliders.swap_remove(&entity) {
        for handle in handles {
            state.world.remove_collider(handle);
        }
    }
}

fn remove_body_and_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    // Attached colliders die with the body inside rapier.
    state.colliders.swap_remove(&entity);
    if let Some(handle) = state.bodies.swap_remove(&entity) {
        state.world.remove_body(handle);
    }
}

fn get_collider_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let handle = state.colliders.get(&entity)?.first()?;
    let collider = state.world.colliders.get(*handle)?;
    let mut map = toml::map::Map::new();
    if let Some(ball) = collider.shape().as_ball() {
        map.insert("shape".into(), toml::Value::String("ball".into()));
        map.insert("radius".into(), toml::Value::Float(f64::from(ball.radius)));
    } else {
        let cuboid = collider.shape().as_cuboid()?;
        map.insert("shape".into(), toml::Value::String("cuboid".into()));
        map.insert(
            "half_extents".into(),
            toml::Value::Array(vec![
                toml::Value::Float(f64::from(cuboid.half_extents.x)),
                toml::Value::Float(f64::from(cuboid.half_extents.y)),
                toml::Value::Float(f64::from(cuboid.half_extents.z)),
            ]),
        );
    }
    Some(toml::Value::Table(map))
}

fn node_pose(eng: &Engine, entity: Entity) -> Result<Pose3> {
    // Make sure globals are up to date even before the first frame ran.
    let root = eng.root();
    balaur_core::scene::propagate_transforms(&mut eng.world_mut(), root);
    let world = eng.world();
    let global = world
        .get::<&balaur_core::GlobalTransform>(entity)
        .map_err(|_| anyhow!("node is dead or not in the scene tree"))?;
    Ok(Pose3::from_parts(global.position, global.rotation))
}

fn add_body(eng: &Engine, entity: Entity, kind: &str) -> Result<()> {
    let builder = match kind {
        "dynamic" => RigidBodyBuilder::dynamic(),
        "fixed" | "static" => RigidBodyBuilder::fixed(),
        "kinematic" => RigidBodyBuilder::kinematic_position_based(),
        other => return Err(anyhow!("unknown body kind '{other}'")),
    };
    let pose = node_pose(eng, entity)?;
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let handle = state.world.insert_body(builder.pose(pose));
    state.bodies.insert(entity, handle);
    Ok(())
}

fn add_collider(eng: &Engine, entity: Entity, builder: ColliderBuilder) -> Result<()> {
    let handle;
    {
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        if let Some(body) = state.bodies.get(&entity).copied() {
            handle = state.world.insert_collider(builder, Some(body));
        } else {
            // No body: static world geometry at the node's current pose.
            drop(state);
            let pose = node_pose(eng, entity)?;
            let state = eng.resource::<PhysicsState>();
            handle = state
                .borrow_mut()
                .world
                .insert_collider(builder.position(pose), None);
        }
    }
    let state = eng.resource::<PhysicsState>();
    state
        .borrow_mut()
        .colliders
        .entry(entity)
        .or_default()
        .push(handle);
    Ok(())
}

fn with_body<R>(
    eng: &Engine,
    entity: Entity,
    f: impl FnOnce(&mut PhysicsState, RigidBodyHandle) -> R,
) -> Result<R> {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    let handle = state
        .bodies
        .get(&entity)
        .copied()
        .ok_or_else(|| anyhow!("node has no rigid body"))?;
    Ok(f(&mut state, handle))
}

fn step_system(eng: &Engine, dt: f32) {
    let state = eng.resource::<PhysicsState>();
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
                    body.set_next_kinematic_position(Pose3::from_parts(t.position, t.rotation));
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

    // Write simulated poses back to the scene tree.
    let world = eng.world();
    for (&entity, &handle) in &state.bodies {
        let body = &state.world.bodies[handle];
        if body.is_fixed() || body.is_kinematic() {
            continue;
        }
        if let Ok(mut t) = world.get::<&mut Transform>(entity) {
            t.position = body.translation();
            t.rotation = *body.rotation();
        }
    }
}

/// Pause, sleeping and gravity. These span the 3D and 2D worlds: a script
/// treats "physics" as one simulation.
fn install_world_controls(m: &mut dyn Bindings<Engine>) {
    // Pause/clear/sleep span both the 3D and the 2D world: editors and
    // games treat "physics" as one simulation.
    m.function("set_paused", |eng: &Engine, paused: bool| {
        let state = eng.resource::<PhysicsState>();
        state.borrow_mut().paused = paused;
        dim2::set_paused(eng, paused);
        Ok(())
    });
    m.function("is_paused", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let v = state.borrow().paused;
        Ok(v)
    });
    // Allow or forbid bodies falling asleep (editors expose this as the
    // "Sleep bodies" toggle).
    m.function("set_sleeping_allowed", |eng: &Engine, allowed: bool| {
        use rapier3d::prelude::RigidBodyActivation;
        let state = eng.resource::<PhysicsState>();
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
        drop(state);
        dim2::set_sleeping_allowed(eng, allowed);
        Ok(())
    });
    // Remove every body and collider (editors use this to reset a
    // play-in-editor session).
    m.function("clear", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
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
        drop(state);
        dim2::clear(eng);
        Ok(())
    });
    m.function("set_gravity", |eng: &Engine, (x, y, z): (f32, f32, f32)| {
        let state = eng.resource::<PhysicsState>();
        state.borrow_mut().world.gravity = Vec3::new(x, y, z);
        Ok(())
    });
}

/// Body and collider creation, impulses, and velocity access.
fn install_body_api(m: &mut dyn Bindings<Engine>) {
    m.function(
        "add_body",
        |eng: &Engine, (node, kind): (NodeId, String)| add_body(eng, entity_of(node)?, &kind),
    );
    m.function(
        "add_ball_collider",
        |eng: &Engine, (node, radius): (NodeId, f32)| {
            add_collider(eng, entity_of(node)?, ColliderBuilder::ball(radius))
        },
    );
    m.function(
        "add_cuboid_collider",
        |eng: &Engine, (node, hx, hy, hz): (NodeId, f32, f32, f32)| {
            add_collider(eng, entity_of(node)?, ColliderBuilder::cuboid(hx, hy, hz))
        },
    );
    m.function(
        "apply_impulse",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse(Vec3::new(x, y, z), true);
            })
        },
    );
    m.function(
        "set_linear_velocity",
        |eng: &Engine, (node, x, y, z): (NodeId, f32, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_linvel(Vec3::new(x, y, z), true);
            })
        },
    );
    m.function("linear_velocity", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            let v = state.world.bodies[handle].linvel();
            (v.x, v.y, v.z)
        })
    });

    // Components (schema-driven: addable and editable from the editor,
    // and usable as scene-file keys). `body = "dynamic"` shorthand keeps
    // working via the schema's shorthand marker.
}

/// Schema-driven components, so bodies and colliders are editable from the
/// editor and usable as scene-file keys.
fn register_physics_components(app: &mut App) {
    app.register_component(
        "body",
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
                let state = eng.resource::<PhysicsState>();
                let state = state.borrow();
                let handle = state.bodies.get(&entity)?;
                let kind = match state.world.bodies[*handle].body_type() {
                    rapier3d::prelude::RigidBodyType::Dynamic => "dynamic",
                    rapier3d::prelude::RigidBodyType::Fixed => "fixed",
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
        "collider",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                r#"shape = { kind = "enum", default = "cuboid", options = ["ball", "cuboid"] }
radius = { kind = "float", default = 0.5, min = 0.01 }
half_extents = { kind = "vec3", default = [0.5, 0.5, 0.5] }"#,
            ),
            apply: Box::new(apply_collider),
            remove: Box::new(|eng, entity| {
                remove_colliders(eng, entity);
                Ok(())
            }),
            get: Box::new(get_collider_params),
        },
    );
}
