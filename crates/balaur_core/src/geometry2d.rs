//! The `geometry2d` script module: polygons as lists of points.
//!
//! Booleans come from `i_overlay`, which works in fixed-point integers
//! internally and so lands on the same vertices on every platform; the
//! triangulation is the engine's own ear clipping. A polygon is a list of
//! `[x, y]` pairs or vectors, outline order, either winding.

use anyhow::{anyhow, Result};
use balaur_script::{Bindings, BindingsExt, Value};
use glamx::Vec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use crate::engine::Engine;

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

/// One boolean of two polygons: a list of shapes, each a list of paths, the
/// first the outline and the rest its holes.
fn boolean(a: &[Vec2], b: &[Vec2], rule: OverlayRule) -> Value {
    let subject: Vec<[f32; 2]> = a.iter().map(|p| [p.x, p.y]).collect();
    let clip: Vec<[f32; 2]> = b.iter().map(|p| [p.x, p.y]).collect();
    let shapes = subject.overlay(&clip, rule, FillRule::EvenOdd);
    Value::List(
        shapes
            .into_iter()
            .map(|shape| {
                Value::List(
                    shape
                        .into_iter()
                        .map(|path| {
                            Value::List(
                                path.into_iter()
                                    .map(|p| Value::Vec2([p[0], p[1]]))
                                    .collect(),
                            )
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
        assert!(segments_intersect(
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(1.0, 1.0)
        )
        .is_none());
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
