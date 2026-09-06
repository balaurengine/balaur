//! Bezier paths: the shape a pen tool draws, as an asset a scene names.
//!
//! A path carries cubic segments and nothing else; what turns one into
//! triangles is [`shape`], and what strokes one is the renderer's polyline.
//! Sampling happens here, on `libm`, so the points a collider is fitted to
//! are the points on screen.

pub mod shape;

use crate::App;
use anyhow::{Result, bail};
use glamx::{Vec2, Vec3};

pub use shape::{extrude, lathe, sweep};

/// The `path2d` asset type: a flat outline, profile or rail.
pub const PATH2D_ASSET_TYPE: &str = "path2d";
/// The `path3d` asset type: the same thing in space.
pub const PATH3D_ASSET_TYPE: &str = "path3d";

/// How far a flattened curve may sit from the true one, in world units.
/// Two millimetres at a metre scale: below what a screen resolves and above
/// what fills a mesh with points nobody asked for.
pub const TOLERANCE: f32 = 0.002;

/// The most times one curve is halved. A cusp would otherwise recurse until
/// the tolerance is smaller than the numbers can express.
const MAX_DEPTH: u32 = 10;

/// A point on a path that can be mixed with its neighbour: what flattening
/// needs and all it needs, so one routine serves both dimensions.
pub trait Anchor: Copy {
    #[must_use]
    fn mix(self, other: Self, t: f32) -> Self;
    #[must_use]
    fn away_from(self, from: Self, to: Self) -> f32;
}

impl Anchor for Vec2 {
    fn mix(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }

    fn away_from(self, from: Self, to: Self) -> f32 {
        let axis = to - from;
        let length = axis.length();
        if length <= f32::MIN_POSITIVE {
            return (self - from).length();
        }
        ((self - from).perp_dot(axis) / length).abs()
    }
}

impl Anchor for Vec3 {
    fn mix(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }

    fn away_from(self, from: Self, to: Self) -> f32 {
        let axis = to - from;
        let length = axis.length();
        if length <= f32::MIN_POSITIVE {
            return (self - from).length();
        }
        (self - from).cross(axis).length() / length
    }
}

/// One cubic, halved until every control point is within `tolerance` of the
/// chord, appending everything after the first point.
///
/// Recursive rather than stepped, so a straight-ish segment costs two points
/// and a tight curl costs what it needs.
pub fn flatten<P: Anchor>(a: P, b: P, c: P, d: P, tolerance: f32, out: &mut Vec<P>) {
    fn halve<P: Anchor>(a: P, b: P, c: P, d: P, tolerance: f32, depth: u32, out: &mut Vec<P>) {
        let flat = b.away_from(a, d).max(c.away_from(a, d));
        if depth >= MAX_DEPTH || flat <= tolerance {
            out.push(d);
            return;
        }
        let (ab, bc, cd) = (a.mix(b, 0.5), b.mix(c, 0.5), c.mix(d, 0.5));
        let (abc, bcd) = (ab.mix(bc, 0.5), bc.mix(cd, 0.5));
        let middle = abc.mix(bcd, 0.5);
        halve(a, ab, abc, middle, tolerance, depth + 1, out);
        halve(middle, bcd, cd, d, tolerance, depth + 1, out);
    }
    halve(a, b, c, d, tolerance, 0, out);
}

/// A path as its author wrote it: a run of cubic control points, four for
/// the first segment and three more for each after it.
///
/// A closed path's last segment returns to the first point, so it carries a
/// multiple of three; two points on their own are read as a straight line,
/// which is the one shape nobody wants to spell in cubics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path<P> {
    pub points: Vec<P>,
    pub closed: bool,
}

/// A flat path: an outline to fill, a profile to revolve, a line to stroke.
pub type Path2d = Path<Vec2>;
/// A path in space: the rail a profile is swept along.
pub type Path3d = Path<Vec3>;

