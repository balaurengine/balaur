//! A thick polyline as triangles: one strip per segment, round joins and
//! caps as fans, UVs running along the length so a texture or a dash
//! pattern can ride it. Written out in `f32` with `libm` trigonometry, so
//! the same chain triangulates identically on every platform.

use glamx::Vec2;

/// How many triangles a half-turn of a round join or cap gets.
const ROUND_STEPS: usize = 8;

/// One drawable piece of a chain, with where along the chain it sits so a
/// gradient can colour it.
pub(crate) struct Piece {
    pub(crate) coords: Vec<Vec2>,
    pub(crate) faces: Vec<[u32; 3]>,
    /// `u` along the chain in world units, `v` across the width in 0..1.
    pub(crate) uvs: Vec<Vec2>,
    /// Where the piece's middle sits along the chain, 0 at the start and 1
    /// at the end.
    pub(crate) along: f32,
}

/// The chain's total length, for the `along` fractions.
fn length(points: &[Vec2]) -> f32 {
    points.windows(2).map(|p| (p[1] - p[0]).length()).sum()
}

/// One straight segment as a quad.
fn segment(a: Vec2, b: Vec2, width: f32, start_u: f32) -> Option<(Piece, f32)> {
    let dir = b - a;
    let len = dir.length();
    if len <= f32::EPSILON {
        return None;
    }
    let normal = Vec2::new(-dir.y, dir.x) / len * (width / 2.0);
    let coords = vec![a + normal, a - normal, b - normal, b + normal];
    let uvs = vec![
        Vec2::new(start_u, 0.0),
        Vec2::new(start_u, 1.0),
        Vec2::new(start_u + len, 1.0),
        Vec2::new(start_u + len, 0.0),
    ];
    let faces = vec![[0, 1, 2], [0, 2, 3]];
    Some((
        Piece {
            coords,
            faces,
            uvs,
            along: 0.0,
        },
        len,
    ))
}

/// A disc at a joint or an end, as a fan; the round join everywhere a
/// segment meets the next, and the round cap at an open end.
fn disc(center: Vec2, width: f32, u: f32) -> Piece {
    let radius = width / 2.0;
    let steps = ROUND_STEPS * 2;
    let mut coords = vec![center];
    let mut uvs = vec![Vec2::new(u, 0.5)];
    for i in 0..steps {
        let angle = std::f32::consts::TAU * i as f32 / steps as f32;
        let (sin, cos) = libm::sincosf(angle);
        coords.push(center + Vec2::new(cos, sin) * radius);
        uvs.push(Vec2::new(u, 0.5 + sin * 0.5));
    }
    let faces = (0..steps)
        .map(|i| [0, 1 + i as u32, 1 + ((i + 1) % steps) as u32])
        .collect();
    Piece {
        coords,
        faces,
        uvs,
        along: 0.0,
    }
}

/// Every piece of a chain, in drawing order. A closed chain joins its last
/// point back to its first; an open one gets a cap at each end.
pub(crate) fn pieces(points: &[Vec2], width: f32, closed: bool) -> Vec<Piece> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut chain: Vec<Vec2> = points.to_vec();
    if closed && chain.len() > 2 && chain.first() != chain.last() {
        chain.push(chain[0]);
    }
    let total = length(&chain).max(f32::EPSILON);
    let mut out = Vec::new();
    let mut u = 0.0;
    for pair in chain.windows(2) {
        let Some((mut piece, len)) = segment(pair[0], pair[1], width, u) else {
            continue;
        };
        piece.along = (u + len / 2.0) / total;
        out.push(piece);
        // A disc after every segment: the joint to the next, or the cap at
        // an open end. On a closed chain the last one covers the seam.
        let mut joint = disc(pair[1], width, u + len);
        joint.along = (u + len) / total;
        out.push(joint);
        u += len;
    }
    if !closed {
        let mut cap = disc(chain[0], width, 0.0);
        cap.along = 0.0;
        out.push(cap);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_points_make_one_quad_and_two_caps() {
        let pieces = pieces(&[Vec2::ZERO, Vec2::new(4.0, 0.0)], 1.0, false);
        assert_eq!(pieces.len(), 3);
        let quad = &pieces[0];
        assert_eq!(quad.coords.len(), 4);
        assert_eq!(quad.faces.len(), 2);
        assert!(
            (quad.uvs[2].x - 4.0).abs() < 1e-6,
            "u runs the segment's length"
        );
        assert!((quad.along - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_closed_triangle_has_a_joint_at_every_corner_and_no_caps() {
        let tri = [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)];
        let pieces = pieces(&tri, 0.1, true);
        // Three segments, each followed by a joint.
        assert_eq!(pieces.len(), 6);
    }

    #[test]
    fn a_joint_is_a_fan_around_the_corner() {
        let pieces = pieces(
            &[Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)],
            0.2,
            false,
        );
        let joint = &pieces[1];
        assert_eq!(joint.coords[0], Vec2::new(1.0, 0.0));
        assert_eq!(joint.faces.len(), ROUND_STEPS * 2);
        for corner in &joint.coords[1..] {
            assert!(((*corner - Vec2::new(1.0, 0.0)).length() - 0.1).abs() < 1e-5);
        }
    }

    #[test]
    fn a_repeated_point_adds_no_piece() {
        let pieces = pieces(&[Vec2::ZERO, Vec2::ZERO, Vec2::new(1.0, 0.0)], 1.0, false);
        // One real segment: its quad, its end joint, and the start cap.
        assert_eq!(pieces.len(), 3);
    }
}
