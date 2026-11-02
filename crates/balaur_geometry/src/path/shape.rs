//! What a path becomes: an outline extruded, a profile revolved, a
//! cross-section swept along a rail.
//!
//! Each is a pure function of the sampled points, so the mesh a collider is
//! fitted to and the mesh on screen come out of the same call.

use super::{Path2d, Path3d, TOLERANCE};
use crate::mesh::MeshData;
use crate::primitive::{Facets, ProfilePoint, revolve};
use anyhow::{Result, bail};
use glamx::{Vec2, Vec3};

/// The outward normal of the edge leaving `here`, for a ring wound
/// counter-clockwise.
fn edge_normal(here: Vec2, next: Vec2) -> Vec2 {
    let along = next - here;
    Vec2::new(along.y, -along.x).normalize_or_zero()
}

/// The ring moved inwards by `distance`, each corner along the bisector of
/// its two edges.
///
/// A miter is clamped: at a spike the exact inset runs away, and a bevel
/// wide enough to eat a feature will fold over it rather than refuse.
fn inset(ring: &[Vec2], distance: f32) -> Vec<Vec2> {
    let count = ring.len();
    (0..count)
        .map(|i| {
            let previous = edge_normal(ring[(i + count - 1) % count], ring[i]);
            let next = edge_normal(ring[i], ring[(i + 1) % count]);
            let miter = (previous + next).normalize_or_zero();
            let reach = miter.dot(next).max(0.25);
            ring[i] - miter * (distance / reach)
        })
        .collect()
}

/// Twice the area a ring encloses; positive when it runs counter-clockwise.
fn signed_area(ring: &[Vec2]) -> f32 {
    (0..ring.len())
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum()
}

/// The outline as a ring wound counter-clockwise, whichever way it was drawn.
fn ring_of(path: &Path2d) -> Result<Vec<Vec2>> {
    let mut ring = path.sample(TOLERANCE)?;
    if ring.len() < 3 {
        bail!("a path with fewer than three corners encloses nothing");
    }
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    Ok(ring)
}

/// A closed outline given thickness along z, with the edges rounded off when
/// `bevel` asks for it.
///
/// # Errors
/// If the path does not close into a ring, or the ring will not triangulate.
pub fn extrude(path: &Path2d, depth: f32, bevel: f32, segments: u32) -> Result<MeshData> {
    let ring = ring_of(path)?;
    let half = depth.max(crate::primitive::MIN_EXTENT) / 2.0;
    let bevel = bevel.clamp(0.0, half * 0.99);
    let steps = if bevel > 0.0 { segments.max(1) } else { 0 };
    // From the middle outwards: the straight wall, then the bevel's arc, then
    // the cap. The same list mirrored gives the other end.
    let mut rings: Vec<(Vec<Vec2>, f32)> = vec![(ring.clone(), half - bevel)];
    for step in 1..=steps {
        let turn = std::f32::consts::FRAC_PI_2 * step as f32 / steps as f32;
        let (sin, cos) = libm::sincosf(turn);
        rings.push((
            inset(&ring, bevel.mul_add(-cos, bevel)),
            bevel.mul_add(sin, half - bevel),
        ));
    }
    Ok(wall(&rings, &ring))
}

/// Stitch the mirrored stack of rings into a closed solid: the walls between
/// consecutive rings, and a cap at each end.
fn wall(rings: &[(Vec<Vec2>, f32)], base: &[Vec2]) -> MeshData {
    let mut build = crate::primitive::Build::default();
    let mut stack: Vec<(Vec<Vec2>, f32)> = rings
        .iter()
        .rev()
        .map(|(ring, z)| (ring.clone(), -z))
        .collect();
    stack.extend(rings.iter().cloned());
    let count = base.len();
    let mut indices: Vec<Vec<u32>> = Vec::with_capacity(stack.len());
    for (ring, z) in &stack {
        let row = ring
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let normal = Vec3::new(p.x, p.y, 0.0).normalize_or_zero();
                build.vertex(
                    Vec3::new(p.x, p.y, *z),
                    normal,
                    Vec2::new(i as f32 / count as f32, *z),
                )
            })
            .collect();
        indices.push(row);
    }
    for pair in indices.windows(2) {
        for i in 0..count {
            let j = (i + 1) % count;
            build.quad(pair[0][i], pair[0][j], pair[1][j], pair[1][i]);
        }
    }
    cap(&mut build, &stack[0].0, stack[0].1, false);
    let last = stack.len() - 1;
    cap(&mut build, &stack[last].0, stack[last].1, true);
    build.finish()
}

