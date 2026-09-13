//! A concave polygon into convex pieces that overlap across their seams.
//!
//! parry's Hertel-Mehlhorn merges a triangulation into convex pieces, exactly
//! and with no resolution to pick. The growing afterwards is this module's
//! own: every piece reaches through each seam it has, so a thin body cannot
//! wedge into one. Erin Catto's [Stuck Inside] is why a seam needs it.
//!
//! [Stuck Inside]: https://box2d.org/posts/2020/04/stuck-inside/

use crate::rapier2d::parry::transformation::{convex_hull, hertel_mehlhorn_idx};
use crate::scalar::Vector2;
use crate::vocabulary::{Opts, keys as k};
use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, Value};
use std::collections::BTreeMap;

/// Two points nearer than this are one point, at the scale a game is built.
const EPSILON: f32 = 1.0e-6;

/// How far the area of a union may drift before it is not convex after all.
const AREA_TOLERANCE: f32 = 1.0e-3;

/// The polygon cut into convex pieces that overlap, each counter-clockwise.
///
/// `overlap` runs from 0, the plain decomposition, to 1, where a piece grows
/// flush with the face that stopped it.
///
/// # Errors
/// If the ring cannot be triangulated: fewer than three distinct points, no
/// area, or a loop that crosses itself.
pub(crate) fn decompose_polygon(polygon: &[Vector2], overlap: f32) -> Result<Vec<Vec<Vector2>>> {
    let ring: Vec<u32> = (0..polygon.len() as u32).collect();
    let triangles = balaur_core::triangulate::triangulate(polygon, &ring)?;
    Ok(decompose(polygon, &triangles, overlap))
}

/// The same, from a triangulation already in hand: a mesh asset's own
/// triangles, holes and interior points included.
pub(crate) fn decompose(
    points: &[Vector2],
    triangles: &[[u32; 3]],
    overlap: f32,
) -> Vec<Vec<Vector2>> {
    grown(points, &pieces(points, triangles), overlap)
}

/// The triangles merged into convex pieces, each an index loop wound
/// counter-clockwise. Two pieces naming the same pair of points share a seam.
///
/// Hertel-Mehlhorn, from parry: at most four times the fewest pieces, usually
/// the fewest, and exact, so there is no resolution to pick.
pub(crate) fn pieces(points: &[Vector2], triangles: &[[u32; 3]]) -> Vec<Vec<u32>> {
    hertel_mehlhorn_idx(points, triangles)
}

/// Every piece grown through every seam it has, as points rather than
/// indices: growing adds vertices no triangulation named.
pub(crate) fn grown(points: &[Vector2], pieces: &[Vec<u32>], overlap: f32) -> Vec<Vec<Vector2>> {
    let seams = seams(pieces);
    let overlap = overlap.clamp(0.0, 1.0);
    pieces
        .iter()
        .enumerate()
        .map(|(which, ring)| {
            let mut out: Vec<Vector2> = Vec::with_capacity(ring.len());
            for at in 0..ring.len() {
                out.push(points[ring[at] as usize]);
                out.extend(growth(points, pieces, &seams, (which, at), overlap));
            }
            tidied(&out)
        })
        .collect()
}

/// Where each directed edge lives, so the piece across a seam is one lookup:
/// the same edge the other way round names it.
fn seams(pieces: &[Vec<u32>]) -> BTreeMap<(u32, u32), usize> {
    let mut out = BTreeMap::new();
    for (which, ring) in pieces.iter().enumerate() {
        for at in 0..ring.len() {
            out.insert((ring[at], step(ring, at, 1)), which);
        }
    }
    out
}

/// The vertex `delta` steps along `ring` from position `at`.
fn step(ring: &[u32], at: usize, delta: usize) -> u32 {
    ring[(at + delta) % ring.len()]
}

/// The vertices a piece gains at the seam on its edge `at`: the far boundary
/// of what it sweeps through that seam, from the edge's start to its end.
fn growth(
    points: &[Vector2],
    pieces: &[Vec<u32>],
    seams: &BTreeMap<(u32, u32), usize>,
    (which, at): (usize, usize),
    overlap: f32,
) -> Vec<Vector2> {
    if overlap <= 0.0 {
        return Vec::new();
    }
    let ring = &pieces[which];
    let point = |index: u32| points[index as usize];
    let (from, to) = (point(ring[at]), point(step(ring, at, 1)));
    let Some(&across) = seams.get(&(step(ring, at, 1), ring[at])) else {
        return Vec::new();
    };
    // What the piece would sweep: past the seam, and between its own two
    // edges at the seam carried on as lines.
    let strip = |shape: &[Vector2]| {
        let past = clipped(shape, to, from);
        let one = clipped(&past, point(step(ring, at, ring.len() - 1)), from);
        clipped(&one, to, point(step(ring, at, 2)))
    };
    let region = reach(points, pieces, (which, across), &strip);
    chain(&trimmed(&region, from, to, overlap), from, to)
}

