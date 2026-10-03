//! The `collider2d` kinds, voxel edits and readers that reach rapier2d:
//! `voxelized_mesh`, `fit`, `convex_polygon`, `voxelized_points`, polyline
//! edges, and the `physics2d` readers 3D already had.

use balaur_physics::PhysicsState2d;
use balaur_physics::rapier2d::math::IVector;
use balaur_script::Value;
use std::fmt::Write as _;

use crate::boot::{self, Booted, entry, number};

fn collider<R>(
    b: &Booted,
    path: &str,
    f: impl FnOnce(&balaur_physics::rapier2d::prelude::Collider) -> R,
) -> R {
    let node = b.node(path);
    let state = b.app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    f(&state.world.colliders[state.colliders[&node][0]])
}

/// A unit square with a point halfway along its bottom edge, and the same
/// square moved to (2, 2).
const SQUARES: &str = r#"[[assets]]
id = "square"
type = "mesh"
positions = [[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]

[[assets]]
id = "far"
type = "mesh"
positions = [[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 3.0]]
"#;

fn booted(colliders: &[(&str, &str)]) -> Booted {
    let mut scene = format!("{SQUARES}\n[[nodes]]\nid = \"n_world\"\nname = \"World\"\n");
    for (name, collider) in colliders {
        let _ = write!(
            scene,
            "\n[[nodes]]\nid = \"n_{name}\"\nname = \"{name}\"\nparent = \"n_world\"\ncollider2d = {collider}\n"
        );
    }
    let b = boot::project(&scene, "");
    let errors = boot::errors();
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    b
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "values written in the file and read back unchanged"
)]
fn the_2d_mesh_kinds_build_what_they_name() {
    let b = booted(&[
        (
            "Polygon",
            r##"{ kind = "convex_polygon", mesh = "#square" }"##,
        ),
        (
            "Collinear",
            r##"{ kind = "convex_polygon", mesh = "#square", keep_collinear = true }"##,
        ),
        ("Fit", r##"{ kind = "fit", mesh = "#far", fit = "aabb" }"##),
        (
            "Voxelized",
            r##"{ kind = "voxelized_mesh", mesh = "#square", voxel_size = 0.25 }"##,
        ),
        (
            "Points",
            r##"{ kind = "voxelized_points", mesh = "#square", voxel_size = 0.5 }"##,
        ),
        (
            "RoundHull",
            r##"{ kind = "convex_hull", mesh = "#square", edge_radius = 0.1 }"##,
        ),
        (
            "Parts",
            r##"{ kind = "convex_decomposition", mesh = "#square", method = "voxels", resolution = 8 }"##,
        ),
    ]);
    let corners = |path: &str| {
        collider(&b, path, |c| {
            c.shape().as_convex_polygon().unwrap().points().len()
        })
    };
    assert_eq!(corners("World/Polygon"), 4, "a collinear point was kept");
    assert_eq!(
        corners("World/Collinear"),
        5,
        "keep_collinear dropped the midpoint"
    );
    let (half, at) = collider(&b, "World/Fit", |c| {
        (
            c.shape().as_cuboid().unwrap().half_extents,
            c.position().translation,
        )
    });
    assert_eq!(half.to_array(), [0.5, 0.5]);
    assert_eq!(
        at.to_array(),
        [2.5, 2.5],
        "the fitted box lost the pose it was fitted at"
    );
    for path in ["World/Voxelized", "World/Points"] {
        let filled = collider(&b, path, |c| {
            c.shape()
                .as_voxels()
                .unwrap()
                .voxels()
                .filter(|v| !v.state.is_empty())
                .count()
        });
        assert!(filled > 0, "{path} filled no cell");
    }
    assert!(collider(&b, "World/RoundHull", |c| c
        .shape()
        .as_round_convex_polygon()
        .is_some()));
    assert!(collider(&b, "World/Parts", |c| {
        c.shape()
            .as_compound()
            .is_some_and(|parts| parts.shapes().iter().all(|(_, p)| p.as_voxels().is_some()))
    }));
}

#[test]
fn a_2d_polyline_takes_its_outline_or_every_edge_when_asked() {
    let b = booted(&[
        ("Chain", r##"{ kind = "polyline", mesh = "#far" }"##),
        (
            "Outline",
            r##"{ kind = "polyline", mesh = "#far", edges = "outline" }"##,
        ),
        (
            "Edges",
            r##"{ kind = "polyline", mesh = "#far", edges = "mesh" }"##,
        ),
    ]);
    let segments = |path: &str| {
        collider(&b, path, |c| {
            c.shape().as_polyline().unwrap().num_segments()
        })
    };
    assert_eq!(segments("World/Chain"), 3);
    assert_eq!(
        segments("World/Outline"),
        4,
        "the square's outline is four edges"
    );
    assert_eq!(
        segments("World/Edges"),
        5,
        "two triangles share one more edge"
    );
}

const GRIDS: &str = r##"[[assets]]
id = "row"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0], [1, 0, 0], [2, 0, 0]]

[[assets]]
id = "one"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0]]

[[nodes]]
id = "n_world"
name = "World"
script = { source = "scripts/s.rn" }

[[nodes]]
id = "n_a"
name = "A"
parent = "n_world"
collider2d = { kind = "voxels", voxels = "#row" }

[[nodes]]
id = "n_b"
name = "B"
parent = "n_world"
collider2d = { kind = "voxels", voxels = "#one" }
transform = { position = [3.0, 0.0, 0.0] }

