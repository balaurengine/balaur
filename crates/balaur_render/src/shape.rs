//! The `shape` and `shape2d` components and the `render.set_*` shape API:
//! untextured primitives, in both dimensions.

use crate::vocabulary::{keys as k, options, words};
use anyhow::Result;
use balaur_core::components::{ComponentDef, prop_bool, prop_f32, prop_i64, prop_str, prop_vec2};
use balaur_core::primitive::{Flat, Solid};
use balaur_core::stroke::{self, Cap, Join, Stroke};
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::{
    Renderable2d, Renderable3d, Shape2d, Shape3d, color_from_params, color_to_toml, set_color,
    set_polyline, set_shape, set_shape2d,
};

pub(crate) fn install_shape_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_sphere", &["shape3d"], "", "Draw the node as a sphere of the given radius in world units, replacing any other 3D shape."),
        ("set_box", &["shape3d"], "", "Draw the node as a box from its three half-extents, in world units, replacing any other 3D shape."),
        ("set_rectangle", &["shape2d"], "", "Draw the node as a rectangle from its two half-extents, in world units, replacing any other 2D shape."),
    ]);
    m.function(
        "set_sphere",
        |eng: &Engine, (node, radius): (NodeId, f32)| {
            set_shape(eng, entity_of(node)?, Shape3d::Solid(Solid::ball(radius)))
        },
    );
    m.function(
        "set_box",
        |eng: &Engine, (node, hx, hy, hz): (NodeId, f32, f32, f32)| {
            set_shape(
                eng,
                entity_of(node)?,
                Shape3d::Solid(Solid::cuboid(hx, hy, hz)),
            )
        },
    );
    m.function(
        "set_rectangle",
        |eng: &Engine, (node, hx, hy): (NodeId, f32, f32)| {
            set_shape2d(eng, entity_of(node)?, Shape2d::Flat(Flat::rect(hx, hy)))
        },
    );
}

// Components below are schema-driven, and each key doubles as a scene key.

/// The `shape` component: 3D primitives, editable from the editor.
/// The `Shape3d` a `shape` component's params describe. The reader is the
/// mesher's, so what a scene names is what a mesh can be built from.
fn shape_from_params(params: &toml::Value) -> Result<Shape3d> {
    Ok(Shape3d::Solid(Solid::from_params(params)?))
}

/// A `shape` component's params for `shape`, or `None` when another
/// component owns it: a mesh is saved by `mesh`, not by this.
fn shape_to_params(shape: Shape3d) -> Option<toml::Value> {
    Some(shape.solid()?.to_params())
}

