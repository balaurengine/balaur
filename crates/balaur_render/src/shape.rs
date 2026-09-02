//! The `shape` and `shape2d` components and the `render.set_*` shape API:
//! untextured primitives, in both dimensions.

use anyhow::{anyhow, Result};
use balaur_core::components::ComponentDef;
use balaur_core::{entity_of, App, Engine};
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::{
    color_from_params, color_to_toml, set_color, set_polyline, set_shape, set_shape2d, Renderable,
    Renderable2d, Shape, Shape2d,
};

pub(crate) fn install_shape_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_ball", &["shape3d"], "Draw the node as a sphere of the given radius in world units, replacing any other 3D shape."),
        ("set_cuboid", &["shape3d"], "Draw the node as a box from its three half-extents, in world units, replacing any other 3D shape."),
        ("set_rect", &["shape2d"], "Draw the node as a rectangle from its two half-extents, in world units, replacing any other 2D shape."),
        ("color", &["shape3d", "shape2d", "sprite", "polygon"], "The node's tint as r, g, b, a channel floats; opaque white when the node draws nothing at all."),
    ]);
    m.function("set_ball", |eng: &Engine, (node, radius): (NodeId, f32)| {
        set_shape(eng, entity_of(node)?, Shape::Ball { radius })
    });
    m.function(
        "set_cuboid",
        |eng: &Engine, (node, hx, hy, hz): (NodeId, f32, f32, f32)| {
            set_shape(eng, entity_of(node)?, Shape::Cuboid { hx, hy, hz })
        },
    );
    m.function(
        "set_rect",
        |eng: &Engine, (node, hx, hy): (NodeId, f32, f32)| {
            set_shape2d(eng, entity_of(node)?, Shape2d::Rect { hx, hy })
        },
    );
    m.function("color", |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let result = if let Ok(r) = world.get::<&Renderable>(entity_of(node)?) {
            (r.color[0], r.color[1], r.color[2], r.color[3])
        } else if let Ok(r) = world.get::<&Renderable2d>(entity_of(node)?) {
            (r.color[0], r.color[1], r.color[2], r.color[3])
        } else {
            (1.0, 1.0, 1.0, 1.0)
        };
        Ok(result)
    });
}

// Components below are schema-driven, and each key doubles as a scene key.

/// The `shape` component: 3D primitives, editable from the editor.
/// The `Shape` a `shape` component's params describe.
fn shape_from_params(params: &toml::Value) -> Result<Shape> {
    let kind = params
        .get("kind")
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
    let height = params
        .get("height")
        .and_then(balaur_core::components::as_f64)
        .unwrap_or(1.0) as f32;
    let radius = radius.max(0.01);
    let height = height.max(0.01);
    let shape = match kind {
        "ball" => Shape::Ball { radius },
        "cuboid" => Shape::Cuboid {
            hx: he(0).max(0.01),
            hy: he(1).max(0.01),
            hz: he(2).max(0.01),
        },
        "capsule" => Shape::Capsule { radius, height },
        "cylinder" => Shape::Cylinder { radius, height },
        "cone" => Shape::Cone { radius, height },
        "plane" => Shape::Plane {
            hx: he(0).max(0.01),
            hz: he(2).max(0.01),
        },
        other => return Err(anyhow!("unknown shape kind '{other}'")),
    };
    Ok(shape)
}

/// A `shape` component's params for `shape`, or `None` when another
/// component owns it.
fn shape_to_params(shape: Shape) -> Option<toml::Value> {
    let mut map = toml::map::Map::new();
    match shape {
        Shape::Ball { radius } => {
            map.insert("kind".into(), toml::Value::String("ball".into()));
            map.insert("radius".into(), toml::Value::Float(f64::from(radius)));
        }
        Shape::Capsule { radius, height }
        | Shape::Cylinder { radius, height }
        | Shape::Cone { radius, height } => {
            let name = match shape {
                Shape::Capsule { .. } => "capsule",
                Shape::Cylinder { .. } => "cylinder",
                _ => "cone",
            };
            map.insert("kind".into(), toml::Value::String(name.into()));
            map.insert("radius".into(), toml::Value::Float(f64::from(radius)));
            map.insert("height".into(), toml::Value::Float(f64::from(height)));
        }
        // A mesh is saved by the `mesh` component, not this one.
        Shape::Mesh => return None,
        Shape::Plane { hx, hz } => {
            map.insert("kind".into(), toml::Value::String("plane".into()));
            map.insert(
                "half_extents".into(),
                toml::Value::Array(vec![
                    toml::Value::Float(f64::from(hx)),
                    toml::Value::Float(0.0),
                    toml::Value::Float(f64::from(hz)),
                ]),
            );
        }
        Shape::Cuboid { hx, hy, hz } => {
            map.insert("kind".into(), toml::Value::String("cuboid".into()));
            map.insert(
                "half_extents".into(),
                toml::Value::Array(vec![
                    toml::Value::Float(f64::from(hx)),
                    toml::Value::Float(f64::from(hy)),
                    toml::Value::Float(f64::from(hz)),
                ]),
            );
        }
    }
    Some(toml::Value::Table(map))
}

