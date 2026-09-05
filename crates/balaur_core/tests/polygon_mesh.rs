//! A `mesh` asset authored as a 2D polygon: pairs for positions, an
//! outline that fills itself, hand-drawn polygons over interior points,
//! and bone weights folded to what a skin reads.

use balaur_core::mesh::MeshData;
use balaur_core::triangulate::triangulate;
use balaur_core::{assets, App, AppConfig};
use glamx::Vec2;

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

/// Parse an inline `mesh` definition through the registered asset type,
/// the way a component property would.
fn mesh(text: &str) -> anyhow::Result<MeshData> {
    let app = app();
    let body: toml::Value = toml::from_str(text)?;
    let reference = assets::define_inline(&app.engine, "mesh", body)?.to_string();
    Ok((*assets::load_typed::<MeshData>(&app.engine, &reference)?).clone())
}

/// Twice the signed area of `points` walked in `ring` order.
fn doubled_area(points: &[Vec2], ring: &[u32]) -> f32 {
    let mut sum = 0.0;
    for i in 0..ring.len() {
        let a = points[ring[i] as usize];
        let b = points[ring[(i + 1) % ring.len()] as usize];
        sum += a.x * b.y - b.x * a.y;
    }
    sum
}

/// Every triangle is counter-clockwise and together they cover the loop's
/// area exactly, which is what a triangulation is.
fn assert_covers(points: &[Vec2], ring: &[u32], triangles: &[[u32; 3]]) {
    assert_eq!(triangles.len(), ring.len() - 2);
    let mut total = 0.0;
    for t in triangles {
        let area = doubled_area(points, t);
        assert!(area > 0.0, "{t:?} is not counter-clockwise: {area}");
        total += area;
    }
    let wanted = doubled_area(points, ring).abs();
    assert!(
        (total - wanted).abs() < 1e-4,
        "covered {total}, loop has {wanted}"
    );
}

#[test]
fn a_square_becomes_two_triangles() {
    let points = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    let ring = [0, 1, 2, 3];
    let triangles = triangulate(&points, &ring).unwrap();
    assert_covers(&points, &ring, &triangles);
}