/// The pieces the strip crosses whole, as one convex region: a leg reaches
/// through a shelf into the table top, but only while the union stays convex.
///
/// The union of two convex pieces is convex exactly when its area is theirs,
/// which is the test, so a strip that leaves through a corner stops there.
fn reach(
    points: &[Vector2],
    pieces: &[Vec<u32>],
    (home, across): (usize, usize),
    strip: &dyn Fn(&[Vector2]) -> Vec<Vector2>,
) -> Vec<Vector2> {
    let shape = |which: usize| {
        let ring: Vec<Vector2> = pieces[which]
            .iter()
            .map(|&index| points[index as usize])
            .collect();
        strip(&ring)
    };
    let mut region = shape(across);
    let mut area = twice_area(&region);
    if area <= EPSILON {
        return Vec::new();
    }
    let mut taken = vec![false; pieces.len()];
    taken[home] = true;
    taken[across] = true;
    let mut growing = true;
    while growing {
        growing = false;
        #[allow(
            clippy::needless_range_loop,
            reason = "the index reads `pieces` and writes `taken`, which cannot borrow at once"
        )]
        for next in 0..pieces.len() {
            let part = if taken[next] { Vec::new() } else { shape(next) };
            let part_area = twice_area(&part);
            if part_area <= EPSILON {
                continue;
            }
            let mut both = region.clone();
            both.extend_from_slice(&part);
            let hull = hull_of(&both);
            let whole = twice_area(&hull);
            if whole - area - part_area > AREA_TOLERANCE * whole {
                continue;
            }
            region = hull;
            area += part_area;
            taken[next] = true;
            growing = true;
        }
    }
    region
}

/// The region pulled back from the face that stopped it, so the two pieces do
/// not both present that face: `overlap` of the depth, and all of it at 1.
fn trimmed(region: &[Vector2], from: Vector2, to: Vector2, overlap: f32) -> Vec<Vector2> {
    if region.len() < 3 {
        return Vec::new();
    }
    let along = to - from;
    let beyond = Vector2::new(along.y, -along.x);
    let length = beyond.length();
    if length <= EPSILON {
        return Vec::new();
    }
    let beyond = beyond / length;
    let depth = region
        .iter()
        .map(|point| (*point - from).dot(beyond))
        .fold(0.0f32, f32::max);
    let face = from + beyond * (depth * overlap);
    clipped(region, face, face + along)
}

/// The far side of the region, from the seam's start round to its end: the
/// points that stand in for the seam once the piece has grown through it.
fn chain(region: &[Vector2], from: Vector2, to: Vector2) -> Vec<Vector2> {
    if region.len() < 3 {
        return Vec::new();
    }
    let nearest = |target: Vector2| {
        (0..region.len()).min_by(|&one, &other| {
            region[one]
                .distance_squared(target)
                .total_cmp(&region[other].distance_squared(target))
        })
    };
    let (Some(start), Some(end)) = (nearest(from), nearest(to)) else {
        return Vec::new();
    };
    if start == end {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut walk = (start + 1) % region.len();
    while walk != end {
        let point = region[walk];
        if point.distance_squared(from) > EPSILON * EPSILON
            && point.distance_squared(to) > EPSILON * EPSILON
        {
            out.push(point);
        }
        walk = (walk + 1) % region.len();
    }
    out
}

/// The hull of a set of points, counter-clockwise. parry asserts on fewer
/// than two, and a set with no area has no hull worth the name.
fn hull_of(points: &[Vector2]) -> Vec<Vector2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    convex_hull(points)
}

/// A grown ring with its repeated points dropped, through the hull that also
/// drops the ones an edge runs straight through.
fn tidied(ring: &[Vector2]) -> Vec<Vector2> {
    if twice_area(ring).abs() <= EPSILON {
        return ring.to_vec();
    }
    hull_of(ring)
}

fn cross(one: Vector2, other: Vector2) -> f32 {
    one.x * other.y - one.y * other.x
}

/// Twice the signed area; positive is counter-clockwise.
fn twice_area(ring: &[Vector2]) -> f32 {
    let mut sum = 0.0;
    for at in 0..ring.len() {
        sum += cross(ring[at], ring[(at + 1) % ring.len()]);
    }
    sum
}

