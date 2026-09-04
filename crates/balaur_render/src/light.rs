//! `light2d` and `occluder2d`: what the 2D light map is built from.
//!
//! Both are resolved headless — the outline an occluder casts from, and the
//! world-space light a backend draws, are computed here from the scene tree,
//! so a test can assert on them without a GPU. The kiss3d backend
//! (`light_map`) only rasterises what these hand it.

use anyhow::{anyhow, Result};
use balaur_core::components::{as_f64, ComponentDef};
use balaur_core::hecs::{Entity, World};
use balaur_core::{Engine, GlobalTransform};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::{Vec2, Vec3};

use crate::{color_from_params, color_to_toml, Renderable2d, Shape2d};

/// Which way a `light2d` throws light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind2d {
    /// Radiates from the node, fading to nothing at `radius`.
    Point,
    /// Parallel rays across the whole view, aimed by the node's rotation.
    Directional,
}

/// The `light2d` component's authored state. The node's global pose places
/// and aims it; `radius` is in world units and does not follow the node's
/// scale.
pub struct Light2d {
    pub kind: LightKind2d,
    pub color: [f32; 4],
    pub radius: f32,
    pub intensity: f32,
    pub shadows: bool,
}

/// The `occluder2d` component: the outline this node blocks light with.
pub struct Occluder2d {
    /// The `mesh` asset the outline came from; empty means it is derived
    /// from the node's `collider2d` or 2D shape by `resolve_occluders_system`.
    pub mesh: String,
    pub closed: bool,
    /// The outline in the node's own space, in order.
    pub points: Vec<Vec2>,
}

/// One light in world space, with everything a light map needs resolved.
#[derive(Clone, Copy)]
pub struct LitLight2d {
    pub position: Vec2,
    /// Where a directional light shines, unit length. A point light's is the
    /// same vector the node aims, and nothing reads it.
    pub direction: Vec2,
    pub color: [f32; 3],
    pub radius: f32,
    pub intensity: f32,
    pub shadows: bool,
    pub kind: LightKind2d,
}

/// A 2D unit vector the node's rotation aims. At rest a `light2d` shines
/// straight down (-y), the direction a sun in a side-on 2D scene comes from.
fn aim(global: &GlobalTransform) -> Vec2 {
    let aimed = global.rotation * Vec3::NEG_Y;
    let flat = Vec2::new(aimed.x, aimed.y);
    flat.try_normalize().unwrap_or(Vec2::NEG_Y)
}

/// Every `light2d` under `root`, in world space and in tree order.
pub fn lights(world: &World, root: Entity) -> Vec<LitLight2d> {
    let mut out = Vec::new();
    for entity in balaur_core::scene::collect_subtree(world, root) {
        let (Ok(light), Ok(global)) = (
            world.get::<&Light2d>(entity),
            world.get::<&GlobalTransform>(entity),
        ) else {
            continue;
        };
        let [r, g, b, _] = light.color;
        out.push(LitLight2d {
            position: Vec2::new(global.position.x, global.position.y),
            direction: aim(&global),
            color: [r, g, b],
            radius: light.radius.max(0.0),
            intensity: light.intensity.max(0.0),
            shadows: light.shadows,
            kind: light.kind,
        });
    }
    out
}

/// One node's occluder outline, in world space and in order.
///
/// A closed outline repeats its first point at the end rather than carrying a
/// flag, the way a closed polyline does here: the caller draws segments, and
/// the join is just one more of them. Empty on a node with no `occluder2d`,
/// and on one whose outline has not resolved to anything.
pub fn outline(world: &World, entity: Entity) -> Vec<Vec2> {
    let (Ok(occluder), Ok(global)) = (
        world.get::<&Occluder2d>(entity),
        world.get::<&GlobalTransform>(entity),
    ) else {
        return Vec::new();
    };
    if occluder.points.len() < 2 {
        return Vec::new();
    }
    let to_world = |p: Vec2| {
        let scaled = Vec3::new(p.x * global.scale.x, p.y * global.scale.y, 0.0);
        let turned = global.rotation * scaled;
        Vec2::new(global.position.x + turned.x, global.position.y + turned.y)
    };
    let mut points: Vec<Vec2> = occluder.points.iter().copied().map(to_world).collect();
    if occluder.closed && points.len() > 2 {
        points.push(points[0]);
    }
    points
}

