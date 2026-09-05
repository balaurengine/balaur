//! Shared machinery for the meshers: a vertex accumulator, a lathe, and the
//! fill that turns a 2D outline into triangles.
//!
//! Every angle goes through `libm` rather than the platform's, so a mesh has
//! the same vertices on every operating system (docs/DETERMINISM.md).

use crate::mesh::MeshData;
use glamx::{Vec2, Vec3};

/// A mesh under construction: one vertex stream and the triangles over it.
#[derive(Default)]
pub(crate) struct Build {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<[u32; 3]>,
}

impl Build {
    pub(crate) fn with_capacity(vertices: usize, triangles: usize) -> Self {
        Self {
            positions: Vec::with_capacity(vertices),
            normals: Vec::with_capacity(vertices),
            uvs: Vec::with_capacity(vertices),
            indices: Vec::with_capacity(triangles),
        }
    }

    /// Append one vertex and hand back its index.
    pub(crate) fn vertex(&mut self, position: Vec3, normal: Vec3, uv: Vec2) -> u32 {
        let index = self.positions.len() as u32;
        self.positions.push(position.to_array());
        self.normals.push(normal.normalize_or_zero().to_array());
        self.uvs.push(uv.to_array());
        index
    }

    /// Append one triangle. A degenerate one is dropped rather than kept: a
    /// lathe's poles produce them, and a zero-area face has no normal.
    pub(crate) fn triangle(&mut self, a: u32, b: u32, c: u32) {
        if a == b || b == c || a == c {
            return;
        }
        let at = |i: u32| Vec3::from_array(self.positions[i as usize]);
        if (at(b) - at(a)).cross(at(c) - at(a)) == Vec3::ZERO {
            return;
        }
        self.indices.push([a, b, c]);
    }

    /// Append a quad as two triangles, wound in the order given.
    pub(crate) fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.triangle(a, b, c);
        self.triangle(a, c, d);
    }

    /// Append a flat-shaded face: fresh vertices sharing one normal, fanned
    /// into triangles. A face cannot share corners with its neighbours, or
    /// the normal would be averaged away.
    ///
    /// The normal is Newell's, summed around the loop, so a quad with two
    /// corners in the same place -- a cap facet meeting the axis -- still has
    /// one rather than a zero-length cross product.
    pub(crate) fn face(&mut self, corners: &[Vec3], uvs: &[Vec2]) {
        if corners.len() < 3 {
            return;
        }
        let mut normal = Vec3::ZERO;
        for (a, b) in corners.iter().zip(corners.iter().cycle().skip(1)) {
            normal += Vec3::new(
                (a.y - b.y) * (a.z + b.z),
                (a.z - b.z) * (a.x + b.x),
                (a.x - b.x) * (a.y + b.y),
            );
        }
        if normal == Vec3::ZERO {
            return;
        }
        let normal = normal.normalize_or_zero();
        let first = self.positions.len() as u32;
        for (corner, uv) in corners.iter().zip(uvs) {
            self.vertex(*corner, normal, *uv);
        }
        for i in 1..corners.len() as u32 - 1 {
            self.triangle(first, first + i, first + i + 1);
        }
    }

    pub(crate) fn finish(self) -> MeshData {
        MeshData {
            positions: self.positions,
            indices: self.indices,
            normals: Some(self.normals),
            uvs: Some(self.uvs),
            source: None,
            text: None,
            skin: None,
        }
    }
}

/// One sample of a lathe profile, in the half-plane the surface is spun from:
/// `position.x` is the distance from the axis and `position.y` the height.
///
/// The normal is in the same half-plane and is spun with the point, which is
/// what lets one profile carry both a smooth wall and a hard cap edge: two
/// samples at the same place with different normals.
#[derive(Clone, Copy, Debug)]
pub struct ProfilePoint {
    pub position: Vec2,
    pub normal: Vec2,
    /// Where this sample sits along the profile, as the mesh's `v`.
    pub v: f32,
}

impl ProfilePoint {
    #[must_use]
    pub fn new(position: Vec2, normal: Vec2, v: f32) -> Self {
        Self {
            position,
            normal,
            v,
        }
    }
}

/// Whether a revolved surface shares its ring vertices or gives every facet
/// its own: smooth is a cylinder, flat is a prism.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Facets {
    Smooth,
    Flat,
}

