//! The `shape3d` and `shape2d` components and the `render.set_*` shape API:
//! untextured primitives, in both dimensions.

use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Enum, Property, Record, Variant, Written};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_geometry::primitive::{Flat, Solid};
use balaur_geometry::stroke::{Cap, Join, Stroke};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::vocabulary::words;
use crate::{
    Renderable2d, Renderable3d, Shape2d, Shape3d, set_color, set_polyline, set_shape, set_shape2d,
};
use balaur_core::layers::Layer;

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

/// What a `shape3d` draws, and the dimensions that primitive reads: the
/// table the mesher builds from (`Solid::from_params`).
#[derive(Clone, Debug, PartialEq, Variant)]
pub enum SolidShape {
    /// A ball
    Sphere(SphereSize),
    /// A box, its edges rounded when `corner_radius` is above zero
    #[option(default)]
    Box(BoxSize),
    /// A cylinder with round ends
    Capsule(CapsuleSize),
    /// A cylinder standing along y
    Cylinder(RoundSize),
    /// A cone standing along y, its point up
    Cone(RoundSize),
    /// A flat quad in the xz plane, for ground and walls
    Plane(PlaneSize),
    /// A ring
    Torus(TorusSize),
    /// A pyramid on a regular base
    Pyramid(PyramidSize),
    /// A prism on a regular base, standing along y
    Prism(PrismSize),
    /// A cylinder with a hole down its axis
    Tube(TubeSize),
}

/// A sphere's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct SphereSize {
    /// Radius
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Cuts around the axis
    #[property(default = 32, min = 3)]
    pub segments: i64,
    /// Cuts along the axis
    #[property(default = 16, min = 3)]
    pub rings: i64,
}

/// A box's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct BoxSize {
    /// Whole size along each axis
    #[property(default = [1.0, 1.0, 1.0])]
    pub size: [f32; 3],
    /// How far the edges are rounded off; zero is a square edge
    #[property(min = 0.0)]
    pub corner_radius: f32,
    /// Cuts around a rounded edge
    #[property(default = 32, min = 3)]
    pub segments: i64,
}

/// A capsule's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct CapsuleSize {
    /// Radius
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Length along y, tip to tip
    #[property(default = 2.0, min = 0.01)]
    pub height: f32,
    /// Cuts around the axis
    #[property(default = 32, min = 3)]
    pub segments: i64,
    /// Cuts across each round end
    #[property(default = 16, min = 3)]
    pub rings: i64,
}

/// A cylinder's or a cone's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct RoundSize {
    /// Radius of the base
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Length along y, base to top
    #[property(default = 2.0, min = 0.01)]
    pub height: f32,
    /// Cuts around the axis
    #[property(default = 32, min = 3)]
    pub segments: i64,
}

/// A plane's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct PlaneSize {
    /// Whole size along x and z; y is not read
    #[property(default = [1.0, 0.0, 1.0])]
    pub size: [f32; 3],
    /// Cuts across each side
    #[property(default = 32, min = 3)]
    pub segments: i64,
}

/// A torus's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct TorusSize {
    /// From the centre to the middle of the ring
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Thickness of the ring
    #[property(default = 0.2, min = 0.01)]
    pub tube_radius: f32,
    /// Cuts around the centre
    #[property(default = 32, min = 3)]
    pub segments: i64,
    /// Cuts around the ring
    #[property(default = 16, min = 3)]
    pub rings: i64,
}

/// A pyramid's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct PyramidSize {
    /// Whole size along each axis
    #[property(default = [1.0, 1.0, 1.0])]
    pub size: [f32; 3],
    /// Flat faces around the base
    #[property(default = 4, min = 3)]
    pub sides: i64,
}

/// A prism's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct PrismSize {
    /// From the axis to each corner of the base
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Length along y
    #[property(default = 2.0, min = 0.01)]
    pub height: f32,
    /// Flat faces around the axis
    #[property(default = 4, min = 3)]
    pub sides: i64,
}

/// A tube's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct TubeSize {
    /// Outer radius
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Radius of the hole
    #[property(default = 0.25, min = 0.01)]
    pub inner_radius: f32,
    /// Length along y
    #[property(default = 2.0, min = 0.01)]
    pub height: f32,
    /// Cuts around the axis
    #[property(default = 32, min = 3)]
    pub segments: i64,
}

