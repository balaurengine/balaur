//! Parametric primitives: the shapes a scene names, as triangles.
//!
//! A shape is a function from its parameters to a [`MeshData`], written here
//! rather than in a renderer, so a collider fitted to a torus, a ray picking
//! one and the triangles on screen are the same triangles. A headless build
//! gets every shape, and the digest covers one a script builds.

mod build;
mod flat;
mod params;
mod solid;

pub use build::{Facets, ProfilePoint, fill, revolve};
pub use flat::{
    capsule_outline, circle_outline, ellipse_outline, ngon_outline, rect_outline, star_outline,
};
pub use solid::{ball, capsule, cone, cuboid, cylinder, plane, prism, pyramid, torus, tube};

use crate::mesh::MeshData;
use glamx::Vec2;

/// The kind each primitive answers to, in a scene, a schema and a script.
pub mod words {
    pub const BALL: &str = "ball";
    pub const CUBOID: &str = "cuboid";
    pub const CAPSULE: &str = "capsule";
    pub const CYLINDER: &str = "cylinder";
    pub const CONE: &str = "cone";
    pub const PLANE: &str = "plane";
    pub const TORUS: &str = "torus";
    pub const PYRAMID: &str = "pyramid";
    pub const PRISM: &str = "prism";
    pub const TUBE: &str = "tube";
    /// The 3D primitives, in the order an inspector offers them.
    pub const SOLIDS: &[&str] = &[
        BALL, CUBOID, CAPSULE, CYLINDER, CONE, PLANE, TORUS, PYRAMID, PRISM, TUBE,
    ];

    pub const CIRCLE: &str = "circle";
    pub const RECT: &str = "rect";
    pub const ELLIPSE: &str = "ellipse";
    pub const STAR: &str = "star";
    pub const NGON: &str = "ngon";
    /// The 2D primitives. A circle is not a ball and a rect is not a cuboid.
    pub const FLATS: &[&str] = &[CIRCLE, RECT, CAPSULE, ELLIPSE, STAR, NGON];
}

/// Every key a primitive reads, spelled once so a schema line, a scene file
/// and the reader behind them cannot drift apart.
pub mod keys {
    pub const KIND: &str = "kind";
    pub const RADIUS: &str = "radius";
    pub const HEIGHT: &str = "height";
    pub const HALF_EXTENTS: &str = "half_extents";
    pub const TUBE_RADIUS: &str = "tube_radius";
    pub const INNER_RADIUS: &str = "inner_radius";
    pub const CORNER_RADIUS: &str = "corner_radius";
    pub const SEGMENTS: &str = "segments";
    pub const RINGS: &str = "rings";
    pub const SIDES: &str = "sides";
    pub const POINTS: &str = "points";
}

/// How finely a curved primitive is cut when nothing says otherwise: around
/// the axis, along it, and how many sides or tips a faceted one takes.
pub const DEFAULT_SEGMENTS: u32 = 32;
pub const DEFAULT_RINGS: u32 = 16;
pub const DEFAULT_SIDES: u32 = 4;
pub const DEFAULT_POINTS: u32 = 5;

/// The smallest a dimension may be. A zero-radius ball is not a point, it is
/// a mesh with no triangles and a collider rapier refuses.
pub const MIN_EXTENT: f32 = 0.01;

/// A 3D primitive and its dimensions: what a node draws, before it is spun
/// into triangles by [`Solid::build`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Solid {
    Ball {
        radius: f32,
        segments: u32,
        rings: u32,
    },
    Cuboid {
        hx: f32,
        hy: f32,
        hz: f32,
        corner_radius: f32,
        segments: u32,
    },
    /// A cylinder with hemispherical caps, principal axis on y. `height` is
    /// the cylindrical part, so the whole thing is `height + 2 * radius` tall.
    Capsule {
        radius: f32,
        height: f32,
        segments: u32,
        rings: u32,
    },
    Cylinder {
        radius: f32,
        height: f32,
        segments: u32,
    },
    Cone {
        radius: f32,
        height: f32,
        segments: u32,
    },
    /// A flat quad in the xz plane, for ground and walls.
    Plane { hx: f32, hz: f32, segments: u32 },
    Torus {
        radius: f32,
        tube_radius: f32,
        segments: u32,
        rings: u32,
    },
    Pyramid {
        hx: f32,
        hy: f32,
        hz: f32,
        sides: u32,
    },
    Prism {
        radius: f32,
        height: f32,
        sides: u32,
    },
    Tube {
        radius: f32,
        inner_radius: f32,
        height: f32,
        segments: u32,
    },
}