/// The surface a profile sweeps out when spun `segments` times around y.
///
/// A closed profile repeats its first sample at the end rather than wrapping,
/// so `v` runs to one and the seam's texture coordinates are right.
#[must_use]
pub fn revolve(profile: &[ProfilePoint], segments: u32, facets: Facets) -> MeshData {
    let segments = segments.max(3) as usize;
    let count = profile.len();
    if count < 2 {
        return MeshData::default();
    }
    let mut build = Build::with_capacity((segments + 1) * count, 2 * segments * count);
    // The seam takes segment zero's sine and cosine rather than a full turn's:
    // `sincosf(TAU)` is not exactly `sincosf(0)`, and the difference is a
    // hairline crack down every revolved shape.
    let around = |i: usize| {
        let u = i as f32 / segments as f32;
        let turn = (i % segments) as f32 / segments as f32;
        let (sin, cos) = libm::sincosf(std::f32::consts::TAU * turn);
        (u, sin, cos)
    };
    let placed = |p: &ProfilePoint, sin: f32, cos: f32| {
        (
            Vec3::new(p.position.x * cos, p.position.y, p.position.x * sin),
            Vec3::new(p.normal.x * cos, p.normal.y, p.normal.x * sin),
        )
    };
    if facets == Facets::Flat {
        for i in 0..segments {
            let (u, sin, cos) = around(i);
            let (next_u, next_sin, next_cos) = around(i + 1);
            for j in 0..count - 1 {
                let (low, _) = placed(&profile[j], sin, cos);
                let (high, _) = placed(&profile[j + 1], sin, cos);
                let (next_high, _) = placed(&profile[j + 1], next_sin, next_cos);
                let (next_low, _) = placed(&profile[j], next_sin, next_cos);
                let (v0, v1) = (profile[j].v, profile[j + 1].v);
                build.face(
                    &[low, high, next_high, next_low],
                    &[
                        Vec2::new(u, v0),
                        Vec2::new(u, v1),
                        Vec2::new(next_u, v1),
                        Vec2::new(next_u, v0),
                    ],
                );
            }
        }
        return build.finish();
    }
    let mut ring = Vec::with_capacity((segments + 1) * count);
    for i in 0..=segments {
        let (u, sin, cos) = around(i);
        for p in profile {
            let (position, normal) = placed(p, sin, cos);
            ring.push(build.vertex(position, normal, Vec2::new(u, p.v)));
        }
    }
    for i in 0..segments {
        for j in 0..count - 1 {
            let low = ring[i * count + j];
            let high = ring[i * count + j + 1];
            let next_high = ring[(i + 1) * count + j + 1];
            let next_low = ring[(i + 1) * count + j];
            build.quad(low, high, next_high, next_low);
        }
    }
    build.finish()
}

/// A closed outline filled with triangles, in the z = 0 plane facing +z.
///
/// The outline is ear-clipped, so a star fills as readily as a circle; a
/// self-crossing one gets whatever the clipper can make of it.
#[must_use]
pub fn fill(outline: &[Vec2]) -> MeshData {
    if outline.len() < 3 {
        return MeshData::default();
    }
    let ring: Vec<u32> = (0..outline.len() as u32).collect();
    let Ok(indices) = crate::triangulate::triangulate(outline, &ring) else {
        return MeshData::default();
    };
    let mut min = outline[0];
    let mut max = outline[0];
    for p in outline {
        min = min.min(*p);
        max = max.max(*p);
    }
    let span = (max - min).max(Vec2::splat(f32::MIN_POSITIVE));
    let mut build = Build::with_capacity(outline.len(), indices.len());
    for p in outline {
        build.vertex(p.extend(0.0), Vec3::Z, (*p - min) / span);
    }
    for [a, b, c] in indices {
        build.triangle(a, b, c);
    }
    build.finish()
}

/// How far a ring of `count` evenly spaced points starting at `offset`
/// reaches along x and y, as a fraction of its radius.
///
/// A hexagon is a full radius wide and less than that tall, so a box fitted
/// to one -- for picking, or for a selection outline -- has to ask.
#[must_use]
pub(crate) fn ring_reach(count: u32, offset: f32) -> Vec2 {
    let count = count.max(3);
    (0..count).fold(Vec2::ZERO, |reach, i| {
        let turn = std::f32::consts::TAU * i as f32 / count as f32;
        let (sin, cos) = libm::sincosf(offset + turn);
        reach.max(Vec2::new(cos.abs(), sin.abs()))
    })
}
