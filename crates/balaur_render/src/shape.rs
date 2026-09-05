//! The `shape` and `shape2d` components and the `render.set_*` shape API:
//! untextured primitives, in both dimensions.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::{
    Renderable, Renderable2d, Shape, Shape2d, color_from_params, color_to_toml, set_color,
    set_polyline, set_shape, set_shape2d,
};

pub(crate) fn install_shape_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_ball", &["shape3d"], "", "Draw the node as a sphere of the given radius in world units, replacing any other 3D shape."),
        ("set_cuboid", &["shape3d"], "", "Draw the node as a box from its three half-extents, in world units, replacing any other 3D shape."),
        ("set_rect", &["shape2d"], "", "Draw the node as a rectangle from its two half-extents, in world units, replacing any other 2D shape."),
        ("color", &["shape3d", "shape2d", "sprite", "polygon"], "", "The node's tint as r, g, b, a channel floats; opaque white when the node draws nothing at all."),
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
        let result = match world.get::<&Renderable>(entity_of(node)?) {
            Ok(r) => (r.color[0], r.color[1], r.color[2], r.color[3]),
            _ => match world.get::<&Renderable2d>(entity_of(node)?) {
                Ok(r) => (r.color[0], r.color[1], r.color[2], r.color[3]),
                _ => (1.0, 1.0, 1.0, 1.0),
            },
        };
        Ok(result)
    });
}

// Components below are schema-driven, and each key doubles as a scene key.

/// The `shape` component: 3D primitives, editable from the editor.
/// The words a `shape`, `shape2d`, `camera` or `light2d` component spells, and
/// the script constants beside them. Written once so a matcher, a schema's
/// `options` list and the read-back cannot disagree.
pub(crate) mod words {
    pub(crate) const BALL: &str = "ball";
    pub(crate) const CUBOID: &str = "cuboid";
    pub(crate) const CAPSULE: &str = "capsule";
    pub(crate) const CYLINDER: &str = "cylinder";
    pub(crate) const CONE: &str = "cone";
    pub(crate) const PLANE: &str = "plane";
    /// The 3D primitives, in the order the inspector offers them.
    pub(crate) const SHAPES: &[&str] = &[BALL, CUBOID, CAPSULE, CYLINDER, CONE, PLANE];

    pub(crate) const CIRCLE: &str = "circle";
    pub(crate) const RECT: &str = "rect";
    pub(crate) const POLYLINE: &str = "polyline";
    /// The 2D primitives. A circle is not a ball and a rect is not a cuboid.
    pub(crate) const SHAPES_2D: &[&str] = &[CIRCLE, RECT, CAPSULE, POLYLINE];

    /// Two more an occluder may read off a collider's params. `balaur_render`
    /// does not depend on `balaur_physics`, so the words are spelled here too.
    pub(crate) const TRIANGLE: &str = "triangle";
    pub(crate) const SEGMENT: &str = "segment";

    pub(crate) const PERSPECTIVE: &str = "3d";
    pub(crate) const ORTHOGRAPHIC: &str = "2d";
    /// Which camera a `camera` node drives.
    pub(crate) const CAMERA_KINDS: &[&str] = &[PERSPECTIVE, ORTHOGRAPHIC];

    pub(crate) const POINT: &str = "point";
    pub(crate) const DIRECTIONAL: &str = "directional";
    /// The 2D lights.
    pub(crate) const LIGHT_KINDS: &[&str] = &[POINT, DIRECTIONAL];
}

/// The words as script constants, so a script writes `render.SHAPE_BALL`
/// rather than spelling "ball" and finding out at runtime that "Ball" fell
/// through to the default. One list: a capsule is a capsule in 2D and 3D.
pub(crate) const CONSTANTS: &[(&str, &str)] = &[
    ("SHAPE_BALL", words::BALL),
    ("SHAPE_CUBOID", words::CUBOID),
    ("SHAPE_CAPSULE", words::CAPSULE),
    ("SHAPE_CYLINDER", words::CYLINDER),
    ("SHAPE_CONE", words::CONE),
    ("SHAPE_PLANE", words::PLANE),
    ("SHAPE_CIRCLE", words::CIRCLE),
    ("SHAPE_RECT", words::RECT),
    ("SHAPE_POLYLINE", words::POLYLINE),
    ("CAMERA_3D", words::PERSPECTIVE),
    ("CAMERA_2D", words::ORTHOGRAPHIC),
    ("LIGHT_POINT", words::POINT),
    ("LIGHT_DIRECTIONAL", words::DIRECTIONAL),
];

