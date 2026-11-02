//! The 3D meshers: parameters in, triangles out, no backend anywhere near.
//!
//! Six of these replace shapes a renderer used to build for itself. Building
//! them here is what lets a collider, a picked ray and a headless test read
//! the vertices the screen draws.

use super::build::{Build, Facets, ProfilePoint, revolve};
use crate::mesh::MeshData;
use glamx::{Vec2, Vec3};

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// Where a pole-to-pole arc sits at `v`, from the south pole to the north.
///
/// The two ends are placed rather than computed: `cosf(PI / 2.0)` is not
/// zero, and a pole built from it is a ragged ring rather than a point.
fn meridian(v: f32) -> Vec2 {
    if v <= 0.0 {
        return Vec2::new(0.0, -1.0);
    }
    if v >= 1.0 {
        return Vec2::new(0.0, 1.0);
    }
    let (sin, cos) = libm::sincosf(PI.mul_add(v, -FRAC_PI_2));
    Vec2::new(cos, sin)
}

/// A sphere, spun from a pole-to-pole arc.
#[must_use]
pub fn ball(radius: f32, segments: u32, rings: u32) -> MeshData {
    let rings = rings.max(2);
    let profile: Vec<ProfilePoint> = (0..=rings)
        .map(|i| {
            let v = i as f32 / rings as f32;
            let direction = meridian(v);
            ProfilePoint::new(radius * direction, direction, v)
        })
        .collect();
    revolve(&profile, segments, Facets::Smooth)
}

/// A cylinder with hemispherical caps, `height` being the straight part.
#[must_use]
pub fn capsule(radius: f32, height: f32, segments: u32, rings: u32) -> MeshData {
    let arc = (rings.max(2) / 2).max(1);
    let half = height / 2.0;
    let mut profile = Vec::with_capacity(2 * (arc as usize + 1));
    for (cap, offset) in [(0, -half), (1, half)] {
        for i in 0..=arc {
            let t = i as f32 / arc as f32;
            // Each cap is half a meridian: the lower one runs pole to equator
            // and the upper one equator to pole, so both ends stay exact.
            let v = f32::midpoint(cap as f32, t);
            let direction = meridian(v);
            let position = Vec2::new(radius * direction.x, radius.mul_add(direction.y, offset));
            profile.push(ProfilePoint::new(position, direction, v));
        }
    }
    revolve(&profile, segments, Facets::Smooth)
}

/// The lathe profile a flat-capped column of `radius` and `height` spins
/// from: a cap, a wall, a cap, with the corners doubled so the edge is hard.
fn column(radius: f32, height: f32) -> [ProfilePoint; 6] {
    let half = height / 2.0;
    let down = Vec2::new(0.0, -1.0);
    let up = Vec2::new(0.0, 1.0);
    let out = Vec2::new(1.0, 0.0);
    [
        ProfilePoint::new(Vec2::new(0.0, -half), down, 0.0),
        ProfilePoint::new(Vec2::new(radius, -half), down, 0.0),
        ProfilePoint::new(Vec2::new(radius, -half), out, 0.0),
        ProfilePoint::new(Vec2::new(radius, half), out, 1.0),
        ProfilePoint::new(Vec2::new(radius, half), up, 1.0),
        ProfilePoint::new(Vec2::new(0.0, half), up, 1.0),
    ]
}

/// A flat-capped cylinder, principal axis on y.
#[must_use]
pub fn cylinder(radius: f32, height: f32, segments: u32) -> MeshData {
    revolve(&column(radius, height), segments, Facets::Smooth)
}

/// A cone standing on its base, apex at `+height / 2`.
#[must_use]
pub fn cone(radius: f32, height: f32, segments: u32) -> MeshData {
    let half = height / 2.0;
    let slant = Vec2::new(height, radius).normalize_or_zero();
    let profile = [
        ProfilePoint::new(Vec2::new(0.0, -half), Vec2::new(0.0, -1.0), 0.0),
        ProfilePoint::new(Vec2::new(radius, -half), Vec2::new(0.0, -1.0), 0.0),
        ProfilePoint::new(Vec2::new(radius, -half), slant, 0.0),
        ProfilePoint::new(Vec2::new(0.0, half), slant, 1.0),
    ];
    revolve(&profile, segments, Facets::Smooth)
}

/// A column with `sides` flat faces: a triangular, hexagonal or any other
/// prism. `radius` reaches the edges, not the faces.
#[must_use]
pub fn prism(radius: f32, height: f32, sides: u32) -> MeshData {
    revolve(&column(radius, height), sides.max(3), Facets::Flat)
}

