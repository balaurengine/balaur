//! The meshers are pure functions, so what they produce can be checked
//! without a GPU: that a solid is closed, that it faces outwards, that it
//! fills the box it claims, and that it comes out the same every time.

use balaur_core::mesh::MeshData;
use balaur_core::primitive::{Flat, Solid};
use glamx::{Vec2, Vec3};
use std::collections::BTreeMap;

/// Six times the volume the triangles enclose, by the divergence theorem.
/// Positive means every face is wound counter-clockwise seen from outside.
fn signed_volume(mesh: &MeshData) -> f64 {
    let at = |i: u32| {
        let p = mesh.positions[i as usize];
        [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
    };
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

/// How many triangles run along each edge, keyed by the two endpoints'
/// positions rather than their indices: a hard edge duplicates its corners,
/// and a mesh is still closed when the copies sit in the same place.
fn edge_counts(mesh: &MeshData) -> BTreeMap<([u32; 3], [u32; 3]), usize> {
    // Adding zero folds a negative zero onto a positive one, which is the
    // only way two vertices in the same place can differ bit for bit.
    let key = |i: u32| mesh.positions[i as usize].map(|v| (v + 0.0).to_bits());
    let mut counts = BTreeMap::new();
    for &[a, b, c] in &mesh.indices {
        for (from, to) in [(a, b), (b, c), (c, a)] {
            let (from, to) = (key(from), key(to));
            let edge = if from <= to { (from, to) } else { (to, from) };
            *counts.entry(edge).or_insert(0) += 1;
        }
    }
    counts
}

fn bounds(mesh: &MeshData) -> ([f32; 3], [f32; 3]) {
    mesh.bounds().expect("a primitive has vertices to measure")
}

/// Every closed solid, at dimensions that are not all the same, so a mesher
/// that mixes up two axes is caught.
fn closed_solids() -> Vec<(&'static str, Solid)> {
    vec![
        ("ball", Solid::ball(0.7)),
        ("cuboid", Solid::cuboid(0.4, 0.9, 0.6)),
        (
            "rounded cuboid",
            Solid::Cuboid {
                hx: 0.4,
                hy: 0.9,
                hz: 0.6,
                corner_radius: 0.15,
                segments: 16,
            },
        ),
        (
            "capsule",
            Solid::Capsule {
                radius: 0.3,
                height: 1.2,
                segments: 24,
                rings: 12,
            },
        ),
        (
            "cylinder",
            Solid::Cylinder {
                radius: 0.5,
                height: 1.4,
                segments: 24,
            },
        ),
        (
            "cone",
            Solid::Cone {
                radius: 0.5,
                height: 1.4,
                segments: 24,
            },
        ),
        (
            "torus",
            Solid::Torus {
                radius: 0.8,
                tube_radius: 0.25,
                segments: 24,
                rings: 12,
            },
        ),
        (
            "pyramid",
            Solid::Pyramid {
                hx: 0.5,
                hy: 0.8,
                hz: 0.3,
                sides: 4,
            },
        ),
        (
            "prism",
            Solid::Prism {
                radius: 0.6,
                height: 1.1,
                sides: 6,
            },
        ),
        (
            "tube",
            Solid::Tube {
                radius: 0.7,
                inner_radius: 0.4,
                height: 1.0,
                segments: 24,
            },
        ),
    ]
}

#[test]
fn every_solid_is_closed() {
    for (name, solid) in closed_solids() {
        let mesh = solid.build();
        assert!(!mesh.indices.is_empty(), "{name} has no triangles");
        let open: Vec<_> = edge_counts(&mesh)
            .into_iter()
            .filter(|(_, count)| *count != 2)
            .collect();
        assert!(
            open.is_empty(),
            "{name} has {} edges not shared by two triangles",
            open.len()
        );
    }
}

#[test]
fn every_solid_faces_outwards() {
    for (name, solid) in closed_solids() {
        let volume = signed_volume(&solid.build());
        assert!(volume > 0.0, "{name} is inside out: volume {volume}");
    }
}

#[test]
fn a_solid_fills_the_box_it_claims() {
    for (name, solid) in closed_solids() {
        let mesh = solid.build();
        let (min, max) = bounds(&mesh);
        let half = solid.half_extents();
        for axis in 0..3 {
            let reach = max[axis].max(-min[axis]);
            assert!(
                (reach - half[axis]).abs() < 0.05 * half[axis].max(0.1),
                "{name} reaches {reach} on axis {axis}, not the {} it claims",
                half[axis]
            );
        }
    }
}

#[test]
fn a_solid_carries_a_normal_and_a_uv_per_vertex() {
    for (name, solid) in closed_solids() {
        let mesh = solid.build();
        let normals = mesh.normals.as_ref().expect("a mesher writes normals");
        let uvs = mesh.uvs.as_ref().expect("a mesher writes texture coordinates");
        assert_eq!(normals.len(), mesh.positions.len(), "{name} normals");
        assert_eq!(uvs.len(), mesh.positions.len(), "{name} uvs");
        for normal in normals {
            let length = Vec3::from_array(*normal).length();
            assert!((length - 1.0).abs() < 1e-3, "{name} has a normal of {length}");
        }
    }
}

#[test]
fn the_same_parameters_give_the_same_mesh() {
    for (name, solid) in closed_solids() {
        assert_eq!(solid.build(), solid.build(), "{name} is not reproducible");
    }
}

#[test]
fn a_plane_is_a_grid_facing_up() {
    let mesh = Solid::Plane {
        hx: 2.0,
        hz: 3.0,
        segments: 4,
    }
    .build();
    assert_eq!(mesh.positions.len(), 25);
    assert_eq!(mesh.indices.len(), 32);
    let (min, max) = bounds(&mesh);
    assert_eq!(min, [-2.0, 0.0, -3.0]);
    assert_eq!(max, [2.0, 0.0, 3.0]);
    for normal in mesh.normals.as_ref().expect("normals") {
        assert_eq!(*normal, [0.0, 1.0, 0.0]);
    }
}

#[test]
fn a_square_cornered_cuboid_is_twelve_triangles() {
    let mesh = Solid::cuboid(0.5, 0.5, 0.5).build();
    assert_eq!(mesh.positions.len(), 24);
    assert_eq!(mesh.indices.len(), 12);
}

/// Twice the area an outline encloses. Positive is counter-clockwise, which
/// is what the ear clipper and a light's shadow both expect.
fn signed_area(outline: &[Vec2]) -> f32 {
    outline
        .iter()
        .zip(outline.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum()
}

fn flats() -> Vec<(&'static str, Flat)> {
    vec![
        ("circle", Flat::circle(0.6)),
        ("rect", Flat::rect(0.8, 0.4)),
        (
            "rounded rect",
            Flat::Rect {
                hx: 0.8,
                hy: 0.4,
                corner_radius: 0.2,
                segments: 16,
            },
        ),
        (
            "ellipse",
            Flat::Ellipse {
                hx: 0.9,
                hy: 0.3,
                segments: 24,
            },
        ),
        (
            "capsule",
            Flat::Capsule {
                radius: 0.3,
                height: 1.0,
                segments: 16,
            },
        ),
        (
            "star",
            Flat::Star {
                points: 5,
                radius: 0.7,
                inner_radius: 0.3,
            },
        ),
        ("ngon", Flat::Ngon { sides: 6, radius: 0.5 }),
    ]
}

#[test]
fn every_outline_runs_counter_clockwise() {
    for (name, flat) in flats() {
        let outline = flat.outline();
        assert!(outline.len() >= 3, "{name} has no outline");
        assert!(signed_area(&outline) > 0.0, "{name} winds the wrong way");
    }
}

#[test]
fn every_flat_shape_fills() {
    for (name, flat) in flats() {
        let mesh = flat.build();
        assert!(!mesh.indices.is_empty(), "{name} filled to nothing");
        let (min, max) = bounds(&mesh);
        let half = flat.half_extents();
        for axis in 0..2 {
            let reach = max[axis].max(-min[axis]);
            assert!(
                (reach - half[axis]).abs() < 0.05 * half[axis].max(0.1),
                "{name} reaches {reach} on axis {axis}, not {}",
                half[axis]
            );
        }
        assert!(mesh.positions.iter().all(|p| p[2] == 0.0), "{name} is not flat");
    }
}

#[test]
fn a_star_keeps_its_notches() {
    let star = Flat::Star {
        points: 5,
        radius: 1.0,
        inner_radius: 0.4,
    };
    let outline = star.outline();
    assert_eq!(outline.len(), 10);
    let reaches: Vec<f32> = outline.iter().map(|p| p.length()).collect();
    for (i, reach) in reaches.iter().enumerate() {
        let want = if i % 2 == 0 { 1.0 } else { 0.4 };
        assert!((reach - want).abs() < 1e-5, "tip {i} reaches {reach}");
    }
}

#[test]
fn a_shape_round_trips_through_its_properties() {
    for (name, solid) in closed_solids() {
        let params = solid.to_params();
        let back = Solid::from_params(&params).expect("a table a shape wrote is readable");
        assert_eq!(back, solid, "{name} did not survive a round trip");
    }
    for (name, flat) in flats() {
        let params = flat.to_params();
        let back = Flat::from_params(&params).expect("a table a shape wrote is readable");
        assert_eq!(back, flat, "{name} did not survive a round trip");
    }
}
