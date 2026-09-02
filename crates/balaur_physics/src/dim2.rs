//! The 2D half of the physics plugin: a rapier2d world living alongside the
//! 3D one. 2D nodes use the regular scene `Transform`: x/y translate, the
//! rotation is about z, and the z coordinate is left untouched.
//!
//! Determinism matches the 3D world: enhanced-determinism rapier, ordered
//! collections, fixed timestep.

use anyhow::{anyhow, Result};
use balaur_core::collections::DetHashMap;
use balaur_core::components::ComponentDef;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine, Stage, Transform};
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::{EulerRot, Pose2, Quat, Rot2, Vec2};
use rapier2d::pipeline::PhysicsWorld as PhysicsWorld2;
use rapier2d::prelude::{
    ColliderBuilder as ColliderBuilder2, ColliderHandle as ColliderHandle2,
    RigidBodyBuilder as RigidBodyBuilder2, RigidBodyHandle as RigidBodyHandle2,
};

use balaur_core::digest::{node_label, Entry, Hasher};
use balaur_core::FIXED_DT;

pub struct PhysicsState2d {
    pub world: PhysicsWorld2,
    pub bodies: DetHashMap<Entity, RigidBodyHandle2>,
    pub colliders: DetHashMap<Entity, Vec<ColliderHandle2>>,
    pub paused: bool,
    /// Mirrors `PhysicsState::sleeping_allowed`; `physics.set_sleeping_allowed`
    /// writes both worlds.
    pub sleeping_allowed: bool,
}

impl PhysicsState2d {
    fn new() -> Self {
        let world = PhysicsWorld2 {
            gravity: Vec2::new(0.0, -9.81),
            ..Default::default()
        };
        Self {
            world,
            bodies: DetHashMap::default(),
            colliders: DetHashMap::default(),
            paused: false,
            sleeping_allowed: true,
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
        "static" => RigidBodyBuilder2::fixed(),
        "kinematic" => RigidBodyBuilder2::kinematic_position_based(),
        other => return Err(anyhow!("unknown body kind '{other}'")),
    };
    let pose = node_pose_2d(eng, entity)?;
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let builder = if state.sleeping_allowed {
        builder
    } else {
        builder.can_sleep(false)
    };
    let handle = state.world.insert_body(builder.pose(pose));
    state.bodies.insert(entity, handle);
    Ok(())
}

fn add_collider(eng: &Engine, entity: Entity, builder: ColliderBuilder2) -> Result<()> {
    let handle;
    {
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        if let Some(body) = state.bodies.get(&entity).copied() {
            handle = state.world.insert_collider(builder, Some(body));
        } else {
            drop(state);
            let pose = node_pose_2d(eng, entity)?;
            let state = eng.resource::<PhysicsState2d>();
            handle = state
                .borrow_mut()
                .world
                .insert_collider(builder.position(pose), None);
        }
    }
    let state = eng.resource::<PhysicsState2d>();
    state
        .borrow_mut()
        .colliders
        .entry(entity)
        .or_default()
        .push(handle);
    Ok(())
}

fn remove_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    if let Some(handles) = state.colliders.swap_remove(&entity) {
        for handle in handles {
            state.world.remove_collider(handle);
        }
    }
}

fn remove_body_and_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    state.colliders.swap_remove(&entity);
    if let Some(handle) = state.bodies.swap_remove(&entity) {
        state.world.remove_body(handle);
    }
}

/// The collider described by `params`, in the `collider2d` schema's own
/// vocabulary — so a script table and a scene-file entry build the same thing.
fn collider_builder(params: &toml::Value) -> Result<ColliderBuilder2> {
    let kind = params
        .get("kind")
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
    let radius = f("radius", 0.5).max(0.01);
    // `height` is the straight part, caps excluded, as it is in 3D.
    let half_height = (f("height", 1.0).max(0.01)) / 2.0;
    let builder = match kind {
        "circle" => ColliderBuilder2::ball(radius),
        "rect" => ColliderBuilder2::cuboid(he(0).max(0.01), he(1).max(0.01)),
        "capsule" => ColliderBuilder2::capsule_y(half_height, radius),
        other => return Err(anyhow!("unknown collider2d kind '{other}'")),
    };
    Ok(builder
        .restitution(f("restitution", 0.0))
        .friction(f("friction", 0.5))
        .density(f("density", 1.0).max(0.001))
        .sensor(
            params
                .get("sensor")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        ))
}

