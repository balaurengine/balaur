//! Paths and what they become: sampling that follows the curve, an outline
//! given thickness, a profile revolved, a cross-section swept along a rail.

use balaur_core::mesh::MeshData;
use balaur_core::path::{Path2d, Path3d, TOLERANCE, extrude, lathe, sweep};
use glamx::{Vec2, Vec3};
use std::collections::BTreeMap;

/// Six times the volume the triangles enclose. Positive means outward.
fn signed_volume(mesh: &MeshData) -> f64 {
    let at = |i: u32| mesh.positions[i as usize].map(f64::from);
    mesh.indices
        .iter()
        .map(|&[a, b, c]| {
            let (a, b, c) = (at(a), at(b), at(c));
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]
        })
        .sum()
}

fn open_edges(mesh: &MeshData) -> usize {
    let key = |i: u32| mesh.positions[i as usize].map(|v| (v + 0.0).to_bits());
    let mut counts: BTreeMap<_, usize> = BTreeMap::new();
    for &[a, b, c] in &mesh.indices {
        for (from, to) in [(a, b), (b, c), (c, a)] {
            let (from, to) = (key(from), key(to));
            let edge = if from <= to { (from, to) } else { (to, from) };
            *counts.entry(edge).or_default() += 1;
        }
    }
    counts.values().filter(|count| **count != 2).count()
}

/// A quarter circle of radius one, as one cubic. The handle length is the
/// usual 0.5523 that puts the curve within a thousandth of the arc.
fn quarter() -> Path2d {
    let k = 0.552_284_8;
    Path2d {
        points: vec![
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, k),
            Vec2::new(k, 1.0),
            Vec2::new(0.0, 1.0),
        ],
        closed: false,
    }
}

/// A unit square as four straight cubics, closed.
fn square() -> Path2d {
    let corners = [
        Vec2::new(-1.0, -1.0),
        Vec2::new(1.0, -1.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(-1.0, 1.0),
    ];
    let mut points = Vec::new();
    for i in 0..4 {
        let (from, to) = (corners[i], corners[(i + 1) % 4]);
        points.push(from);
        points.push(from.lerp(to, 1.0 / 3.0));
        points.push(from.lerp(to, 2.0 / 3.0));
    }
    Path2d {
        points,
        closed: true,
    }
}

#[test]
fn a_sampled_curve_stays_within_the_tolerance() {
    let points = quarter().sample(TOLERANCE).expect("one cubic samples");
    assert!(points.len() > 4, "a curve is more than its ends");
    for p in &points {
        let off = (p.length() - 1.0).abs();
        assert!(off < 0.01, "a point sat {off} off the arc");
    }
    assert_eq!(points[0], Vec2::new(1.0, 0.0));
    assert_eq!(*points.last().expect("an end"), Vec2::new(0.0, 1.0));
}

/// A straight run needs no subdivision, and a curve needs more the tighter
/// the tolerance: flattening follows the shape rather than a fixed count.
#[test]
fn flattening_spends_points_where_the_curve_bends() {
    let straight = Path2d {
        points: vec![
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(3.0, 0.0),
        ],
        closed: false,
    };
    assert_eq!(straight.sample(TOLERANCE).expect("straight").len(), 2);
    let coarse = quarter().sample(0.05).expect("coarse").len();
    let fine = quarter().sample(0.0001).expect("fine").len();
    assert!(fine > coarse, "{fine} against {coarse}");
}

#[test]
fn a_path_with_a_broken_segment_count_is_refused() {
    let broken = Path2d {
        points: vec![Vec2::ZERO, Vec2::X, Vec2::Y],
        closed: false,
    };
    assert!(broken.sample(TOLERANCE).is_err());
    let two_points = Path2d {
        points: vec![Vec2::ZERO, Vec2::X],
        closed: false,
    };
    assert_eq!(two_points.sample(TOLERANCE).expect("a line").len(), 2);
}

#[test]
fn an_extruded_outline_is_a_closed_solid() {
    let mesh = extrude(&square(), 1.0, 0.0, 1).expect("a square extrudes");
    assert_eq!(open_edges(&mesh), 0, "an extrusion is closed");
    assert!(signed_volume(&mesh) > 0.0, "an extrusion faces outwards");
    let (min, max) = mesh.bounds().expect("bounds");
    assert!((max[0] - 1.0).abs() < 1e-5 && (min[0] + 1.0).abs() < 1e-5);
    assert!(
        (max[2] - 0.5).abs() < 1e-5,
        "depth one is half a unit each way"
    );
}

/// A bevel takes material off the edges, so the same outline at the same
/// depth encloses less once it is rounded.
#[test]
fn a_bevel_rounds_the_edges_off() {
    let square_solid = extrude(&square(), 1.0, 0.0, 1).expect("square");
    let rounded = extrude(&square(), 1.0, 0.2, 4).expect("rounded");
    assert_eq!(
        open_edges(&rounded),
        0,
        "a bevelled extrusion is still closed"
    );
    assert!(
        signed_volume(&rounded) < signed_volume(&square_solid),
        "a bevel should have removed material"
    );
    assert!(rounded.indices.len() > square_solid.indices.len());
}

/// A half circle revolved is a sphere: the profile decides the solid, and
/// the lathe only spins it.
#[test]
fn a_revolved_profile_makes_a_solid_of_revolution() {
    let profile = Path2d {
        points: vec![
            Vec2::new(0.0, -1.0),
            Vec2::new(1.2, -1.0),
            Vec2::new(1.2, 1.0),
            Vec2::new(0.0, 1.0),
        ],
        closed: false,
    };
    let mesh = lathe(&profile, 24).expect("a profile revolves");
    assert!(!mesh.indices.is_empty());
    let (min, max) = mesh.bounds().expect("bounds");
    assert!((max[1] - 1.0).abs() < 1e-4, "it reaches the profile's top");
    assert!((min[1] + 1.0).abs() < 1e-4);
    assert!(max[0] > 0.5, "and stands off the axis");
    assert!(signed_volume(&mesh) > 0.0, "wound outwards");
}

#[test]
fn a_swept_profile_follows_its_rail() {
    let rail = Path3d {
        points: vec![
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(0.0, 2.0, 4.0),
            Vec3::new(0.0, 2.0, 6.0),
        ],
        closed: false,
    };
    let mesh = sweep(&rail, &square(), true).expect("a square sweeps");
    assert!(!mesh.indices.is_empty());
    let (min, max) = mesh.bounds().expect("bounds");
    assert!(
        min[2] < 0.5 && max[2] > 5.0,
        "it runs the length of the rail"
    );
    assert!(max[1] > 1.5, "and climbs with it");
}

#[test]
fn a_rail_too_short_to_sweep_says_so() {
    let stub = Path3d {
        points: vec![Vec3::ZERO],
        closed: false,
    };
    assert!(sweep(&stub, &square(), true).is_err());
}

#[test]
fn the_same_path_gives_the_same_mesh() {
    assert_eq!(
        extrude(&square(), 1.0, 0.1, 3).expect("a"),
        extrude(&square(), 1.0, 0.1, 3).expect("b")
    );
}
