//! Loops of vertices into triangles, through `i_triangle`.
//!
//! One triangulator in the tree, and this is where it is named: it works in
//! fixed point, so a fill lands on the same triangles on every platform, and
//! it takes an outline with holes, which is what a letter's counters need.

use anyhow::{Result, bail};
use glamx::Vec2;
use i_triangle::float::triangulator::Triangulator;

/// Triangulate the loop `ring` (indices into `points`, in outline order) into
/// counter-clockwise triangles over those same points. Either winding is fine.
///
/// # Errors
/// If an index is out of range, or fewer than three distinct points remain
/// once repeated ones are dropped, or the loop has no area. And if the loop
/// crosses itself: filling one correctly needs a vertex at the crossing, and
/// this form can only name vertices the caller wrote — [`triangulate_shape`]
/// is the form that returns the points a fill needed.
pub fn triangulate(points: &[Vec2], ring: &[u32]) -> Result<Vec<[u32; 3]>> {
    for &index in ring {
        if index as usize >= points.len() {
            bail!(
                "a polygon names vertex {index} but only {} were given",
                points.len()
            );
        }
    }
    let ring = without_repeats(points, ring);
    if ring.len() < 3 {
        bail!("a polygon needs at least three distinct vertices");
    }
    if signed_area(points, &ring) == 0.0 {
        bail!("a polygon has no area, so there is nothing to fill");
    }
    let contour: Vec<[f32; 2]> = ring
        .iter()
        .map(|&index| points[index as usize].to_array())
        .collect();
    let (filled, triangles) = fill(&[contour]);
    let mapped: Vec<Option<u32>> = filled
        .iter()
        .map(|point| index_of(points, &ring, *point))
        .collect();
    let mut out = Vec::with_capacity(triangles.len());
    for corners in triangles {
        let mut resolved = [0u32; 3];
        for (slot, corner) in resolved.iter_mut().zip(corners) {
            let Some(index) = mapped[corner as usize] else {
                let [x, y] = filled[corner as usize];
                bail!(
                    "this loop crosses itself at ({x}, {y}), and filling it needs a vertex there: \
                     draw it as loops that do not cross"
                );
            };
            *slot = index;
        }
        if resolved[0] == resolved[1] || resolved[1] == resolved[2] || resolved[2] == resolved[0] {
            continue;
        }
        out.push(counter_clockwise(points, resolved));
    }
    if out.is_empty() {
        bail!("a polygon has no area, so there is nothing to fill");
    }
    Ok(out)
}

/// Fill one outline and its holes together, and report the points the fill
/// needed: the ones given, and any the triangulator had to add where two
/// edges cross.
///
/// Contours go in together so a hole is subtracted rather than filled over.
#[must_use]
pub fn triangulate_shape(contours: &[Vec<[f32; 2]>]) -> (Vec<[f32; 2]>, Vec<[u32; 3]>) {
    fill(contours)
}

fn fill(contours: &[Vec<[f32; 2]>]) -> (Vec<[f32; 2]>, Vec<[u32; 3]>) {
    let mut triangulator: Triangulator<u32, i32> = Triangulator::default();
    let filled = triangulator.triangulate(&contours.to_vec());
    let triangles = filled.indices.as_chunks::<3>().0.to_vec();
    (filled.points, triangles)
}

/// The ring vertex a filled point came from. Exact first, because the
/// triangulator hands back the coordinates it was given; then nearest, for a
/// point far enough from the origin that its fixed-point form rounds.
fn index_of(points: &[Vec2], ring: &[u32], point: [f32; 2]) -> Option<u32> {
    if let Some(&index) = ring
        .iter()
        .find(|&&index| points[index as usize].to_array() == point)
    {
        return Some(index);
    }
    let target = Vec2::new(point[0], point[1]);
    let scale = ring
        .iter()
        .map(|&index| points[index as usize].abs().max_element())
        .fold(1.0f32, f32::max);
    let mut best: Option<(u32, f32)> = None;
    for &index in ring {
        let distance = (points[index as usize] - target).length_squared();
        if best.is_none_or(|(_, so_far)| distance < so_far) {
            best = Some((index, distance));
        }
    }
    let tolerance = scale * 1.0e-5;
    best.filter(|(_, distance)| *distance <= tolerance * tolerance)
        .map(|(index, _)| index)
}

/// The triangle wound counter-clockwise, whichever way it arrived.
fn counter_clockwise(points: &[Vec2], corners: [u32; 3]) -> [u32; 3] {
    let [a, b, c] = corners.map(|index| points[index as usize]);
    if cross(a, b, c) < 0.0 {
        [corners[0], corners[2], corners[1]]
    } else {
        corners
    }
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

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}
