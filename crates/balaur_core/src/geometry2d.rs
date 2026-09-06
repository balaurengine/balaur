//! The `geometry2d` script module: polygons as lists of points.
//!
//! Booleans come from `i_overlay`, which works in fixed-point integers
//! internally and so lands on the same vertices on every platform; the
//! triangulation is the engine's own ear clipping. A polygon is a list of
//! `[x, y]` pairs or vectors, outline order, either winding.
//!
//! [`trace`] and [`simplify`] are the shape half of the editor's Trace
//! button: an alpha mask in, an outline out. They are here rather than in
//! the renderer because they touch no image format and no GPU — the caller
//! decodes the picture and hands over booleans — which is what lets a
//! headless test assert the outline of a square.

use anyhow::{Result, anyhow};
use balaur_script::{Bindings, BindingsExt, Value};
use glamx::Vec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use crate::engine::Engine;

/// A pixel index as the coordinate the boundary walk counts in, which reaches
/// one past the mask on every side.
fn sample(index: usize) -> isize {
    isize::try_from(index).unwrap_or(isize::MAX - 1)
}

/// A pixel index as the corner coordinate a traced loop is built from.
fn corner(index: usize) -> i32 {
    i32::try_from(index).unwrap_or(i32::MAX - 1)
}

/// Every closed boundary between the set and the unset pixels of a mask,
/// in pixel-corner coordinates with y downward, largest loop first.
///
/// Marching the boundary rather than the pixels: each set pixel with an unset
/// neighbour contributes that one edge, directed so the set side is on the
/// left, and the edges chain head-to-tail into loops. The result is
/// watertight and lands on integers, so tracing the same picture twice gives
/// the same vertices on every platform.
///
/// A mask shorter than `width * height` is read as unset past its end, which
/// is what an image that failed to decode fully looks like.
#[must_use]
pub fn trace(mask: &[bool], width: usize, height: usize) -> Vec<Vec<Vec2>> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let set = |x: isize, y: isize| -> bool {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return false;
        };
        if x >= width || y >= height {
            return false;
        }
        mask.get(y * width + x).copied().unwrap_or(false)
    };
    // Edges keyed by where they start, as a list: a diagonal touch starts two
    // there, and taking whichever is left splits it into two loops.
    let mut edges: std::collections::BTreeMap<(i32, i32), Vec<(i32, i32)>> =
        std::collections::BTreeMap::new();
    let mut edge = |from: (i32, i32), to: (i32, i32)| {
        edges.entry(from).or_default().push(to);
    };
    for row in 0..height {
        for column in 0..width {
            let (x, y) = (sample(column), sample(row));
            if !set(x, y) {
                continue;
            }
            let (x0, y0) = (corner(column), corner(row));
            let (x1, y1) = (x0 + 1, y0 + 1);
            if !set(x, y - 1) {
                edge((x0, y0), (x1, y0));
            }
            if !set(x + 1, y) {
                edge((x1, y0), (x1, y1));
            }
            if !set(x, y + 1) {
                edge((x1, y1), (x0, y1));
            }
            if !set(x - 1, y) {
                edge((x0, y1), (x0, y0));
            }
        }
    }
    let mut loops = Vec::new();
    // `BTreeMap` rather than a hash map: the loop a trace starts from decides
    // the order of the answer, and that order has to be the same every run.
    while let Some(&start) = edges.keys().next() {
        let mut points = Vec::new();
        let mut at = start;
        while let Some(nexts) = edges.get_mut(&at) {
            let Some(next) = nexts.pop() else { break };
            if nexts.is_empty() {
                edges.remove(&at);
            }
            points.push(Vec2::new(at.0 as f32, at.1 as f32));
            at = next;
            if at == start {
                break;
            }
        }
        // Three points is the least that encloses anything; a shorter walk
        // is a dead end left by a diagonal touch and has no area to keep.
        if points.len() >= 3 {
            loops.push(drop_collinear(&points));
        }
    }
    loops.sort_by(|a, b| {
        doubled_area(b)
            .abs()
            .total_cmp(&doubled_area(a).abs())
            .then_with(|| a.len().cmp(&b.len()))
    });
    loops
}

/// The points of a traced loop with every one that sits on the straight line
/// between its neighbours removed. A pixel boundary is mostly straight runs,
/// and this turns each into its two ends before anything else looks at it.
fn drop_collinear(points: &[Vec2]) -> Vec<Vec2> {
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (before, at, after) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let (a, b) = (at - before, after - at);
        if (a.x * b.y - a.y * b.x).abs() > f32::EPSILON {
            out.push(at);
        }
    }
    if out.len() >= 3 { out } else { points.to_vec() }
}