/// What a `shape2d` draws, and the dimensions that shape reads: the table
/// the mesher builds from (`Flat::from_params`).
#[derive(Clone, Debug, PartialEq, Variant)]
pub enum FlatShape {
    /// A disc
    Circle(CircleSize),
    /// A rectangle, its corners rounded when `corner_radius` is above zero
    #[option(default)]
    Rectangle(RectangleSize),
    /// A rectangle with round ends, standing along y
    Capsule(FlatCapsuleSize),
    /// An ellipse
    Ellipse(EllipseSize),
    /// A star
    Star(StarSize),
    /// A regular polygon
    Ngon(NgonSize),
}

/// A circle's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct CircleSize {
    /// Radius
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Cuts around the edge
    #[property(default = 32, min = 3)]
    pub segments: i64,
}

/// A rectangle's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct RectangleSize {
    /// Whole size along each axis
    #[property(default = [1.0, 1.0])]
    pub size: [f32; 2],
    /// How far the corners are rounded off; zero is a square corner
    #[property(min = 0.0)]
    pub corner_radius: f32,
    /// Cuts around a rounded corner
    #[property(default = 32, min = 3)]
    pub segments: i64,
}

/// A 2D capsule's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct FlatCapsuleSize {
    /// Radius of the round ends
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// Length along y, tip to tip
    #[property(default = 2.0, min = 0.01)]
    pub height: f32,
    /// Cuts around each round end
    #[property(default = 32, min = 3)]
    pub segments: i64,
}

/// An ellipse's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct EllipseSize {
    /// Whole size along each axis
    #[property(default = [1.0, 1.0])]
    pub size: [f32; 2],
    /// Cuts around the edge
    #[property(default = 32, min = 3)]
    pub segments: i64,
}

/// A star's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct StarSize {
    /// Tips
    #[property(default = 5, min = 3)]
    pub points: i64,
    /// From the centre to a tip
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
    /// From the centre to the notch between two tips
    #[property(default = 0.2, min = 0.01)]
    pub inner_radius: f32,
}

/// A regular polygon's dimensions.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct NgonSize {
    /// Sides
    #[property(default = 4, min = 3)]
    pub sides: i64,
    /// From the centre to each corner
    #[property(default = 0.5, min = 0.01)]
    pub radius: f32,
}

/// How a polyline's segments meet: `balaur_geometry::stroke::Join`'s words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum LineJoin {
    Round,
    Miter,
    Bevel,
}

/// How an open polyline ends: `balaur_geometry::stroke::Cap`'s words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum LineCap {
    Round,
    Butt,
    Square,
}

/// A 3D primitive at the node, tinted by `color` and drawn with `texture`: `shape` is a `sphere`, `box`, `capsule`, `cylinder`, `cone`, `plane`, `torus`, `pyramid`, `prism` or `tube`, and the dimensions that kind reads.
// The `shape3d` component; the node draws the [`Renderable3d`] each write
// leaves beside it.
#[derive(Component)]
#[component(name = "shape3d", on_remove = shape3d_removed)]
pub struct Primitive3d {
    /// The primitive drawn, and its dimensions
    pub shape: SolidShape,
    /// Tint, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [0.8, 0.8, 0.8, 1.0])]
    pub color: [f32; 4],
    /// Image file, project-relative, or a `texture` asset, drawn over the shape's own UVs; empty draws the colour alone
    #[property(asset = "texture")]
    pub texture: String,
    /// The material this draws with; empty draws with the built-in one
    #[property(asset = "material")]
    pub material: String,
    /// Whether this casts a shadow from the lights that cast
    #[property(default = true, group = "shadows")]
    pub cast_shadow: bool,
    /// The layers this is drawn on: a `camera3d` draws it when their `render_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub render_layers: Vec<Layer>,
    /// The light layers this is lit on: a `light3d` lights it when their `light_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub light_layers: Vec<Layer>,
    /// Whether shadows land on it; off lights it as if nothing stood between it and every light
    #[property(default = true, group = "shadows")]
    pub receive_shadows: bool,
    /// The id `render.snap_aov("segmentation")` colours this node by; nodes sharing one share a colour. Zero takes one of its own, from 1 up
    #[property(min = 0, group = "layers")]
    pub segmentation_id: i64,
}