/// Build and insert the collider described by `params`, replacing any
/// existing one.
fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let builder = collider_builder(params)?;
    remove_colliders(eng, entity);
    add_collider(eng, entity, builder)
}

fn get_collider_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = state.colliders.get(&entity)?.first()?;
    let collider = state.world.colliders.get(*handle)?;
    let mut map = toml::map::Map::new();
    if let Some(ball) = collider.shape().as_ball() {
        map.insert("kind".into(), toml::Value::String("circle".into()));
        map.insert("radius".into(), toml::Value::Float(f64::from(ball.radius)));
    } else if let Some(capsule) = collider.shape().as_capsule() {
        map.insert("kind".into(), toml::Value::String("capsule".into()));
        map.insert(
            "radius".into(),
            toml::Value::Float(f64::from(capsule.radius)),
        );
        // `height` is the straight part, caps excluded, as it is in 3D.
        let straight = (capsule.segment.b - capsule.segment.a).length();
        map.insert("height".into(), toml::Value::Float(f64::from(straight)));
    } else {
        let cuboid = collider.shape().as_cuboid()?;
        map.insert("kind".into(), toml::Value::String("rect".into()));
        map.insert(
            "half_extents".into(),
            toml::Value::Array(vec![
                toml::Value::Float(f64::from(cuboid.half_extents.x)),
                toml::Value::Float(f64::from(cuboid.half_extents.y)),
            ]),
        );
    }
    map.insert(
        "restitution".into(),
        toml::Value::Float(f64::from(collider.restitution())),
    );
    map.insert(
        "friction".into(),
        toml::Value::Float(f64::from(collider.friction())),
    );
    map.insert(
        "density".into(),
        toml::Value::Float(f64::from(collider.density())),
    );
    map.insert("sensor".into(), toml::Value::Boolean(collider.is_sensor()));
    Some(toml::Value::Table(map))
}

fn with_body<R>(
    eng: &Engine,
    entity: Entity,
    f: impl FnOnce(&mut PhysicsState2d, RigidBodyHandle2) -> R,
) -> Result<R> {
    let state = eng.resource::<PhysicsState2d>();
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
    let state = eng.resource::<PhysicsState2d>();
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

/// Nodes whose colliders intersect this node's, sorted by entity bits so the
/// order is deterministic. Rapier tracks pairs only when one side is a sensor.
pub fn overlaps(eng: &Engine, entity: Entity) -> Vec<Entity> {
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let Some(handles) = state.colliders.get(&entity) else {
        return Vec::new();
    };
    let mut others: Vec<ColliderHandle2> = Vec::new();
    for &handle in handles {
        for (h1, _, h2, _, intersecting) in state.world.intersection_pairs_with(handle) {
            if intersecting {
                others.push(if h1 == handle { h2 } else { h1 });
            }
        }
    }
    let mut hits: Vec<Entity> = state
        .colliders
        .iter()
        .filter(|&(&e, hs)| e != entity && hs.iter().any(|h| others.contains(h)))
        .map(|(&e, _)| e)
        .collect();
    hits.sort_unstable_by_key(|e| e.to_bits());
    hits
}

fn step_system(eng: &Engine, _dt: f32) {
    let state = eng.resource::<PhysicsState2d>();
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

    // Exactly one step: Stage::FixedUpdate already repeats at FIXED_DT, and a
    // second accumulator here would drift out of step with the scripts.
    state.world.integration_parameters.dt = FIXED_DT;
    state.world.step();

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
    let state = eng.resource::<PhysicsState2d>();
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
    let state = eng.resource::<PhysicsState2d>();
    state.borrow_mut().paused = paused;
}

pub fn set_sleeping_allowed(eng: &Engine, allowed: bool) {
    use rapier2d::prelude::RigidBodyActivation;
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    state.sleeping_allowed = allowed;
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
    build_physics2d(app);

    let mut m = app.script_module("physics2d")?;
    crate::install_constants(&mut *m, crate::BODY_KINDS, crate::SHAPE_KINDS_2D);
    install_physics2d_api(&mut *m);
    register_physics2d_components(app);

    app.register_preset(
        "rigid_body2d",
        balaur_core::presets::preset(
            "A 2D body physics simulates, with a rect collider",
            &["2d", "physics"],
            &[("body2d", Some("kind = \"dynamic\"")), ("collider2d", None)],
        )?,
    );
    app.register_preset(
        "static_body2d",
        balaur_core::presets::preset(
            "An immovable 2D body with a rect collider: ground, walls",
            &["2d", "physics"],
            &[("body2d", Some("kind = \"static\"")), ("collider2d", None)],
        )?,
    );

    Ok(())
}

/// The 2D world and the system that steps it, mirroring `PhysicsPlugin::build`.
fn build_physics2d(app: &mut App) {
    app.engine.insert_resource(PhysicsState2d::new());
    app.add_system(Stage::FixedUpdate, step_system);
    build_physics2d_digest(app);
    build_physics2d_snapshot(app);
}

/// The 2D twin of the 3D snapshot source.
#[derive(serde::Deserialize)]
struct PhysicsFrame2d {
    world: PhysicsWorld2,
    bodies: Vec<(u64, RigidBodyHandle2)>,
    colliders: Vec<(u64, Vec<ColliderHandle2>)>,
    paused: bool,
    sleeping_allowed: bool,
}

#[derive(serde::Serialize)]
struct PhysicsFrameRef2d<'a> {
    world: &'a PhysicsWorld2,
    bodies: Vec<(u64, RigidBodyHandle2)>,
    colliders: Vec<(u64, Vec<ColliderHandle2>)>,
    paused: bool,
    sleeping_allowed: bool,
}

