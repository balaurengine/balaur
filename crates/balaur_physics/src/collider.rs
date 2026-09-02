//! The `collider3d` component: every shape it can take, and the overlap
//! query that reads them back.

use anyhow::{anyhow, bail, Result};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine};
use rapier3d::math::Vector;
use rapier3d::prelude::{ColliderBuilder, ColliderHandle};

use crate::body::remove_body_and_colliders;
use crate::{node_pose, PhysicsState};


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


/// The collider kinds built from a `mesh` asset, so the model the renderer
/// draws and the shape it collides with stay one authored thing.
fn mesh_collider(eng: &Engine, params: &toml::Value, kind: &str) -> Result<ColliderBuilder> {
    let mesh = collider_mesh(eng, params)?;
    let points: Vec<Vector> = mesh
        .positions
        .iter()
        .map(|p| Vector::new(p[0], p[1], p[2]))
        .collect();
    match kind {
        "trimesh" => ColliderBuilder::trimesh(points, mesh.indices.clone())
            .map_err(|e| anyhow!("that mesh cannot be a trimesh collider: {e}")),
        "convex_hull" => Ok(ColliderBuilder::convex_hull(&points).unwrap_or_else(|| {
            // Degenerate input (every point on one line or plane) has no hull.
            // The node keeps a collider rather than losing one silently.
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
        })),
        _ => {
            if points.len() < 2 {
                bail!(
                    "a polyline collider needs at least two points, not {}",
                    points.len()
                );
            }
            // `None` chains the points in order, which is what a mesh's vertex
            // list means; a closed loop repeats the first point at the end.
            Ok(ColliderBuilder::polyline(points, None))
        }
    }
}


/// Terrain from a `heightfield` asset. The extent belongs to the collider, so
/// one grid can be placed at several sizes.
fn heightfield_collider(
    eng: &Engine,
    params: &toml::Value,
    extent: Vector,
) -> Result<ColliderBuilder> {
    let reference = params
        .get("heightfield")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("a heightfield collider needs a `heightfield` asset"))?;
    let field = balaur_core::assets::load_typed::<balaur_core::heightfield::HeightfieldData>(
        eng, reference,
    )?;
    // The asset checked its own shape, so Array2's assert cannot fire. Through
    // rapier's re-export, so parry cannot drift from rapier's own version.
    let grid =
        rapier3d::parry::utils::Array2::new(field.rows, field.columns, field.heights.clone());
    Ok(ColliderBuilder::heightfield(grid, extent))
}


/// The collider described by `params`, in the `collider` schema's own
/// vocabulary — so a script table and a scene-file entry build the same thing.
pub(crate) fn collider_builder(eng: &Engine, params: &toml::Value) -> Result<ColliderBuilder> {
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
        "trimesh" | "convex_hull" | "polyline" => mesh_collider(eng, params, kind)?,
        "heightfield" => heightfield_collider(eng, params, point("scale", [1.0, 1.0, 1.0]))?,
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
pub(crate) fn apply_collider(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let builder = collider_builder(eng, params)?;
    remove_colliders(eng, entity);
    add_collider(eng, entity, builder)
}


pub(crate) fn remove_colliders(eng: &Engine, entity: Entity) {
    let state = eng.resource::<PhysicsState>();
    let mut state = state.borrow_mut();
    if let Some(handles) = state.colliders.swap_remove(&entity) {
        for handle in handles {
            state.world.remove_collider(handle);
        }
    }
}


/// The shape half of a `collider3d`'s params.
///
/// `None` for the asset-backed kinds: rapier keeps the geometry, not the
/// file it came from, so there is nothing to write back.
fn collider_shape_params(
    shape: &dyn rapier3d::geometry::Shape,
) -> Option<toml::map::Map<String, toml::Value>> {
    let f = |v: f32| toml::Value::Float(f64::from(v));
    let vec3 = |x: f32, y: f32, z: f32| toml::Value::Array(vec![f(x), f(y), f(z)]);
    let mut map = toml::map::Map::new();
    if let Some(ball) = shape.as_ball() {
        map.insert("kind".into(), "ball".into());
        map.insert("radius".into(), f(ball.radius));
        return Some(map);
    }
    if let Some(cuboid) = shape.as_cuboid() {
        map.insert("kind".into(), "cuboid".into());
        let he = cuboid.half_extents;
        map.insert("half_extents".into(), vec3(he.x, he.y, he.z));
        return Some(map);
    }
    if let Some(capsule) = shape.as_capsule() {
        map.insert("kind".into(), "capsule".into());
        map.insert("radius".into(), f(capsule.radius));
        // `height` is the straight part: the segment, caps excluded.
        let straight = (capsule.segment.b - capsule.segment.a).length();
        map.insert("height".into(), f(straight));
        return Some(map);
    }
    if let Some(cylinder) = shape.as_cylinder() {
        map.insert("kind".into(), "cylinder".into());
        map.insert("radius".into(), f(cylinder.radius));
        map.insert("height".into(), f(cylinder.half_height * 2.0));
        return Some(map);
    }
    if let Some(cone) = shape.as_cone() {
        map.insert("kind".into(), "cone".into());
        map.insert("radius".into(), f(cone.radius));
        map.insert("height".into(), f(cone.half_height * 2.0));
        return Some(map);
    }
    if let Some(tri) = shape.as_triangle() {
        map.insert("kind".into(), "triangle".into());
        map.insert("a".into(), vec3(tri.a.x, tri.a.y, tri.a.z));
        map.insert("b".into(), vec3(tri.b.x, tri.b.y, tri.b.z));
        map.insert("c".into(), vec3(tri.c.x, tri.c.y, tri.c.z));
        return Some(map);
    }
    None
}


pub(crate) fn get_collider_params(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let handle = state.colliders.get(&entity)?.first()?;
    let collider = state.world.colliders.get(*handle)?;
    let mut map = collider_shape_params(collider.shape())?;
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


pub(crate) fn add_collider(eng: &Engine, entity: Entity, builder: ColliderBuilder) -> Result<()> {
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

/// The `collider3d` key. Not backed by a component type either: it writes
/// into [`crate::PhysicsState`].
pub(crate) fn register_collider_component(app: &mut App) {
    app.register_component(
        "collider3d",
        ComponentDef {
            doc: "The shape the 3D physics world sees for this node, and the surface it presents: friction, bounciness and density. With a `body3d` it moves with the body; on its own it is static geometry a scene can be built from. A sensor reports overlaps without pushing anything.",
            schema: ComponentDef::parse_schema(
                "collider3d",
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
