//! `tile_collision`: a tile map's own cells, as one voxel collider.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::scene::find_node;
use balaur_physics::PhysicsState2d;
use balaur_physics::rapier2d::math::IVector;

/// The log buffer is global and tests run in parallel.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A two-by-two map of one-unit cells, three of them solid. `pixels_per_unit`
/// matches `tile_size`, so a cell is one world unit and cell 0,0 has its
/// top-left corner on the node.
const SCENE: &str = r##"[[assets]]
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
script = "scripts/s.rn"

[nodes.tilemap]
tileset = "#dungeon"
cells = """
11
1.
"""
pixels_per_unit = 16.0

[nodes.tile_collision]
friction = 0.9
"##;

fn run(script: &str, frames: u32) -> (App, Vec<String>) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), SCENE).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
    let errors = balaur_core::logbuf::recent(50)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    (app, errors)
}

/// Whether each of the map's four cells is filled, read off the shape. The
/// grid's keys count up where its rows count down, so row 0 is key -1.
fn filled(app: &App) -> [bool; 4] {
    let world = app.engine.world();
    let node = find_node(&world, app.engine.root(), "Map").expect("the scene's map");
    drop(world);
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = *state
        .colliders
        .get(&node)
        .and_then(|handles| handles.first())
        .expect("the map built a collider");
    let voxels = state.world.colliders[handle]
        .shape()
        .as_voxels()
        .expect("solid cells build one voxel collider, not a pile of cuboids");
    let at = |x, y| {
        voxels
            .voxel_state(IVector::new(x, y))
            .is_some_and(|cell| !cell.is_empty())
    };
    [at(0, -1), at(1, -1), at(0, -2), at(1, -2)]
}

#[test]
fn the_solid_cells_of_a_map_become_one_voxel_collider() {
    let (app, errors) = run("", 1);
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    assert_eq!(
        filled(&app),
        [true, true, true, false],
        "three tiles say `collision = \"full\"`, and the fourth cell is empty"
    );
    let world = app.engine.world();
    let node = find_node(&world, app.engine.root(), "Map").unwrap();
    drop(world);
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let handle = *state.colliders[&node].first().unwrap();
    let collider = &state.world.colliders[handle];
    let at = collider.position().translation;
    assert!(
        at.x.abs() < 1e-5 && at.y.abs() < 1e-5,
        "the voxel lattice is the map's own, so the collider needs no offset ({at:?})"
    );
    assert!(
        (collider.friction() - 0.9).abs() < 1e-5,
        "the component's material reaches the shape"
    );
}

#[test]
fn digging_a_cell_rebuilds_what_the_map_collides_with() {
    let (app, errors) = run(
        r#"pub fn init(this) {
    render::set_cell(this.node, 0, 0, -1);
}
"#,
        2,
    );
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
    assert_eq!(
        filled(&app),
        [false, true, true, false],
        "the cell the script cleared is gone from the collider too"
    );
}