/// Every occluder segment under `root`, in world space.
pub fn occluder_edges(world: &World, root: Entity) -> Vec<[Vec2; 2]> {
    let mut out = Vec::new();
    for entity in balaur_core::scene::collect_subtree(world, root) {
        let points = outline(world, entity);
        for pair in points.windows(2) {
            out.push([pair[0], pair[1]]);
        }
    }
    out
}

/// `render.outline`: the outline a node blocks 2D light with, for a tool that
/// draws it. A derived one comes back too, so an editor gizmo shows the
/// outline the light map will actually use rather than the one authored.
pub(crate) fn install_occluder_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "outline",
        &["occluder2d"],
        "",
        "The outline this node blocks 2D light with, in world space: x then y for each point in turn, with the first repeated at the end when the outline is closed. Empty on a node with no `occluder2d`.",
    )]);
    m.function("outline", |eng: &Engine, node: NodeId| {
        let entity = balaur_core::entity_of(node)?;
        let world = eng.world();
        Ok(outline(&world, entity)
            .into_iter()
            .flat_map(|p| [p.x, p.y])
            .collect::<Vec<f32>>())
    });
}

/// The quad an edge casts away from `light`: the edge itself, then both ends
/// pushed `far` further from the light, so the polygon covers everything the
/// light could still have reached.
///
/// Wound in order (a, b, b far, a far); the caller draws it unculled, so a
/// light on the other side of the edge flips the winding harmlessly.
#[must_use]
pub fn shadow_quad(edge: [Vec2; 2], light: &LitLight2d, far: f32) -> [Vec2; 4] {
    let away = |p: Vec2| match light.kind {
        LightKind2d::Directional => p + light.direction * far,
        LightKind2d::Point => {
            let out = p - light.position;
            // An edge point sitting on the light has no direction to be
            // pushed in; the sliver it leaves shadows nothing.
            out.try_normalize().map_or(p, |unit| p + unit * far)
        }
    };
    [edge[0], edge[1], away(edge[1]), away(edge[0])]
}

fn set_light(eng: &Engine, entity: Entity, next: Light2d) -> Result<()> {
    let mut world = eng.world_mut();
    if let Ok(mut light) = world.get::<&mut Light2d>(entity) {
        *light = next;
        return Ok(());
    }
    world
        .insert_one(entity, next)
        .map_err(|_| anyhow!("node is dead"))
}

const LIGHT_SCHEMA: &str = r#"kind = { type = "enum", default = "point", options = ["point", "directional"], description = "A point light fades to nothing at `radius`; a directional one lights the whole view" }
color = { type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Light colour, as channel floats or #rrggbb / #rrggbbaa" }
radius = { type = "float", default = 6.0, min = 0.0, description = "How far a point light reaches, in world units" }
intensity = { type = "float", default = 1.0, min = 0.0, description = "Brightness multiplier; over 1 blows past white" }
shadows = { type = "bool", default = true, description = "Whether `occluder2d` outlines cast shadows from this light" }"#;

/// The `light2d` component. The node's position places it and its rotation
/// aims it; the light map pass in the backend draws it.
pub(crate) fn register_light2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "light2d",
        ComponentDef {
            doc: "A 2D light: the node's position places it, its rotation aims a directional one, and everything drawn under it — sprites, polygons, tiles, a 3D scene behind them — is multiplied by the light map the scene's lights build. A scene with no `light2d` draws exactly as it does unlit; the first one added makes everything else fall to the camera's `ambient`. Debug lines and particles draw after the light map and stay unlit.",
            schema: ComponentDef::parse_schema("light2d", LIGHT_SCHEMA),
            tags: &["2d", "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let kind = match params.get("kind").and_then(|v| v.as_str()).unwrap_or("point") {
                    "point" => LightKind2d::Point,
                    "directional" => LightKind2d::Directional,
                    other => return Err(anyhow!("unknown light2d kind '{other}'")),
                };
                let num = |key: &str, default: f64| {
                    params.get(key).and_then(as_f64).unwrap_or(default) as f32
                };
                set_light(
                    eng,
                    entity,
                    Light2d {
                        kind,
                        color: color_from_params(params),
                        radius: num("radius", 6.0).max(0.0),
                        intensity: num("intensity", 1.0).max(0.0),
                        shadows: params
                            .get("shadows")
                            .and_then(toml::Value::as_bool)
                            .unwrap_or(true),
                    },
                )
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Light2d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let light = world.get::<&Light2d>(entity).ok()?;
                let kind = match light.kind {
                    LightKind2d::Point => "point",
                    LightKind2d::Directional => "directional",
                };
                let mut map = toml::map::Map::new();
                map.insert("kind".into(), toml::Value::String(kind.into()));
                map.insert("color".into(), color_to_toml(light.color));
                map.insert("radius".into(), toml::Value::Float(f64::from(light.radius)));
                map.insert(
                    "intensity".into(),
                    toml::Value::Float(f64::from(light.intensity)),
                );
                map.insert("shadows".into(), toml::Value::Boolean(light.shadows));
                Some(toml::Value::Table(map))
            }),
        },
    );
}