/// The ring kept to the left of the line through `from` toward `to`.
fn clipped(ring: &[Vector2], from: Vector2, to: Vector2) -> Vec<Vector2> {
    let side = |point: Vector2| cross(to - from, point - from);
    let mut out = Vec::with_capacity(ring.len() + 1);
    for at in 0..ring.len() {
        let (here, next) = (ring[at], ring[(at + 1) % ring.len()]);
        let (inside, ahead) = (side(here), side(next));
        if inside >= 0.0 {
            out.push(here);
        }
        if (inside > 0.0 && ahead < 0.0) || (inside < 0.0 && ahead > 0.0) {
            out.push(here + (next - here) * (inside / (inside - ahead)));
        }
    }
    out
}

/// `geometry2d.convex_decomposition`: the pieces a script or the editor can
/// draw, tune, or hand back one at a time as `convex_hull` colliders.
pub(crate) fn install_decompose_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "convex_decomposition",
        &[],
        "(polygon: list, opts: table?) -> list",
        "The polygon cut into convex pieces that overlap across their seams, so nothing wedges into one; `overlap` from 0 to 1 says how far each piece grows, 0.9 by default.",
    )]);
    m.function(
        "convex_decomposition",
        |_: &Engine, (polygon, opts): (Value, Option<Value>)| {
            let points = polygon_of(&polygon)?;
            let overlap = Opts(opts.as_ref()).f32(k::OVERLAP, 0.9);
            Ok(Value::List(
                decompose_polygon(&points, overlap)?
                    .iter()
                    .map(|piece| {
                        Value::List(
                            piece
                                .iter()
                                .map(|point| Value::Vec2([point.x, point.y]))
                                .collect(),
                        )
                    })
                    .collect(),
            ))
        },
    );
}

