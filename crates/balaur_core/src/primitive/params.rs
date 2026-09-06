//! Primitives as property tables: one reader and one writer, so a `shape3d`
//! component, a `mesh` asset naming a shape and a scene round-trip all spell
//! the same keys with the same defaults.

use super::{
    DEFAULT_POINTS, DEFAULT_RINGS, DEFAULT_SEGMENTS, DEFAULT_SIDES, Flat, MIN_EXTENT, Solid, keys,
    words,
};
use anyhow::{Result, anyhow};
use toml::Value;

/// A float property, floored at [`MIN_EXTENT`] so no dimension is zero.
fn extent(params: &Value, key: &str, fallback: f32) -> f32 {
    params
        .get(key)
        .and_then(crate::components::as_f64)
        .map_or(fallback, |v| v as f32)
        .max(MIN_EXTENT)
}

/// A float property that is allowed to be zero: a corner radius of none is a
/// square corner, not a refused one.
fn optional(params: &Value, key: &str, fallback: f32) -> f32 {
    params
        .get(key)
        .and_then(crate::components::as_f64)
        .map_or(fallback, |v| v as f32)
        .max(0.0)
}

/// A count property, floored at three: nothing curved survives fewer.
fn count(params: &Value, key: &str, fallback: u32) -> u32 {
    params
        .get(key)
        .and_then(crate::components::as_f64)
        .map_or(fallback, |v| v as u32)
        .max(3)
}

/// One component of a `half_extents` array.
fn half(params: &Value, index: usize) -> f32 {
    params
        .get(keys::HALF_EXTENTS)
        .and_then(Value::as_array)
        .and_then(|a| a.get(index))
        .and_then(crate::components::as_f64)
        .map_or(0.5, |v| v as f32)
        .max(MIN_EXTENT)
}

fn float(value: f32) -> Value {
    Value::Float(f64::from(value))
}

fn integer(value: u32) -> Value {
    Value::Integer(i64::from(value))
}

fn extents(values: [f32; 3]) -> Value {
    Value::Array(values.into_iter().map(float).collect())
}

impl Solid {
    /// The shape a property table describes.
    ///
    /// # Errors
    /// If `kind` names no primitive.
    pub fn from_params(params: &Value) -> Result<Self> {
        let kind = params
            .get(keys::KIND)
            .and_then(Value::as_str)
            .unwrap_or(words::CUBOID);
        let radius = extent(params, keys::RADIUS, 0.5);
        let height = extent(params, keys::HEIGHT, 1.0);
        let segments = count(params, keys::SEGMENTS, DEFAULT_SEGMENTS);
        let rings = count(params, keys::RINGS, DEFAULT_RINGS);
        let sides = count(params, keys::SIDES, DEFAULT_SIDES);
        Ok(match kind {
            words::BALL => Self::Ball {
                radius,
                segments,
                rings,
            },
            words::CUBOID => Self::Cuboid {
                hx: half(params, 0),
                hy: half(params, 1),
                hz: half(params, 2),
                corner_radius: optional(params, keys::CORNER_RADIUS, 0.0),
                segments,
            },
            words::CAPSULE => Self::Capsule {
                radius,
                height,
                segments,
                rings,
            },
            words::CYLINDER => Self::Cylinder {
                radius,
                height,
                segments,
            },
            words::CONE => Self::Cone {
                radius,
                height,
                segments,
            },
            words::PLANE => Self::Plane {
                hx: half(params, 0),
                hz: half(params, 2),
                segments,
            },
            words::TORUS => Self::Torus {
                radius,
                tube_radius: extent(params, keys::TUBE_RADIUS, 0.2),
                segments,
                rings,
            },
            words::PYRAMID => Self::Pyramid {
                hx: half(params, 0),
                hy: half(params, 1),
                hz: half(params, 2),
                sides,
            },
            words::PRISM => Self::Prism {
                radius,
                height,
                sides,
            },
            words::TUBE => Self::Tube {
                radius,
                inner_radius: extent(params, keys::INNER_RADIUS, 0.25),
                height,
                segments,
            },
            other => return Err(anyhow!("unknown shape kind '{other}'")),
        })
    }

