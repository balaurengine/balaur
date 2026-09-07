//! Two solids joined, cut and intersected. The answers are checked against
//! the volume the arithmetic says they should have, so a tree that keeps a
//! face it should have dropped is caught by the number and not by eye.

use balaur_core::csg::{Op, combine};
use balaur_core::mesh::MeshData;
use balaur_core::primitive::Solid;
use glamx::Vec3;
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
    assert_eq!(open_edges(&mesh), 0, "a union is a closed solid");
}

#[test]
fn a_difference_takes_the_overlap_out_of_the_first() {
    let (a, b) = pair();
    let mesh = combine(&a, &b, Op::Difference);
    close(volume(&mesh), 1.0 - 0.125, "difference");
    assert_eq!(open_edges(&mesh), 0, "a difference is a closed solid");
}

#[test]
fn an_intersection_is_only_the_overlap() {
    let (a, b) = pair();
    let mesh = combine(&a, &b, Op::Intersection);
    close(volume(&mesh), 0.125, "intersection");
    assert_eq!(open_edges(&mesh), 0, "an intersection is a closed solid");
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