impl Default for Solid {
    fn default() -> Self {
        Self::cuboid(0.5, 0.5, 0.5)
    }
}

impl Solid {
    /// A ball at the default tessellation, for a caller with only a radius.
    #[must_use]
    pub const fn ball(radius: f32) -> Self {
        Self::Ball {
            radius,
            segments: DEFAULT_SEGMENTS,
            rings: DEFAULT_RINGS,
        }
    }

    /// A square-edged box, for a caller with only half-extents.
    #[must_use]
    pub const fn cuboid(hx: f32, hy: f32, hz: f32) -> Self {
        Self::Cuboid {
            hx,
            hy,
            hz,
            corner_radius: 0.0,
            segments: DEFAULT_SEGMENTS,
        }
    }

    /// The triangles this shape is made of, in the node's own space.
    #[must_use]
    pub fn build(&self) -> MeshData {
        match *self {
            Self::Ball {
                radius,
                segments,
                rings,
            } => ball(radius, segments, rings),
            Self::Cuboid {
                hx,
                hy,
                hz,
                corner_radius,
                segments,
            } => cuboid(hx, hy, hz, corner_radius, segments),
            Self::Capsule {
                radius,
                height,
                segments,
                rings,
            } => capsule(radius, height, segments, rings),
            Self::Cylinder {
                radius,
                height,
                segments,
            } => cylinder(radius, height, segments),
            Self::Cone {
                radius,
                height,
                segments,
            } => cone(radius, height, segments),
            Self::Plane { hx, hz, segments } => plane(hx, hz, segments),
            Self::Torus {
                radius,
                tube_radius,
                segments,
                rings,
            } => torus(radius, tube_radius, segments, rings),
            Self::Pyramid { hx, hy, hz, sides } => pyramid(hx, hy, hz, sides),
            Self::Prism {
                radius,
                height,
                sides,
            } => prism(radius, height, sides),
            Self::Tube {
                radius,
                inner_radius,
                height,
                segments,
            } => tube(radius, inner_radius, height, segments),
        }
    }

    /// The word this shape answers to.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Ball { .. } => words::BALL,
            Self::Cuboid { .. } => words::CUBOID,
            Self::Capsule { .. } => words::CAPSULE,
            Self::Cylinder { .. } => words::CYLINDER,
            Self::Cone { .. } => words::CONE,
            Self::Plane { .. } => words::PLANE,
            Self::Torus { .. } => words::TORUS,
            Self::Pyramid { .. } => words::PYRAMID,
            Self::Prism { .. } => words::PRISM,
            Self::Tube { .. } => words::TUBE,
        }
    }

    /// Half the box this shape fills, centred on the node: what sizes a
    /// selection outline and what a ray is tested against first.
    #[must_use]
    pub fn half_extents(&self) -> [f32; 3] {
        match *self {
            Self::Ball { radius, .. } => [radius; 3],
            Self::Cuboid { hx, hy, hz, .. } => [hx, hy, hz],
            Self::Capsule { radius, height, .. } => [radius, height / 2.0 + radius, radius],
            Self::Cylinder { radius, height, .. } | Self::Cone { radius, height, .. } => {
                [radius, height / 2.0, radius]
            }
            // A quad has no thickness; picking one needs some, or the slab
            // test divides by zero and every ray misses it.
            Self::Plane { hx, hz, .. } => [hx, 1e-4, hz],
            Self::Torus {
                radius,
                tube_radius,
                ..
            } => [radius + tube_radius, tube_radius, radius + tube_radius],
            Self::Pyramid { hx, hy, hz, sides } => {
                let reach = solid::base_reach(sides);
                [hx * reach.x, hy, hz * reach.y]
            }
            Self::Prism {
                radius,
                height,
                sides,
            } => {
                let reach = solid::prism_reach(sides);
                [radius * reach.x, height / 2.0, radius * reach.y]
            }
            Self::Tube { radius, height, .. } => [radius, height / 2.0, radius],
        }
    }

    /// The three numbers a tool shows beside the kind, in the order the
    /// dimensions occupy space, so x and z always mean the same thing.
    #[must_use]
    pub fn dimensions(&self) -> [f32; 3] {
        match *self {
            Self::Ball { radius, .. } => [radius; 3],
            Self::Cuboid { hx, hy, hz, .. } | Self::Pyramid { hx, hy, hz, .. } => [hx, hy, hz],
            Self::Capsule { radius, height, .. }
            | Self::Cylinder { radius, height, .. }
            | Self::Cone { radius, height, .. }
            | Self::Prism { radius, height, .. } => [radius, height, radius],
            Self::Plane { hx, hz, .. } => [hx, 0.0, hz],
            Self::Torus {
                radius,
                tube_radius,
                ..
            } => [radius, tube_radius, radius],
            Self::Tube {
                radius,
                inner_radius,
                height,
                ..
            } => [radius, height, inner_radius],
        }
    }
}