/// Ramer-Douglas-Peucker on a closed loop: drop every point closer than
/// `tolerance` to the line its kept neighbours span.
///
/// The loop is cut at its first point and the one furthest from it, and each
/// half simplified as an open chain, because RDP needs two fixed ends and a
/// closed loop has none — cutting anywhere else lets the split itself survive
/// as a vertex the shape does not need.
#[must_use]
pub fn simplify(points: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    if points.len() < 4 || tolerance <= 0.0 {
        return points.to_vec();
    }
    let first = 0;
    let second = (1..points.len())
        .max_by(|&a, &b| {
            (points[a] - points[first])
                .length_squared()
                .total_cmp(&(points[b] - points[first]).length_squared())
        })
        .unwrap_or(0);
    let mut out = Vec::new();
    let mut half = |from: usize, to: usize| {
        let chain: Vec<Vec2> = if from < to {
            points[from..=to].to_vec()
        } else {
            points[from..]
                .iter()
                .chain(&points[..=to])
                .copied()
                .collect()
        };
        let mut kept = rdp(&chain, tolerance);
        // The last point of one half is the first of the other; keeping both
        // would double every cut vertex.
        if kept.len() > 1 {
            kept.pop();
        }
        out.extend(kept);
    };
    half(first, second);
    half(second, first);
    if out.len() >= 3 { out } else { points.to_vec() }
}

/// RDP on an open chain, keeping both ends.
fn rdp(points: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (a, b) = (points[0], points[points.len() - 1]);
    let mut worst = (0usize, 0.0f32);
    for (i, &p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let d = point_to_segment(p, a, b);
        if d > worst.1 {
            worst = (i, d);
        }
    }
    if worst.1 <= tolerance {
        return vec![a, b];
    }
    let mut out = rdp(&points[..=worst.0], tolerance);
    out.pop();
    out.extend(rdp(&points[worst.0..], tolerance));
    out
}

/// How far `p` is from the segment `a`-`b`, which is the distance to the
/// nearer end when the segment has no length to project onto.
fn point_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len = ab.length_squared();
    if len <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

fn point_of(value: &Value) -> Result<Vec2> {
    match value {
        Value::Vec2([x, y]) | Value::Vec3([x, y, _]) => Ok(Vec2::new(*x, *y)),
        Value::List(pair) if pair.len() >= 2 => {
            let n = |v: &Value| match v {
                Value::Num(n) => Ok(*n as f32),
                Value::Int(n) => Ok(*n as f32),
                other => Err(anyhow!("a coordinate should be a number, got {other:?}")),
            };
            Ok(Vec2::new(n(&pair[0])?, n(&pair[1])?))
        }
        other => Err(anyhow!("a point is [x, y] or a vector, got {other:?}")),
    }
}

fn polygon_of(value: &Value) -> Result<Vec<Vec2>> {
    let Value::List(items) = value else {
        return Err(anyhow!("a polygon is a list of points, got {value:?}"));
    };
    items.iter().map(point_of).collect()
}

fn polygon_value(points: &[Vec2]) -> Value {
    Value::List(points.iter().map(|p| Value::Vec2([p.x, p.y])).collect())
}

/// Twice the signed area; positive counter-clockwise.
fn doubled_area(points: &[Vec2]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum()
}