fn build_physics2d_snapshot(app: &mut App) {
    app.add_snapshot_source(
        "physics2d",
        |eng| {
            let state = eng.resource::<PhysicsState2d>();
            let state = state.borrow();
            let frame = PhysicsFrameRef2d {
                world: &state.world,
                bodies: state
                    .bodies
                    .iter()
                    .map(|(e, h)| (e.to_bits().get(), *h))
                    .collect(),
                colliders: state
                    .colliders
                    .iter()
                    .map(|(e, h)| (e.to_bits().get(), h.clone()))
                    .collect(),
                paused: state.paused,
                sleeping_allowed: state.sleeping_allowed,
            };
            serde_json::to_value(frame).unwrap_or(serde_json::Value::Null)
        },
        |eng, value| {
            let frame: PhysicsFrame2d = match serde_json::from_value(value.clone()) {
                Ok(frame) => frame,
                Err(e) => {
                    tracing::error!(error = %e, "restoring the 2D physics world");
                    return;
                }
            };
            let state = eng.resource::<PhysicsState2d>();
            let mut state = state.borrow_mut();
            state.world = frame.world;
            state.paused = frame.paused;
            state.sleeping_allowed = frame.sleeping_allowed;
            state.bodies = frame
                .bodies
                .into_iter()
                .filter_map(|(bits, h)| Some((Entity::from_bits(bits)?, h)))
                .collect();
            state.colliders = frame
                .colliders
                .into_iter()
                .filter_map(|(bits, h)| Some((Entity::from_bits(bits)?, h)))
                .collect();
        },
    );
}

/// The 2D twin of the 3D source: velocity and sleep state, which no
/// component `get` reports.
fn build_physics2d_digest(app: &mut App) {
    app.add_digest_source("physics2d", |eng, out| {
        let Some(state) = eng.try_resource::<PhysicsState2d>() else {
            return;
        };
        let state = state.borrow();
        let world = eng.world();
        for (&entity, &handle) in &state.bodies {
            let body = &state.world.bodies[handle];
            let v = body.linvel();
            let mut h = Hasher::new();
            for value in [v.x, v.y, body.angvel()] {
                h.write_f32(value);
            }
            h.write(&[u8::from(body.is_sleeping())]);
            out.push(Entry {
                label: node_label(&world, entity),
                digest: h.finish(),
            });
        }
    });
}

