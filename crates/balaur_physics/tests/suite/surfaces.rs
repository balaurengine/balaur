//! What the contact hook does with a collider's side-table row: a one-way
//! platform's exact axis and angle, a conveyor's surface velocity, and the
//! same rows on a tile map's cells.

use crate::boot::{self, Booted};

fn booted(scene: &str) -> Booted {
    let b = boot::project(scene, "");
    let errors = boot::errors();
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    b
}

/// A platform tilted 16.7 degrees off up and a ball dropped on it: the
/// default 0.1 radian window lets the ball through, a 0.5 radian one holds it.
#[test]
fn a_one_way_platform_holds_its_exact_axis_and_angle() {
    let mut b = booted(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_narrow"
name = "Narrow"
parent = "n_world"
collider2d = { kind = "rectangle", size = [2.0, 0.2], one_way = true, one_way_axis = [0.3, 1.0] }

[[nodes]]
id = "n_wide"
name = "Wide"
parent = "n_world"
collider2d = { kind = "rectangle", size = [2.0, 0.2], one_way = true, one_way_axis = [0.3, 1.0], one_way_angle = 0.5 }
transform = { position = [10.0, 0.0, 0.0] }

[[nodes]]
id = "n_through"
name = "Through"
parent = "n_world"
body2d = { kind = "dynamic" }
collider2d = { kind = "circle", radius = 0.25 }
transform = { position = [0.0, 1.0, 0.0] }

[[nodes]]
id = "n_held"
name = "Held"
parent = "n_world"
body2d = { kind = "dynamic" }
collider2d = { kind = "circle", radius = 0.25 }
transform = { position = [10.0, 1.0, 0.0] }
"#,
    );
    b.tick(90);
    let through = b.position(b.node("World/Through"))[1];
    let held = b.position(b.node("World/Held"))[1];
    assert!(
        through < -1.0,
        "a contact 16.7 degrees off the axis held the ball at {through}"
    );
    assert!(
        held > 0.2,
        "a 0.5 radian window let the ball through to {held}"
    );
}

#[test]
fn a_conveyor_carries_what_rests_on_it() {
    let mut b = booted(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_belt"
name = "Belt"
parent = "n_world"
collider3d = { kind = "box", size = [40.0, 1.0, 4.0], surface_velocity = [2.0, 0.0, 0.0] }

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
body3d = { kind = "dynamic" }
collider3d = { kind = "box" }
transform = { position = [0.0, 1.0, 0.0] }
"#,
    );
    b.tick(60);
    let x = b.position(b.node("World/Crate"))[0];
    assert!(
        x > 0.8,
        "the belt carried the crate {x} in a second at 2 a second"
    );
    let back = b.get(b.node("World/Belt"), "collider3d");
    assert_eq!(boot::floats(&back, "surface_velocity"), [2.0, 0.0, 0.0]);
}

#[test]
fn a_2d_conveyor_carries_what_rests_on_it() {
    let mut b = booted(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_belt"
name = "Belt"
parent = "n_world"
collider2d = { kind = "rectangle", size = [40.0, 1.0], surface_velocity = [-2.0, 0.0] }

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_world"
body2d = { kind = "dynamic", lock_rotation = true }
collider2d = { kind = "rectangle" }
transform = { position = [0.0, 1.0, 0.0] }
"#,
    );
    b.tick(60);
    let x = b.position(b.node("World/Crate"))[0];
    assert!(
        x < -0.8,
        "the belt carried the crate {x} in a second at -2 a second"
    );
}

/// Two slope tiles, the second flipped upside down: its one-way axis turns
/// with its polygon.
const FLIPPED: &str = r##"[[assets]]
id = "slopes"
type = "tileset"
texture = "art/dungeon.png"
tile_size = 16
columns = 4

[assets.tiles.1]
collision = [[[16, 0], [16, 16], [0, 16]]]
one_way = true

[[nodes]]
id = "n_map"
name = "Map"

[nodes.tilemap]
tileset = "#slopes"
cells = [[1, 1]]
flags = [[0, 2]]
pixels_per_unit = 16.0

[nodes.tile_collision]
one_way_axis = [0.0, 1.0]
one_way_angle = 0.3
"##;

#[test]
fn a_flipped_one_way_tile_turns_its_axis_with_it() {
    let mut b = booted(FLIPPED);
    b.tick(1);
    let taken = balaur_core::snapshot::capture(&b.app.engine);
    let rows = taken.0["physics2d"]["surfaces"]
        .as_array()
        .expect("the frame's surfaces");
    let mut ups: Vec<f64> = rows
        .iter()
        .map(|row| {
            let platform = &row[1]["one_way"];
            assert!((platform[1].as_f64().unwrap() - 0.3).abs() < 1e-6, "{row}");
            platform[0][1].as_f64().unwrap()
        })
        .collect();
    ups.sort_by(f64::total_cmp);
    assert_eq!(ups, [-1.0, 1.0], "the flipped tile kept the upright axis");
}

/// One tile whose polygon is an L, so its hull is not its shape.
const CORNER: &str = r##"[[assets]]
id = "corner"
type = "tileset"
texture = "art/dungeon.png"
tile_size = 16
columns = 4

[assets.tiles.1]
collision = [[[0, 0], [8, 0], [8, 8], [16, 8], [16, 16], [0, 16]]]

[[nodes]]
id = "n_map"
name = "Map"

[nodes.tilemap]
tileset = "#corner"
cells = [[1]]
pixels_per_unit = 16.0
"##;

#[test]
fn a_tile_polygon_is_fitted_rounded_or_cut_as_asked() {
    let shape = |table: &str| {
        let mut b = booted(&format!("{CORNER}\n[nodes.tile_collision]\n{table}\n"));
        b.tick(1);
        let map = b.node("Map");
        let state = b.app.engine.resource::<balaur_physics::PhysicsState2d>();
        let state = state.borrow();
        let collider = &state.world.colliders[state.colliders[&map][0]];
        let shape = collider.shape();
        (
            shape.as_cuboid().map(|c| c.half_extents.to_array()),
            shape.as_round_convex_polygon().is_some(),
            shape.as_compound().map(|c| c.shapes().len()),
        )
    };
    assert_eq!(shape("fit = \"aabb\"").0, Some([0.5, 0.5]));
    assert!(
        shape("edge_radius = 0.05").1,
        "edge_radius left the hull sharp"
    );
    let pieces = shape("fit = \"convex_decomposition\"").2;
    assert!(
        pieces.is_some_and(|n| n >= 2),
        "an L cut into {pieces:?} pieces"
    );
}

#[test]
fn a_tile_map_conveyor_carries_what_rests_on_it() {
    let mut b = booted(
        r##"[[assets]]
id = "dungeon"
type = "tileset"
texture = "art/dungeon.png"
tile_size = 16
columns = 4

[assets.tiles.1]
collision = "full"

[[nodes]]
id = "n_map"
name = "Map"

[nodes.tilemap]
tileset = "#dungeon"
cells = [[1, 1, 1, 1, 1, 1, 1, 1, 1, 1]]
pixels_per_unit = 16.0

[nodes.tile_collision]
surface_velocity = [2.0, 0.0]

[[nodes]]
id = "n_crate"
name = "Crate"
parent = "n_map"
body2d = { kind = "dynamic", lock_rotation = true }
collider2d = { kind = "rectangle", size = [0.5, 0.5] }
transform = { position = [2.0, 0.5, 0.0] }
"##,
    );
    b.tick(60);
    let x = b.position(b.node("Map/Crate"))[0];
    assert!(x > 2.8, "the map's cells carried the crate to {x}");
}
