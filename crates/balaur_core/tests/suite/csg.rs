//! Two solids joined, cut and intersected. The answers are checked against
//! the volume the arithmetic says they should have, so a tree that keeps a
//! face it should have dropped is caught by the number and not by eye.

use balaur_core::csg::{Op, combine};
use balaur_core::mesh::MeshData;
use balaur_core::primitive::Solid;
use glamx::{Quat, Vec3};
use std::collections::BTreeMap;

/// The volume the triangles enclose, by the divergence theorem.
fn volume(mesh: &MeshData) -> f64 {
    let at = |i: u32| mesh.positions[i as usize].map(f64::from);
    let six: f64 = mesh
        .indices
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
        .sum();
    six / 6.0
}

/// Edges not run exactly once each way: none on a closed solid whose
/// triangles all wind the same way round.
fn unsealed_edges(mesh: &MeshData) -> usize {
    let key = |i: u32| mesh.positions[i as usize].map(|v| (v + 0.0).to_bits());
    let mut runs: BTreeMap<_, (usize, usize)> = BTreeMap::new();
    for &[a, b, c] in &mesh.indices {
        for (from, to) in [(a, b), (b, c), (c, a)] {
            let (from, to) = (key(from), key(to));
            if from <= to {
                runs.entry((from, to)).or_default().0 += 1;
            } else {
                runs.entry((to, from)).or_default().1 += 1;
            }
        }
    }
    runs.values().filter(|run| **run != (1, 1)).count()
}

fn moved(mut mesh: MeshData, rotation: Quat, by: Vec3) -> MeshData {
    for p in &mut mesh.positions {
        *p = (rotation * Vec3::from_array(*p) + by).to_array();
    }
    if let Some(normals) = &mut mesh.normals {
        for n in normals {
            *n = (rotation * Vec3::from_array(*n)).to_array();
        }
    }
    mesh
}

/// All three operations on `a` and `b` come out closed, and their volumes
/// add up the way sets do. Returns the volume they overlap in.
fn closed_and_consistent(a: &MeshData, b: &MeshData, what: &str) -> f64 {
    let [union, difference, intersection] =
        [Op::Union, Op::Difference, Op::Intersection].map(|op| {
            let mesh = combine(a, b, op);
            assert_eq!(unsealed_edges(&mesh), 0, "{what}: {op:?} is a closed solid");
            volume(&mesh)
        });
    let (a, b) = (volume(a), volume(b));
    close(
        union + intersection,
        a + b,
        &format!("{what}: union and overlap"),
    );
    close(
        difference + intersection,
        a,
        &format!("{what}: difference and overlap"),
    );
    intersection
}

/// How much of a ball lies inside the box `[-half, half]` on every axis,
/// summed in columns along z on a fine grid.
fn ball_inside_box(centre: Vec3, radius: f32, half: f32) -> f64 {
    let (c, r, h) = (centre.as_dvec3(), f64::from(radius), f64::from(half));
    let steps = 400;
    let cell = 2.0 * r / f64::from(steps);
    let mut total = 0.0;
    for i in 0..steps {
        for j in 0..steps {
            let x = c.x - r + (f64::from(i) + 0.5) * cell;
            let y = c.y - r + (f64::from(j) + 0.5) * cell;
            let rim = r * r - (x - c.x).powi(2) - (y - c.y).powi(2);
            if rim <= 0.0 || x.abs() > h || y.abs() > h {
                continue;
            }
            let reach = rim.sqrt();
            let (low, high) = ((c.z - reach).max(-h), (c.z + reach).min(h));
            total += (high - low).max(0.0) * cell * cell;
        }
    }
    total
}

/// A cube of side `size` with its corner at `at`.
fn cube(at: Vec3, size: f32) -> MeshData {
    let half = size / 2.0;
    let mut mesh = Solid::cuboid(half, half, half).build();
    let centre = at + Vec3::splat(half);
    for p in &mut mesh.positions {
        *p = (Vec3::from_array(*p) + centre).to_array();
    }
    mesh
}