[[nodes]]
id = "n_alone"
name = "Alone"
parent = "n_world"
collider2d = { kind = "voxels", voxels = "#row" }
transform = { position = [0.0, 5.0, 0.0] }

[[nodes]]
id = "n_cropped"
name = "Cropped"
parent = "n_world"
collider2d = { kind = "voxels", voxels = "#row" }
transform = { position = [0.0, 10.0, 0.0] }
"##;

#[test]
fn a_2d_voxel_grid_resizes_crops_and_joins_its_neighbour() {
    let mut b = boot::project(
        GRIDS,
        r#"pub fn init(this) {
    let a = this.node.get_node("A").collider2d;
    a.combine_voxels(this.node.get_node("B"));
    let cropped = this.node.get_node("Cropped").collider2d;
    cropped.crop_voxels(0, 0, 1, 0);
    let alone = this.node.get_node("Alone").collider2d;
    alone.set_voxel_size(0.5, 0.5);
}

pub fn probe(this) {
    this.node.get_node("Alone").collider2d.voxel_size()
}
"#,
    );
    b.tick(1);
    let state_at = |path: &str, x: i32| {
        collider(&b, path, |c| {
            c.shape()
                .as_voxels()
                .unwrap()
                .voxel_state(IVector::new(x, 0))
        })
    };
    assert_ne!(
        state_at("World/A", 2),
        state_at("World/Alone", 2),
        "combining left the end cell unaware of its neighbour"
    );
    assert!(
        state_at("World/Cropped", 2)
            .is_none_or(balaur_physics::rapier2d::prelude::VoxelState::is_empty),
        "crop kept a cell outside the range"
    );
    assert!(state_at("World/Cropped", 0).is_some_and(|s| !s.is_empty()));
    assert_eq!(b.call(b.node("World"), "probe"), Value::Vec2([0.5, 0.5]));
}

#[test]
fn the_2d_readers_answer_what_rapier_works_out() {
    let b = boot::project(
        r##"[[assets]]
id = "one"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0]]

[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ball"
name = "Ball"
parent = "n_world"
script = { source = "scripts/s.rn" }
body2d = { kind = "dynamic", gravity_scale = 0.0, initial_linear_velocity = [6.0, 0.0] }
collider2d = { kind = "circle", radius = 0.5 }

[[nodes]]
id = "n_wall"
name = "Wall"
parent = "n_world"
collider2d = { kind = "rectangle", size = [1.0, 4.0] }
transform = { position = [3.0, 0.0, 0.0] }

[[nodes]]
id = "n_cells"
name = "Cells"
parent = "n_world"
collider2d = { kind = "voxels", voxels = "#one" }
transform = { position = [0.0, 8.0, 0.0] }
"##,
        r#"pub fn probe(this) {
    let c = this.node.collider2d;
    let wall = this.node.parent().get_node("Wall");
    let cells = this.node.parent().get_node("Cells").collider2d;
    let (ax, ay, bx, by) = c.aabb();
    let (sx, sy, tx, ty) = c.swept_aabb();
    let (cx, cy, dx, dy) = c.collision_aabb();
    let (ex, ey, fx, fy) = c.broad_phase_aabb();
    let found = [bx, tx, dx, fx, c.collider_mass(), c.collider_volume(),
        physics2d::closest_points(this.node, wall),
        physics2d::time_of_impact(this.node, wall, #{ velocity_a: [6.0, 0.0] }),
        c.collider_mass_properties(), c.pose_in_body(), c.handles(), cells.collider_mesh(), true];
    c.set_collider(#{ kind: "rectangle", size: [2.0, 2.0] });
    found
}
"#,
    );
    let ball = b.node("World/Ball");
    let Value::List(found) = b.call(ball, "probe") else {
        panic!("the probe answered no list: {:#?}", boot::errors());
    };
    assert_eq!(found[12], Value::Bool(true), "the probe ran");
    let (right, swept, collision, broad) = (
        number(&found[0]),
        number(&found[1]),
        number(&found[2]),
        number(&found[3]),
    );
    let step = 6.0 * f64::from(balaur_core::fixed_dt());
    assert!(
        (swept - (right + step)).abs() < 1e-4,
        "the sweep ends at {swept}, not {}",
        right + step
    );
    assert!(collision >= right && broad >= right);
    let area = std::f64::consts::PI * 0.25;
    assert!((number(&found[4]) - area).abs() < 1e-3, "{:?}", found[4]);
    assert!((number(&found[5]) - area).abs() < 1e-3, "{:?}", found[5]);
    assert_eq!(*entry(&found[6], "a"), Value::Vec2([0.5, 0.0]));
    assert_eq!(*entry(&found[6], "b"), Value::Vec2([2.5, 0.0]));
    assert!(
        (number(entry(&found[7], "distance")) - 2.0 / 6.0).abs() < 1e-3,
        "{:?}",
        found[7]
    );
    assert!((number(entry(&found[8], "mass")) - area).abs() < 1e-3);
    assert_eq!(*entry(&found[9], "position"), Value::Vec2([0.0, 0.0]));
    assert!(
        !matches!(entry(&found[10], "body"), Value::Nil),
        "handles named no body"
    );
    assert!(matches!(entry(&found[11], "points"), Value::List(p) if !p.is_empty()));
    let replaced = collider(&b, "World/Ball", |c| {
        c.shape().as_cuboid().map(|s| s.half_extents.to_array())
    });
    assert_eq!(replaced, Some([1.0, 1.0]), "set_collider left the circle");
}
