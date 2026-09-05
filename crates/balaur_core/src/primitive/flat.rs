//! The 2D meshers. Each one is an outline, counter-clockwise and closed;
//! filling it is [`super::build::fill`] and tracing it is what a light's
//! occluder and a 2D collider read, so all three agree on the same points.

use glamx::Vec2;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// A point on the circle of `radius` at `angle`, through `libm` so the same
/// outline comes out on every platform.
fn on_circle(radius: f32, angle: f32) -> Vec2 {
    let (sin, cos) = libm::sincosf(angle);
    Vec2::new(radius * cos, radius * sin)
}

/// A circle traced with `segments` points.
#[must_use]
pub fn circle_outline(radius: f32, segments: u32) -> Vec<Vec2> {
    ellipse_outline(radius, radius, segments)
}

/// An axis-aligned ellipse traced with `segments` points.
#[must_use]
pub fn ellipse_outline(hx: f32, hy: f32, segments: u32) -> Vec<Vec2> {
    let segments = segments.max(3);
    (0..segments)
        .map(|i| {
            let point = on_circle(1.0, TAU * i as f32 / segments as f32);
            Vec2::new(hx * point.x, hy * point.y)
        })
        .collect()
}

/// A regular polygon of `sides`, first vertex straight up, so a triangle
/// points where a player expects it to.
#[must_use]
pub fn ngon_outline(sides: u32, radius: f32) -> Vec<Vec2> {
    let sides = sides.max(3);
    (0..sides)
        .map(|i| on_circle(radius, TAU.mul_add(i as f32 / sides as f32, FRAC_PI_2)))
        .collect()
}

/// A star of `points` tips, alternating `radius` and `inner_radius`, first
/// tip straight up.
#[must_use]
pub fn star_outline(points: u32, radius: f32, inner_radius: f32) -> Vec<Vec2> {
    let points = points.max(3);
    let inner = inner_radius.clamp(0.001, radius.max(0.002));
    (0..points * 2)
        .map(|i| {
            let reach = if i % 2 == 0 { radius } else { inner };
            on_circle(reach, PI.mul_add(i as f32 / points as f32, FRAC_PI_2))
        })
        .collect()
}

/// A rectangle, its corners rounded when `corner_radius` asks for it.
#[must_use]
pub fn rect_outline(hx: f32, hy: f32, corner_radius: f32, segments: u32) -> Vec<Vec2> {
    let radius = corner_radius.clamp(0.0, hx.min(hy).max(0.0));
    if radius <= 0.0 {
        return vec![
            Vec2::new(hx, hy),
            Vec2::new(-hx, hy),
            Vec2::new(-hx, -hy),
            Vec2::new(hx, -hy),
        ];
    }
    let arc = (segments.max(4) / 4).max(1);
    let (x, y) = (hx - radius, hy - radius);
    let corners = [(x, y), (-x, y), (-x, -y), (x, -y)];
    let mut out = Vec::with_capacity(4 * (arc as usize + 1));
    for (corner, (px, py)) in corners.into_iter().enumerate() {
        let turn = FRAC_PI_2 * corner as f32;
        for i in 0..=arc {
            let angle = FRAC_PI_2.mul_add(i as f32 / arc as f32, turn);
            out.push(Vec2::new(px, py) + on_circle(radius, angle));
        }
    }
    out
}

/// A 2D capsule: the straight part is `height`, the caps add `radius` at each
/// end, which is the meaning `collider2d` gives them.
#[must_use]
pub fn capsule_outline(radius: f32, height: f32, segments: u32) -> Vec<Vec2> {
    let arc = (segments.max(4) / 2).max(2);
    let half = Vec2::new(0.0, height / 2.0);
    let mut out = Vec::with_capacity(2 * (arc as usize + 1));
    for (offset, turn) in [(half, 0.0), (-half, PI)] {
        for i in 0..=arc {
            out.push(offset + on_circle(radius, PI.mul_add(i as f32 / arc as f32, turn)));
        }
    }
    out
}

/// How far a regular polygon of `sides` reaches along x and y, as a fraction
/// of its radius. It starts pointing up, so y is always the full radius.
#[must_use]
pub(super) fn ngon_reach(sides: u32) -> Vec2 {
    super::build::ring_reach(sides, FRAC_PI_2)
}

/// The same for one of a star's two rings: the tips, or the notches between
/// them, which sit half a step further round.
#[must_use]
pub(super) fn star_reach(points: u32, tips: bool) -> Vec2 {
    let points = points.max(3);
    let offset = if tips {
        FRAC_PI_2
    } else {
        FRAC_PI_2 + PI / points as f32
    };
    super::build::ring_reach(points, offset)
}