/// The words a schema property offers, as its `options` list.
pub(crate) fn options(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `Shape` a `shape` component's params describe.
fn shape_from_params(params: &toml::Value) -> Result<Shape> {
    let kind = params
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or(words::CUBOID);
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
        words::BALL => Shape::Ball { radius },
        words::CUBOID => Shape::Cuboid {
            hx: he(0).max(0.01),
            hy: he(1).max(0.01),
            hz: he(2).max(0.01),
        },
        words::CAPSULE => Shape::Capsule { radius, height },
        words::CYLINDER => Shape::Cylinder { radius, height },
        words::CONE => Shape::Cone { radius, height },
        words::PLANE => Shape::Plane {
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
            map.insert("kind".into(), toml::Value::String(words::BALL.into()));
            map.insert("radius".into(), toml::Value::Float(f64::from(radius)));
        }
        Shape::Capsule { radius, height }
        | Shape::Cylinder { radius, height }
        | Shape::Cone { radius, height } => {
            let name = match shape {
                Shape::Capsule { .. } => words::CAPSULE,
                Shape::Cylinder { .. } => words::CYLINDER,
                _ => words::CONE,
            };
            map.insert("kind".into(), toml::Value::String(name.into()));
            map.insert("radius".into(), toml::Value::Float(f64::from(radius)));
            map.insert("height".into(), toml::Value::Float(f64::from(height)));
        }
        // A mesh is saved by the `mesh` component, not this one.
        Shape::Mesh => return None,
        Shape::Plane { hx, hz } => {
            map.insert("kind".into(), toml::Value::String(words::PLANE.into()));
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
            map.insert("kind".into(), toml::Value::String(words::CUBOID.into()));
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

pub(crate) fn register_shape_component(reg: &mut Registry<'_>) {
    let shapes = options(words::SHAPES);
    let default = words::CUBOID;
    reg.register_component(
        "shape3d",
        ComponentDef {
            doc: "An untextured 3D primitive drawn at the node -- ball, cuboid, capsule, cylinder, cone or plane -- sized in world units and tinted by `color`.",
            schema: ComponentDef::parse_schema(
                "shape3d",
                &format!(
                    r#"kind = {{ type = "enum", default = "{default}", options = [{shapes}], description = "Rendered 3D shape" }}
radius = {{ type = "float", default = 0.5, min = 0.01, description = "Radius, for every kind but cuboid" }}
height = {{ type = "float", default = 1.0, min = 0.01, description = "Length along y, for capsule, cylinder and cone" }}
half_extents = {{ type = "vec3", default = [0.5, 0.5, 0.5], description = "Half-sizes of the cuboid, when kind is cuboid" }}
color = {{ type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }}
material = {{ type = "asset", asset = "material", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#
                ),
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

/// A polyline's gradient and texture, from its params. A gradient with no
/// alpha is no gradient: the schema's default.
fn line_style_from_params(params: &toml::Value) -> crate::LineStyle {
    let gradient = params.get("gradient").map(|_| {
        let table = toml::Value::Table(
            [("color".to_string(), params["gradient"].clone())]
                .into_iter()
                .collect(),
        );
        color_from_params(&table)
    });
    crate::LineStyle {
        gradient: gradient.filter(|c| c[3] > 0.0),
        texture: params
            .get("texture")
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

/// A `shape2d` component's params, as the shape plus -- for a polyline --
/// the mesh asset its points come from.
fn shape2d_from_params(params: &toml::Value) -> Result<(Shape2d, Option<String>)> {
    let kind = params
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or(words::RECT);
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
        words::CIRCLE => Shape2d::Circle {
            radius: radius.max(0.01),
        },
        words::RECT => Shape2d::Rect {
            hx: he(0).max(0.01),
            hy: he(1).max(0.01),
        },
        words::CAPSULE => Shape2d::Capsule {
            radius: radius.max(0.01),
            height: params
                .get("height")
                .and_then(balaur_core::components::as_f64)
                .unwrap_or(1.0)
                .max(0.01) as f32,
        },
        words::POLYLINE => {
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
pub(crate) fn register_shape2d_component(reg: &mut Registry<'_>) {
    let shapes = options(words::SHAPES_2D);
    let default = words::RECT;
    reg.register_component(
        "shape2d",
        ComponentDef {
            doc: "An untextured 2D primitive drawn at the node -- circle, rect, capsule or a polyline traced through a mesh asset's points -- sized in world units.",
            schema: ComponentDef::parse_schema(
                "shape2d",
                &format!(
                    r#"kind = {{ type = "enum", default = "{default}", options = [{shapes}], description = "Rendered 2D shape" }}
radius = {{ type = "float", default = 0.5, min = 0.01, description = "Radius, when kind is circle or capsule" }}
height = {{ type = "float", default = 1.0, min = 0.01, description = "Length along y of the straight part, when kind is capsule" }}
mesh = {{ type = "asset", asset = "mesh", default = "", description = "Points of a polyline, taken from a mesh asset's vertices" }}
width = {{ type = "float", default = 0.02, min = 0.001, description = "Line thickness in world units, when kind is polyline" }}
closed = {{ type = "bool", default = false, description = "Join the last point back to the first, making a polygon outline" }}
gradient = {{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The colour a polyline fades to at its far end, from `color` at its start; a zero alpha means no gradient" }}
texture = {{ type = "string", default = "", description = "An image drawn along a polyline, repeating once per world unit of its length" }}
half_extents = {{ type = "vec2", default = [0.5, 0.5], description = "Half-sizes of the rect, when kind is rect" }}
color = {{ type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }}
material = {{ type = "asset", asset = "material", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#
                ),
            ),
            tags: &["2d", "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let (shape, polyline) = shape2d_from_params(params)?;
                match polyline {
                    Some(source) => {
                        let style = line_style_from_params(params);
                        set_polyline(eng, entity, source, shape, style)?;
                    }
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
                        if let Some(style) = &renderable.line {
                            if let Some(gradient) = style.gradient {
                                map.insert("gradient".into(), color_to_toml(gradient));
                            }
                            if !style.texture.is_empty() {
                                map.insert(
                                    "texture".into(),
                                    toml::Value::String(style.texture.clone()),
                                );
                            }
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