/// `physics2d`: bodies, colliders, gravity, velocities, contact impulse and
/// overlap queries.
fn install_physics2d_api(m: &mut dyn Bindings<Engine>) {
    m.drives(&["body2d", "collider2d"]);
    // Constructors, so a 2D body can be built from script rather than only
    // declared in a scene file.
    m.function(
        "add_body",
        |eng: &Engine, (node, kind): (NodeId, String)| add_body(eng, entity_of(node)?, &kind),
    );
    // Takes the `collider2d` component's own table (`kind`, `radius`,
    // `half_extents`, `restitution`, `friction`, `density`), so one
    // vocabulary covers scripts and scene files.
    m.function(
        "add_collider",
        |eng: &Engine, (node, params): (NodeId, balaur_script::Value)| {
            let params = balaur_core::node_api::to_toml(&params)?;
            add_collider(eng, entity_of(node)?, collider_builder(&params)?)
        },
    );
    // No reader by design (N8): `PhysicsState2d`'s rapier world already
    // holds the gravity vector; add `physics2d.gravity` when a caller needs
    // to read it back.
    m.function("set_gravity", |eng: &Engine, (x, y): (f32, f32)| {
        let state = eng.resource::<PhysicsState2d>();
        state.borrow_mut().world.gravity = Vec2::new(x, y);
        Ok(())
    });
    m.function(
        "apply_impulse",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].apply_impulse(Vec2::new(x, y), true);
            })
        },
    );
    m.function(
        "set_linear_velocity",
        |eng: &Engine, (node, x, y): (NodeId, f32, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_linvel(Vec2::new(x, y), true);
            })
        },
    );
    m.function("linear_velocity", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            let v = state.world.bodies[handle].linvel();
            (v.x, v.y)
        })
    });
    m.function(
        "set_angular_velocity",
        |eng: &Engine, (node, w): (NodeId, f32)| {
            with_body(eng, entity_of(node)?, |state, handle| {
                state.world.bodies[handle].set_angvel(w, true);
            })
        },
    );
    m.function("angular_velocity", |eng: &Engine, node: NodeId| {
        with_body(eng, entity_of(node)?, |state, handle| {
            state.world.bodies[handle].angvel()
        })
    });
    m.function("max_contact_impulse", |eng: &Engine, node: NodeId| {
        Ok(max_contact_impulse(eng, entity_of(node)?))
    });
    // Sensor pairs only: rapier's narrow phase reports an intersection only
    // when at least one of the two colliders is a sensor.
    m.function("overlaps", |eng: &Engine, node: NodeId| {
        Ok(overlaps(eng, entity_of(node)?)
            .into_iter()
            .map(balaur_core::node_id_of)
            .collect::<Vec<_>>())
    });
}

/// The `body2d` and `collider2d` component keys. Neither is backed by a
/// component type: both write into `PhysicsState2d`.
fn register_physics2d_components(app: &mut App) {
    app.register_component(
        "body2d",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "body2d",
                r#"kind = { type = "enum", default = "dynamic", options = ["dynamic", "static", "kinematic"], shorthand = true, description = "How 2D physics drives the node: simulated, immovable, or moved by script" }"#,
            ),
            tags: &["2d", "physics"],
            expects: &[],
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
                let state = eng.resource::<PhysicsState2d>();
                let state = state.borrow();
                let handle = state.bodies.get(&entity)?;
                let kind = match state.world.bodies[*handle].body_type() {
                    rapier2d::prelude::RigidBodyType::Dynamic => "dynamic",
                    rapier2d::prelude::RigidBodyType::Fixed => "static",
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
                "collider2d",
                r#"kind = { type = "enum", default = "rect", options = ["circle", "rect", "capsule"], description = "Collision shape" }
height = { type = "float", default = 1.0, min = 0.01, description = "Length along y of the straight part, when kind is capsule" }
radius = { type = "float", default = 0.5, min = 0.01, description = "Circle radius, when kind is circle" }
half_extents = { type = "vec2", default = [0.5, 0.5], description = "Half-sizes of the rect, when kind is rect" }
restitution = { type = "float", default = 0.0, min = 0.0, max = 1.0, description = "Bounciness: 0 is a dead stop, 1 a full rebound" }
friction = { type = "float", default = 0.5, min = 0.0, description = "Surface friction; 0 is ice" }
density = { type = "float", default = 1.0, min = 0.001, description = "Mass per area, so the shape's size sets its mass" }
sensor = { type = "bool", default = false, description = "Detects overlaps without colliding: bodies pass through and are reported" }"#,
            ),
            tags: &["2d", "physics"],
            expects: &[],
            apply: Box::new(apply_collider),
            remove: Box::new(|eng, entity| {
                remove_colliders(eng, entity);
                Ok(())
            }),
            get: Box::new(get_collider_params),
        },
    );
}