/// Two unit cubes overlapping in an eighth of their volume, which makes
/// every answer a round number: 1 + 1 - 1/8, 1 - 1/8, and 1/8.
fn pair() -> (MeshData, MeshData) {
    (cube(Vec3::ZERO, 1.0), cube(Vec3::new(0.5, 0.5, 0.5), 1.0))
}

fn close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() < 0.02,
        "{what}: got {got}, expected {want}"
    );
}

#[test]
fn a_union_is_both_solids_less_the_overlap() {
    let (a, b) = pair();
    let mesh = combine(&a, &b, Op::Union);
    close(volume(&mesh), 2.0 - 0.125, "union");
    assert_eq!(unsealed_edges(&mesh), 0, "a union is a closed solid");
}

#[test]
fn a_difference_takes_the_overlap_out_of_the_first() {
    let (a, b) = pair();
    let mesh = combine(&a, &b, Op::Difference);
    close(volume(&mesh), 1.0 - 0.125, "difference");
    assert_eq!(unsealed_edges(&mesh), 0, "a difference is a closed solid");
}

#[test]
fn an_intersection_is_only_the_overlap() {
    let (a, b) = pair();
    let mesh = combine(&a, &b, Op::Intersection);
    close(volume(&mesh), 0.125, "intersection");
    assert_eq!(
        unsealed_edges(&mesh),
        0,
        "an intersection is a closed solid"
    );
}

/// Order matters for a difference and for nothing else.
#[test]
fn a_difference_is_not_the_other_way_round() {
    let (a, b) = pair();
    let forward = volume(&combine(&a, &b, Op::Difference));
    let backward = volume(&combine(&b, &a, Op::Difference));
    close(forward, backward, "two equal cubes cut each other equally");
    let big = cube(Vec3::ZERO, 2.0);
    let small = cube(Vec3::new(0.5, 0.5, 0.5), 1.0);
    close(
        volume(&combine(&big, &small, Op::Difference)),
        7.0,
        "big minus small",
    );
    close(
        volume(&combine(&small, &big, Op::Difference)),
        0.0,
        "small minus the big one that swallows it",
    );
}

/// Solids that do not touch: a union is both, an intersection is nothing.
#[test]
fn solids_that_miss_each_other_answer_plainly() {
    let a = cube(Vec3::ZERO, 1.0);
    let b = cube(Vec3::new(5.0, 0.0, 0.0), 1.0);
    close(
        volume(&combine(&a, &b, Op::Union)),
        2.0,
        "two separate cubes",
    );
    close(
        volume(&combine(&a, &b, Op::Difference)),
        1.0,
        "nothing taken",
    );
    assert!(
        combine(&a, &b, Op::Intersection).indices.is_empty(),
        "nothing in common"
    );
}

/// A cut across a curved solid keeps the smooth normals the sphere carried
/// rather than dropping to flat ones.
#[test]
fn a_cut_keeps_the_shading_of_what_it_cut() {
    let ball = Solid::ball(1.0).build();
    let knife = cube(Vec3::new(0.0, 0.0, 0.0), 4.0);
    let mesh = combine(&ball, &knife, Op::Difference);
    let normals = mesh.normals.as_ref().expect("normals survive a cut");
    assert_eq!(normals.len(), mesh.positions.len());
    let rounded = normals
        .iter()
        .zip(&mesh.positions)
        .filter(|(n, p)| {
            let (n, p) = (Vec3::from_array(**n), Vec3::from_array(**p));
            p.length() > 0.9 && n.dot(p.normalize_or_zero()) > 0.99
        })
        .count();
    assert!(
        rounded > 20,
        "only {rounded} vertices kept the ball's normal"
    );
}

#[test]
fn the_same_two_solids_give_the_same_result() {
    let (a, b) = pair();
    assert_eq!(
        combine(&a, &b, Op::Union),
        combine(&a, &b, Op::Union),
        "a boolean is reproducible"
    );
}

#[test]
fn every_operation_has_a_word_and_answers_to_it() {
    for op in [Op::Union, Op::Difference, Op::Intersection] {
        assert_eq!(Op::from_word(op.word()), Some(op));
    }
    assert_eq!(Op::from_word("smoosh"), None);
}