/// Ray casting: a point on an edge counts as inside.
fn contains(points: &[Vec2], p: Vec2) -> bool {
    let n = points.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (points[i], points[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Where two segments cross, if they do; touching at an endpoint counts.
fn segments_intersect(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Option<Vec2> {
    let r = a2 - a1;
    let s = b2 - b1;
    let denominator = r.x * s.y - r.y * s.x;
    if denominator.abs() <= f32::EPSILON {
        return None;
    }
    let q = b1 - a1;
    let t = (q.x * s.y - q.y * s.x) / denominator;
    let u = (q.x * r.y - q.y * r.x) / denominator;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| a1 + r * t)
}

/// Andrew's monotone chain, counter-clockwise, no three collinear.
fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut sorted: Vec<Vec2> = points.to_vec();
    sorted.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<Vec2> = Vec::with_capacity(sorted.len() * 2);
    for &p in &sorted {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower = hull.len() + 1;
    for &p in sorted.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull
}

/// One boolean of two polygons, as the shapes it leaves: each a list of
/// paths, the first the outline and the rest its holes.
///
/// The Rust half of the script call below, so a `boolean2d` component and a
/// script get the same answer out of the same two rings.
#[must_use]
pub fn overlay(a: &[Vec2], b: &[Vec2], op: crate::csg::Op) -> Vec<Vec<Vec<Vec2>>> {
    overlay_with(
        a,
        b,
        match op {
            crate::csg::Op::Union => OverlayRule::Union,
            crate::csg::Op::Difference => OverlayRule::Difference,
            crate::csg::Op::Intersection => OverlayRule::Intersect,
        },
    )
}

/// The same, keyed by i_overlay's own rule: the script call reaches two more
/// of them than the three a component offers.
fn overlay_with(a: &[Vec2], b: &[Vec2], rule: OverlayRule) -> Vec<Vec<Vec<Vec2>>> {
    let subject: Vec<[f32; 2]> = a.iter().map(|p| [p.x, p.y]).collect();
    let clip: Vec<[f32; 2]> = b.iter().map(|p| [p.x, p.y]).collect();
    subject
        .overlay(&clip, rule, FillRule::EvenOdd)
        .into_iter()
        .map(|shape| {
            shape
                .into_iter()
                .map(|path| path.into_iter().map(|p| Vec2::new(p[0], p[1])).collect())
                .collect()
        })
        .collect()
}

/// One boolean of two polygons: a list of shapes, each a list of paths, the
/// first the outline and the rest its holes.
fn boolean(a: &[Vec2], b: &[Vec2], rule: OverlayRule) -> Value {
    let shapes = overlay_with(a, b, rule);
    Value::List(
        shapes
            .into_iter()
            .map(|shape| {
                Value::List(
                    shape
                        .into_iter()
                        .map(|path| {
                            Value::List(path.into_iter().map(|p| Value::Vec2([p.x, p.y])).collect())
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

pub(crate) fn install_geometry2d_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Polygons on the plane, as lists of `[x, y]` points in outline order: \
         triangulation, booleans, hulls and containment. Every answer is the \
         same on every platform: the booleans run in fixed point and the rest \
         is plain arithmetic.",
    );
    m.describe(&[
        ("triangulate", &[], "(polygon: list) -> list", "The polygon cut into triangles, as `[i, j, k]` triples indexing its points, counter-clockwise; either winding is accepted."),
        ("contains", &[], "(polygon: list, point: vec2) -> bool", "Whether a point lies inside the polygon; a point on an edge counts as inside."),
        ("segments_intersect", &[], "(a1: vec2, a2: vec2, b1: vec2, b2: vec2) -> vec2", "Where two segments cross, or nil when they do not; touching at an endpoint counts."),
        ("area", &[], "(polygon: list) -> float", "The polygon's area, always positive whatever its winding."),
        ("is_clockwise", &[], "(polygon: list) -> bool", "Whether the points run clockwise, with y up."),
        ("convex_hull", &[], "(points: list) -> list", "The smallest convex polygon around the points, counter-clockwise."),
        ("union", &[], "(a: list, b: list) -> list", "Everything inside either polygon: a list of shapes, each a list of paths whose first is the outline and the rest holes."),
        ("intersection", &[], "(a: list, b: list) -> list", "Everything inside both polygons, shaped as `union` shapes it."),
        ("difference", &[], "(a: list, b: list) -> list", "Everything inside the first polygon and outside the second, shaped as `union` shapes it."),
    ]);
    m.function("triangulate", |_: &Engine, polygon: Value| {
        let points = polygon_of(&polygon)?;
        let ring: Vec<u32> = (0..points.len() as u32).collect();
        let triangles = crate::triangulate::triangulate(&points, &ring)?;
        Ok(Value::List(
            triangles
                .into_iter()
                .map(|[i, j, k]| {
                    Value::List(vec![
                        Value::Int(i64::from(i)),
                        Value::Int(i64::from(j)),
                        Value::Int(i64::from(k)),
                    ])
                })
                .collect(),
        ))
    });
    m.function(
        "contains",
        |_: &Engine, (polygon, point): (Value, Value)| {
            Ok(contains(&polygon_of(&polygon)?, point_of(&point)?))
        },
    );
    m.function(
        "segments_intersect",
        |_: &Engine, (a1, a2, b1, b2): (Value, Value, Value, Value)| {
            let hit = segments_intersect(
                point_of(&a1)?,
                point_of(&a2)?,
                point_of(&b1)?,
                point_of(&b2)?,
            );
            Ok(hit.map_or(Value::Nil, |p| Value::Vec2([p.x, p.y])))
        },
    );
    m.function("area", |_: &Engine, polygon: Value| {
        Ok(f64::from(doubled_area(&polygon_of(&polygon)?).abs() / 2.0))
    });
    m.function("is_clockwise", |_: &Engine, polygon: Value| {
        Ok(doubled_area(&polygon_of(&polygon)?) < 0.0)
    });
    m.function("convex_hull", |_: &Engine, points: Value| {
        Ok(polygon_value(&convex_hull(&polygon_of(&points)?)))
    });
    m.function("union", |_: &Engine, (a, b): (Value, Value)| {
        Ok(boolean(
            &polygon_of(&a)?,
            &polygon_of(&b)?,
            OverlayRule::Union,
        ))
    });
    m.function("intersection", |_: &Engine, (a, b): (Value, Value)| {
        Ok(boolean(
            &polygon_of(&a)?,
            &polygon_of(&b)?,
            OverlayRule::Intersect,
        ))
    });
    m.function("difference", |_: &Engine, (a, b): (Value, Value)| {
        Ok(boolean(
            &polygon_of(&a)?,
            &polygon_of(&b)?,
            OverlayRule::Difference,
        ))
    });
}

#[cfg(test)]
mod tests {
    use super::{simplify, trace};

    /// A filled square, traced and simplified, is four corners — not four
    /// corners with the two cuts left in twice.
    #[test]
    fn a_simplified_loop_names_each_corner_once() {
        let mask = vec![true; 10 * 10];
        let outline = trace(&mask, 10, 10).remove(0);
        let corners = simplify(&outline, 2.0);
        assert_eq!(corners.len(), 4, "a square has four corners: {corners:?}");
        for (at, corner) in corners.iter().enumerate() {
            let next = corners[(at + 1) % corners.len()];
            assert_ne!(*corner, next, "a corner is repeated");
        }
    }

    /// Tracing is the shape of the mask, whatever else is in the picture.
    #[test]
    fn a_hole_is_a_loop_of_its_own() {
        let mut mask = vec![true; 5 * 5];
        mask[2 * 5 + 2] = false;
        let loops = trace(&mask, 5, 5);
        assert_eq!(loops.len(), 2, "the outline and the hole");
        assert_eq!(loops[1].len(), 4, "the hole is one pixel square");
    }

    use super::*;

    fn square(x: f32, y: f32, side: f32) -> Vec<Vec2> {
        vec![
            Vec2::new(x, y),
            Vec2::new(x + side, y),
            Vec2::new(x + side, y + side),
            Vec2::new(x, y + side),
        ]
    }

    #[test]
    fn a_point_inside_a_square_is_contained_and_one_outside_is_not() {
        let s = square(0.0, 0.0, 2.0);
        assert!(contains(&s, Vec2::new(1.0, 1.0)));
        assert!(!contains(&s, Vec2::new(3.0, 1.0)));
    }

    #[test]
    fn crossing_segments_meet_where_they_should() {
        let hit = segments_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(0.0, 2.0),
            Vec2::new(2.0, 0.0),
        )
        .unwrap();
        assert!((hit - Vec2::new(1.0, 1.0)).length() < 1e-6);
        assert!(
            segments_intersect(
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0)
            )
            .is_none()
        );
    }

    #[test]
    fn the_hull_of_a_square_with_a_point_inside_is_the_square() {
        let mut points = square(0.0, 0.0, 2.0);
        points.push(Vec2::new(1.0, 1.0));
        let hull = convex_hull(&points);
        assert_eq!(hull.len(), 4);
        assert!(doubled_area(&hull) > 0.0, "counter-clockwise");
    }

    #[test]
    fn two_overlapping_squares_union_into_one_shape_of_the_right_area() {
        let a = square(0.0, 0.0, 2.0);
        let b = square(1.0, 0.0, 2.0);
        let Value::List(shapes) = boolean(&a, &b, OverlayRule::Union) else {
            panic!("a list of shapes")
        };
        assert_eq!(shapes.len(), 1);
        let Value::List(paths) = &shapes[0] else {
            panic!("a list of paths")
        };
        let outline = polygon_of(&paths[0]).unwrap();
        assert!((doubled_area(&outline).abs() / 2.0 - 6.0).abs() < 1e-4);
        let Value::List(inter) = boolean(&a, &b, OverlayRule::Intersect) else {
            panic!("a list")
        };
        let Value::List(paths) = &inter[0] else {
            panic!("paths")
        };
        let overlap = polygon_of(&paths[0]).unwrap();
        assert!((doubled_area(&overlap).abs() / 2.0 - 2.0).abs() < 1e-4);
    }

    /// A mask from a picture drawn in text: `#` is opaque, anything else is
    /// not. Every trace test below reads as the shape it is testing.
    fn mask(rows: &[&str]) -> (Vec<bool>, usize, usize) {
        let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut out = Vec::with_capacity(width * rows.len());
        for row in rows {
            for x in 0..width {
                out.push(row.as_bytes().get(x) == Some(&b'#'));
            }
        }
        (out, width, rows.len())
    }

    #[test]
    fn a_solid_square_traces_to_its_four_corners() {
        let (bits, w, h) = mask(&["....", ".##.", ".##.", "...."]);
        let loops = trace(&bits, w, h);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 4, "{:?}", loops[0]);
        assert!((doubled_area(&loops[0]).abs() / 2.0 - 4.0).abs() < 1e-4);
    }

    #[test]
    fn a_ring_traces_its_outside_and_its_hole_largest_first() {
        let (bits, w, h) = mask(&["###", "#.#", "###"]);
        let loops = trace(&bits, w, h);
        assert_eq!(loops.len(), 2, "an outline and a hole");
        assert!((doubled_area(&loops[0]).abs() / 2.0 - 9.0).abs() < 1e-4);
        assert!((doubled_area(&loops[1]).abs() / 2.0 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn two_separate_blobs_trace_as_two_loops() {
        let (bits, w, h) = mask(&["#.#", "...", "#.#"]);
        assert_eq!(trace(&bits, w, h).len(), 4);
    }

    #[test]
    fn an_empty_mask_traces_to_nothing() {
        let (bits, w, h) = mask(&["...", "..."]);
        assert!(trace(&bits, w, h).is_empty());
        assert!(trace(&[], 0, 0).is_empty());
        // Short of what the size claims: the rest reads as transparent.
        assert!(trace(&[true], 4, 4).len() <= 1);
    }

    #[test]
    fn a_trace_lands_on_the_same_vertices_twice() {
        let (bits, w, h) = mask(&[".##.", "####", "#..#", "####"]);
        assert_eq!(
            format!("{:?}", trace(&bits, w, h)),
            format!("{:?}", trace(&bits, w, h))
        );
    }

    #[test]
    fn simplify_drops_a_point_on_the_line_and_keeps_a_corner() {
        // A square with a point halfway along its top edge.
        let square = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let kept = simplify(&square, 0.5);
        assert_eq!(kept.len(), 4, "{kept:?}");
        assert!((doubled_area(&kept).abs() / 2.0 - 100.0).abs() < 1e-3);
    }

    #[test]
    fn simplify_leaves_a_shape_that_is_already_minimal() {
        let square = square(0.0, 0.0, 2.0);
        assert_eq!(simplify(&square, 0.5).len(), 4);
        // Nothing to do, and nothing thrown away, at either extreme.
        assert_eq!(simplify(&square, 0.0).len(), 4);
        assert_eq!(simplify(&square[..3], 1.0).len(), 3);
    }

    #[test]
    fn a_staircase_simplifies_toward_its_diagonal() {
        let (bits, w, h) = mask(&["#...", "##..", "###.", "####"]);
        let outline = &trace(&bits, w, h)[0];
        let rough = simplify(outline, 0.1).len();
        let smooth = simplify(outline, 2.0).len();
        assert!(smooth < rough, "{smooth} should be fewer than {rough}");
        assert!(smooth >= 3, "a polygon needs three points");
    }

    #[test]
    fn a_boolean_lands_on_the_same_vertices_twice() {
        let a = square(0.0, 0.0, 3.0);
        let b = vec![
            Vec2::new(1.5, -1.0),
            Vec2::new(4.0, 1.5),
            Vec2::new(1.5, 4.0),
        ];
        let first = format!("{:?}", boolean(&a, &b, OverlayRule::Difference));
        let second = format!("{:?}", boolean(&a, &b, OverlayRule::Difference));
        assert_eq!(first, second);
    }
}
