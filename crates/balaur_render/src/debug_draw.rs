//! The `debug_draw` component: a wireframe over every triangle edge of a
//! drawn node, a dot on every vertex, and whether its surface draws at all.
//!
//! One component for every drawable of either dimension, kept on the node
//! beside the drawable rather than in fifteen copies of its rows, and read
//! wherever that drawable's flags are pushed onto the scene. kiss3d's own
//! materials draw it; a node drawn through one of Balaur's pipelines draws it
//! through a companion where it can (`overlay::companion_3d`), and a skinned
//! mesh, a skinned polygon and text drawn through a material do not yet.

use balaur_core::components::{ComponentDef, as_f64};
#[cfg(feature = "window")]
use balaur_core::hecs::{Entity, World};
use balaur_plugin::Registry;

use crate::vocabulary::{keys as k, options, words};

/// Whether a wireframe's width or a vertex's size scales with the view or
/// stays in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sizing {
    #[default]
    World,
    Screen,
}

impl Sizing {
    fn of(word: &str) -> Self {
        if word == words::SCREEN {
            Self::Screen
        } else {
            Self::World
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::World => words::WORLD,
            Self::Screen => words::SCREEN,
        }
    }

    /// kiss3d's `use_perspective` flag.
    #[cfg(feature = "window")]
    pub(crate) fn perspective(self) -> bool {
        self == Self::World
    }
}

/// What a node draws to show its geometry. The default draws the surface
/// and nothing else, which is what a node without the component draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugDraw {
    pub draw_surface: bool,
    /// Zero draws no wireframe.
    pub wireframe_width: f32,
    /// An alpha of zero takes the node's own colour.
    pub wireframe_color: [f32; 4],
    pub wireframe_sizing: Sizing,
    /// Zero draws no vertices.
    pub dot_size: f32,
    pub dot_color: [f32; 4],
    pub dot_sizing: Sizing,
}

impl Default for DebugDraw {
    fn default() -> Self {
        Self {
            draw_surface: true,
            wireframe_width: 0.0,
            wireframe_color: [0.0; 4],
            wireframe_sizing: Sizing::World,
            dot_size: 0.0,
            dot_color: [0.0; 4],
            dot_sizing: Sizing::World,
        }
    }
}

impl DebugDraw {
    #[cfg(feature = "window")]
    /// What `entity` draws to show its geometry: its component, or the
    /// default where it has none.
    pub(crate) fn of(world: &World, entity: Entity) -> Self {
        world
            .get::<&Self>(entity)
            .map_or_else(|_| Self::default(), |found| *found)
    }

    /// Whether anything is drawn beside the surface.
    pub(crate) fn draws_lines(&self) -> bool {
        self.wireframe_width > 0.0 || self.dot_size > 0.0
    }

    #[cfg(feature = "window")]
    /// This drawing with the surface left out, as a companion draws it.
    pub(crate) fn lines_only(self) -> Self {
        Self {
            draw_surface: false,
            ..self
        }
    }

    fn from_params(params: &toml::Value) -> Self {
        let d = Self::default();
        let number = |key: &str| params.get(key).and_then(as_f64).map_or(0.0, |v| v as f32);
        let sizing = |key: &str| {
            Sizing::of(
                params
                    .get(key)
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default(),
            )
        };
        Self {
            draw_surface: params
                .get(k::DRAW_SURFACE)
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            wireframe_width: number(k::WIREFRAME_WIDTH).max(0.0),
            wireframe_color: crate::color_from_key(params, k::WIREFRAME_COLOR, d.wireframe_color),
            wireframe_sizing: sizing(k::WIREFRAME_SIZING),
            dot_size: number(k::DOT_SIZE).max(0.0),
            dot_color: crate::color_from_key(params, k::DOT_COLOR, d.dot_color),
            dot_sizing: sizing(k::DOT_SIZING),
        }
    }

    fn to_params(self) -> toml::Value {
        let float = |value: f32| toml::Value::Float(f64::from(value));
        let word = |sizing: Sizing| toml::Value::String(sizing.word().to_owned());
        let mut map = toml::map::Map::new();
        map.insert(
            k::DRAW_SURFACE.into(),
            toml::Value::Boolean(self.draw_surface),
        );
        map.insert(k::WIREFRAME_WIDTH.into(), float(self.wireframe_width));
        map.insert(
            k::WIREFRAME_COLOR.into(),
            crate::color_to_toml(self.wireframe_color),
        );
        map.insert(k::WIREFRAME_SIZING.into(), word(self.wireframe_sizing));
        map.insert(k::DOT_SIZE.into(), float(self.dot_size));
        map.insert(k::DOT_COLOR.into(), crate::color_to_toml(self.dot_color));
        map.insert(k::DOT_SIZING.into(), word(self.dot_sizing));
        toml::Value::Table(map)
    }
}