/// A 2D primitive at the node, tinted by `color` and drawn with `texture`: `shape` is a `circle`, `rectangle`, `capsule`, `ellipse`, `star` or `ngon`, and the dimensions that kind reads. A stroked line is `line2d`.
// The `shape2d` component; the node draws the [`Renderable2d`] each write
// leaves beside it.
#[derive(Component)]
#[component(name = "shape2d", on_remove = shape2d_removed)]
pub struct Primitive2d {
    /// The shape drawn, and its dimensions
    pub shape: FlatShape,
    /// Tint, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [0.8, 0.8, 0.8, 1.0])]
    pub color: [f32; 4],
    /// An image, or a `texture` asset, the shape draws with over its own UVs
    #[property(asset = "texture")]
    pub texture: String,
    /// The material this draws with; empty draws with the built-in one
    #[property(asset = "material")]
    pub material: String,
}

/// A stroked line at the node through the points of a `mesh` or `path2d` asset, tinted by `color` and drawn with `texture`.
// The node draws the [`Renderable2d`] each write leaves beside it, as a
// `shape2d` does.
#[derive(Component)]
#[component(name = "line2d", on_remove = line2d_removed)]
pub struct Line2d {
    /// Where the line's points come from: a `mesh` asset's vertices, or a `path2d` asset, which is sampled into points and so draws as a stroked curve
    #[property(asset = "mesh")]
    pub mesh: String,
    /// Tint, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [0.8, 0.8, 0.8, 1.0])]
    pub color: [f32; 4],
    /// An image, or a `texture` asset, the line draws with, repeating once per world unit of its length
    #[property(asset = "texture")]
    pub texture: String,
    /// The material this draws with; empty draws with the built-in one
    #[property(asset = "material")]
    pub material: String,
    /// Join the last point back to the first, making a polygon outline
    pub closed: bool,
    /// Line thickness in world units
    #[property(default = 0.02, min = 0.001)]
    pub width: f32,
    /// Multipliers on `width` at the line's start and end, blended along it; anything but [1, 1] draws round joins and caps
    #[property(default = [1.0, 1.0])]
    pub taper: [f32; 2],
    /// How the line's segments meet
    #[property(default = LineJoin::Round)]
    pub join: LineJoin,
    /// How an open line ends
    #[property(default = LineCap::Round)]
    pub cap: LineCap,
    /// How far a miter join may reach, in half-widths, before its corner is cut to a bevel
    #[property(default = 4.0, min = 1.0)]
    pub miter_limit: f32,
    /// Cuts around a round join or cap
    #[property(default = 32, min = 3)]
    pub segments: i64,
    /// The colour the line fades to at its far end, from `color` at its start; a zero alpha means no gradient
    #[property(color, group = "gradient")]
    pub gradient: [f32; 4],
    /// How many colours the gradient steps through along the line
    #[property(default = 32, min = 1, group = "gradient")]
    pub gradient_steps: i64,
}

/// What a write of `shape3d` leaves to be worked out again.
fn shape3d_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    shape3d_laid_out(eng, entity)
}

/// A `shape3d`'s primitive, tint, texture, lighting and material, worked
/// out again from what was written. The dimensions are read by the mesher's
/// own reader, so what a scene names is what a mesh can be built from.
fn shape3d_laid_out(eng: &Engine, entity: Entity) -> Result<()> {
    let (solid, color, texture, material, lighting) = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Primitive3d>(entity) else {
            return Ok(());
        };
        (
            Solid::from_params(&asked.shape.to_value()),
            asked.color,
            asked.texture.clone(),
            asked.material.clone(),
            crate::Lighting3d::of(
                asked.cast_shadow,
                &asked.light_layers,
                &asked.render_layers,
                asked.receive_shadows,
                asked.segmentation_id,
            ),
        )
    };
    set_shape(eng, entity, Shape3d::Solid(solid?))?;
    set_color(eng, entity, color)?;
    crate::set_texture_3d(eng, entity, &texture)?;
    crate::set_lighting(eng, entity, &lighting);
    crate::material::set_material_3d(eng, entity, &material)
}

