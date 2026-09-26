//! What a 2D soft body is drawn as: its skin, its cells, its outline filled,
//! or its segments as a ribbon, whichever its layout left it with.

use std::collections::{BTreeMap, BTreeSet};

use glamx::Vec2;

use crate::rapier2d::prelude::SoftBody;
use crate::scalar::{self, Pose2, Vector2};

/// Positions in the node's space, and the triangles over them.
pub(super) type Drawn = (Vec<[f32; 2]>, Vec<[u32; 3]>);

/// On a node whose body carries its mesh as a skin: that mesh's triangles,
/// which the skin, an outline, does not keep.
pub(super) struct SkinTriangles(pub(super) Vec<[u32; 3]>);

/// What `body` is drawn as, in the space `inverse` takes world points into.
/// `skin_triangles` answers the triangles of the mesh a skinned body carries.
pub(super) fn drawn(
    body: &SoftBody,
    inverse: Pose2,
    skin_triangles: impl FnOnce() -> Vec<[u32; 3]>,
) -> Drawn {
    let local = |p: Vector2| scalar::a2(inverse * p);
    // The skin's vertices are the mesh's, one for one, so a node's own
    // polygon deforms with them rather than being replaced.
    if let Some(skin) = body.meshes().find(|mesh| mesh.is_skinned()) {
        return (
            skin.vertex_positions(body).map(local).collect(),
            skin_triangles(),
        );
    }
    let positions: Vec<[f32; 2]> = body.particle_positions().map(local).collect();
    if !body.cells().is_empty() {
        let cells = body.cells().iter().map(|cell| cell.vertices).collect();
        return (positions, cells);
    }
    match rings(body.boundary()) {
        Some(rings) => {
            let triangles = filled(body, &rings);
            (positions, triangles)
        }
        None => ribbon(
            &positions,
            body.boundary(),
            scalar::f32_of(body.particle_radius()),
        ),
    }
}

/// The boundary as closed rings, when every vertex on it has two neighbours;
/// `None` when one end is open, as a rope's is.
fn rings(boundary: &[[u32; 2]]) -> Option<Vec<Vec<u32>>> {
    let mut beside: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &[a, b] in boundary {
        beside.entry(a).or_default().push(b);
        beside.entry(b).or_default().push(a);
    }
    if beside.is_empty() || beside.values().any(|n| n.len() != 2) {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for &start in beside.keys() {
        if !seen.insert(start) {
            continue;
        }
        let mut ring = vec![start];
        let (mut came, mut at) = (start, beside[&start][0]);
        while at != start && seen.insert(at) {
            ring.push(at);
            let next = &beside[&at];
            let step = if next[0] == came { next[1] } else { next[0] };
            (came, at) = (at, step);
        }
        out.push(ring);
    }
    Some(out)
}

/// The rings filled over the shape the body was built in, which does not
/// move, so the triangles stay the same from one step to the next.
fn filled(body: &SoftBody, rings: &[Vec<u32>]) -> Vec<[u32; 3]> {
    let rest: Vec<Vec2> = body
        .particles()
        .iter()
        .map(|p| Vec2::from_array(scalar::a2(p.initial_rest_position())))
        .collect();
    rings
        .iter()
        .filter(|ring| ring.len() >= 3)
        .filter_map(|ring| balaur_core::triangulate::triangulate(&rest, ring).ok())
        .flatten()
        .collect()
}

/// Segments drawn as a strip `2 * half` wide: two vertices a particle, either
/// side of it along the mean of its segments' normals.
fn ribbon(points: &[[f32; 2]], segments: &[[u32; 2]], half: f32) -> Drawn {
    let at = |i: u32| Vec2::from_array(points[i as usize]);
    let within = |&&[a, b]: &&[u32; 2]| (a as usize) < points.len() && (b as usize) < points.len();
    let mut normals = vec![Vec2::ZERO; points.len()];
    for &[a, b] in segments.iter().filter(within) {
        let normal = (at(b) - at(a)).perp().normalize_or_zero();
        normals[a as usize] += normal;
        normals[b as usize] += normal;
    }
    let mut positions = Vec::with_capacity(points.len() * 2);
    for (i, normal) in normals.iter().enumerate() {
        let side = match normal.normalize_or_zero() {
            Vec2::ZERO => Vec2::Y,
            unit => unit,
        } * half;
        let p = at(i as u32);
        positions.push((p + side).to_array());
        positions.push((p - side).to_array());
    }
    let triangles = segments
        .iter()
        .filter(within)
        .flat_map(|&[a, b]| {
            let (left_a, right_a, left_b, right_b) = (2 * a, 2 * a + 1, 2 * b, 2 * b + 1);
            [[left_a, right_a, right_b], [left_a, right_b, left_b]]
        })
        .collect();
    (positions, triangles)
}

#[cfg(test)]
mod tests {
    use super::{ribbon, rings};

    #[test]
    fn a_closed_boundary_is_one_ring_and_an_open_one_is_none() {
        let square = [[0, 1], [1, 2], [2, 3], [3, 0]];
        assert_eq!(rings(&square), Some(vec![vec![0, 1, 2, 3]]));
        assert_eq!(rings(&[[0, 1], [1, 2]]), None, "a chain has two open ends");
    }

    #[test]
    fn a_ribbon_is_two_vertices_a_point_and_two_triangles_a_segment() {
        let (positions, triangles) = ribbon(
            &[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]],
            &[[0, 1], [1, 2]],
            0.1,
        );
        assert_eq!(positions.len(), 6);
        assert_eq!(triangles.len(), 4);
        assert!(
            (positions[0][1] - positions[1][1]).abs() > 0.19,
            "the strip has its width"
        );
    }
}