#[test]
fn a_concave_outline_is_filled_without_bridging_the_notch() {
    // An L shape: the notch at (1, 1) must not be covered.
    let points = [
        Vec2::new(0.0, 0.0),
        Vec2::new(2.0, 0.0),
        Vec2::new(2.0, 1.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(1.0, 2.0),
        Vec2::new(0.0, 2.0),
    ];
    let ring = [0, 1, 2, 3, 4, 5];
    let triangles = triangulate(&points, &ring).unwrap();
    assert_covers(&points, &ring, &triangles);
    let outside = Vec2::new(1.5, 1.5);
    for t in &triangles {
        let [a, b, c] = t.map(|i| points[i as usize]);
        let cross =
            |o: Vec2, p: Vec2, q: Vec2| (p.x - o.x) * (q.y - o.y) - (p.y - o.y) * (q.x - o.x);
        let inside =
            cross(a, b, outside) > 0.0 && cross(b, c, outside) > 0.0 && cross(c, a, outside) > 0.0;
        assert!(!inside, "{t:?} covers the notch");
    }
}

#[test]
fn a_clockwise_loop_gives_the_same_cover_as_the_counter_clockwise_one() {
    let points = [
        Vec2::new(0.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(1.0, 0.0),
    ];
    let triangles = triangulate(&points, &[0, 1, 2, 3]).unwrap();
    assert_covers(&points, &[3, 2, 1, 0], &triangles);
}

#[test]
fn a_repeated_closing_vertex_and_a_doubled_corner_are_dropped() {
    let points = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    let triangles = triangulate(&points, &[0, 1, 2, 3, 4, 0]).unwrap();
    assert_eq!(triangles.len(), 2);
}

#[test]
fn a_loop_with_no_area_or_too_few_points_is_refused() {
    let points = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(2.0, 0.0),
    ];
    assert!(triangulate(&points, &[0, 1, 2])
        .unwrap_err()
        .to_string()
        .contains("no area"));
    assert!(triangulate(&points, &[0, 1])
        .unwrap_err()
        .to_string()
        .contains("three"));
    assert!(triangulate(&points, &[0, 1, 9])
        .unwrap_err()
        .to_string()
        .contains("vertex 9"));
}

#[test]
fn a_self_crossing_loop_still_yields_a_triangulation() {
    // Two edges cross, so it is not simple, but it has area and the walk
    // must still finish with n - 2 triangles.
    let points = [
        Vec2::new(0.0, 0.0),
        Vec2::new(3.0, 0.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, 2.0),
    ];
    let triangles = triangulate(&points, &[0, 1, 2, 3]).unwrap();
    assert_eq!(triangles.len(), 2);
}

#[test]
fn pairs_are_2d_positions_and_the_outline_fills_itself() {
    let mesh = mesh("positions = [[0, 0], [1, 0], [1, 1], [0, 1]]").unwrap();
    assert_eq!(bits(mesh.positions[2]), bits([1.0, 1.0, 0.0]));
    assert_eq!(mesh.indices.len(), 2);
    assert!(mesh.skin.is_none());
}

#[test]
fn internal_vertices_stay_out_of_the_outline_until_a_polygon_names_them() {
    let outline_only =
        mesh("positions = [[0, 0], [1, 0], [1, 1], [0, 1], [0.5, 0.5]]\ninternal = 1").unwrap();
    assert_eq!(outline_only.indices.len(), 2);
    assert!(outline_only.indices.iter().all(|t| !t.contains(&4)));
    let drawn = mesh(
        "positions = [[0, 0], [1, 0], [1, 1], [0, 1], [0.5, 0.5]]\ninternal = 1\n\
         polygons = [[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]",
    )
    .unwrap();
    assert_eq!(drawn.indices.len(), 4);
    assert!(drawn.indices.iter().all(|t| t.contains(&4)));
}

#[test]
fn a_polygon_loop_of_five_is_ear_clipped_into_three() {
    let drawn = mesh(
        "positions = [[0, 0], [2, 0], [2, 1], [1, 1], [1, 2], [0, 2]]\n\
         polygons = [[0, 1, 2, 3], [0, 3, 4, 5]]",
    )
    .unwrap();
    assert_eq!(drawn.indices.len(), 4);
}

#[test]
fn indices_and_polygons_together_are_a_contradiction() {
    let err = mesh_err(
        "positions = [[0, 0], [1, 0], [0, 1]]\nindices = [[0, 1, 2]]\npolygons = [[0, 1, 2]]",
    );
    assert!(err.contains("one or the other"), "{err}");
}

#[test]
fn uvs_ride_along_and_must_match_the_vertex_count() {
    let mesh =
        mesh("positions = [[0, 0], [1, 0], [0, 1]]\nuvs = [[0, 1], [1, 1], [0, 0]]").unwrap();
    assert_eq!(bits(mesh.uvs.unwrap()[1]), bits([1.0, 1.0]));
    let err = mesh_err("positions = [[0, 0], [1, 0], [0, 1]]\nuvs = [[0, 1]]");
    assert!(err.contains("one per vertex"), "{err}");
}

/// The whole error chain: the parser's reason sits under the asset layer's
/// "parsing asset" context.
fn mesh_err(text: &str) -> String {
    format!("{:#}", mesh(text).unwrap_err())
}

fn bits<const N: usize>(a: [f32; N]) -> [u32; N] {
    a.map(f32::to_bits)
}

const SKINNED: &str = r#"
positions = [[-0.5, 1], [0.5, 0], [-0.5, -1]]

[[skin.bones]]
path = "Hip"
weights = [1.0, 0.5, 0.0]

[[skin.bones]]
path = "Hip/Thigh"
weights = [0.0, 0.5, 1.0]
"#;

#[test]
fn bone_weights_fold_to_per_vertex_joints_that_sum_to_one() {
    let mesh = mesh(SKINNED).unwrap();
    let skin = mesh.skin.unwrap();
    assert_eq!(skin.bones, vec!["Hip".to_string(), "Hip/Thigh".to_string()]);
    assert_eq!(skin.joints[0], [0, 0, 0, 0]);
    assert_eq!(bits(skin.weights[0]), bits([1.0, 0.0, 0.0, 0.0]));
    // Equal weights: the earlier bone takes the first slot.
    assert_eq!(skin.joints[1], [0, 1, 0, 0]);
    assert_eq!(bits(skin.weights[1]), bits([0.5, 0.5, 0.0, 0.0]));
    assert_eq!(skin.joints[2], [1, 0, 0, 0]);
    assert_eq!(bits(skin.weights[2]), bits([1.0, 0.0, 0.0, 0.0]));
}

#[test]
fn more_than_four_influences_keep_the_heaviest_four_renormalised() {
    let mut text = String::from("positions = [[0, 0], [1, 0], [0, 1]]\n");
    for (i, weight) in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6].iter().enumerate() {
        use std::fmt::Write as _;
        writeln!(
            text,
            "[[skin.bones]]\npath = \"B{i}\"\nweights = [{weight}, 0, 0]"
        )
        .unwrap();
    }
    let skin = mesh(&text).unwrap().skin.unwrap();
    assert_eq!(skin.joints[0], [5, 4, 3, 2]);
    let total: f32 = skin.weights[0].iter().sum();
    assert!((total - 1.0).abs() < 1e-6);
    assert!(skin.weights[0][0] > skin.weights[0][3]);
    // Vertices no bone claims are left with zero weights.
    assert_eq!(bits(skin.weights[1]), bits([0.0; 4]));
}

#[test]
fn a_skin_whose_weights_do_not_match_the_vertices_is_refused() {
    let err = mesh_err(
        "positions = [[0, 0], [1, 0], [0, 1]]\n[[skin.bones]]\npath = \"Hip\"\nweights = [1, 1]",
    );
    assert!(err.contains("2 weights for 3 vertices"), "{err}");
    let err = mesh_err("positions = [[0, 0], [1, 0], [0, 1]]\n[[skin.bones]]\npath = \"Hip\"\nweights = [1, -1, 0]");
    assert!(err.contains("0 or more"), "{err}");
    let err = mesh_err("positions = [[0, 0], [1, 0], [0, 1]]\n[[skin.bones]]\nweights = [1, 0, 0]");
    assert!(err.contains("`path`"), "{err}");
}