/// The sizing row of `what`: `world` scales with the view, `screen` stays
/// in pixels, in either dimension.
fn sizing_line(what: &str) -> String {
    format!(
        r#"{{ type = "enum", default = "{}", options = [{}], description = "What `{what}` counts: `world` scales with the view, `screen` is pixels" }}"#,
        words::WORLD,
        options(words::SIZINGS)
    )
}

fn schema() -> String {
    ComponentDef::schema(&[
        (
            k::DRAW_SURFACE,
            r#"{ type = "bool", default = true, description = "Whether the surface draws; off leaves the wireframe and the dots alone" }"#,
        ),
        (
            k::WIREFRAME_WIDTH,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Width of a line drawn along every triangle edge; zero draws none. Not on WebGL2" }"#,
        ),
        (
            k::WIREFRAME_COLOR,
            r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The wireframe's colour; an alpha of zero takes the node's own" }"#,
        ),
        (k::WIREFRAME_SIZING, &sizing_line(k::WIREFRAME_WIDTH)),
        (
            k::DOT_SIZE,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Size of a dot drawn on every vertex; zero draws none. Not on WebGL2" }"#,
        ),
        (
            k::DOT_COLOR,
            r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The dots' colour; an alpha of zero takes the node's own" }"#,
        ),
        (k::DOT_SIZING, &sizing_line(k::DOT_SIZE)),
    ])
}

/// The drawables a `debug_draw` shows the geometry of, any one of which
/// will do.
const DRAWABLES: &[&str] = &[
    "boolean2d",
    "boolean3d",
    "mesh",
    "multimesh2d",
    "multimesh3d",
    "particles2d",
    "particles3d",
    "polygon",
    "shape2d",
    "shape3d",
    "sprite",
    "text2d",
    "text3d",
    "tilemap",
];

pub(crate) fn register_debug_draw_component(reg: &mut Registry<'_>) {
    reg.register_component(
        k::DEBUG_DRAW,
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "A wireframe over every triangle edge of the node's drawing, a dot on every vertex, and whether its surface draws at all, for any drawable in either dimension.",
            schema: ComponentDef::parse_schema(k::DEBUG_DRAW, &schema()),
            tags: &["render"],
            expects: DRAWABLES,
            apply: Box::new(|eng, entity, params| {
                eng.world_mut()
                    .insert_one(entity, DebugDraw::from_params(params))
                    .map_err(|_| anyhow::anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<DebugDraw>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let found = world.get::<&DebugDraw>(entity).ok()?;
                Some(found.to_params())
            }),
        },
    );
}

/// A colour with no alpha is kiss3d's "take the object's own".
#[cfg(feature = "window")]
fn color_or_object(color: [f32; 4]) -> Option<kiss3d::color::Color> {
    let [r, g, b, a] = color;
    (a > 0.0).then(|| kiss3d::color::Color::new(r, g, b, a))
}

/// Put the drawing on a 3D node.
#[cfg(feature = "window")]
pub(crate) fn apply_3d(node: &mut kiss3d::scene::SceneNode3d, debug: &DebugDraw) {
    node.set_lines_width(debug.wireframe_width, debug.wireframe_sizing.perspective());
    node.set_lines_color(color_or_object(debug.wireframe_color));
    node.set_points_size(debug.dot_size, debug.dot_sizing.perspective());
    node.set_points_color(color_or_object(debug.dot_color));
    node.set_surface_rendering_activation(debug.draw_surface);
}

/// Put the drawing on a 2D node and every piece under it.
#[cfg(feature = "window")]
pub(crate) fn apply_2d(node: &mut kiss3d::scene::SceneNode2d, debug: &DebugDraw) {
    node.set_lines_width_recursive(debug.wireframe_width, debug.wireframe_sizing.perspective());
    node.set_lines_color_recursive(color_or_object(debug.wireframe_color));
    node.set_points_size_recursive(debug.dot_size, debug.dot_sizing.perspective());
    node.set_points_color_recursive(color_or_object(debug.dot_color));
    node.set_surface_rendering_activation_recursive(debug.draw_surface);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawing_reads_back_as_it_was_written() {
        let params: toml::Value = toml::from_str(
            "wireframe_width = 2.0\nwireframe_sizing = \"screen\"\nwireframe_color = [1.0, 0.0, 0.0, 1.0]\ndot_size = 4.0\ndraw_surface = false",
        )
        .unwrap();
        let debug = DebugDraw::from_params(&params);
        assert_eq!(debug.wireframe_sizing, Sizing::Screen);
        assert!(!debug.draw_surface);
        assert_eq!(DebugDraw::from_params(&debug.to_params()), debug);
    }
}
