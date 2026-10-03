//! A snapshot frame puts the physics world back exactly, whatever shapes it
//! holds: a restored run steps the same as one that never stopped.

use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_core::scene::find_node;

/// A project whose scene is `scene`, booted and loaded. Holds [`crate::LOG`]
/// while the caller keeps the guard, so the restore's errors are its own.
fn boot(scene: &str) -> (App, tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = crate::LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    (app, dir, guard)
}

fn errors() -> Vec<String> {
    balaur_core::logbuf::recent(50)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| format!("{} {:?}", e.message, e.fields))
        .collect()
}

fn node(app: &App, path: &str) -> Entity {
    find_node(&app.engine.world(), app.engine.root(), path).expect("the scene's node")
}

/// Where `body` is after each of `steps` more ticks.
fn path(app: &mut App, body: Entity, steps: usize) -> Vec<[f32; 3]> {
    (0..steps)
        .map(|_| {
            app.tick(1.0 / 60.0);
            let world = app.engine.world();
            let t = world.get::<&balaur_core::Transform>(body).unwrap();
            [t.position.x, t.position.y, t.position.z]
        })
        .collect()
}

/// Steps on from a snapshot, restores it, and steps the same ticks again.
fn replayed(app: &mut App, body: Entity) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let taken = balaur_core::snapshot::capture(&app.engine);
    let straight = path(app, body, 30);
    balaur_core::snapshot::restore(&app.engine, &taken);
    let errors = errors();
    assert!(errors.is_empty(), "the restore logged errors: {errors:#?}");
    (straight, path(app, body, 30))
}

#[test]
fn a_voxel_world_restores_exactly() {
    let (mut app, _dir, _log) = boot(
        r##"[[assets]]
id = "ground"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[-1, 0, -1], [0, 0, -1], [-1, 0, 0], [0, 0, 0]]

[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ground"
name = "Ground"
parent = "n_world"
collider3d = { kind = "voxels", voxels = "#ground" }

[[nodes]]
id = "n_ball"
name = "Ball"
parent = "n_world"
body3d = { kind = "dynamic" }
collider3d = { kind = "sphere", radius = 0.3 }
transform = { position = [0.2, 2.5, 0.1] }
"##,
    );
    let ball = node(&app, "World/Ball");
    path(&mut app, ball, 20);
    let (straight, restored) = replayed(&mut app, ball);
    assert!(
        straight.last().unwrap()[1] > 0.9,
        "the ball fell through the voxels: {:?}",
        straight.last()
    );
    assert_eq!(
        straight, restored,
        "a restored voxel world stepped differently"
    );
}

#[test]
fn a_one_way_world_restores_exactly() {
    let (mut app, _dir, _log) = boot(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_ledge"
name = "Ledge"
parent = "n_world"
collider2d = { kind = "rectangle", size = [4.0, 0.2], one_way = true }

[[nodes]]
id = "n_ball"
name = "Ball"
parent = "n_world"
body2d = { kind = "dynamic" }
collider2d = { kind = "circle", radius = 0.25 }
transform = { position = [0.0, -1.0, 0.0] }
"#,
    );
    let ball = node(&app, "World/Ball");
    {
        let state = app.engine.resource::<balaur_physics::PhysicsState2d>();
        let mut state = state.borrow_mut();
        let handle = state.bodies[&ball];
        let up = balaur_physics::rapier2d::math::Vector::new(0.0, 7.0);
        state.world.bodies[handle].set_linvel(up, true);
    }
    path(&mut app, ball, 5);
    let (straight, restored) = replayed(&mut app, ball);
    assert!(
        straight.last().unwrap()[1] > 0.0,
        "the ball stopped under the platform: {:?}",
        straight.last()
    );
    assert_eq!(
        straight, restored,
        "a restored one-way world stepped differently"
    );
}

/// The map's rows in the 2D frame, by name.
fn tile_rows(app: &App, field: &str) -> usize {
    let taken = balaur_core::snapshot::capture(&app.engine);
    taken.0["physics2d"][field]
        .as_array()
        .unwrap_or_else(|| panic!("the 2D frame has no {field}"))
        .len()
}

#[test]
fn a_freed_map_leaves_nothing_in_the_frame() {
    let (mut app, _dir, _log) = boot(
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
cells = [[1, 1]]
pixels_per_unit = 16.0

[nodes.tile_collision]
"##,
    );
    app.tick(1.0 / 60.0);
    let fields = ["tile_params", "tile_built", "tile_colliders"];
    for field in fields {
        assert_eq!(tile_rows(&app, field), 1, "the live map has no {field} row");
    }
    let map = node(&app, "Map");
    balaur_core::scene::free_subtree(&mut app.engine.world_mut(), map);
    app.tick(1.0 / 60.0);
    for field in fields {
        assert_eq!(
            tile_rows(&app, field),
            0,
            "the freed map left a {field} row"
        );
    }
}

/// The params tables a frame carries, in both worlds, by entity.
fn params_tables(app: &App) -> Vec<toml::Value> {
    let three = app.engine.resource::<balaur_physics::PhysicsState3d>();
    let three = three.borrow();
    let two = app.engine.resource::<balaur_physics::PhysicsState2d>();
    let two = two.borrow();
    let mut out: Vec<toml::Value> = Vec::new();
    for map in [
        &three.collider_params,
        &three.joint_params,
        &two.collider_params,
    ] {
        let mut rows: Vec<_> = map.iter().collect();
        rows.sort_by_key(|(entity, _)| entity.to_bits());
        out.extend(rows.into_iter().map(|(_, params)| params.clone()));
    }
    out
}

/// A frame carries what the scene wrote, not every schema default, and the
/// restore puts the defaults back: the tables match the ones taken.
#[test]
fn a_frame_keeps_what_the_scene_wrote_and_restores_every_default() {
    let (mut app, _dir, _log) = boot(
        r#"[[nodes]]
id = "n_world"
name = "World"

[[nodes]]
id = "n_anchor"
name = "Anchor"
parent = "n_world"
body3d = { kind = "static" }
collider3d = { kind = "sphere", radius = 0.3 }

[[nodes]]
id = "n_ball"
name = "Ball"
parent = "n_world"
body3d = { kind = "dynamic" }
collider3d = { kind = "sphere", radius = 0.3, offset = [-0.0, 0.0, 0.0] }
joint3d = { kind = "hinge", connected_body = "../Anchor", anchor = [-1.0, 0.0, 0.0] }
transform = { position = [1.0, 0.0, 0.0] }

[[nodes]]
id = "n_disc"
name = "Disc"
parent = "n_world"
body2d = { kind = "dynamic" }
collider2d = { kind = "circle", radius = 0.25 }
"#,
    );
    let ball = node(&app, "World/Ball");
    path(&mut app, ball, 5);
    let taken = balaur_core::snapshot::capture(&app.engine);
    let anchor = &taken.0["physics"]["collider_params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row[0][0] == "n_anchor")
        .expect("the anchor's collider row")[1];
    let mut kept: Vec<&str> = anchor
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    kept.sort_unstable();
    assert_eq!(
        kept,
        ["kind", "radius"],
        "the frame holds defaults: {anchor}"
    );
    let before = params_tables(&app);
    path(&mut app, ball, 5);
    balaur_core::snapshot::restore(&app.engine, &taken);
    let errors = errors();
    assert!(errors.is_empty(), "the restore logged errors: {errors:#?}");
    assert_eq!(
        params_tables(&app),
        before,
        "a restored table differs from the one taken"
    );
}