const OCCLUDER_SCHEMA: &str = r#"mesh = { type = "asset", asset = "mesh", default = "", description = "Outline points in order, [x, y] in the node's space; empty derives the outline from the node's `collider2d`, then from its 2D shape" }
closed = { type = "bool", default = true, description = "Whether the last point joins the first, making the outline a loop" }"#;

/// The `occluder2d` component. An outline with no `mesh` is derived every
/// tick by [`resolve_occluders_system`], so a collider added after the
/// occluder still fills it in.
pub(crate) fn register_occluder2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "occluder2d",
        ComponentDef {
            doc: "The outline this node blocks 2D light with. Left empty it follows the node's `collider2d`, or failing that its circle, capsule, rect or sprite shape, so the thing a player sees is the thing that casts the shadow. Every edge casts, so an occluder stands in its own shadow: a node that should stay lit wants a smaller outline or a light with `shadows = false`.",
            schema: ComponentDef::parse_schema("occluder2d", OCCLUDER_SCHEMA),
            tags: &["2d", "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let mesh = params
                    .get("mesh")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let closed = params
                    .get("closed")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true);
                let points = if mesh.is_empty() {
                    Vec::new()
                } else {
                    mesh_outline(eng, &mesh)
                };
                let next = Occluder2d {
                    mesh,
                    closed,
                    points,
                };
                let mut world = eng.world_mut();
                if let Ok(mut occluder) = world.get::<&mut Occluder2d>(entity) {
                    *occluder = next;
                    return Ok(());
                }
                world
                    .insert_one(entity, next)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Occluder2d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let occluder = world.get::<&Occluder2d>(entity).ok()?;
                let mut map = toml::map::Map::new();
                map.insert("mesh".into(), toml::Value::String(occluder.mesh.clone()));
                map.insert("closed".into(), toml::Value::Boolean(occluder.closed));
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// The mesh asset's points as an outline, in order. A reference that will not
/// load is warned about and occludes nothing: one bad asset must not take the
/// scene down.
fn mesh_outline(eng: &Engine, reference: &str) -> Vec<Vec2> {
    let loaded = balaur_core::assets::load_typed::<balaur_core::mesh::MeshData>(eng, reference)
        .and_then(|definition| balaur_core::mesh::load_from(eng, &definition));
    match loaded {
        Ok(data) => data
            .positions
            .iter()
            .map(|p| Vec2::new(p[0], p[1]))
            .collect(),
        Err(why) => {
            tracing::warn!("occluder2d mesh '{reference}': {why:#}");
            Vec::new()
        }
    }
}

/// Fill in the outline of every `occluder2d` that named no mesh, from the
/// node's `collider2d` if it has one and from its 2D shape otherwise.
///
/// Every tick rather than once at `apply`: scene files apply component keys
/// in the order they are written, so an occluder declared above its collider
/// would otherwise resolve to nothing and stay that way.
pub(crate) fn resolve_occluders_system(eng: &Engine, _dt: f32) {
    let derived: Vec<Entity> = {
        let world = eng.world();
        let mut derived = Vec::new();
        for (entity, occluder) in &mut world.query::<(Entity, &Occluder2d)>() {
            if occluder.mesh.is_empty() {
                derived.push(entity);
            }
        }
        derived
    };
    // Outlines first, world borrows after: `collider_outline` reaches through
    // the registry into another plugin, which takes its own borrows.
    let resolved: Vec<(Entity, Vec<Vec2>)> = derived
        .into_iter()
        .map(|entity| {
            let points =
                collider_outline(eng, entity).unwrap_or_else(|| shape_outline(eng, entity));
            (entity, points)
        })
        .collect();
    let world = eng.world_mut();
    for (entity, points) in resolved {
        if let Ok(mut occluder) = world.get::<&mut Occluder2d>(entity) {
            if occluder.points != points {
                occluder.points = points;
            }
        }
    }
}

/// The node's `collider2d` outline, read through the component registry so
/// the renderer never links against the physics plugin. `None` when there is
/// no collider, or when its shape has no outline to trace (a heightfield, a
/// mesh-backed one — those want an explicit `mesh` on the occluder).
fn collider_outline(eng: &Engine, entity: Entity) -> Option<Vec<Vec2>> {
    let params = {
        let registry = eng.try_resource::<balaur_core::ComponentRegistry>()?;
        let registry = registry.borrow();
        let def = registry.def("collider2d")?;
        (def.get)(eng, entity)?
    };
    let num = |key: &str, default: f32| {
        params
            .get(key)
            .and_then(as_f64)
            .map_or(default, |v| v as f32)
    };
    let point = |key: &str| {
        let axis = |i: usize| {
            params
                .get(key)
                .and_then(|v| v.as_array())
                .and_then(|a| a.get(i))
                .and_then(as_f64)
                .unwrap_or(0.0) as f32
        };
        Vec2::new(axis(0), axis(1))
    };
    match params.get("kind").and_then(toml::Value::as_str)? {
        "circle" => Some(circle_outline(num("radius", 0.5))),
        "rect" => {
            let he = point("half_extents");
            Some(rect_outline(he.x, he.y))
        }
        "capsule" => Some(capsule_outline(num("radius", 0.5), num("height", 1.0))),
        "triangle" => Some(vec![point("a"), point("b"), point("c")]),
        "segment" => Some(vec![point("a"), point("b")]),
        _ => None,
    }
}

/// The node's 2D shape as an outline: the fallback when nothing else says
/// what this node's silhouette is. A polyline or polygon is not one of them —
/// its points live in a mesh asset, which the occluder's own `mesh` names.
fn shape_outline(eng: &Engine, entity: Entity) -> Vec<Vec2> {
    let world = eng.world();
    let Ok(renderable) = world.get::<&Renderable2d>(entity) else {
        return Vec::new();
    };
    match renderable.shape {
        Shape2d::Circle { radius } => circle_outline(radius),
        Shape2d::Capsule { radius, height } => capsule_outline(radius, height),
        Shape2d::Rect { hx, hy } | Shape2d::Sprite { hx, hy } => rect_outline(hx, hy),
        Shape2d::Polyline { .. } | Shape2d::Polygon => Vec::new(),
    }
}

fn rect_outline(hx: f32, hy: f32) -> Vec<Vec2> {
    vec![
        Vec2::new(-hx, -hy),
        Vec2::new(hx, -hy),
        Vec2::new(hx, hy),
        Vec2::new(-hx, hy),
    ]
}

/// How many segments a traced circle or capsule cap gets. Sixteen is where a
/// shadow's edge stops looking faceted at ordinary 2D zooms.
const CIRCLE_SEGMENTS: usize = 16;

/// `libm` rather than the platform's: an outline is not simulation state, but
/// the house rule is one sin/cos for the whole tree (DETERMINISM.md).
fn on_circle(radius: f32, turn: f32) -> Vec2 {
    let (sin, cos) = libm::sincosf(turn);
    Vec2::new(cos * radius, sin * radius)
}

fn circle_outline(radius: f32) -> Vec<Vec2> {
    let step = std::f32::consts::TAU / CIRCLE_SEGMENTS as f32;
    (0..CIRCLE_SEGMENTS)
        .map(|i| on_circle(radius, i as f32 * step))
        .collect()
}

/// A capsule's outline: the two caps traced, joined by the straight sides.
/// `height` is the straight part, as it is on `collider2d`.
fn capsule_outline(radius: f32, height: f32) -> Vec<Vec2> {
    let half = height / 2.0;
    let arc = CIRCLE_SEGMENTS / 2;
    let step = std::f32::consts::PI / arc as f32;
    let mut points = Vec::with_capacity(2 * (arc + 1));
    for i in 0..=arc {
        points.push(on_circle(radius, i as f32 * step) + Vec2::new(0.0, half));
    }
    for i in 0..=arc {
        points
            .push(on_circle(radius, std::f32::consts::PI + i as f32 * step) - Vec2::new(0.0, half));
    }
    points
}
