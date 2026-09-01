//! Rapier physics as a Balaur plugin.
//!
//! This crate doubles as the reference for wrapping a Rust library for
//! scripting: insert a resource, add a system, declare script functions, and
//! optionally teach scene files new keys. Nothing else is required.
//!
//! v0 constraint: physics-driven nodes are treated as world-space (their
//! simulated pose is written to the local transform). Nest them under
//! non-moving parents only.

use anyhow::{anyhow, bail, Result};
use balaur_core::collections::DetHashMap;
use balaur_core::components::ComponentDef;
use balaur_core::entity_of;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine, Plugin, Stage, Transform};
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::{Pose3, Vec3};
use rapier3d::math::Vector;
use rapier3d::pipeline::PhysicsWorld;
use rapier3d::prelude::{ColliderBuilder, ColliderHandle, RigidBodyBuilder, RigidBodyHandle};

pub mod dim2;
pub use dim2::PhysicsState2d;

use balaur_core::digest::{node_label, Entry, Hasher};
use balaur_core::FIXED_DT;

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
    /// Whether bodies may fall asleep. Recorded here (rapier keeps it
    /// per-body) so `physics.sleeping_allowed` can read it back and bodies
    /// created later inherit it.
    pub sleeping_allowed: bool,
    accumulator: f32,
}

