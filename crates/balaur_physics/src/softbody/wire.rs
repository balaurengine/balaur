//! What a 3D wire is drawn as: a rope collides through segments, which have
//! no faces, so each becomes a thin tube as wide as the rope's particles.

use std::collections::BTreeMap;

use glamx::Vec3;

/// Vertices around each particle's ring.
const SIDES: u32 = 6;

/// Segments drawn as a tube `radius` wide: one ring of `SIDES` vertices a
/// point, `2 * SIDES` triangles a segment. The triangles depend only on the
/// segments, so a moving rope keeps its topology.
pub(super) fn tube(
    points: &[[f32; 3]],
    segments: &[[u32; 2]],
    radius: f32,
) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let count = points.len() as u32;
    let segments: Vec<[u32; 2]> = segments
        .iter()
        .copied()
        .filter(|&[a, b]| a < count && b < count && a != b)
        .collect();
    let at = |i: u32| Vec3::from_array(points[i as usize]);
    let mut tangents = vec![Vec3::ZERO; points.len()];
    let mut beside: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &[a, b] in &segments {
        let along = (at(b) - at(a)).normalize_or_zero();
        tangents[a as usize] += along;
        tangents[b as usize] += along;
        beside.entry(a).or_default().push(b);
        beside.entry(b).or_default().push(a);
    }
    let tangents: Vec<Vec3> = tangents
        .iter()
        .map(|t| match t.normalize_or_zero() {
            Vec3::ZERO => Vec3::Y,
            unit => unit,
        })
        .collect();
    let normals = transported_normals(&tangents, &beside);
    let mut positions = Vec::with_capacity(points.len() * SIDES as usize);
    for (i, (tangent, normal)) in tangents.iter().zip(&normals).enumerate() {
        let binormal = tangent.cross(*normal);
        let centre = at(i as u32);
        for side in 0..SIDES {
            let angle = std::f32::consts::TAU * side as f32 / SIDES as f32;
            let out = *normal * libm::cosf(angle) + binormal * libm::sinf(angle);
            positions.push((centre + out * radius).to_array());
        }
    }
    let mut triangles = Vec::with_capacity(segments.len() * 2 * SIDES as usize);
    for &[a, b] in &segments {
        for side in 0..SIDES {
            let next = (side + 1) % SIDES;
            let (a_here, a_next) = (a * SIDES + side, a * SIDES + next);
            let (b_here, b_next) = (b * SIDES + side, b * SIDES + next);
            triangles.push([a_here, a_next, b_next]);
            triangles.push([a_here, b_next, b_here]);
        }
    }
    (positions, triangles)
}

/// A normal per point, carried from neighbour to neighbour along the chain so
/// consecutive rings line up rather than twisting where a fixed axis would.
fn transported_normals(tangents: &[Vec3], beside: &BTreeMap<u32, Vec<u32>>) -> Vec<Vec3> {
    let mut normals: Vec<Option<Vec3>> = vec![None; tangents.len()];
    // Open ends first, so a rope's frame starts at an end, not partway along.
    let mut starts: Vec<u32> = beside
        .iter()
        .filter(|(_, next)| next.len() == 1)
        .map(|(&i, _)| i)
        .collect();
    starts.extend(0..tangents.len() as u32);
    for start in starts {
        if normals[start as usize].is_some() {
            continue;
        }
        normals[start as usize] = Some(tangents[start as usize].any_orthonormal_vector());
        let mut stack = vec![start];
        while let Some(from) = stack.pop() {
            let carried = normals[from as usize].unwrap_or(Vec3::X);
            for &to in beside.get(&from).map_or(&[][..], Vec::as_slice) {
                if normals[to as usize].is_some() {
                    continue;
                }
                let tangent = tangents[to as usize];
                let normal = match (carried - tangent * carried.dot(tangent)).normalize_or_zero() {
                    Vec3::ZERO => tangent.any_orthonormal_vector(),
                    unit => unit,
                };
                normals[to as usize] = Some(normal);
                stack.push(to);
            }
        }
    }
    normals
        .into_iter()
        .zip(tangents)
        .map(|(normal, tangent)| normal.unwrap_or_else(|| tangent.any_orthonormal_vector()))
        .collect()
}

#[cfg(test)]
mod tests {
    use glamx::Vec3;

    use super::{SIDES, tube};

    #[test]
    fn a_tube_is_a_ring_a_point_and_two_triangles_a_side_a_segment() {
        let points = [[0.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, -2.0, 0.0]];
        let (positions, triangles) = tube(&points, &[[0, 1], [1, 2]], 0.1);
        assert_eq!(positions.len(), 3 * SIDES as usize);
        assert_eq!(triangles.len(), 2 * 2 * SIDES as usize);
        let wide = positions[..SIDES as usize]
            .iter()
            .all(|p| (Vec3::from_array(*p).length() - 0.1).abs() < 1.0e-5);
        assert!(wide, "the first ring is not the radius out from its point");
    }

    #[test]
    fn every_index_a_tube_draws_names_one_of_its_vertices() {
        let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
        let padded = [[0, 1], [1, 2], [2, u32::MAX]];
        let (positions, triangles) = tube(&points, &padded, 0.05);
        assert_eq!(
            triangles.len(),
            2 * 2 * SIDES as usize,
            "the padded segment was drawn"
        );
        assert!(
            triangles
                .iter()
                .flatten()
                .all(|&i| (i as usize) < positions.len())
        );
    }

    #[test]
    fn a_tube_faces_outward() {
        let points = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let (positions, triangles) = tube(&points, &[[0, 1]], 0.1);
        for [a, b, c] in triangles {
            let [a, b, c] = [a, b, c].map(|i| Vec3::from_array(positions[i as usize]));
            let normal = (b - a).cross(c - a);
            let middle = (a + b + c) / 3.0;
            let out = Vec3::new(middle.x, 0.0, middle.z);
            assert!(normal.dot(out) > 0.0, "a face points into the tube");
        }
    }
}