pub(crate) fn register_shape_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "shape3d",
        ComponentDef {
            warnings: None,
            doc: "An untextured 3D primitive at the node, tinted by `color`. `kind` is `sphere`, `box`, `capsule`, `cylinder`, `cone`, `plane`, `torus`, `pyramid`, `prism` or `tube`.",
            schema: ComponentDef::parse_schema(
                "shape3d",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Rendered 3D shape" }}"#, words::BOX, options(words::SHAPES))),
                    (k::RADIUS, r#"{ type = "float", default = 0.5, min = 0.01, description = "Radius, for every kind but box, plane and pyramid" }"#),
                    (k::HEIGHT, r#"{ type = "float", default = 1.0, min = 0.01, description = "Length along y, for capsule, cylinder, cone, prism and tube" }"#),
                    (k::HALF_EXTENTS, r#"{ type = "vec3", default = [0.5, 0.5, 0.5], description = "Half-sizes, when kind is box, plane or pyramid" }"#),
                    (k::TUBE_RADIUS, r#"{ type = "float", default = 0.2, min = 0.01, description = "Thickness of the ring, when kind is torus" }"#),
                    (k::INNER_RADIUS, r#"{ type = "float", default = 0.25, min = 0.01, description = "Radius of the hole, when kind is tube" }"#),
                    (k::CORNER_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "How far the edges are rounded off, when kind is box; zero is a square edge" }"#),
                    (k::SEGMENTS, r#"{ type = "int", default = 32, min = 3, description = "Cuts around the axis, or across a plane" }"#),
                    (k::RINGS, r#"{ type = "int", default = 16, min = 3, description = "Cuts along the axis, for ball, capsule and torus" }"#),
                    (k::SIDES, r#"{ type = "int", default = 4, min = 3, description = "Flat faces, when kind is pyramid or prism" }"#),
                    (k::COLOR, r#"{ type = "color", default = [0.8, 0.8, 0.8, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::MATERIAL, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The material this draws with; empty draws with the built-in one" }}"#, crate::material::MATERIAL_ASSET_TYPE)),
                    (k::CAST_SHADOW, r#"{ type = "bool", default = true, description = "Whether this casts a shadow from the lights that cast" }"#),
                    (k::LIGHT_LAYERS, r#"{ type = "int", default = -1, description = "Light-layer bitmask; a `light3d` lights this when their masks share a bit. -1 is every layer" }"#),
                ]),
            ),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                set_shape(eng, entity, shape_from_params(params)?)?;
                set_color(eng, entity, color_from_params(params))?;
                crate::lighting_from_params(eng, entity, params);
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
                let _ = world.remove_one::<Renderable3d>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let renderable = world.get::<&Renderable3d>(entity).ok()?;
                let mut params = shape_to_params(renderable.shape)?;
                if let Some(map) = params.as_table_mut() {
                    map.insert(k::COLOR.into(), color_to_toml(renderable.color));
                    map.insert(
                        "material".into(),
                        toml::Value::String(renderable.material.clone()),
                    );
                    map.insert(k::CAST_SHADOW.into(), toml::Value::Boolean(renderable.shadows));
                    map.insert(
                        k::LIGHT_LAYERS.into(),
                        toml::Value::Integer(i64::from(renderable.layers.cast_signed())),
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
        gradient_steps: prop_i64(params, k::GRADIENT_STEPS).clamp(1, i64::from(u32::MAX)) as u32,
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
    let kind = prop_str(params, k::KIND);
    // A polyline is the one 2D kind with no dimensions of its own: its points
    // come from a mesh asset, so it is read here rather than by the mesher.
    if kind == words::POLYLINE {
        let source = params
            .get(k::MESH)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let shape = Shape2d::Polyline(Stroke {
            width: prop_f32(params, k::WIDTH).max(0.001),
            closed: prop_bool(params, k::CLOSED),
            join: Join::from_word(prop_str(params, k::JOIN)).unwrap_or_default(),
            cap: Cap::from_word(prop_str(params, k::CAP)).unwrap_or_default(),
            miter_limit: prop_f32(params, k::MITER_LIMIT),
            taper: prop_vec2(params, k::TAPER),
            segments: prop_i64(params, k::SEGMENTS).max(1).cast_unsigned() as u32,
        });
        return Ok((shape, Some(source)));
    }
    Ok((Shape2d::Flat(Flat::from_params(params)?), None))
}

/// A polyline's params, as `shape2d` saves them.
fn polyline_params(
    map: &mut toml::map::Map<String, toml::Value>,
    stroke: &Stroke,
    renderable: &crate::Renderable2d,
) {
    let float = |v: f32| toml::Value::Float(f64::from(v));
    let word = |w: &str| toml::Value::String(w.into());
    map.insert(k::KIND.into(), word(words::POLYLINE));
    map.insert(k::WIDTH.into(), float(stroke.width));
    map.insert(k::CLOSED.into(), toml::Value::Boolean(stroke.closed));
    map.insert(k::JOIN.into(), word(stroke.join.word()));
    map.insert(k::CAP.into(), word(stroke.cap.word()));
    map.insert(k::MITER_LIMIT.into(), float(stroke.miter_limit));
    let whole = |v: u32| toml::Value::Integer(i64::from(v));
    map.insert(k::SEGMENTS.into(), whole(stroke.segments));
    map.insert(
        k::TAPER.into(),
        toml::Value::Array(stroke.taper.map(float).to_vec()),
    );
    if let Some(source) = &renderable.polyline {
        map.insert(k::MESH.into(), word(source));
    }
    if let Some(style) = &renderable.line {
        if let Some(gradient) = style.gradient {
            map.insert(k::GRADIENT.into(), color_to_toml(gradient));
            map.insert(k::GRADIENT_STEPS.into(), whole(style.gradient_steps));
        }
        if !style.texture.is_empty() {
            map.insert(k::TEXTURE.into(), word(&style.texture));
        }
    }
}

/// The `shape2d` component: 2D primitives.
pub(crate) fn register_shape2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        "shape2d",
        ComponentDef {
            warnings: None,
            doc: "An untextured 2D primitive at the node. `kind` is `circle`, `rectangle`, `capsule`, `ellipse`, `star`, `ngon` or `polyline`; a `polyline` follows a `mesh` or `path2d` asset.",
            schema: ComponentDef::parse_schema(
                "shape2d",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::KIND, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Rendered 2D shape" }}"#, words::RECTANGLE, options(&words::shapes_2d()))),
                    (k::RADIUS, r#"{ type = "float", default = 0.5, min = 0.01, description = "Radius, when kind is circle, capsule, star or ngon" }"#),
                    (k::HEIGHT, r#"{ type = "float", default = 1.0, min = 0.01, description = "Length along y of the straight part, when kind is capsule" }"#),
                    (k::MESH, r#"{ type = "asset", asset = "mesh", default = "", description = "Where a polyline's points come from: a `mesh` asset's vertices, or a `path2d` asset, which is sampled into points and so draws as a stroked curve" }"#),
                    (k::WIDTH, r#"{ type = "float", default = 0.02, min = 0.001, description = "Line thickness in world units, when kind is polyline" }"#),
                    (k::CLOSED, r#"{ type = "bool", default = false, description = "Join the last point back to the first, making a polygon outline" }"#),
                    (k::JOIN, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How a polyline's segments meet" }}"#, stroke::ROUND, options(stroke::JOINS))),
                    (k::CAP, &format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "How an open polyline ends" }}"#, stroke::ROUND, options(stroke::CAPS))),
                    (k::MITER_LIMIT, r#"{ type = "float", default = 4.0, min = 1.0, description = "How far a miter join may reach, in half-widths, before its corner is cut to a bevel" }"#),
                    (k::TAPER, r#"{ type = "vec2", default = [1.0, 1.0], description = "Multipliers on `width` at a polyline's start and end, blended along it; anything but [1, 1] draws round joins and caps" }"#),
                    (k::GRADIENT, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The colour a polyline fades to at its far end, from `color` at its start; a zero alpha means no gradient" }"#),
                    (k::GRADIENT_STEPS, &format!(r#"{{ type = "int", default = {}, min = 1, description = "How many colours a polyline's gradient steps through along its length" }}"#, stroke::GRADIENT_STEPS)),
                    (k::TEXTURE, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "An image, or a `texture` asset, drawn along a polyline, repeating once per world unit of its length" }}"#, balaur_core::texture_asset::TEXTURE_ASSET_TYPE)),
                    (k::HALF_EXTENTS, r#"{ type = "vec2", default = [0.5, 0.5], description = "Half-sizes, when kind is rectangle or ellipse" }"#),
                    (k::INNER_RADIUS, r#"{ type = "float", default = 0.2, min = 0.01, description = "How far the notches between a star's tips reach" }"#),
                    (k::CORNER_RADIUS, r#"{ type = "float", default = 0.0, min = 0.0, description = "How far the corners are rounded off, when kind is rectangle; zero is a square corner" }"#),
                    (k::POINTS, r#"{ type = "int", default = 5, min = 3, description = "Tips, when kind is star" }"#),
                    (k::SIDES, r#"{ type = "int", default = 4, min = 3, description = "Sides, when kind is ngon" }"#),
                    (k::SEGMENTS, r#"{ type = "int", default = 32, min = 3, description = "Cuts around a circle, an ellipse, a rounded corner, or a polyline's round joins and caps" }"#),
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
                    Shape2d::Polyline(stroke) => polyline_params(&mut map, &stroke, &renderable),
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