/// What a removed `Primitive3d` leaves behind, taken with it.
fn shape3d_removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<Renderable3d>(entity);
}

pub(crate) fn register_shape_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Primitive3d::NAME,
        ComponentDef {
            tags: &[words::PERSPECTIVE, "render"],
            ..ComponentDef::of::<Primitive3d>(Some(shape3d_written))
        },
    );
}

/// What a write of `shape2d` leaves to be worked out again.
fn shape2d_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    shape2d_laid_out(eng, entity)
}

/// A `shape2d`'s shape, tint, texture and material, worked out again from
/// what was written.
fn shape2d_laid_out(eng: &Engine, entity: Entity) -> Result<()> {
    let (flat, color, texture, material) = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Primitive2d>(entity) else {
            return Ok(());
        };
        (
            Flat::from_params(&asked.shape.to_value()),
            asked.color,
            asked.texture.clone(),
            asked.material.clone(),
        )
    };
    set_shape2d(eng, entity, Shape2d::Flat(flat?))?;
    set_color(eng, entity, color)?;
    crate::set_texture_2d(eng, entity, &texture)?;
    crate::material::set_material_2d(eng, entity, &material)
}

/// What a write of `line2d` leaves to be worked out again.
fn line2d_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    line2d_laid_out(eng, entity)
}

/// A `line2d`'s stroke, the points it follows, its tint, texture and
/// material, worked out again from what was written.
fn line2d_laid_out(eng: &Engine, entity: Entity) -> Result<()> {
    let (stroke, source, style, color, texture, material) = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Line2d>(entity) else {
            return Ok(());
        };
        let stroke = Stroke {
            width: asked.width.max(0.001),
            closed: asked.closed,
            join: Join::from_word(asked.join.option()).unwrap_or_default(),
            cap: Cap::from_word(asked.cap.option()).unwrap_or_default(),
            miter_limit: asked.miter_limit,
            taper: asked.taper,
            segments: u32::try_from(asked.segments.max(1)).unwrap_or(u32::MAX),
        };
        // A gradient with no alpha is no gradient: the schema's default.
        let style = crate::LineStyle {
            gradient: Some(asked.gradient).filter(|gradient| gradient[3] > 0.0),
            gradient_steps: u32::try_from(asked.gradient_steps.max(1)).unwrap_or(u32::MAX),
        };
        (
            stroke,
            asked.mesh.clone(),
            style,
            asked.color,
            asked.texture.clone(),
            asked.material.clone(),
        )
    };
    set_polyline(eng, entity, source, Shape2d::Polyline(stroke), style)?;
    set_color(eng, entity, color)?;
    crate::set_texture_2d(eng, entity, &texture)?;
    crate::material::set_material_2d(eng, entity, &material)
}

/// What a removed `Primitive2d` leaves behind, taken with it.
fn shape2d_removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<Renderable2d>(entity);
}

/// The `shape2d` component: 2D primitives.
pub(crate) fn register_shape2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Primitive2d::NAME,
        ComponentDef {
            tags: &[words::ORTHOGRAPHIC, "render"],
            ..ComponentDef::of::<Primitive2d>(Some(shape2d_written))
        },
    );
}

/// What a removed `Line2d` leaves behind, taken with it.
fn line2d_removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<Renderable2d>(entity);
}

/// The `line2d` component: a stroked line through an asset's points.
pub(crate) fn register_line2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Line2d::NAME,
        ComponentDef {
            tags: &[words::ORTHOGRAPHIC, "render"],
            ..ComponentDef::of::<Line2d>(Some(line2d_written))
        },
    );
}

#[cfg(test)]
mod tests {
    use balaur_geometry::primitive::words as primitive;
    use balaur_geometry::stroke;

    use super::{FlatShape, LineCap, LineJoin, SolidShape};

    /// The kinds a shape offers are the ones the mesher builds, and a line's
    /// joins and caps the ones a stroke reads: core owns each list.
    #[test]
    fn a_shape_offers_the_words_core_reads() {
        assert_eq!(SolidShape::OPTIONS, primitive::SOLIDS);
        assert_eq!(FlatShape::OPTIONS, primitive::FLATS);
        assert_eq!(LineJoin::OPTIONS, stroke::JOINS);
        assert_eq!(LineCap::OPTIONS, stroke::CAPS);
    }
}