/// A ring of `radius` with a tube of `tube_radius` around it, lying in xz.
#[must_use]
pub fn torus(radius: f32, tube_radius: f32, segments: u32, rings: u32) -> MeshData {
    let rings = rings.max(3);
    let profile: Vec<ProfilePoint> = (0..=rings)
        .map(|i| {
            let v = i as f32 / rings as f32;
            // The closing sample takes the first one's angle, not a whole
            // turn's, so the tube meets itself exactly where it started.
            let turn = (i % rings) as f32 / rings as f32;
            let (sin, cos) = libm::sincosf(std::f32::consts::TAU * turn);
            let position = Vec2::new(tube_radius.mul_add(cos, radius), tube_radius * sin);
            ProfilePoint::new(position, Vec2::new(cos, sin), v)
        })
        .collect();
    revolve(&profile, segments, Facets::Smooth)
}

/// A pipe: a cylinder with a cylindrical hole down its axis. An
/// `inner_radius` of zero or more than `radius` is clamped to leave a wall.
#[must_use]
pub fn tube(radius: f32, inner_radius: f32, height: f32, segments: u32) -> MeshData {
    let outer = radius;
    let inner = inner_radius.clamp(0.0, outer * 0.999);
    let half = height / 2.0;
    let (up, down) = (Vec2::new(0.0, 1.0), Vec2::new(0.0, -1.0));
    let (out, into) = (Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0));
    let profile = [
        ProfilePoint::new(Vec2::new(outer, -half), out, 0.0),
        ProfilePoint::new(Vec2::new(outer, half), out, 0.25),
        ProfilePoint::new(Vec2::new(outer, half), up, 0.25),
        ProfilePoint::new(Vec2::new(inner, half), up, 0.5),
        ProfilePoint::new(Vec2::new(inner, half), into, 0.5),
        ProfilePoint::new(Vec2::new(inner, -half), into, 0.75),
        ProfilePoint::new(Vec2::new(inner, -half), down, 0.75),
        ProfilePoint::new(Vec2::new(outer, -half), down, 1.0),
        ProfilePoint::new(Vec2::new(outer, -half), out, 1.0),
    ];
    revolve(&profile, segments, Facets::Smooth)
}

/// A pyramid on a `sides`-sided base, apex at `+hy`. The base is sized so
/// four sides give exactly the box `hx` by `hz` describes.
#[must_use]
pub fn pyramid(hx: f32, hy: f32, hz: f32, sides: u32) -> MeshData {
    let sides = sides.max(3);
    let apothem = libm::cosf(PI / sides as f32).max(f32::MIN_POSITIVE);
    let base: Vec<Vec3> = (0..sides)
        .map(|i| {
            let (sin, cos) = libm::sincosf(std::f32::consts::TAU * (i as f32 + 0.5) / sides as f32);
            Vec3::new(hx * cos / apothem, -hy, hz * sin / apothem)
        })
        .collect();
    let apex = Vec3::new(0.0, hy, 0.0);
    let centre = Vec3::new(0.0, -hy, 0.0);
    let mut build = Build::with_capacity(6 * sides as usize, 2 * sides as usize);
    for i in 0..sides as usize {
        let (near, far) = (base[i], base[(i + 1) % sides as usize]);
        let u = i as f32 / sides as f32;
        let next_u = (i + 1) as f32 / sides as f32;
        build.face(
            &[centre, near, far],
            &[Vec2::splat(0.5), Vec2::new(u, 0.0), Vec2::new(next_u, 0.0)],
        );
        build.face(
            &[near, apex, far],
            &[Vec2::new(u, 0.0), Vec2::new(u, 1.0), Vec2::new(next_u, 0.0)],
        );
    }
    build.finish()
}

/// A flat quad in the xz plane, cut into `segments` by `segments` cells so a
/// vertex shader or a light has something to work with.
#[must_use]
pub fn plane(hx: f32, hz: f32, segments: u32) -> MeshData {
    let cells = segments.max(1) as usize;
    let mut build = Build::with_capacity((cells + 1) * (cells + 1), 2 * cells * cells);
    let mut grid = Vec::with_capacity((cells + 1) * (cells + 1));
    for i in 0..=cells {
        let u = i as f32 / cells as f32;
        for j in 0..=cells {
            let v = j as f32 / cells as f32;
            let position = Vec3::new(hx.mul_add(2.0 * u, -hx), 0.0, hz.mul_add(2.0 * v, -hz));
            grid.push(build.vertex(position, Vec3::Y, Vec2::new(u, v)));
        }
    }
    let stride = cells + 1;
    for i in 0..cells {
        for j in 0..cells {
            let a = grid[i * stride + j];
            let b = grid[(i + 1) * stride + j];
            let c = grid[(i + 1) * stride + j + 1];
            let d = grid[i * stride + j + 1];
            build.quad(a, d, c, b);
        }
    }
    build.finish()
}

