//! The `cloner` component, headless: which nodes get copies, where the
//! copies land once the cloner is turned or moved, and what a bake reads.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, AppConfig, components};
use balaur_render::{Clones, RenderPlugin};
use glamx::{Quat, Vec3};

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    (dir, app)
}

fn node(app: &App, name: &str, parent: Entity) -> Entity {
    scene::spawn_node(&mut app.engine.world_mut(), name, parent)
}

fn add(app: &App, entity: Entity, key: &str, params: &str) {
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, key, Some(&params)).unwrap();
}

fn cube(app: &App, parent: Entity, name: &str) -> Entity {
    let entity = node(app, name, parent);
    add(
        app,
        entity,
        "shape3d",
        "kind = \"cuboid\"\nhalf_extents = [0.5, 0.5, 0.5]",
    );
    entity
}

/// Where each copy of `entity` sits in the world.
fn placements(app: &App, entity: Entity) -> Vec<Vec3> {
    let world = app.engine.world();
    let clones = world.get::<&Clones>(entity).expect("a cloned node");
    clones.0.iter().map(|m| m.w_axis.truncate()).collect()
}

#[test]
fn a_cloner_gives_its_child_a_pose_per_copy() {
    let (_dir, mut app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Row", root);
    add(
        &app,
        owner,
        "cloner",
        "mode = \"linear\"\ncount = 4\nstep = [2.0, 0.0, 0.0]",
    );
    let child = cube(&app, owner, "Post");
    app.tick(1.0 / 60.0);
    let at = placements(&app, child);
    assert_eq!(at.len(), 4);
    assert_eq!(
        at[0],
        Vec3::ZERO,
        "the first copy is where the node already is"
    );
    assert_eq!(at[3], Vec3::new(6.0, 0.0, 0.0));
}

/// The whole subtree is the template, not just the first child.
#[test]
fn every_drawn_node_under_a_cloner_is_multiplied() {
    let (_dir, mut app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Row", root);
    add(
        &app,
        owner,
        "cloner",
        "mode = \"linear\"\ncount = 3\nstep = [1.0, 0.0, 0.0]",
    );
    let trunk = cube(&app, owner, "Trunk");
    let branch = cube(&app, trunk, "Branch");
    {
        let world = app.engine.world_mut();
        if let Ok(mut transform) = world.get::<&mut Transform>(branch) {
            transform.position = Vec3::new(0.0, 2.0, 0.0);
        }
    }
    app.tick(1.0 / 60.0);
    assert_eq!(placements(&app, trunk).len(), 3);
    let branches = placements(&app, branch);
    assert_eq!(branches.len(), 3);
    assert_eq!(
        branches[0],
        Vec3::new(0.0, 2.0, 0.0),
        "its own offset is kept"
    );
    assert_eq!(branches[2], Vec3::new(2.0, 2.0, 0.0), "and carried along");
}

/// The arrangement is written in the cloner's own space, so turning the
/// cloner turns the whole row rather than sliding it.
#[test]
fn turning_the_cloner_turns_the_arrangement() {
    let (_dir, mut app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Row", root);
    add(
        &app,
        owner,
        "cloner",
        "mode = \"linear\"\ncount = 2\nstep = [2.0, 0.0, 0.0]",
    );
    let child = cube(&app, owner, "Post");
    {
        let world = app.engine.world_mut();
        if let Ok(mut transform) = world.get::<&mut Transform>(owner) {
            transform.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        }
    }
    app.tick(1.0 / 60.0);
    let at = placements(&app, child);
    let second = at[1];
    assert!(
        second.x.abs() < 1e-4 && (second.z + 2.0).abs() < 1e-4,
        "a quarter turn puts the second copy on -z, not on x: {second:?}"
    );
}

#[test]
fn a_grid_cloner_lays_its_child_out_in_a_box() {
    let (_dir, mut app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Field", root);
    add(
        &app,
        owner,
        "cloner",
        "mode = \"grid\"\ncounts = [4, 1, 3]\nstep = [1.5, 0.0, 2.0]",
    );
    let child = cube(&app, owner, "Post");
    app.tick(1.0 / 60.0);
    let at = placements(&app, child);
    assert_eq!(at.len(), 12);
    let far = at.iter().fold(Vec3::ZERO, |far, p| far.max(*p));
    assert_eq!(far, Vec3::new(4.5, 0.0, 4.0));
}

/// Removing the cloner puts the node back to drawing once.
#[test]
fn a_node_stops_being_cloned_when_the_cloner_goes() {
    let (_dir, mut app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Row", root);
    add(&app, owner, "cloner", "mode = \"linear\"\ncount = 3");
    let child = cube(&app, owner, "Post");
    app.tick(1.0 / 60.0);
    assert_eq!(placements(&app, child).len(), 3);
    components::remove(&app.engine, owner, "cloner").unwrap();
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    assert!(
        world.get::<&Clones>(child).is_err(),
        "the copies should be gone with the cloner"
    );
}

#[test]
fn a_cloner_reads_back_as_it_was_written() {
    let (_dir, app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Ring", root);
    add(
        &app,
        owner,
        "cloner",
        "mode = \"radial\"\ncount = 6\nradius = 3.5\nseed = 9\nrandom = 0.25",
    );
    let back = components::get(&app.engine, owner, "cloner").expect("it reads back");
    assert_eq!(
        back.get("mode").and_then(toml::Value::as_str),
        Some("radial")
    );
    assert_eq!(back.get("count").and_then(toml::Value::as_integer), Some(6));
    assert_eq!(back.get("seed").and_then(toml::Value::as_integer), Some(9));
}

#[test]
fn an_unknown_mode_is_refused_rather_than_guessed() {
    let (_dir, app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Row", root);
    let params: toml::Value = toml::from_str("mode = \"sprinkle\"").unwrap();
    assert!(components::add(&app.engine, owner, "cloner", Some(&params)).is_err());
}