    /// The property table this shape reads back as: its kind, the dimensions
    /// that kind uses, and the tessellation, and nothing that would confuse
    /// an inspector by belonging to another kind.
    #[must_use]
    pub fn to_params(self) -> Value {
        let mut map = toml::map::Map::new();
        map.insert(keys::KIND.into(), Value::String(self.kind().into()));
        let mut put = |key: &str, value: Value| {
            map.insert(key.into(), value);
        };
        match self {
            Self::Ball {
                radius,
                segments,
                rings,
            } => {
                put(keys::RADIUS, float(radius));
                put(keys::SEGMENTS, integer(segments));
                put(keys::RINGS, integer(rings));
            }
            Self::Cuboid {
                hx,
                hy,
                hz,
                corner_radius,
                segments,
            } => {
                put(keys::HALF_EXTENTS, extents([hx, hy, hz]));
                put(keys::CORNER_RADIUS, float(corner_radius));
                put(keys::SEGMENTS, integer(segments));
            }
            Self::Capsule {
                radius,
                height,
                segments,
                rings,
            } => {
                put(keys::RADIUS, float(radius));
                put(keys::HEIGHT, float(height));
                put(keys::SEGMENTS, integer(segments));
                put(keys::RINGS, integer(rings));
            }
            Self::Cylinder {
                radius,
                height,
                segments,
            }
            | Self::Cone {
                radius,
                height,
                segments,
            } => {
                put(keys::RADIUS, float(radius));
                put(keys::HEIGHT, float(height));
                put(keys::SEGMENTS, integer(segments));
            }
            Self::Plane { hx, hz, segments } => {
                put(keys::HALF_EXTENTS, extents([hx, 0.0, hz]));
                put(keys::SEGMENTS, integer(segments));
            }
            Self::Torus {
                radius,
                tube_radius,
                segments,
                rings,
            } => {
                put(keys::RADIUS, float(radius));
                put(keys::TUBE_RADIUS, float(tube_radius));
                put(keys::SEGMENTS, integer(segments));
                put(keys::RINGS, integer(rings));
            }
            Self::Pyramid { hx, hy, hz, sides } => {
                put(keys::HALF_EXTENTS, extents([hx, hy, hz]));
                put(keys::SIDES, integer(sides));
            }
            Self::Prism {
                radius,
                height,
                sides,
            } => {
                put(keys::RADIUS, float(radius));
                put(keys::HEIGHT, float(height));
                put(keys::SIDES, integer(sides));
            }
            Self::Tube {
                radius,
                inner_radius,
                height,
                segments,
            } => {
                put(keys::RADIUS, float(radius));
                put(keys::INNER_RADIUS, float(inner_radius));
                put(keys::HEIGHT, float(height));
                put(keys::SEGMENTS, integer(segments));
            }
        }
        Value::Table(map)
    }
}

impl Flat {
    /// The 2D shape a property table describes.
    ///
    /// # Errors
    /// If `kind` names no 2D primitive.
    pub fn from_params(params: &Value) -> Result<Self> {
        let kind = params
            .get(keys::KIND)
            .and_then(Value::as_str)
            .unwrap_or(words::RECT);
        let radius = extent(params, keys::RADIUS, 0.5);
        let segments = count(params, keys::SEGMENTS, DEFAULT_SEGMENTS);
        Ok(match kind {
            words::CIRCLE => Self::Circle { radius, segments },
            words::ELLIPSE => Self::Ellipse {
                hx: half(params, 0),
                hy: half(params, 1),
                segments,
            },
            words::RECT => Self::Rect {
                hx: half(params, 0),
                hy: half(params, 1),
                corner_radius: optional(params, keys::CORNER_RADIUS, 0.0),
                segments,
            },
            words::CAPSULE => Self::Capsule {
                radius,
                height: extent(params, keys::HEIGHT, 1.0),
                segments,
            },
            words::STAR => Self::Star {
                points: count(params, keys::POINTS, DEFAULT_POINTS),
                radius,
                inner_radius: extent(params, keys::INNER_RADIUS, 0.2),
            },
            words::NGON => Self::Ngon {
                sides: count(params, keys::SIDES, DEFAULT_SIDES),
                radius,
            },
            other => return Err(anyhow!("unknown shape2d kind '{other}'")),
        })
    }

    /// The property table this shape reads back as.
    #[must_use]
    pub fn to_params(self) -> Value {
        let mut map = toml::map::Map::new();
        map.insert(keys::KIND.into(), Value::String(self.kind().into()));
        let mut put = |key: &str, value: Value| {
            map.insert(key.into(), value);
        };
        let pair = |hx: f32, hy: f32| Value::Array(vec![float(hx), float(hy)]);
        match self {
            Self::Circle { radius, segments } => {
                put(keys::RADIUS, float(radius));
                put(keys::SEGMENTS, integer(segments));
            }
            Self::Ellipse { hx, hy, segments } => {
                put(keys::HALF_EXTENTS, pair(hx, hy));
                put(keys::SEGMENTS, integer(segments));
            }
            Self::Rect {
                hx,
                hy,
                corner_radius,
                segments,
            } => {
                put(keys::HALF_EXTENTS, pair(hx, hy));
                put(keys::CORNER_RADIUS, float(corner_radius));
                put(keys::SEGMENTS, integer(segments));
            }
            Self::Capsule {
                radius,
                height,
                segments,
            } => {
                put(keys::RADIUS, float(radius));
                put(keys::HEIGHT, float(height));
                put(keys::SEGMENTS, integer(segments));
            }
            Self::Star {
                points,
                radius,
                inner_radius,
            } => {
                put(keys::POINTS, integer(points));
                put(keys::RADIUS, float(radius));
                put(keys::INNER_RADIUS, float(inner_radius));
            }
            Self::Ngon { sides, radius } => {
                put(keys::SIDES, integer(sides));
                put(keys::RADIUS, float(radius));
            }
        }
        Value::Table(map)
    }
}