/// A script's list of `[x, y]` pairs or vectors as a ring.
fn polygon_of(value: &Value) -> Result<Vec<Vector2>> {
    let Value::List(points) = value else {
        return Err(anyhow!("a polygon is a list of points, got {value:?}"));
    };
    points
        .iter()
        .map(|point| match point {
            Value::Vec2([x, y]) | Value::Vec3([x, y, _]) => Ok(Vector2::new(*x, *y)),
            Value::List(pair) if pair.len() >= 2 => {
                let at = |value: &Value| match value {
                    Value::Num(number) => Ok(*number as f32),
                    Value::Int(number) => Ok(*number as f32),
                    other => Err(anyhow!("a coordinate should be a number, got {other:?}")),
                };
                Ok(Vector2::new(at(&pair[0])?, at(&pair[1])?))
            }
            other => Err(anyhow!("a point is [x, y] or a vector, got {other:?}")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{decompose_polygon, twice_area};
    use crate::scalar::Vector2;

    fn ring(points: &[[f32; 2]]) -> Vec<Vector2> {
        points.iter().map(|&[x, y]| Vector2::new(x, y)).collect()
    }

    /// A table: a top from y 1 to 1.2, and two legs hanging to y 0.
    fn table() -> Vec<Vector2> {
        ring(&[
            [0.0, 0.0],
            [0.3, 0.0],
            [0.3, 1.0],
            [1.7, 1.0],
            [1.7, 0.0],
            [2.0, 0.0],
            [2.0, 1.2],
            [0.0, 1.2],
        ])
    }

    /// The table as Catto draws it, which a triangulation does not always
    /// hand us: a slab over two legs, each seam a straight cut across.
    fn slab_and_legs() -> (Vec<Vector2>, Vec<Vec<u32>>) {
        let points = ring(&[
            [0.0, 0.0],
            [0.3, 0.0],
            [0.3, 1.0],
            [0.0, 1.0],
            [1.7, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.7, 1.0],
            [2.0, 1.2],
            [0.0, 1.2],
        ]);
        let pieces = vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![3, 2, 7, 6, 8, 9]];
        (points, pieces)
    }

    fn area(piece: &[Vector2]) -> f32 {
        twice_area(piece).abs() / 2.0
    }

    fn highest(piece: &[Vector2]) -> f32 {
        piece.iter().map(|point| point.y).fold(f32::MIN, f32::max)
    }

    /// Every piece has to be convex, or the whole exercise is pointless.
    fn all_convex(pieces: &[Vec<Vector2>]) {
        for piece in pieces {
            assert!(piece.len() >= 3, "a piece collapsed to {piece:?}");
            for at in 0..piece.len() {
                let here = piece[at];
                let before = piece[(at + piece.len() - 1) % piece.len()];
                let after = piece[(at + 1) % piece.len()];
                let turn = super::cross(here - before, after - here);
                assert!(turn >= -1.0e-4, "a piece is not convex: {piece:?}");
            }
        }
    }

    #[test]
    fn a_table_cuts_into_pieces_that_tile_it() {
        let table = table();
        let pieces = decompose_polygon(&table, 0.0).unwrap();
        assert_eq!(pieces.len(), 3);
        all_convex(&pieces);
        let total: f32 = pieces.iter().map(|piece| area(piece)).sum();
        assert!((total - area(&table)).abs() < 1.0e-3, "{total} covered");
    }

    /// The point of the whole thing: the legs reach into the slab, so there
    /// is no channel between them for a beam to wedge into.
    #[test]
    fn a_leg_reaches_through_the_slab_above_it() {
        let (points, pieces) = slab_and_legs();
        let grown = super::grown(&points, &pieces, 0.9);
        all_convex(&grown);
        assert!((highest(&grown[0]) - 1.18).abs() < 1.0e-3, "{:?}", grown[0]);
        assert!((highest(&grown[1]) - 1.18).abs() < 1.0e-3, "{:?}", grown[1]);
        assert_eq!(grown[2].len(), 4, "the slab took a corner: {:?}", grown[2]);
    }

    /// The slab's edges either side of a seam are collinear with it, so its
    /// strip has no width and it never grows down into a leg.
    #[test]
    fn the_slab_does_not_grow_into_the_legs() {
        let (points, pieces) = slab_and_legs();
        let grown = super::grown(&points, &pieces, 1.0);
        let lowest = grown[2]
            .iter()
            .map(|point| point.y)
            .fold(f32::MAX, f32::min);
        assert!(
            (lowest - 1.0).abs() < 1.0e-4,
            "the slab reached down to {lowest}"
        );
    }

    /// A leg grows through a shelf into the top above it, while the strip
    /// crosses each seam whole.
    #[test]
    fn a_leg_reaches_through_two_pieces() {
        let points = ring(&[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
            [1.0, 3.0],
            [0.0, 3.0],
        ]);
        let pieces = vec![vec![0, 1, 2, 3], vec![3, 2, 4, 5], vec![5, 4, 6, 7]];
        let grown = super::grown(&points, &pieces, 1.0);
        all_convex(&grown);
        let reached = highest(&grown[0]);
        assert!(
            (reached - 3.0).abs() < 1.0e-3,
            "the leg stopped at {reached}"
        );
    }

    /// A grown piece may take from its neighbours, never from outside.
    #[test]
    fn nothing_grows_outside_the_polygon() {
        let table = table();
        for piece in decompose_polygon(&table, 1.0).unwrap() {
            let spilled: f32 =
                balaur_core::geometry2d::overlay(&piece, &table, balaur_core::csg::Op::Difference)
                    .iter()
                    .flatten()
                    .map(|path| area(path))
                    .sum();
            assert!(
                spilled < 1.0e-3,
                "{spilled} of a piece is outside the table"
            );
        }
    }

    /// At zero the pieces are the plain decomposition, for a script that
    /// would rather place the overlap itself.
    #[test]
    fn no_overlap_leaves_the_pieces_alone() {
        let table = table();
        let plain: f32 = decompose_polygon(&table, 0.0)
            .unwrap()
            .iter()
            .map(|piece| area(piece))
            .sum();
        let grown: f32 = decompose_polygon(&table, 0.9)
            .unwrap()
            .iter()
            .map(|piece| area(piece))
            .sum();
        assert!((plain - area(&table)).abs() < 1.0e-3);
        assert!(grown > plain + 0.05, "{grown} is no larger than {plain}");
    }

    /// A ring keeps its hole: the pieces cover the material, not the middle.
    #[test]
    fn a_square_with_a_hole_keeps_the_hole() {
        let outline = vec![[0.0, 0.0], [3.0, 0.0], [3.0, 3.0], [0.0, 3.0]];
        let hole = vec![[1.0, 1.0], [1.0, 2.0], [2.0, 2.0], [2.0, 1.0]];
        let (filled, triangles) = balaur_core::triangulate::triangulate_shape(&[outline, hole]);
        let points: Vec<Vector2> = filled.iter().map(|&[x, y]| Vector2::new(x, y)).collect();
        let pieces = super::decompose(&points, &triangles, 0.0);
        all_convex(&pieces);
        let total: f32 = pieces.iter().map(|piece| area(piece)).sum();
        assert!((total - 8.0).abs() < 1.0e-3, "{total} covered, not 8");
    }

    /// The same points twice give the same pieces, or a replay diverges.
    #[test]
    fn the_pieces_are_the_same_every_run() {
        assert_eq!(
            decompose_polygon(&table(), 0.9).unwrap(),
            decompose_polygon(&table(), 0.9).unwrap()
        );
    }
}
