//! The `shape` and `shape2d` components and the `render.set_*` shape API:
//! untextured primitives, in both dimensions.

use crate::shape::keys as k;
use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_core::primitive::{Flat, Solid};
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
        set_shape(eng, entity_of(node)?, Shape::Solid(Solid::ball(radius)))
    });
    m.function(
        "set_cuboid",
        |eng: &Engine, (node, hx, hy, hz): (NodeId, f32, f32, f32)| {
            set_shape(
                eng,
                entity_of(node)?,
                Shape::Solid(Solid::cuboid(hx, hy, hz)),
            )
        },
    );
    m.function(
        "set_rect",
        |eng: &Engine, (node, hx, hy): (NodeId, f32, f32)| {
            set_shape2d(eng, entity_of(node)?, Shape2d::Flat(Flat::rect(hx, hy)))
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
    use balaur_core::primitive::words as p;

    pub(crate) const BALL: &str = p::BALL;
    pub(crate) const CUBOID: &str = p::CUBOID;
    pub(crate) const CAPSULE: &str = p::CAPSULE;
    pub(crate) const CYLINDER: &str = p::CYLINDER;
    pub(crate) const CONE: &str = p::CONE;
    pub(crate) const PLANE: &str = p::PLANE;
    pub(crate) const TORUS: &str = p::TORUS;
    pub(crate) const PYRAMID: &str = p::PYRAMID;
    pub(crate) const PRISM: &str = p::PRISM;
    pub(crate) const TUBE: &str = p::TUBE;
    /// The 3D primitives, in the order the inspector offers them. The mesher
    /// owns the list, so a kind it can build is a kind a scene can name.
    pub(crate) const SHAPES: &[&str] = p::SOLIDS;

    pub(crate) const CIRCLE: &str = p::CIRCLE;
    pub(crate) const RECT: &str = p::RECT;
    pub(crate) const ELLIPSE: &str = p::ELLIPSE;
    pub(crate) const STAR: &str = p::STAR;
    pub(crate) const NGON: &str = p::NGON;
    pub(crate) const POLYLINE: &str = "polyline";
    /// The 2D primitives, and the chain of points that is not one of them.
    /// A circle is not a ball and a rect is not a cuboid.
    pub(crate) const SHAPES_2D: &[&str] = &[CIRCLE, RECT, CAPSULE, ELLIPSE, STAR, NGON, POLYLINE];

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
    ("SHAPE_TORUS", words::TORUS),
    ("SHAPE_PYRAMID", words::PYRAMID),
    ("SHAPE_PRISM", words::PRISM),
    ("SHAPE_TUBE", words::TUBE),
    ("SHAPE_CIRCLE", words::CIRCLE),
    ("SHAPE_RECT", words::RECT),
    ("SHAPE_ELLIPSE", words::ELLIPSE),
    ("SHAPE_STAR", words::STAR),
    ("SHAPE_NGON", words::NGON),
    ("SHAPE_POLYLINE", words::POLYLINE),
    ("CAMERA_3D", words::PERSPECTIVE),
    ("CAMERA_2D", words::ORTHOGRAPHIC),
    ("LIGHT_POINT", words::POINT),
    ("LIGHT_DIRECTIONAL", words::DIRECTIONAL),
];

/// Every property key the render components spell, so a schema line and the
/// reader behind it name the same key.
pub(crate) mod keys {
    use balaur_core::primitive::keys as p;

    /// A primitive's keys are the mesher's, so a schema line here and the
    /// reader there cannot drift apart.
    pub(crate) const CORNER_RADIUS: &str = p::CORNER_RADIUS;
    pub(crate) const INNER_RADIUS: &str = p::INNER_RADIUS;
    pub(crate) const POINTS: &str = p::POINTS;
    pub(crate) const RINGS: &str = p::RINGS;
    pub(crate) const SEGMENTS: &str = p::SEGMENTS;
    pub(crate) const SIDES: &str = p::SIDES;
    pub(crate) const TUBE_RADIUS: &str = p::TUBE_RADIUS;

    pub(crate) const A: &str = "a";
    pub(crate) const AMBIENT: &str = "ambient";
    pub(crate) const ANGLE: &str = "angle";
    pub(crate) const B: &str = "b";
    pub(crate) const BLOOM_INTENSITY: &str = "bloom_intensity";
    pub(crate) const BLOOM_THRESHOLD: &str = "bloom_threshold";
    pub(crate) const C: &str = "c";
    pub(crate) const CELLS: &str = "cells";
    pub(crate) const CLOSED: &str = "closed";
    pub(crate) const COLOR: &str = "color";
    pub(crate) const COLOR_END: &str = "color_end";
    pub(crate) const COLUMNS: &str = "columns";
    pub(crate) const CURRENT: &str = "current";
    pub(crate) const EMITTING: &str = "emitting";
    pub(crate) const EXPLOSIVENESS: &str = "explosiveness";
    pub(crate) const FLIP_X: &str = "flip_x";
    pub(crate) const FLIP_Y: &str = "flip_y";
    pub(crate) const FRAME: &str = "frame";
    pub(crate) const GRADIENT: &str = "gradient";
    pub(crate) const GRAVITY: &str = "gravity";
    pub(crate) const HALF_EXTENTS: &str = p::HALF_EXTENTS;
    pub(crate) const HEIGHT: &str = p::HEIGHT;
    pub(crate) const INTENSITY: &str = "intensity";
    pub(crate) const KIND: &str = p::KIND;
    pub(crate) const LIFETIME: &str = "lifetime";
    pub(crate) const LOOK_AT: &str = "look_at";
    pub(crate) const MATERIAL: &str = "material";
    pub(crate) const MESH: &str = "mesh";
    pub(crate) const ONE_SHOT: &str = "one_shot";
    pub(crate) const PIXELS_PER_UNIT: &str = "pixels_per_unit";
    pub(crate) const POST: &str = "post";
    pub(crate) const RADIUS: &str = p::RADIUS;
    pub(crate) const RATE: &str = "rate";
    pub(crate) const REGION_ORIGIN: &str = "region_origin";
    pub(crate) const REGION_SIZE: &str = "region_size";
    pub(crate) const ROWS: &str = "rows";
    pub(crate) const SHEET: &str = "sheet";
    pub(crate) const SIZE: &str = "size";
    pub(crate) const SIZE_END: &str = "size_end";
    pub(crate) const SKELETON: &str = "skeleton";
    pub(crate) const SOURCE: &str = "source";
    pub(crate) const SPEED: &str = "speed";
    pub(crate) const SPREAD: &str = "spread";
    pub(crate) const TEXTURE: &str = "texture";
    pub(crate) const TILESET: &str = "tileset";
    pub(crate) const WIDTH: &str = "width";
    pub(crate) const ZOOM: &str = "zoom";
}

/// The words a schema property offers, as its `options` list.
pub(crate) fn options(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `Shape` a `shape` component's params describe. The reader is the
/// mesher's, so what a scene names is what a mesh can be built from.
fn shape_from_params(params: &toml::Value) -> Result<Shape> {
    Ok(Shape::Solid(Solid::from_params(params)?))
}

/// A `shape` component's params for `shape`, or `None` when another
/// component owns it: a mesh is saved by `mesh`, not by this.
fn shape_to_params(shape: Shape) -> Option<toml::Value> {
    Some(shape.solid()?.to_params())
}

pub(crate) fn register_shape_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "shape3d",
        ComponentDef {
            doc: "An untextured 3D primitive drawn at the node -- ball, cuboid, capsule, cylinder, cone, plane, torus, pyramid, prism or tube -- sized in world units and tinted by `color`. Built as a mesh, so a collider fitted to it collides what is drawn.",
            schema: ComponentDef::parse_schema(
                "shape3d",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Rendered 3D shape" }}"#, words::CUBOID, options(words::SHAPES))),
                    (k::RADIUS, r#"{ type = "float", default = 0.5, min = 0.01, description = "Radius, for every kind but cuboid, plane and pyramid" }"#),
                    (k::HEIGHT, r#"{ type = "float", default = 1.0, min = 0.01, description = "Length along y, for capsule, cylinder, cone, prism and tube" }"#),
                    (k::HALF_EXTENTS, r#"{ type = "vec3", default = [0.5, 0.5, 0.5], description = "Half-sizes, when kind is cuboid, plane or pyramid" }"#),
                    (k::TUBE_RADIUS, r#"{ type = "float", default = 0.2, min = 0.01, description = "Thickness of the ring, when kind is torus" }"#),
                    (k::INNER_RADIUS, r#"{ type = "float", default = 0.25, min = 0.01, description = "Radius of the hole, when kind is tube" }"#),
                    (k::CORNER_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "How far the edges are rounded off, when kind is cuboid; zero is a square edge" }"#),
                    (k::SEGMENTS, r#"{ type = "int", default = 32, min = 3, description = "Cuts around the axis, or across a plane" }"#),
                    (k::RINGS, r#"{ type = "int", default = 16, min = 3, description = "Cuts along the axis, for ball, capsule and torus" }"#),
                    (k::SIDES, r#"{ type = "int", default = 4, min = 3, description = "Flat faces, when kind is pyramid or prism" }"#),
                    (k::COLOR, r#"{ type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::MATERIAL, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#, crate::material::MATERIAL_ASSET_TYPE)),
                ]),
            ),
            tags: &[words::PERSPECTIVE, "render"],
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
                    map.insert(k::COLOR.into(), color_to_toml(renderable.color));
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
    let gradient = params.get(k::GRADIENT).map(|_| {
        let table = toml::Value::Table(
            [("color".to_string(), params[k::GRADIENT].clone())]
                .into_iter()
                .collect(),
        );
        color_from_params(&table)
    });
    crate::LineStyle {
        gradient: gradient.filter(|c| c[3] > 0.0),
        texture: params
            .get(k::TEXTURE)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

/// A `shape2d` component's params, as the shape plus -- for a polyline --
/// the mesh asset its points come from.
fn shape2d_from_params(params: &toml::Value) -> Result<(Shape2d, Option<String>)> {
    let kind = params
        .get(k::KIND)
        .and_then(|v| v.as_str())
        .unwrap_or(words::RECT);
    // A polyline is the one 2D kind with no dimensions of its own: its points
    // come from a mesh asset, so it is read here rather than by the mesher.
    if kind == words::POLYLINE {
        let source = params
            .get(k::MESH)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let shape = Shape2d::Polyline {
            width: params
                .get(k::WIDTH)
                .and_then(balaur_core::components::as_f64)
                .unwrap_or(0.02)
                .max(0.001) as f32,
            closed: params
                .get(k::CLOSED)
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        };
        return Ok((shape, Some(source)));
    }
    Ok((Shape2d::Flat(Flat::from_params(params)?), None))
}

/// The `shape2d` component: 2D primitives.
pub(crate) fn register_shape2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "shape2d",
        ComponentDef {
            doc: "An untextured 2D primitive drawn at the node -- circle, rect, capsule, ellipse, star, ngon, or a polyline through a `mesh` asset's points or a stroked `path2d` -- sized in world units.",
            schema: ComponentDef::parse_schema(
                "shape2d",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Rendered 2D shape" }}"#, words::RECT, options(words::SHAPES_2D))),
                    (k::RADIUS, r#"{ type = "float", default = 0.5, min = 0.01, description = "Radius, when kind is circle, capsule, star or ngon" }"#),
                    (k::HEIGHT, r#"{ type = "float", default = 1.0, min = 0.01, description = "Length along y of the straight part, when kind is capsule" }"#),
                    (k::MESH, r#"{ type = "asset", asset = "mesh", default = "", description = "Where a polyline's points come from: a `mesh` asset's vertices, or a `path2d` asset, which is sampled into points and so draws as a stroked curve" }"#),
                    (k::WIDTH, r#"{ type = "float", default = 0.02, min = 0.001, description = "Line thickness in world units, when kind is polyline" }"#),
                    (k::CLOSED, r#"{ type = "bool", default = false, description = "Join the last point back to the first, making a polygon outline" }"#),
                    (k::GRADIENT, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The colour a polyline fades to at its far end, from `color` at its start; a zero alpha means no gradient" }"#),
                    (k::TEXTURE, r#"{ type = "string", default = "", description = "An image drawn along a polyline, repeating once per world unit of its length" }"#),
                    (k::HALF_EXTENTS, r#"{ type = "vec2", default = [0.5, 0.5], description = "Half-sizes, when kind is rect or ellipse" }"#),
                    (k::INNER_RADIUS, r#"{ type = "float", default = 0.2, min = 0.01, description = "How far the notches between a star's tips reach" }"#),
                    (k::CORNER_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "How far the corners are rounded off, when kind is rect; zero is a square corner" }"#),
                    (k::POINTS, r#"{ type = "int", default = 5, min = 3, description = "Tips, when kind is star" }"#),
                    (k::SIDES, r#"{ type = "int", default = 4, min = 3, description = "Sides, when kind is ngon" }"#),
                    (k::SEGMENTS, r#"{ type = "int", default = 32, min = 3, description = "Cuts around a circle, an ellipse or a rounded corner" }"#),
                    (k::COLOR, r#"{ type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::MATERIAL, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#, crate::material::MATERIAL_ASSET_TYPE)),
                ]),
            ),
            tags: &[words::ORTHOGRAPHIC, "render"],
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
                    Shape2d::Flat(flat) => {
                        if let Some(table) = flat.to_params().as_table() {
                            map.extend(table.clone());
                        }
                    }
                    Shape2d::Polyline { width, closed } => {
                        map.insert(k::KIND.into(), toml::Value::String(words::POLYLINE.into()));
                        map.insert(k::WIDTH.into(), toml::Value::Float(f64::from(width)));
                        map.insert(k::CLOSED.into(), toml::Value::Boolean(closed));
                        if let Some(source) = renderable.polyline.clone() {
                            map.insert(k::MESH.into(), toml::Value::String(source));
                        }
                        if let Some(style) = &renderable.line {
                            if let Some(gradient) = style.gradient {
                                map.insert(k::GRADIENT.into(), color_to_toml(gradient));
                            }
                            if !style.texture.is_empty() {
                                map.insert(
                                    k::TEXTURE.into(),
                                    toml::Value::String(style.texture.clone()),
                                );
                            }
                        }
                    }
                }
                map.insert(k::COLOR.into(), color_to_toml(renderable.color));
                map.insert(
                    "material".into(),
                    toml::Value::String(renderable.material.clone()),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
}