impl<P: Anchor> Path<P> {
    /// The path as a polyline, one point per corner and as many as each
    /// curve needs. A closed path does not repeat its first point.
    ///
    /// # Errors
    /// If the control points do not divide into whole cubic segments.
    pub fn sample(&self, tolerance: f32) -> Result<Vec<P>> {
        let count = self.points.len();
        if count == 2 && !self.closed {
            return Ok(self.points.clone());
        }
        let segments = self.segment_count()?;
        let mut out = Vec::with_capacity(count * 4);
        out.push(self.points[0]);
        for segment in 0..segments {
            let at = |offset: usize| self.points[(segment * 3 + offset) % count];
            flatten(at(0), at(1), at(2), at(3), tolerance, &mut out);
        }
        if self.closed {
            out.pop();
        }
        Ok(out)
    }

    /// How many cubics the control points make.
    ///
    /// # Errors
    /// If there are too few, or the count leaves a segment unfinished.
    pub fn segment_count(&self) -> Result<usize> {
        let count = self.points.len();
        if self.closed {
            if count < 3 || !count.is_multiple_of(3) {
                bail!(
                    "a closed path needs three control points per segment, and has {count} in all"
                );
            }
            return Ok(count / 3);
        }
        if count < 4 || !(count - 1).is_multiple_of(3) {
            bail!(
                "an open path needs four control points and three more per segment after that, and has {count} in all"
            );
        }
        Ok((count - 1) / 3)
    }
}

/// What a definition table holds, for the generated reference.
const PATH_ASSET_DOC: &str = r#"A bezier path: `points` is a run of cubic control points -- an anchor, two
handles, the next anchor, and three more for every segment after that -- and
`closed` joins the last segment back to the first anchor. Two points on their
own are read as a straight line.

A `path2d` is flat and a `path3d` is in space. What fills, extrudes, revolves
or sweeps one is a `mesh` asset naming it; what strokes one is `shape2d`.

```toml
[[assets]]
id = "outline"
type = "path2d"
closed = true
points = [[0, 0], [1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0]]
```"#;

/// Register both path types, so a scene, a mesh and a stroke all name a
/// curve the same way.
pub(crate) fn register_path_assets(app: &mut App) {
    app.register_asset_type(PATH2D_ASSET_TYPE, "paths", PATH_ASSET_DOC, |value| {
        Ok(std::rc::Rc::new(parse_2d(value)?) as std::rc::Rc<dyn std::any::Any>)
    });
    app.register_asset_type(PATH3D_ASSET_TYPE, "paths", PATH_ASSET_DOC, |value| {
        Ok(std::rc::Rc::new(parse_3d(value)?) as std::rc::Rc<dyn std::any::Any>)
    });
}

fn closed_flag(value: &toml::Value) -> bool {
    value
        .get("closed")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

/// The `points` list, each row of `width` numbers.
fn rows(value: &toml::Value, width: usize) -> Result<Vec<[f32; 3]>> {
    let Some(points) = value.get("points").and_then(toml::Value::as_array) else {
        bail!("a path needs a `points` list of control points");
    };
    let mut out = Vec::with_capacity(points.len());
    for (i, row) in points.iter().enumerate() {
        let row = row
            .as_array()
            .filter(|r| r.len() == width)
            .ok_or_else(|| anyhow::anyhow!("a path's `points[{i}]` is not {width} numbers"))?;
        let mut point = [0.0f32; 3];
        for (slot, number) in point.iter_mut().zip(row) {
            *slot = crate::components::as_f64(number)
                .ok_or_else(|| anyhow::anyhow!("a path's `points[{i}]` holds a non-number"))?
                as f32;
        }
        out.push(point);
    }
    Ok(out)
}

fn parse_2d(value: &toml::Value) -> Result<Path2d> {
    let path = Path2d {
        points: rows(value, 2)?
            .into_iter()
            .map(|p| Vec2::new(p[0], p[1]))
            .collect(),
        closed: closed_flag(value),
    };
    path.segment_count().map(|_| path)
}

fn parse_3d(value: &toml::Value) -> Result<Path3d> {
    let path = Path3d {
        points: rows(value, 3)?.into_iter().map(Vec3::from_array).collect(),
        closed: closed_flag(value),
    };
    path.segment_count().map(|_| path)
}