impl PhysicsState {
    fn new() -> Self {
        Self {
            world: PhysicsWorld::default(),
            bodies: DetHashMap::default(),
            colliders: DetHashMap::default(),
            paused: false,
            sleeping_allowed: true,
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
        build_physics_digest(app);

        let mut m = app.script_module("physics")?;
        install_constants(&mut *m, BODY_KINDS, SHAPE_KINDS);
        install_world_controls(&mut *m);
        install_body_api(&mut *m);
        register_physics_components(app);
        register_physics_presets(app)?;

        dim2::build(app)?;
        Ok(())
    }
}

/// Velocity and sleep state, which no component `get` reports: two peers
/// can agree on every position and still be about to diverge.
fn build_physics_digest(app: &mut App) {
    app.add_digest_source("physics", |eng, out| {
        let Some(state) = eng.try_resource::<PhysicsState>() else {
            return;
        };
        let state = state.borrow();
        let world = eng.world();
        for (&entity, &handle) in &state.bodies {
            let body = &state.world.bodies[handle];
            let (v, w) = (body.linvel(), body.angvel());
            let mut h = Hasher::new();
            for value in [v.x, v.y, v.z, w.x, w.y, w.z] {
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

/// The 3D body presets. Dimension is unmarked on 3D names (N/D5).
fn register_physics_presets(app: &mut App) -> Result<()> {
    app.register_preset(
        "rigid_body",
        balaur_core::presets::preset(
            "A body physics simulates, with a box collider",
            &["3d", "physics"],
            &[("body", Some("kind = \"dynamic\"")), ("collider", None)],
        )?,
    );
    app.register_preset(
        "static_body",
        balaur_core::presets::preset(
            "An immovable body with a box collider: ground, walls",
            &["3d", "physics"],
            &[("body", Some("kind = \"static\"")), ("collider", None)],
        )?,
    );
    Ok(())
}

/// The geometry a mesh-backed collider names, through the same asset the
/// renderer uses.
fn collider_mesh(eng: &Engine, params: &toml::Value) -> Result<balaur_core::mesh::MeshData> {
    let reference = params
        .get("mesh")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a trimesh or convex_hull collider needs a `mesh` asset"))?;
    let definition =
        balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)?;
    balaur_core::mesh::load_from(eng, &definition)
}

/// The collider described by `params`, in the `collider` schema's own
/// vocabulary — so a script table and a scene-file entry build the same thing.
fn collider_builder(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder> {
    let kind = params
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("cuboid");
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
    let point = |key: &str, fallback: [f32; 3]| {
        let read = |i: usize| {
            params
                .get(key)
                .and_then(|v| v.as_array())
                .and_then(|a| a.get(i))
                .and_then(balaur_core::components::as_f64)
                .map(|v| v as f32)
        };
        Vector::new(
            read(0).unwrap_or(fallback[0]),
            read(1).unwrap_or(fallback[1]),
            read(2).unwrap_or(fallback[2]),
        )
    };
    let radius = f("radius", 0.5).max(0.01);
    // rapier measures these from the centre; `height` is the whole straight
    // part, matching what the `shape` component means by it.
    let half_height = (f("height", 1.0).max(0.01)) / 2.0;
    let builder = match kind {
        "ball" => ColliderBuilder::ball(radius),
        "cuboid" => ColliderBuilder::cuboid(he(0).max(0.01), he(1).max(0.01), he(2).max(0.01)),
        "capsule" => ColliderBuilder::capsule_y(half_height, radius),
        "cylinder" => ColliderBuilder::cylinder(half_height, radius),
        "cone" => ColliderBuilder::cone(half_height, radius),
        "triangle" => ColliderBuilder::triangle(
            point("a", [0.0, 0.0, 0.0]),
            point("b", [1.0, 0.0, 0.0]),
            point("c", [0.0, 1.0, 0.0]),
        ),
        // The same `mesh` asset the renderer draws, so a model and the shape
        // it collides with are one authored thing that cannot drift apart.
        "trimesh" => {
            let mesh = collider_mesh(eng, params)?;
            let points = mesh
                .positions
                .iter()
                .map(|p| Vector::new(p[0], p[1], p[2]))
                .collect();
            ColliderBuilder::trimesh(points, mesh.indices.clone())
                .map_err(|e| anyhow!("that mesh cannot be a trimesh collider: {e}"))?
        }
        "convex_hull" => {
            let mesh = collider_mesh(eng, params)?;
            let points: Vec<Vector> = mesh
                .positions
                .iter()
                .map(|p| Vector::new(p[0], p[1], p[2]))
                .collect();
            // Degenerate input (every point on one line or plane) has no hull.
            // The node keeps a collider rather than losing one silently.
            ColliderBuilder::convex_hull(&points).unwrap_or_else(|| {
                let (min, max) = mesh.bounds().unwrap_or(([-0.5; 3], [0.5; 3]));
                tracing::warn!(
                    "convex_hull: those {} points are degenerate; using their bounding box",
                    points.len()
                );
                ColliderBuilder::cuboid(
                    ((max[0] - min[0]) / 2.0).max(0.01),
                    ((max[1] - min[1]) / 2.0).max(0.01),
                    ((max[2] - min[2]) / 2.0).max(0.01),
                )
            })
        }
        "polyline" => {
            let mesh = collider_mesh(eng, params)?;
            let points: Vec<Vector> = mesh
                .positions
                .iter()
                .map(|p| Vector::new(p[0], p[1], p[2]))
                .collect();
            if points.len() < 2 {
                bail!(
                    "a polyline collider needs at least two points, not {}",
                    points.len()
                );
            }
            // `None` chains the points in order, which is what a mesh's vertex
            // list means; a closed loop repeats the first point at the end.
            ColliderBuilder::polyline(points, None)
        }
        "heightfield" => {
            let reference = params
                .get("heightfield")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("a heightfield collider needs a `heightfield` asset"))?;
            let field = balaur_core::assets::load_typed::<balaur_core::heightfield::HeightfieldData>(
                eng, reference,
            )?;
            // The asset checked its own shape, so Array2's assert cannot fire.
            // Through rapier's own re-export, so parry cannot drift to a
            // different version than the one rapier was built against.
            let grid = rapier3d::parry::utils::Array2::new(
                field.rows,
                field.columns,
                field.heights.clone(),
            );
            let extent = point("scale", [1.0, 1.0, 1.0]);
            ColliderBuilder::heightfield(grid, extent)
        }
        other => return Err(anyhow!("unknown collider kind '{other}'")),
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
/// existing one (attached to the entity's body when it has one).
fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let builder = collider_builder(eng, params)?;
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
        map.insert("kind".into(), toml::Value::String("ball".into()));
        map.insert("radius".into(), toml::Value::Float(f64::from(ball.radius)));
    } else {
        let cuboid = collider.shape().as_cuboid()?;
        map.insert("kind".into(), toml::Value::String("cuboid".into()));
        map.insert(
            "half_extents".into(),
            toml::Value::Array(vec![
                toml::Value::Float(f64::from(cuboid.half_extents.x)),
                toml::Value::Float(f64::from(cuboid.half_extents.y)),
                toml::Value::Float(f64::from(cuboid.half_extents.z)),
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
        "static" => RigidBodyBuilder::fixed(),
        "kinematic" => RigidBodyBuilder::kinematic_position_based(),
        other => return Err(anyhow!("unknown body kind '{other}'")),
    };
    let pose = node_pose(eng, entity)?;
    let state = eng.resource::<PhysicsState>();
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

fn add_collider(eng: &Engine, entity: Entity, builder: ColliderBuilder) -> Result<()> {
    let handle;
    {
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        if let Some(body) = state.bodies.get(&entity).copied() {
            // A hollow shape has no interior, so rapier cannot derive an
            // inertia tensor for it. The body still simulates, badly; saying so
            // beats leaving someone to wonder why it tumbles.
            if state
                .world
                .bodies
                .get(body)
                .is_some_and(rapier3d::prelude::RigidBody::is_dynamic)
                && matches!(
                    builder.shape.as_typed_shape(),
                    rapier3d::prelude::TypedShape::TriMesh(_)
                        | rapier3d::prelude::TypedShape::Polyline(_)
                        | rapier3d::prelude::TypedShape::HeightField(_)
                )
            {
                tracing::warn!(
                    "a dynamic body with a trimesh, polyline or heightfield collider has no \
                     well-defined mass; give it a convex_hull or a primitive, or make it static"
                );
            }
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

/// Nodes whose colliders intersect this node's, sorted by entity bits so the
/// order is deterministic. Rapier tracks pairs only when one side is a sensor.
pub fn overlaps(eng: &Engine, entity: Entity) -> Vec<Entity> {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let Some(handles) = state.colliders.get(&entity) else {
        return Vec::new();
    };
    let mut others: Vec<ColliderHandle> = Vec::new();
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

/// Pause, sleeping and gravity.
fn install_world_controls(m: &mut dyn Bindings<Engine>) {
    // Spans BOTH the 3D and the 2D world, unlike the rest of `physics`:
    // editors and games treat "physics" as one simulation.
    m.function("set_paused", |eng: &Engine, paused: bool| {
        let state = eng.resource::<PhysicsState>();
        state.borrow_mut().paused = paused;
        dim2::set_paused(eng, paused);
        Ok(())
    });
    // Spans BOTH worlds: `set_paused` pauses them together, so one answer
    // is the truth for both.
    m.function("is_paused", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let v = state.borrow().paused;
        Ok(v)
    });
    // Allow or forbid bodies falling asleep (editors expose this as the
    // "Sleep bodies" toggle). Spans BOTH worlds, and applies to bodies
    // added later as well as to the ones alive now.
    m.function("set_sleeping_allowed", |eng: &Engine, allowed: bool| {
        use rapier3d::prelude::RigidBodyActivation;
        let state = eng.resource::<PhysicsState>();
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
        drop(state);
        dim2::set_sleeping_allowed(eng, allowed);
        Ok(())
    });
    // Reads back what `set_sleeping_allowed` last wrote (true by default).
    // Spans BOTH worlds, because the setter writes both.
    m.function("sleeping_allowed", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let v = state.borrow().sleeping_allowed;
        Ok(v)
    });
    // Remove every body and collider (editors use this to reset a
    // play-in-editor session). Spans BOTH worlds.
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
    // 3D only, unlike the four above; the 2D world has `physics2d.set_gravity`.
    // No reader by design: add `physics.gravity` when a caller needs it back.
    m.function("set_gravity", |eng: &Engine, (x, y, z): (f32, f32, f32)| {
        let state = eng.resource::<PhysicsState>();
        state.borrow_mut().world.gravity = Vec3::new(x, y, z);
        Ok(())
    });
}

/// Body and collider creation, impulses, velocity access and overlap queries.
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
    // Sensor pairs only: rapier's narrow phase reports an intersection only
    // when at least one of the two colliders is a sensor.
    m.function("overlaps", |eng: &Engine, node: NodeId| {
        Ok(overlaps(eng, entity_of(node)?)
            .into_iter()
            .map(balaur_core::node_id_of)
            .collect::<Vec<_>>())
    });
}

/// Body kinds the 3D and 2D worlds both accept, so a script writes
/// `physics.BODY_DYNAMIC` rather than spelling "dynamic" and finding out at
/// runtime that "Dynamic" silently fell through to the default.
pub const BODY_KINDS: &[(&str, &str)] = &[
    ("BODY_DYNAMIC", "dynamic"),
    ("BODY_STATIC", "static"),
    ("BODY_KINEMATIC", "kinematic"),
];

/// Collider shapes for the 3D world.
pub const SHAPE_KINDS: &[(&str, &str)] = &[("SHAPE_BALL", "ball"), ("SHAPE_CUBOID", "cuboid")];

/// Collider shapes for the 2D world.
pub const SHAPE_KINDS_2D: &[(&str, &str)] = &[("SHAPE_CIRCLE", "circle"), ("SHAPE_RECT", "rect")];

pub(crate) fn install_constants(
    m: &mut dyn Bindings<Engine>,
    bodies: &[(&str, &str)],
    shapes: &[(&str, &str)],
) {
    for (name, value) in bodies.iter().chain(shapes) {
        m.constant(name, balaur_script::Value::Str((*value).to_string()));
    }
}

/// Schema-driven components, so bodies and colliders are addable and editable
/// from the editor and usable as scene-file keys. The `body = "dynamic"`
/// shorthand keeps working via the schema's `shorthand` marker.
///
/// Neither key is backed by a component type: both write into `PhysicsState`.
fn register_physics_components(app: &mut App) {
    app.register_component(
        "body",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "body",
                r#"kind = { type = "enum", default = "dynamic", options = ["dynamic", "static", "kinematic"], shorthand = true, description = "How physics drives the node: simulated, immovable, or moved by script" }"#,
            ),
            tags: &["3d", "physics"],
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
                let state = eng.resource::<PhysicsState>();
                let state = state.borrow();
                let handle = state.bodies.get(&entity)?;
                let kind = match state.world.bodies[*handle].body_type() {
                    rapier3d::prelude::RigidBodyType::Dynamic => "dynamic",
                    rapier3d::prelude::RigidBodyType::Fixed => "static",
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
                "collider",
                r#"kind = { type = "enum", default = "cuboid", options = ["ball", "cuboid", "capsule", "cylinder", "cone", "triangle", "trimesh", "convex_hull", "polyline", "heightfield"], description = "Collision shape" }
radius = { type = "float", default = 0.5, min = 0.01, description = "Radius, for ball, capsule, cylinder and cone" }
height = { type = "float", default = 1.0, min = 0.01, description = "Length along y of the straight part, for capsule, cylinder and cone" }
half_extents = { type = "vec3", default = [0.5, 0.5, 0.5], description = "Half-sizes of the cuboid, when kind is cuboid" }
a = { type = "vec3", default = [0.0, 0.0, 0.0], description = "First corner, when kind is triangle" }
b = { type = "vec3", default = [1.0, 0.0, 0.0], description = "Second corner, when kind is triangle" }
c = { type = "vec3", default = [0.0, 1.0, 0.0], description = "Third corner, when kind is triangle" }
mesh = { type = "asset", asset = "mesh", default = "", description = "Geometry for a trimesh, convex_hull or polyline collider" }
heightfield = { type = "asset", asset = "heightfield", default = "", description = "Terrain grid, when kind is heightfield" }
scale = { type = "vec3", default = [1.0, 1.0, 1.0], description = "Cell size and height scale of a heightfield" }
restitution = { type = "float", default = 0.0, min = 0.0, max = 1.0, description = "Bounciness: 0 is a dead stop, 1 a full rebound" }
friction = { type = "float", default = 0.5, min = 0.0, description = "Surface friction; 0 is ice" }
density = { type = "float", default = 1.0, min = 0.001, description = "Mass per volume, so the shape's size sets its mass" }
sensor = { type = "bool", default = false, description = "Detects overlaps without colliding: bodies pass through and are reported" }"#,
            ),
            tags: &["3d", "physics"],
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
