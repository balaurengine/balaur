//! The `debug_draw` component: a wireframe over every triangle edge of a
//! drawn node, a dot on every vertex, and whether its surface draws at all.
//!
//! One component for every drawable of either dimension, kept on the node
//! beside the drawable rather than in fifteen copies of its rows, and read
//! wherever that drawable's flags are pushed onto the scene. kiss3d's own
//! materials draw it; a node drawn through one of Balaur's pipelines draws it
//! through a companion where it can (`overlay::companion_3d`), and a skinned
//! mesh, a skinned polygon and text drawn through a material do not yet.

use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Enum};
#[cfg(feature = "window")]
use balaur_core::hecs::{Entity, World};
use balaur_plugin::Registry;

/// Whether a wireframe's width or a vertex's size scales with the view or
/// stays in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Sizing {
    World,
    Screen,
}

impl Sizing {
    /// kiss3d's `use_perspective` flag.
    #[cfg(feature = "window")]
    pub(crate) fn perspective(self) -> bool {
        self == Self::World
    }
}

/// A wireframe over every triangle edge of the node's drawing, a dot on every vertex, and whether its surface draws at all, for any drawable in either dimension.
// The default draws the surface and nothing else, which is what a node
// without the component draws.
#[derive(Clone, Copy, Debug, PartialEq, Component)]
#[component(name = "debug_draw")]
pub struct DebugDraw {
    /// Whether the surface draws; off leaves the wireframe and the dots alone
    #[property(default = true)]
    pub draw_surface: bool,
    /// Width of a line drawn along every triangle edge; zero draws none. Not on WebGL2
    #[property(min = 0.0)]
    pub wireframe_width: f32,
    /// The wireframe's colour; an alpha of zero takes the node's own
    #[property(color)]
    pub wireframe_color: [f32; 4],
    /// What `wireframe_width` counts: `world` scales with the view, `screen` is pixels
    #[property(default = Sizing::World)]
    pub wireframe_sizing: Sizing,
    /// Size of a dot drawn on every vertex; zero draws none. Not on WebGL2
    #[property(min = 0.0)]
    pub dot_size: f32,
    /// The dots' colour; an alpha of zero takes the node's own
    #[property(color)]
    pub dot_color: [f32; 4],
    /// What `dot_size` counts: `world` scales with the view, `screen` is pixels
    #[property(default = Sizing::World)]
    pub dot_sizing: Sizing,
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
        DebugDraw::NAME,
        ComponentDef {
            tags: &["render"],
            expects: DRAWABLES,
            ..ComponentDef::of::<DebugDraw>(None)
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
    node.set_lines_width(
        debug.wireframe_width.max(0.0),
        debug.wireframe_sizing.perspective(),
    );
    node.set_lines_color(color_or_object(debug.wireframe_color));
    node.set_points_size(debug.dot_size.max(0.0), debug.dot_sizing.perspective());
    node.set_points_color(color_or_object(debug.dot_color));
    node.set_surface_rendering_activation(debug.draw_surface);
}

/// Put the drawing on a 2D node and every piece under it.
#[cfg(feature = "window")]
pub(crate) fn apply_2d(node: &mut kiss3d::scene::SceneNode2d, debug: &DebugDraw) {
    node.set_lines_width_recursive(
        debug.wireframe_width.max(0.0),
        debug.wireframe_sizing.perspective(),
    );
    node.set_lines_color_recursive(color_or_object(debug.wireframe_color));
    node.set_points_size_recursive(debug.dot_size.max(0.0), debug.dot_sizing.perspective());
    node.set_points_color_recursive(color_or_object(debug.dot_color));
    node.set_surface_rendering_activation_recursive(debug.draw_surface);
}

#[cfg(test)]
mod tests {
    use balaur_core::components::derived::{from_table, to_table};

    use super::*;

    #[test]
    fn a_drawing_reads_back_as_it_was_written() {
        let params: toml::Value = toml::from_str(
            "wireframe_width = 2.0\nwireframe_sizing = \"screen\"\nwireframe_color = [1.0, 0.0, 0.0, 1.0]\ndot_size = 4.0\ndraw_surface = false",
        )
        .unwrap();
        let debug: DebugDraw = from_table(&params).unwrap();
        assert_eq!(debug.wireframe_sizing, Sizing::Screen);
        assert!(!debug.draw_surface);
        assert_eq!(from_table::<DebugDraw>(&to_table(&debug)).unwrap(), debug);
    }
}
