//! `boolean3d` and `boolean2d`, headless: a node drawn as its children
//! combined, the operands hidden, and the result recomputed when one moves.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Appearance, Transform};
use balaur_core::{App, AppConfig, components};
use balaur_render::{RenderPlugin, Renderable, Renderable2d, Shape, Shape2d};
use glamx::Vec3;

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

fn place(app: &App, entity: Entity, at: Vec3) {
    let world = app.engine.world_mut();
    if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
        transform.position = at;
    }
}

/// The volume the built triangles enclose.
fn volume(app: &App, entity: Entity) -> f64 {
    let world = app.engine.world();
    let renderable = world
        .get::<&Renderable>(entity)
        .expect("a built renderable");
    let mesh = renderable.built.as_deref().expect("built geometry");
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

/// A boolean over two unit cubes, the second offset so an eighth overlaps.
fn two_cubes(app: &App, op: &str) -> (Entity, Entity, Entity) {
    let root = app.engine.root();
    let owner = node(app, "Cut", root);
    add(app, owner, "boolean3d", &format!("op = \"{op}\""));
    let a = node(app, "A", owner);
    add(
        app,
        a,
        "shape3d",
        "kind = \"cuboid\"\nhalf_extents = [0.5, 0.5, 0.5]",
    );
    let b = node(app, "B", owner);
    add(
        app,
        b,
        "shape3d",
        "kind = \"cuboid\"\nhalf_extents = [0.5, 0.5, 0.5]",
    );
    place(app, b, Vec3::splat(0.5));
    (owner, a, b)
}

#[test]
fn a_union_draws_both_children_as_one_solid() {
    let (_dir, mut app) = app();
    let (owner, _, _) = two_cubes(&app, "union");
    app.tick(1.0 / 60.0);
    let got = volume(&app, owner);
    assert!((got - (2.0 - 0.125)).abs() < 0.02, "union came to {got}");
    let world = app.engine.world();
    let renderable = world.get::<&Renderable>(owner).unwrap();
    assert!(matches!(renderable.shape, Shape::Built));
}

#[test]
fn a_difference_takes_the_second_child_out_of_the_first() {
    let (_dir, mut app) = app();
    let (owner, _, _) = two_cubes(&app, "difference");
    app.tick(1.0 / 60.0);
    let got = volume(&app, owner);
    assert!(
        (got - (1.0 - 0.125)).abs() < 0.02,
        "difference came to {got}"
    );
}

#[test]
fn an_intersection_keeps_only_the_overlap() {
    let (_dir, mut app) = app();
    let (owner, _, _) = two_cubes(&app, "intersection");
    app.tick(1.0 / 60.0);
    let got = volume(&app, owner);
    assert!((got - 0.125).abs() < 0.02, "intersection came to {got}");
}

/// The operands stay in the tree so they can still be edited, but the node
/// draws the result and not the parts.
#[test]
fn the_operands_stay_in_the_tree_and_stop_drawing() {
    let (_dir, mut app) = app();
    let (_, a, b) = two_cubes(&app, "union");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    for child in [a, b] {
        assert!(world.get::<&Renderable>(child).is_ok(), "still a node");
        let appearance = world.get::<&Appearance>(child).expect("an appearance");
        assert!(!appearance.visible, "an operand should not draw itself");
    }
}

/// Moving an operand is a different answer, and the node works it out again.
#[test]
fn moving_an_operand_recomputes_the_result() {
    let (_dir, mut app) = app();
    let (owner, _, b) = two_cubes(&app, "union");
    app.tick(1.0 / 60.0);
    let overlapping = volume(&app, owner);
    place(&app, b, Vec3::new(5.0, 0.0, 0.0));
    app.tick(1.0 / 60.0);
    let apart = volume(&app, owner);
    assert!(
        (apart - 2.0).abs() < 0.02,
        "moved apart the union is two whole cubes, got {apart}"
    );
    assert!(apart > overlapping, "{apart} against {overlapping}");
}

/// A still frame recomputes nothing: the version only moves when an operand
/// does, which is what keeps a boolean off the per-tick budget.
#[test]
fn a_boolean_that_did_not_change_is_not_rebuilt() {
    let (_dir, mut app) = app();
    let (owner, _, _) = two_cubes(&app, "union");
    app.tick(1.0 / 60.0);
    let first = app
        .engine
        .world()
        .get::<&Renderable>(owner)
        .unwrap()
        .version;
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    let later = app
        .engine
        .world()
        .get::<&Renderable>(owner)
        .unwrap()
        .version;
    assert_eq!(first, later, "nothing moved, so nothing was rebuilt");
}

/// Booleans nest: a node whose child is itself a boolean takes that child's
/// result as one operand.
#[test]
fn a_boolean_can_take_another_booleans_result() {
    let (_dir, mut app) = app();
    let root = app.engine.root();
    let outer = node(&app, "Outer", root);
    add(&app, outer, "boolean3d", "op = \"union\"");
    let inner = node(&app, "Inner", outer);
    add(&app, inner, "boolean3d", "op = \"union\"");
    let a = node(&app, "A", inner);
    add(
        &app,
        a,
        "shape3d",
        "kind = \"cuboid\"\nhalf_extents = [0.5, 0.5, 0.5]",
    );
    let b = node(&app, "B", inner);
    add(
        &app,
        b,
        "shape3d",
        "kind = \"cuboid\"\nhalf_extents = [0.5, 0.5, 0.5]",
    );
    place(&app, b, Vec3::new(3.0, 0.0, 0.0));
    // Twice: the inner one settles first, the outer one reads its result.
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    let got = volume(&app, outer);
    assert!(
        (got - 2.0).abs() < 0.05,
        "two cubes through two booleans: {got}"
    );
}

#[test]
fn a_two_dimensional_boolean_fills_the_shapes_combined() {
    let (_dir, mut app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Cut", root);
    add(&app, owner, "boolean2d", "op = \"difference\"");
    let a = node(&app, "A", owner);
    add(
        &app,
        a,
        "shape2d",
        "kind = \"rect\"\nhalf_extents = [1.0, 1.0]",
    );
    let b = node(&app, "B", owner);
    add(
        &app,
        b,
        "shape2d",
        "kind = \"rect\"\nhalf_extents = [1.0, 1.0]",
    );
    place(&app, b, Vec3::new(1.0, 0.0, 0.0));
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let renderable = world.get::<&Renderable2d>(owner).expect("a 2D renderable");
    assert!(renderable.shape == Shape2d::Polygon);
    let polygon = renderable.polygon.as_deref().expect("a filled outline");
    assert!(!polygon.indices.is_empty(), "the result fills");
    let widest = polygon
        .positions
        .iter()
        .fold(f32::MIN, |widest, p| widest.max(p.x));
    assert!(widest <= 0.01, "the right half should have been cut away");
}

#[test]
fn an_unknown_operation_is_refused_rather_than_guessed() {
    let (_dir, app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Cut", root);
    let params: toml::Value = toml::from_str("op = \"smoosh\"").unwrap();
    assert!(components::add(&app.engine, owner, "boolean3d", Some(&params)).is_err());
}
