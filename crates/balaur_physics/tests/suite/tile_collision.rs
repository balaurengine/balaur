//! `tile_collision`: a tile map's own cells, as one voxel collider.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::scene::find_node;
use balaur_physics::PhysicsState2d;
use balaur_physics::rapier2d::math::IVector;

/// The log buffer is global and tests run in parallel.
use crate::LOG;

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
script = { source = "scripts/s.rn" }

[nodes.tilemap]
tileset = "#dungeon"
cells = [
  [ 1,  1],
  [ 1, -1],
]
pixels_per_unit = 16.0

[nodes.tile_collision]
friction = 0.9
"##;

fn run(script: &str, frames: u32) -> (App, Vec<String>) {
    run_scene(SCENE, script, frames)
}

fn run_scene(scene: &str, script: &str, frames: u32) -> (App, Vec<String>) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    boot_scene(scene, script, frames)
}

/// [`run_scene`] for a caller already holding [`LOG`].
fn boot_scene(scene: &str, script: &str, frames: u32) -> (App, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
    (app, logged_errors())
}

/// Every error logged since the buffer was last cleared, with its fields.
fn logged_errors() -> Vec<String> {
    balaur_core::logbuf::recent(50)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| format!("{} {:?}", e.message, e.fields))
        .collect()
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
        r"pub fn init(this) {
    this.node.tilemap.set_cell(0, 0, -1);
}
",
        2,
    );
    assert!(errors.is_empty(), "the script logged errors: {errors:#?}");
    assert_eq!(
        filled(&app),
        [false, true, true, false],
        "the cell the script cleared is gone from the collider too"
    );
}

/// Two cells of a slope that a body passes through from below, the second of
/// them mirrored: the collider has to be the picture, and it has to keep the
/// behaviour the tile names.
const SLOPES: &str = r##"[[assets]]
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
flags = [[0, 1]]
pixels_per_unit = 16.0

[nodes.tile_collision]
friction = 0.5
"##;

/// The corner above the middle of each shaped cell's triangle, in the cell's
/// own space: which side the slope rises on.
fn slope_peaks(app: &App) -> Vec<f32> {
    let world = app.engine.world();
    let node = find_node(&world, app.engine.root(), "Map").expect("the scene's map");
    drop(world);
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    state.colliders[&node]
        .iter()
        .map(|handle| {
            let collider = &state.world.colliders[*handle];
            assert!(
                collider.active_hooks().contains(
                    balaur_physics::rapier2d::prelude::ActiveHooks::MODIFY_SOLVER_CONTACTS
                ),
                "a one-way tile asks for the contact hook whatever shape it is"
            );
            let hull = collider
                .shape()
                .as_convex_polygon()
                .expect("a tile's polygons build a hull");
            let top = hull
                .points()
                .iter()
                .max_by(|a, b| a.y.total_cmp(&b.y))
                .expect("the hull has points");
            top.x
        })
        .collect()
}

#[test]
fn a_turned_tile_collides_the_way_it_is_drawn_and_keeps_being_one_way() {
    let (app, errors) = run_scene(SLOPES, "", 1);
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    let peaks = slope_peaks(&app);
    assert_eq!(peaks.len(), 2, "one collider per shaped cell: {peaks:?}");
    assert!(peaks[0] > 0.0, "the upright slope rises on the right");
    assert!(peaks[1] < 0.0, "and the mirrored one on the left");
}

/// The map node, found in a booted scene.
fn map_node(app: &App) -> balaur_core::hecs::Entity {
    find_node(&app.engine.world(), app.engine.root(), "Map").expect("the scene's map")
}

#[test]
fn one_way_on_the_component_lets_a_body_up_through_full_cells() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scene = SCENE
        .replace("script = { source = \"scripts/s.rn\" }\n", "")
        .replace("friction = 0.9", "one_way = true")
        + r#"
[[nodes]]
id = "n_ball"
name = "Ball"
parent = "n_map"
body2d = { kind = "dynamic", gravity_scale = 0.0 }
collider2d = { kind = "circle", radius = 0.25 }

[nodes.transform]
position = [0.5, -4.0, 0.0]
"#;
    let (mut app, errors) = boot_scene(&scene, "", 1);
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    let ball = find_node(&app.engine.world(), app.engine.root(), "Map/Ball").unwrap();
    {
        let state = app.engine.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        let handle = state.bodies[&ball];
        let up = balaur_physics::rapier2d::math::Vector::new(0.0, 6.0);
        state.world.bodies[handle].set_linvel(up, true);
    }
    for _ in 0..60 {
        app.tick(1.0 / 60.0);
    }
    let y = app
        .engine
        .world()
        .get::<&balaur_core::Transform>(ball)
        .unwrap()
        .position
        .y;
    assert!(y > 0.5, "the ball stopped under the map at y = {y}");
}