/// A 2D primitive and its dimensions. Filling one is [`Flat::build`] and
/// tracing it is [`Flat::outline`]; a light's shadow reads the second.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Flat {
    Circle {
        radius: f32,
        segments: u32,
    },
    Ellipse {
        hx: f32,
        hy: f32,
        segments: u32,
    },
    Rect {
        hx: f32,
        hy: f32,
        corner_radius: f32,
        segments: u32,
    },
    /// The straight part is `height`; the caps add `radius` at each end, the
    /// same meaning the `collider2d` capsule gives them.
    Capsule {
        radius: f32,
        height: f32,
        segments: u32,
    },
    Star {
        points: u32,
        radius: f32,
        inner_radius: f32,
    },
    Ngon {
        sides: u32,
        radius: f32,
    },
}

impl Default for Flat {
    fn default() -> Self {
        Self::rect(0.5, 0.5)
    }
}

impl Flat {
    /// A square-cornered rectangle, for a caller with only half-extents.
    #[must_use]
    pub const fn rect(hx: f32, hy: f32) -> Self {
        Self::Rect {
            hx,
            hy,
            corner_radius: 0.0,
            segments: DEFAULT_SEGMENTS,
        }
    }

    /// A circle at the default tessellation.
    #[must_use]
    pub const fn circle(radius: f32) -> Self {
        Self::Circle {
            radius,
            segments: DEFAULT_SEGMENTS,
        }
    }

    /// The closed outline, counter-clockwise.
    #[must_use]
    pub fn outline(&self) -> Vec<Vec2> {
        match *self {
            Self::Circle { radius, segments } => circle_outline(radius, segments),
            Self::Ellipse { hx, hy, segments } => ellipse_outline(hx, hy, segments),
            Self::Rect {
                hx,
                hy,
                corner_radius,
                segments,
            } => rect_outline(hx, hy, corner_radius, segments),
            Self::Capsule {
                radius,
                height,
                segments,
            } => capsule_outline(radius, height, segments),
            Self::Star {
                points,
                radius,
                inner_radius,
            } => star_outline(points, radius, inner_radius),
            Self::Ngon { sides, radius } => ngon_outline(sides, radius),
        }
    }

    /// The outline filled with triangles, in the z = 0 plane.
    #[must_use]
    pub fn build(&self) -> MeshData {
        fill(&self.outline())
    }

    /// The word this shape answers to.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Circle { .. } => words::CIRCLE,
            Self::Ellipse { .. } => words::ELLIPSE,
            Self::Rect { .. } => words::RECT,
            Self::Capsule { .. } => words::CAPSULE,
            Self::Star { .. } => words::STAR,
            Self::Ngon { .. } => words::NGON,
        }
    }

    /// Half the box this shape fills, centred on the node.
    #[must_use]
    pub fn half_extents(&self) -> [f32; 2] {
        match *self {
            Self::Circle { radius, .. } => [radius; 2],
            Self::Ellipse { hx, hy, .. } | Self::Rect { hx, hy, .. } => [hx, hy],
            Self::Capsule { radius, height, .. } => [radius, height / 2.0 + radius],
            // A polygon is narrower than its radius on at least one axis, and
            // a star alternates two radii, so both are measured rather than
            // assumed round.
            Self::Ngon { sides, radius } => (radius * flat::ngon_reach(sides)).to_array(),
            Self::Star {
                points,
                radius,
                inner_radius,
            } => (radius * flat::star_reach(points, true))
                .max(inner_radius * flat::star_reach(points, false))
                .to_array(),
        }
    }

    /// The two numbers a tool shows beside the kind.
    #[must_use]
    pub fn dimensions(&self) -> [f32; 2] {
        match *self {
            Self::Capsule { radius, height, .. } => [radius, height],
            other => other.half_extents(),
        }
    }
}
