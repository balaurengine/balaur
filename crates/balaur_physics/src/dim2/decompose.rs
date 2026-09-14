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
            let chains: Vec<Vec<Vector2>> = (0..ring.len())
                .map(|at| growth(points, pieces, &seams, (which, at), overlap))
                .collect();
            kept(points, ring, &chains)
        })
        .collect()
}

/// A piece grows through as many of its seams as leave it convex, deepest
/// first. Two seams facing different ways can each be sound alone and bulge
/// past the polygon together, so each is kept only while the ring holds.
fn kept(points: &[Vector2], ring: &[u32], chains: &[Vec<Vector2>]) -> Vec<Vector2> {
    let mut order: Vec<usize> = (0..chains.len())
        .filter(|&at| !chains[at].is_empty())
        .collect();
    order.sort_by(|&one, &other| {
        gained(points, ring, chains, other)
            .total_cmp(&gained(points, ring, chains, one))
            .then(one.cmp(&other))
    });
    let mut keep = vec![false; chains.len()];
    for at in order {
        keep[at] = true;
        if !convex_ring(&assembled(points, ring, chains, &keep)) {
            keep[at] = false;
        }
    }
    let out = assembled(points, ring, chains, &keep);
    // The hull only drops the points an edge runs straight through, and is
    // the shape itself once the ring is convex.
    if convex_ring(&out) {
        hull_of(&out)
    } else {
        out
    }
}

/// The ring with the seams `keep` names replaced by what grew through them.
fn assembled(
    points: &[Vector2],
    ring: &[u32],
    chains: &[Vec<Vector2>],
    keep: &[bool],
) -> Vec<Vector2> {
    let mut out: Vec<Vector2> = Vec::with_capacity(ring.len());
    for at in 0..ring.len() {
        out.push(points[ring[at] as usize]);
        if keep[at] {
            out.extend(chains[at].iter().copied());
        }
    }
    out
}

/// Twice the area one seam's growth adds, for taking the deepest first.
fn gained(points: &[Vector2], ring: &[u32], chains: &[Vec<Vector2>], at: usize) -> f32 {
    let mut patch = vec![points[ring[at] as usize]];
    patch.extend(chains[at].iter().copied());
    patch.push(points[step(ring, at, 1) as usize]);
    twice_area(&patch).abs()
}