/// The `boolean3d` a scene draws: a 1.2 box with a 0.6 ball cut out of the
/// corner it sits over.
fn box_and_corner_ball() -> (MeshData, MeshData, Vec3) {
    let centre = Vec3::splat(0.4);
    let ball = moved(Solid::ball(0.6).build(), Quat::IDENTITY, centre);
    (Solid::cuboid(0.6, 0.6, 0.6).build(), ball, centre)
}

#[test]
fn a_ball_cut_out_of_a_box_corner_leaves_the_rest_of_the_box_closed() {
    let (boxed, ball, centre) = box_and_corner_ball();
    let mesh = combine(&boxed, &ball, Op::Difference);
    assert_eq!(unsealed_edges(&mesh), 0, "the cut box is a closed solid");
    close(
        volume(&mesh),
        volume(&boxed) - ball_inside_box(centre, 0.6, 0.6),
        "box less the corner the ball covers",
    );
}

#[test]
fn a_box_and_a_ball_over_its_corner_join_and_overlap_as_closed_solids() {
    let (boxed, ball, centre) = box_and_corner_ball();
    let inside = ball_inside_box(centre, 0.6, 0.6);
    let overlap = closed_and_consistent(&boxed, &ball, "box and ball");
    close(overlap, inside, "the corner the ball covers");
    let union = volume(&combine(&boxed, &ball, Op::Union));
    close(
        union,
        volume(&boxed) + volume(&ball) - inside,
        "box and ball",
    );
}

#[test]
fn boxes_offset_on_every_axis_combine_into_closed_solids() {
    let a = Solid::cuboid(0.5, 0.5, 0.5).build();
    let b = moved(
        Solid::cuboid(0.4, 0.3, 0.6).build(),
        Quat::IDENTITY,
        Vec3::new(0.3, 0.45, -0.2),
    );
    // Overlap on each axis: [-0.1, 0.5], [0.15, 0.5], [-0.5, 0.4].
    let overlap = closed_and_consistent(&a, &b, "offset boxes");
    close(overlap, 0.6 * 0.35 * 0.9, "offset boxes overlap");
    let turned = moved(
        Solid::cuboid(0.4, 0.3, 0.6).build(),
        Quat::from_rotation_y(0.5) * Quat::from_rotation_x(0.35),
        Vec3::new(0.3, 0.45, -0.2),
    );
    closed_and_consistent(&a, &turned, "turned boxes");
}

#[test]
fn a_cylinder_and_a_box_combine_into_closed_solids() {
    let cylinder = Solid::Cylinder {
        radius: 0.5,
        height: 1.0,
        segments: 32,
    }
    .build();
    // The box holds the cylinder's +x half exactly: x = 0 runs through two
    // of its 32 sides' corners.
    let half = moved(
        Solid::cuboid(0.5, 0.75, 0.75).build(),
        Quat::IDENTITY,
        Vec3::new(0.5, 0.0, 0.0),
    );
    let overlap = closed_and_consistent(&cylinder, &half, "cylinder and half box");
    close(overlap, volume(&cylinder) / 2.0, "half the cylinder");
    let tilted = moved(
        cylinder,
        Quat::from_rotation_z(0.6),
        Vec3::new(0.2, 0.3, 0.1),
    );
    closed_and_consistent(&tilted, &half, "tilted cylinder and box");
}

#[test]
fn two_balls_overlap_in_a_closed_lens() {
    let a = Solid::ball(0.6).build();
    let b = moved(
        Solid::ball(0.6).build(),
        Quat::IDENTITY,
        Vec3::new(0.5, 0.0, 0.0),
    );
    let overlap = closed_and_consistent(&a, &b, "two balls");
    // A lens of two equal balls `d` apart: pi (4r + d) (2r - d)^2 / 12.
    let (r, d) = (0.6, 0.5);
    let lens = std::f64::consts::PI * (4.0 * r + d) * (2.0 * r - d).powi(2) / 12.0;
    close(overlap, lens, "the lens two balls share");
}
