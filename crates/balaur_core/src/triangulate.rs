//! Ear clipping: one hand-drawn loop of vertices into triangles.
//!
//! Written out rather than taken from a crate because the result has to be
//! the same on every platform — it is `f32` arithmetic and comparisons, no
//! transcendentals — and because a polygon someone traced over a sprite is
//! small enough that the quadratic walk never shows up.

use anyhow::{Result, bail};
use glamx::Vec2;

/// Triangulate the loop `ring` (indices into `points`, in outline order)
/// into counter-clockwise triangles.
///
/// A clockwise loop is turned around first, so either winding is fine. A
/// loop that crosses itself gets *some* triangulation rather than an error:
/// the walk clips whatever it can and never stalls.
///
/// # Errors
/// If an index is out of range, or fewer than three distinct points remain
/// once repeated ones are dropped, or the loop has no area.
pub fn triangulate(points: &[Vec2], ring: &[u32]) -> Result<Vec<[u32; 3]>> {
    for &index in ring {
        if index as usize >= points.len() {
            bail!(
                "a polygon names vertex {index} but only {} were given",
                points.len()
            );
        }
    }
    let mut ring = without_repeats(points, ring);
    if ring.len() < 3 {
        bail!("a polygon needs at least three distinct vertices");
    }
    let area = signed_area(points, &ring);
    if area == 0.0 {
        bail!("a polygon has no area, so there is nothing to fill");
    }
    if area < 0.0 {
        ring.reverse();
    }
    let mut out = Vec::with_capacity(ring.len() - 2);
    while ring.len() > 3 {
        let ear = (0..ring.len())
            .find(|&i| is_ear(points, &ring, i))
            .or_else(|| (0..ring.len()).find(|&i| is_convex(points, &ring, i)))
            .unwrap_or(0);
        out.push(triangle_at(&ring, ear));
        ring.remove(ear);
    }
    out.push([ring[0], ring[1], ring[2]]);
    Ok(out)
}

/// The loop with consecutive coincident vertices dropped, and the closing
/// repeat of the first vertex with them.
fn without_repeats(points: &[Vec2], ring: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(ring.len());
    for &index in ring {
        let same_as_last = out
            .last()
            .is_some_and(|&last| last == index || points[last as usize] == points[index as usize]);
        if !same_as_last {
            out.push(index);
        }
    }
    while out.len() > 1
        && (out[0] == out[out.len() - 1]
            || points[out[0] as usize] == points[out[out.len() - 1] as usize])
    {
        out.pop();
    }
    out
}

/// Twice the signed area (shoelace); positive is counter-clockwise.
fn signed_area(points: &[Vec2], ring: &[u32]) -> f32 {
    let mut sum = 0.0;
    for i in 0..ring.len() {
        let a = points[ring[i] as usize];
        let b = points[ring[(i + 1) % ring.len()] as usize];
        sum += a.x * b.y - b.x * a.y;
    }
    sum
}

fn triangle_at(ring: &[u32], i: usize) -> [u32; 3] {
    let n = ring.len();
    [ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]]
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Whether the corner at `i` turns left, on a counter-clockwise ring.
fn is_convex(points: &[Vec2], ring: &[u32], i: usize) -> bool {
    let [p, c, n] = triangle_at(ring, i).map(|index| points[index as usize]);
    cross(p, c, n) > 0.0
}

/// A convex corner whose triangle holds no other vertex of the ring. A
/// vertex on the triangle's edge counts as inside, so a clipped ear never
/// leaves a zero-area sliver behind.
fn is_ear(points: &[Vec2], ring: &[u32], i: usize) -> bool {
    if !is_convex(points, ring, i) {
        return false;
    }
    let corners = triangle_at(ring, i);
    let [p, c, n] = corners.map(|index| points[index as usize]);
    ring.iter()
        .filter(|index| !corners.contains(index))
        .map(|&index| points[index as usize])
        .filter(|&q| q != p && q != c && q != n)
        .all(|q| !inside(p, c, n, q))
}

/// Whether `q` is inside or on the counter-clockwise triangle `a b c`.
fn inside(a: Vec2, b: Vec2, c: Vec2, q: Vec2) -> bool {
    cross(a, b, q) >= 0.0 && cross(b, c, q) >= 0.0 && cross(c, a, q) >= 0.0
}