pub(crate) fn register_shape_component(app: &mut App) {
    app.register_component(
        "shape3d",
        ComponentDef {
            doc: "An untextured 3D primitive drawn at the node -- ball, cuboid, capsule, cylinder, cone or plane -- sized in world units and tinted by `color`.",
            schema: ComponentDef::parse_schema(
                "shape3d",
                r#"kind = { type = "enum", default = "cuboid", options = ["ball", "cuboid", "capsule", "cylinder", "cone", "plane"], description = "Rendered 3D shape" }
radius = { type = "float", default = 0.5, min = 0.01, description = "Radius, for every kind but cuboid" }
height = { type = "float", default = 1.0, min = 0.01, description = "Length along y, for capsule, cylinder and cone" }
half_extents = { type = "vec3", default = [0.5, 0.5, 0.5], description = "Half-sizes of the cuboid, when kind is cuboid" }
color = { type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }
material = { type = "asset", asset = "material", default = "", description = "The material this draws with; empty draws with the built-in one" }"#,
            ),
            tags: &["3d", "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                set_shape(eng, entity, shape_from_params(params)?)?;
                set_color(eng, entity, color_from_params(params))?;
                crate::material::set_material_3d(
                    eng,
                    entity,
                    params
                        .get("material")
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default(),
                )
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Renderable>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let renderable = world.get::<&Renderable>(entity).ok()?;
                let mut params = shape_to_params(renderable.shape)?;
                if let Some(map) = params.as_table_mut() {
                    map.insert("color".into(), color_to_toml(renderable.color));
                    map.insert(
                        "material".into(),
                        toml::Value::String(renderable.material.clone()),
                    );
                }
                Some(params)
            }),
        },
    );
}

/// A `shape2d` component's params, as the shape plus -- for a polyline --
/// the mesh asset its points come from.
fn shape2d_from_params(params: &toml::Value) -> Result<(Shape2d, Option<String>)> {
    let kind = params
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("rect");
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
    let shape = match kind {
        "circle" => Shape2d::Circle {
            radius: radius.max(0.01),
        },
        "rect" => Shape2d::Rect {
            hx: he(0).max(0.01),
            hy: he(1).max(0.01),
        },
        "capsule" => Shape2d::Capsule {
            radius: radius.max(0.01),
            height: params
                .get("height")
                .and_then(balaur_core::components::as_f64)
                .unwrap_or(1.0)
                .max(0.01) as f32,
        },
        "polyline" => {
            let source = params
                .get("mesh")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let shape = Shape2d::Polyline {
                width: params
                    .get("width")
                    .and_then(balaur_core::components::as_f64)
                    .unwrap_or(0.02)
                    .max(0.001) as f32,
                closed: params
                    .get("closed")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
            };
            return Ok((shape, Some(source)));
        }
        other => return Err(anyhow!("unknown shape2d kind '{other}'")),
    };
    Ok((shape, None))
}

/// The `shape2d` component: 2D primitives.
pub(crate) fn register_shape2d_component(app: &mut App) {
    app.register_component(
        "shape2d",
        ComponentDef {
            doc: "An untextured 2D primitive drawn at the node -- circle, rect, capsule or a polyline traced through a mesh asset's points -- sized in world units.",
            schema: ComponentDef::parse_schema(
                "shape2d",
                r#"kind = { type = "enum", default = "rect", options = ["circle", "rect", "capsule", "polyline"], description = "Rendered 2D shape" }
radius = { type = "float", default = 0.5, min = 0.01, description = "Radius, when kind is circle or capsule" }
height = { type = "float", default = 1.0, min = 0.01, description = "Length along y of the straight part, when kind is capsule" }
mesh = { type = "asset", asset = "mesh", default = "", description = "Points of a polyline, taken from a mesh asset's vertices" }
width = { type = "float", default = 0.02, min = 0.001, description = "Line thickness in world units, when kind is polyline" }
closed = { type = "bool", default = false, description = "Join the last point back to the first, making a polygon outline" }
half_extents = { type = "vec2", default = [0.5, 0.5], description = "Half-sizes of the rect, when kind is rect" }
color = { type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }
material = { type = "asset", asset = "material", default = "", description = "The material this draws with; empty draws with the built-in one" }"#,
            ),
            tags: &["2d", "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let (shape, polyline) = shape2d_from_params(params)?;
                match polyline {
                    Some(source) => set_polyline(eng, entity, source, shape)?,
                    None => set_shape2d(eng, entity, shape)?,
                }
                set_color(eng, entity, color_from_params(params))?;
                crate::material::set_material_2d(
                    eng,
                    entity,
                    params
                        .get("material")
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default(),
                )
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Renderable2d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let renderable = world.get::<&Renderable2d>(entity).ok()?;
                let mut map = toml::map::Map::new();
                match renderable.shape {
                    // A sprite is saved by the `sprite` component and a
                    // polygon by `polygon`, not this one.
                    Shape2d::Sprite { .. } | Shape2d::Polygon => return None,
                    Shape2d::Circle { radius } => {
                        map.insert("kind".into(), toml::Value::String("circle".into()));
                        map.insert("radius".into(), toml::Value::Float(f64::from(radius)));
                    }
                    Shape2d::Polyline { width, closed } => {
                        map.insert("kind".into(), toml::Value::String("polyline".into()));
                        map.insert("width".into(), toml::Value::Float(f64::from(width)));
                        map.insert("closed".into(), toml::Value::Boolean(closed));
                        if let Some(source) = renderable.polyline.clone() {
                            map.insert("mesh".into(), toml::Value::String(source));
                        }
                    }
                    Shape2d::Capsule { radius, height } => {
                        map.insert("kind".into(), toml::Value::String("capsule".into()));
                        map.insert("radius".into(), toml::Value::Float(f64::from(radius)));
                        map.insert("height".into(), toml::Value::Float(f64::from(height)));
                    }
                    Shape2d::Rect { hx, hy } => {
                        map.insert("kind".into(), toml::Value::String("rect".into()));
                        map.insert(
                            "half_extents".into(),
                            toml::Value::Array(vec![
                                toml::Value::Float(f64::from(hx)),
                                toml::Value::Float(f64::from(hy)),
                            ]),
                        );
                    }
                }
                map.insert("color".into(), color_to_toml(renderable.color));
                map.insert(
                    "material".into(),
                    toml::Value::String(renderable.material.clone()),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
}