/// One end of an extrusion, triangulated flat and facing away from the solid.
fn cap(build: &mut crate::primitive::Build, ring: &[Vec2], z: f32, front: bool) {
    let loop_indices: Vec<u32> = (0..ring.len() as u32).collect();
    let Ok(triangles) = crate::triangulate::triangulate(ring, &loop_indices) else {
        return;
    };
    let normal = if front { Vec3::Z } else { -Vec3::Z };
    let first = build.vertex_count();
    for p in ring {
        build.vertex(Vec3::new(p.x, p.y, z), normal, *p);
    }
    for [a, b, c] in triangles {
        if front {
            build.triangle(first + a, first + b, first + c);
        } else {
            build.triangle(first + a, first + c, first + b);
        }
    }
}

/// A profile spun around y: `path` is read in the half-plane, `x` being the
/// distance from the axis and `y` the height.
///
/// # Errors
/// If the profile does not sample into at least two points.
pub fn lathe(path: &Path2d, segments: u32) -> Result<MeshData> {
    let points = path.sample(TOLERANCE)?;
    if points.len() < 2 {
        bail!("a lathe needs a profile of at least two points");
    }
    let total = points.len() - 1;
    let profile: Vec<ProfilePoint> = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let before = points[i.saturating_sub(1)];
            let after = points[(i + 1).min(total)];
            let along = after - before;
            // The surface faces the way the profile turns: for a profile
            // climbing in y, that is straight out from the axis.
            let normal = Vec2::new(along.y, -along.x).normalize_or_zero();
            ProfilePoint::new(
                Vec2::new(p.x.max(0.0), p.y),
                normal,
                i as f32 / total as f32,
            )
        })
        .collect();
    Ok(revolve(&profile, segments, Facets::Smooth))
}

/// A cross-section carried along a rail, turned to follow it.
///
/// The frame is parallel-transported rather than rebuilt from a fixed up
/// vector, so a rail that loops over its own top does not flip the profile
/// upside down halfway round.
///
/// # Errors
/// If either path is too short to sweep.
pub fn sweep(rail: &Path3d, profile: &Path2d, closed_profile: bool) -> Result<MeshData> {
    let spine = rail.sample(TOLERANCE)?;
    let mut section = profile.sample(TOLERANCE)?;
    if spine.len() < 2 {
        bail!("a sweep needs a rail of at least two points");
    }
    if section.len() < 3 {
        bail!("a sweep needs a profile of at least three points");
    }
    if closed_profile && signed_area(&section) < 0.0 {
        section.reverse();
    }
    let frames = transport(&spine, rail.closed);
    let mut build = crate::primitive::Build::default();
    let rings: Vec<Vec<u32>> = frames
        .iter()
        .enumerate()
        .map(|(i, (origin, right, up))| {
            let v = i as f32 / (frames.len() - 1).max(1) as f32;
            section
                .iter()
                .enumerate()
                .map(|(j, p)| {
                    let offset = *right * p.x + *up * p.y;
                    let normal = offset.normalize_or_zero();
                    build.vertex(
                        *origin + offset,
                        normal,
                        Vec2::new(j as f32 / section.len() as f32, v),
                    )
                })
                .collect()
        })
        .collect();
    let count = section.len();
    let last = if closed_profile { count } else { count - 1 };
    for pair in rings.windows(2) {
        for i in 0..last {
            let j = (i + 1) % count;
            build.quad(pair[0][i], pair[0][j], pair[1][j], pair[1][i]);
        }
    }
    Ok(build.finish())
}

/// A frame at every point of the spine: where it is, and the two directions
/// across it, each rotated off the one before by the least turn that lines
/// it up with the new tangent.
fn transport(spine: &[Vec3], closed: bool) -> Vec<(Vec3, Vec3, Vec3)> {
    let count = spine.len();
    let tangent = |i: usize| {
        let before = if i == 0 {
            if closed { spine[count - 1] } else { spine[0] }
        } else {
            spine[i - 1]
        };
        let after = if i + 1 >= count {
            if closed { spine[0] } else { spine[count - 1] }
        } else {
            spine[i + 1]
        };
        (after - before).normalize_or_zero()
    };
    let first = tangent(0);
    // Any direction across the tangent will do to start; the world's y unless
    // the rail already runs that way.
    let seed = if first.dot(Vec3::Y).abs() > 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let mut right = seed.cross(first).normalize_or_zero();
    let mut up = first.cross(right).normalize_or_zero();
    let mut out = Vec::with_capacity(count);
    let mut previous = first;
    for (i, point) in spine.iter().enumerate() {
        let now = tangent(i);
        let axis = previous.cross(now);
        if axis.length_squared() > 1e-12 {
            let angle = libm::atan2f(axis.length(), previous.dot(now));
            let turn = glamx::Quat::from_axis_angle(axis.normalize_or_zero(), angle);
            right = turn * right;
            up = turn * up;
        }
        previous = now;
        out.push((*point, right, up));
    }
    out
}