/// Whether every corner turns the same way, measured as the sine of the turn
/// so the answer does not move with the shape's size.
fn convex_ring(ring: &[Vector2]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    (0..ring.len()).all(|at| {
        let here = ring[at];
        let one = here - ring[(at + ring.len() - 1) % ring.len()];
        let other = ring[(at + 1) % ring.len()] - here;
        let scale = one.length() * other.length();
        scale <= EPSILON || cross(one, other) / scale >= -1.0e-4
    })
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

#[cfg(test)]
mod stress {
    use super::{decompose_polygon, pieces as cut_pieces};
    use crate::scalar::Vector2;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as f32) / ((1u64 << 31) as f32)
        }
    }

    fn star(seed: u64, corners: usize, low: f32, high: f32) -> Vec<Vector2> {
        let mut rng = Rng(seed);
        (0..corners)
            .map(|at| {
                let angle = std::f32::consts::TAU * (at as f32) / (corners as f32);
                let radius = low + rng.next() * (high - low);
                Vector2::new(libm::cosf(angle) * radius, libm::sinf(angle) * radius)
            })
            .collect()
    }

    fn comb(teeth: usize) -> Vec<Vector2> {
        let width = teeth as f32 * 2.0;
        let mut out = vec![Vector2::new(0.0, 0.0)];
        for tooth in 0..teeth {
            let x = tooth as f32 * 2.0;
            out.push(Vector2::new(x + 0.6, 0.0));
            out.push(Vector2::new(x + 0.6, -2.0));
            out.push(Vector2::new(x + 1.4, -2.0));
            out.push(Vector2::new(x + 1.4, 0.0));
        }
        out.push(Vector2::new(width, 0.0));
        out.push(Vector2::new(width, 1.0));
        out.push(Vector2::new(0.0, 1.0));
        out
    }

    fn zigzag(folds: usize) -> Vec<Vector2> {
        let mut top = Vec::new();
        let mut bottom = Vec::new();
        for at in 0..=folds {
            let x = at as f32;
            let y = if at % 2 == 0 { 0.0 } else { 1.5 };
            top.push(Vector2::new(x, y + 0.4));
            bottom.push(Vector2::new(x, y));
        }
        bottom.reverse();
        top.extend(bottom);
        top
    }

    fn inside(ring: &[Vector2], point: Vector2) -> bool {
        let mut hit = false;
        let mut j = ring.len() - 1;
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[j]);
            if (a.y > point.y) != (b.y > point.y) {
                let x = (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x;
                if point.x < x {
                    hit = !hit;
                }
            }
            j = i;
        }
        hit
    }

    fn near_edge(ring: &[Vector2], point: Vector2, skip: f32) -> bool {
        (0..ring.len()).any(|at| {
            let a = ring[at];
            let edge = ring[(at + 1) % ring.len()] - a;
            let len = edge.length_squared();
            let t = if len <= 0.0 {
                0.0
            } else {
                ((point - a).dot(edge) / len).clamp(0.0, 1.0)
            };
            (point - (a + edge * t)).length() < skip
        })
    }

    fn convex(piece: &[Vector2]) -> bool {
        (0..piece.len()).all(|at| {
            let here = piece[at];
            let one = here - piece[(at + piece.len() - 1) % piece.len()];
            let other = piece[(at + 1) % piece.len()] - here;
            one.x * other.y - one.y * other.x >= -1.0e-3
        })
    }

    /// Every check one shape gets: convex pieces, and a collider that covers
    /// the polygon and nothing else.
    fn probe(name: &str, polygon: &[Vector2], overlap: f32) -> usize {
        let grown = decompose_polygon(polygon, overlap).unwrap();
        let ring: Vec<u32> = (0..polygon.len() as u32).collect();
        let plain = cut_pieces(
            polygon,
            &balaur_core::triangulate::triangulate(polygon, &ring).unwrap(),
        );
        let bent = grown.iter().filter(|piece| !convex(piece)).count();

        let mut low = Vector2::new(f32::MAX, f32::MAX);
        let mut high = Vector2::new(f32::MIN, f32::MIN);
        for point in polygon {
            low = low.min(*point);
            high = high.max(*point);
        }
        let span = high - low;
        let (steps, skip) = (80usize, span.max_element() * 0.01);
        let (mut tested, mut missing, mut extra, mut stacked) = (0, 0, 0, 0);
        for ix in 0..steps {
            for iy in 0..steps {
                let point = Vector2::new(
                    low.x + span.x * (ix as f32 + 0.5) / steps as f32,
                    low.y + span.y * (iy as f32 + 0.5) / steps as f32,
                );
                if near_edge(polygon, point, skip) {
                    continue;
                }
                tested += 1;
                let want = inside(polygon, point);
                let held = grown.iter().filter(|piece| inside(piece, point)).count();
                if want && held == 0 {
                    missing += 1;
                }
                if !want && held > 0 {
                    extra += 1;
                }
                stacked = stacked.max(held);
            }
        }
        assert_eq!(
            (bent, missing, extra),
            (0, 0, 0),
            "{name}: {} points, {} pieces, {bent} bent, {missing} of {tested} samples uncovered, \
             {extra} covered outside the polygon, worst stack {stacked}",
            polygon.len(),
            plain.len(),
        );
        stacked
    }

    /// The invariants on shapes nobody drew by hand: every piece convex, the
    /// pieces covering the polygon, and none of them covering anything else.
    #[test]
    fn arbitrary_shapes_hold_up() {
        let mut bad = 0;
        for seed in 0..10u64 {
            let corners = 6 + (seed as usize % 5) * 6;
            bad += probe(
                &format!("star {seed}/{corners}"),
                &star(seed * 7919 + 13, corners, 0.35, 1.0),
                0.9,
            );
        }
        for teeth in [2usize, 4, 8, 16] {
            bad += probe(&format!("comb {teeth}"), &comb(teeth), 0.9);
        }
        for folds in [3usize, 8, 20] {
            bad += probe(&format!("zigzag {folds}"), &zigzag(folds), 0.9);
        }
        for overlap in [0.0f32, 0.25, 0.5, 1.0] {
            bad += probe(
                &format!("star @ {overlap}"),
                &star(13, 18, 0.35, 1.0),
                overlap,
            );
        }
        for corners in [60usize, 120, 240] {
            bad += probe(
                &format!("star {corners}"),
                &star(99, corners, 0.5, 1.0),
                0.9,
            );
        }
        assert!(bad > 0, "nothing was measured");
    }
}