/// Where a face's grid samples sit along one axis: bunched into the corner
/// arcs and sparse across the flat middle, so the rounding is smooth without
/// spending vertices on a face that is already flat.
///
/// A radius of zero collapses every sample to the two edges, which is how the
/// plain box falls out of the same code as the rounded one.
fn arc_samples(inner: f32, radius: f32, arc: u32) -> Vec<f32> {
    let mut out = Vec::with_capacity(2 * (arc as usize + 1));
    let at = |k: u32| radius.mul_add(libm::tanf(FRAC_PI_4 * k as f32 / arc as f32), inner);
    for k in (0..=arc).rev() {
        out.push(-at(k));
    }
    for k in 0..=arc {
        out.push(at(k));
    }
    out.dedup();
    out
}

/// The six faces as (axis, sign, u axis, v axis), the two grid axes ordered
/// so that u cross v is the outward normal and every face winds the same way.
const FACES: [(usize, f32, usize, usize); 6] = [
    (0, 1.0, 1, 2),
    (0, -1.0, 2, 1),
    (1, 1.0, 2, 0),
    (1, -1.0, 0, 2),
    (2, 1.0, 0, 1),
    (2, -1.0, 1, 0),
];

/// A box, optionally with rounded edges and corners.
///
/// Each face is sampled on a grid of the outer box and pushed onto the
/// rounded one: a sample is clamped into the inner box and then offset by the
/// radius along the way it moved. Interior samples move straight out, so the
/// flat faces stay flat; edge and corner samples sweep the quarter surfaces.
#[must_use]
pub fn cuboid(hx: f32, hy: f32, hz: f32, corner_radius: f32, segments: u32) -> MeshData {
    let half = [hx, hy, hz];
    let smallest = half.iter().copied().fold(f32::INFINITY, f32::min);
    let radius = corner_radius.clamp(0.0, smallest.max(0.0));
    let inner = Vec3::from_array(half) - Vec3::splat(radius);
    let inner = inner.max(Vec3::ZERO);
    let arc = if radius > 0.0 {
        (segments / 4).max(1)
    } else {
        1
    };
    let samples = [
        arc_samples(inner.x, radius, arc),
        arc_samples(inner.y, radius, arc),
        arc_samples(inner.z, radius, arc),
    ];
    let project = |p: Vec3, face: Vec3| {
        if radius <= 0.0 {
            return (p, face);
        }
        let q = p.min(inner).max(-inner);
        let normal = (p - q).normalize_or_zero();
        (normal.mul_add(Vec3::splat(radius), q), normal)
    };
    let mut build = Build::default();
    for (axis, sign, u_axis, v_axis) in FACES {
        let mut face = [0.0; 3];
        face[axis] = sign;
        let (us, vs) = (&samples[u_axis], &samples[v_axis]);
        let mut grid = Vec::with_capacity(us.len() * vs.len());
        for (i, &u) in us.iter().enumerate() {
            for (j, &v) in vs.iter().enumerate() {
                let mut p = [0.0; 3];
                p[axis] = sign * half[axis];
                p[u_axis] = u;
                p[v_axis] = v;
                let (position, normal) = project(Vec3::from_array(p), Vec3::from_array(face));
                let uv = Vec2::new(
                    i as f32 / (us.len() - 1).max(1) as f32,
                    j as f32 / (vs.len() - 1).max(1) as f32,
                );
                grid.push(build.vertex(position, normal, uv));
            }
        }
        for i in 0..us.len() - 1 {
            for j in 0..vs.len() - 1 {
                let stride = vs.len();
                build.quad(
                    grid[i * stride + j],
                    grid[(i + 1) * stride + j],
                    grid[(i + 1) * stride + j + 1],
                    grid[i * stride + j + 1],
                );
            }
        }
    }
    build.finish()
}

/// How far a pyramid's base reaches along x and z, as a multiple of its
/// half-extents. Four sides land exactly on the box; three stretch to twice
/// it on one axis, because a triangle inscribed that way has a corner there.
#[must_use]
pub(super) fn base_reach(sides: u32) -> Vec2 {
    let sides = sides.max(3);
    let apothem = libm::cosf(PI / sides as f32).max(f32::MIN_POSITIVE);
    super::build::ring_reach(sides, PI / sides as f32) / apothem
}

/// How far a prism's cross-section reaches along x and z, as a multiple of
/// its radius: the same question, for a ring that starts on the x axis.
#[must_use]
pub(super) fn prism_reach(sides: u32) -> Vec2 {
    super::build::ring_reach(sides, 0.0)
}