#[test]
fn one_way_on_the_component_reaches_plain_shaped_tiles() {
    let scene = SLOPES
        .replace("one_way = true\n", "")
        .replace("friction = 0.5", "one_way = true");
    let (app, errors) = run_scene(&scene, "", 1);
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    let map = map_node(&app);
    let taken = balaur_core::snapshot::capture(&app.engine);
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    for handle in &state.colliders[&map] {
        let collider = &state.world.colliders[*handle];
        assert!(
            collider
                .active_hooks()
                .contains(balaur_physics::rapier2d::prelude::ActiveHooks::MODIFY_SOLVER_CONTACTS),
            "a plain tile under one_way asks for no contact hook"
        );
    }
    let rows = taken.0["physics2d"]["surfaces"]
        .as_array()
        .map_or(0, Vec::len);
    assert_eq!(
        rows,
        state.colliders[&map].len(),
        "a plain tile under one_way carries no platform axis"
    );
}

#[test]
fn a_maps_mass_is_the_whole_maps() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scene = SLOPES.to_string() + "\n[nodes.body2d]\nkind = \"dynamic\"\n";
    let (mut app, errors) = boot_scene(&scene, "", 0);
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    let map = map_node(&app);
    let mass: toml::Value = toml::from_str("mass = 6.0").unwrap();
    balaur_core::components::patch(&app.engine, map, "tile_collision", &mass).unwrap();
    app.tick(1.0 / 60.0);
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    assert_eq!(
        state.colliders[&map].len(),
        2,
        "two shaped cells, two colliders"
    );
    let mass = state.world.bodies[state.bodies[&map]].mass();
    assert!((mass - 6.0).abs() < 1e-3, "a map of mass 6 weighs {mass}");
}

/// The centre and the spin of a dynamic map whose `tile_collision` is patched
/// with `mass`, in the body's own space.
fn stated_mass(patch: &str) -> ([f32; 2], f32) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scene = SLOPES.to_string() + "\n[nodes.body2d]\nkind = \"dynamic\"\n";
    let (mut app, errors) = boot_scene(&scene, "", 0);
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    let map = map_node(&app);
    let mass: toml::Value = toml::from_str(patch).unwrap();
    balaur_core::components::patch(&app.engine, map, "tile_collision", &mass).unwrap();
    app.tick(1.0 / 60.0);
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let props = state.world.bodies[state.bodies[&map]]
        .mass_properties()
        .local_mprops;
    (props.local_com.to_array(), props.principal_inertia())
}

#[test]
fn a_map_takes_the_centre_of_mass_and_the_inertia_it_states() {
    let (com, inertia) = stated_mass("mass = 6.0\ncenter_of_mass = [0.5, -0.25]\ninertia = 2.0");
    assert!(
        (com[0] - 0.5).abs() < 1e-4 && (com[1] + 0.25).abs() < 1e-4,
        "the map's centre is {com:?}"
    );
    assert!(
        (inertia - 2.0).abs() < 1e-3,
        "the map spins with inertia {inertia}"
    );
    let (com, inertia) = stated_mass("mass = 6.0\ncenter_of_mass = [0.5, -0.25]");
    assert!((com[0] - 0.5).abs() < 1e-4, "the map's centre is {com:?}");
    assert!(
        inertia > 0.0,
        "a stated centre with no inertia keeps the shapes' own spin"
    );
}

/// Plain shaped tiles: a voxel grid's cell map and a one-way axis above
/// `user_data`'s low 64 bits both fail the snapshot's JSON.
#[test]
fn a_restored_map_rebuilds_from_its_own_colliders() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, errors) = boot_scene(&SLOPES.replace("one_way = true\n", ""), "", 1);
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    let map = map_node(&app);
    let count = |app: &App| {
        let state = app.engine.resource::<PhysicsState2d>();
        let state = state.borrow();
        state.colliders.get(&map).map_or(0, Vec::len)
    };
    let dig = |app: &App, column: usize| {
        let world = app.engine.world();
        let mut grid = world.get::<&mut balaur_core::tiles::TileGrid>(map).unwrap();
        grid.rows[0][column] = None;
        grid.version += 1;
    };
    assert_eq!(count(&app), 2, "two shaped cells, two colliders");
    let taken = balaur_core::snapshot::capture(&app.engine);
    dig(&app, 0);
    app.tick(1.0 / 60.0);
    assert_eq!(count(&app), 1, "digging a cell rebuilds the map");

    balaur_core::snapshot::restore(&app.engine, &taken);
    let errors = logged_errors();
    assert!(errors.is_empty(), "the restore logged errors: {errors:#?}");
    assert_eq!(
        count(&app),
        2,
        "the restored world holds both cells' colliders"
    );
    dig(&app, 1);
    app.tick(1.0 / 60.0);
    assert_eq!(
        count(&app),
        0,
        "the rebuild after the restore left colliders the map no longer has"
    );
}
