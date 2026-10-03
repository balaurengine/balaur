//! `boolean3d` and `boolean2d`, headless: a node drawn as its children
//! combined, the operands hidden, and the result recomputed when one moves.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Appearance, Transform};
use balaur_core::{App, AppConfig, components};
use balaur_render::{RenderPlugin, Renderable2d, Renderable3d, Shape2d, Shape3d};
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
        .get::<&Renderable3d>(entity)
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
    add(app, owner, "boolean3d", &format!("operation = \"{op}\""));
    let a = node(app, "A", owner);
    add(app, a, "shape3d", "kind = \"box\"\nsize = [1.0, 1.0, 1.0]");
    let b = node(app, "B", owner);
    add(app, b, "shape3d", "kind = \"box\"\nsize = [1.0, 1.0, 1.0]");
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
    let renderable = world.get::<&Renderable3d>(owner).unwrap();
    assert!(matches!(renderable.shape, Shape3d::Built));
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
        assert!(world.get::<&Renderable3d>(child).is_ok(), "still a node");
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
        .get::<&Renderable3d>(owner)
        .unwrap()
        .version;
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    let later = app
        .engine
        .world()
        .get::<&Renderable3d>(owner)
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
    add(&app, outer, "boolean3d", "operation = \"union\"");
    let inner = node(&app, "Inner", outer);
    add(&app, inner, "boolean3d", "operation = \"union\"");
    let a = node(&app, "A", inner);
    add(&app, a, "shape3d", "kind = \"box\"\nsize = [1.0, 1.0, 1.0]");
    let b = node(&app, "B", inner);
    add(&app, b, "shape3d", "kind = \"box\"\nsize = [1.0, 1.0, 1.0]");
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
    add(&app, owner, "boolean2d", "operation = \"difference\"");
    let a = node(&app, "A", owner);
    add(
        &app,
        a,
        "shape2d",
        "kind = \"rectangle\"\nsize = [2.0, 2.0]",
    );
    let b = node(&app, "B", owner);
    add(
        &app,
        b,
        "shape2d",
        "kind = \"rectangle\"\nsize = [2.0, 2.0]",
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
    drop(renderable);
    drop(world);
    // Drawn as a polygon, but the node carries no `polygon` component: one
    // reported would be saved and inspected as the node's own.
    assert!(components::get(&app.engine, owner, "polygon").is_none());
}

#[test]
fn an_unknown_operation_is_refused_rather_than_guessed() {
    let (_dir, app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Cut", root);
    let params: toml::Value = toml::from_str("operation = \"smoosh\"").unwrap();
    assert!(components::add(&app.engine, owner, "boolean3d", Some(&params)).is_err());
}

/// The area the 2D result's triangles cover, holes left out.
fn filled_area(app: &App, entity: Entity) -> f32 {
    let world = app.engine.world();
    let renderable = world.get::<&Renderable2d>(entity).expect("a 2D renderable");
    let polygon = renderable.polygon.as_deref().expect("a filled outline");
    polygon
        .indices
        .iter()
        .map(|&[a, b, c]| {
            let (a, b, c) = (
                polygon.positions[a as usize],
                polygon.positions[b as usize],
                polygon.positions[c as usize],
            );
            ((b - a).perp_dot(c - a) / 2.0).abs()
        })
        .sum()
}

/// A 2D boolean over a square and a second square of `inner` side at `at`.
fn two_squares(app: &App, params: &str, inner: f32, at: Vec3) -> Entity {
    let root = app.engine.root();
    let owner = node(app, "Cut", root);
    add(app, owner, "boolean2d", params);
    let a = node(app, "A", owner);
    add(app, a, "shape2d", "kind = \"rectangle\"\nsize = [4.0, 4.0]");
    let b = node(app, "B", owner);
    add(
        app,
        b,
        "shape2d",
        &format!("kind = \"rectangle\"\nsize = [{inner}, {inner}]"),
    );
    place(app, b, at);
    owner
}

#[test]
fn a_square_cut_from_the_middle_of_another_leaves_a_hole() {
    let (_dir, mut app) = app();
    let owner = two_squares(&app, "operation = \"difference\"", 2.0, Vec3::ZERO);
    app.tick(1.0 / 60.0);
    let area = filled_area(&app, owner);
    assert!(
        (area - 12.0).abs() < 1e-3,
        "16 less the 4 of the hole, got {area}"
    );
}

#[test]
fn a_symmetric_difference_keeps_what_only_one_square_covers() {
    let (_dir, mut app) = app();
    let owner = two_squares(
        &app,
        "operation = \"symmetric_difference\"",
        4.0,
        Vec3::new(2.0, 0.0, 0.0),
    );
    app.tick(1.0 / 60.0);
    let area = filled_area(&app, owner);
    assert!(
        (area - 16.0).abs() < 1e-3,
        "two 4x2 strips each side, got {area}"
    );
}

#[test]
fn a_reverse_difference_takes_the_first_square_out_of_the_second() {
    let (_dir, mut app) = app();
    let owner = two_squares(
        &app,
        "operation = \"reverse_difference\"",
        4.0,
        Vec3::new(2.0, 0.0, 0.0),
    );
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let renderable = world.get::<&Renderable2d>(owner).unwrap();
    let polygon = renderable.polygon.as_deref().unwrap();
    let leftmost = polygon
        .positions
        .iter()
        .fold(f32::MAX, |left, p| left.min(p.x));
    assert!(
        leftmost >= 1.99,
        "only the second square's right half stays: {leftmost}"
    );
}

#[test]
fn a_piece_under_min_area_is_dropped() {
    let (_dir, mut app) = app();
    let small = Vec3::new(10.0, 0.0, 0.0);
    let kept = two_squares(&app, "operation = \"union\"", 0.5, small);
    app.tick(1.0 / 60.0);
    assert!(
        (filled_area(&app, kept) - 16.25).abs() < 1e-3,
        "control: both pieces"
    );
    let (_dir, mut app) = self::app();
    let dropped = two_squares(&app, "operation = \"union\"\nmin_area = 1.0", 0.5, small);
    app.tick(1.0 / 60.0);
    assert!((filled_area(&app, dropped) - 16.0).abs() < 1e-3);
}

#[test]
fn a_two_dimensional_boolean_reads_back_every_key_it_takes() {
    let (_dir, mut app) = app();
    let owner = two_squares(
        &app,
        "operation = \"union\"\nfill_rule = \"non_zero\"\nmin_area = 0.5\nkeep_collinear = true\nclean_result = false\ncolor = [1.0, 0.0, 0.0, 1.0]\nmaterial = \"materials/glow.toml\"",
        2.0,
        Vec3::ZERO,
    );
    app.tick(1.0 / 60.0);
    let read = components::get(&app.engine, owner, "boolean2d").unwrap();
    assert_eq!(read["fill_rule"].as_str(), Some("non_zero"));
    assert_eq!(read["min_area"].as_float(), Some(0.5));
    assert_eq!(read["keep_collinear"].as_bool(), Some(true));
    assert_eq!(read["clean_result"].as_bool(), Some(false));
    assert_eq!(read["material"].as_str(), Some("materials/glow.toml"));
    let world = app.engine.world();
    let renderable = world.get::<&Renderable2d>(owner).unwrap();
    assert!(
        crate::same(renderable.color, [1.0, 0.0, 0.0, 1.0]),
        "the tint reaches the result"
    );
    assert_eq!(renderable.material, "materials/glow.toml");
}

#[test]
fn a_three_dimensional_boolean_dresses_its_result() {
    let (_dir, mut app) = app();
    let root = app.engine.root();
    let owner = node(&app, "Cut", root);
    add(
        &app,
        owner,
        "boolean3d",
        "operation = \"union\"\ncolor = [0.0, 1.0, 0.0, 1.0]\ntexture = \"rock.png\"\ncast_shadow = false\nlight_layers = 2\nrender_layers = 4",
    );
    let a = node(&app, "A", owner);
    add(&app, a, "shape3d", "kind = \"box\"");
    app.tick(1.0 / 60.0);
    {
        let world = app.engine.world();
        let renderable = world.get::<&Renderable3d>(owner).unwrap();
        assert!(crate::same(renderable.color, [0.0, 1.0, 0.0, 1.0]));
        assert_eq!(renderable.texture, "rock.png");
        assert!(!renderable.shadows);
        assert_eq!((renderable.layers, renderable.render_layers), (2, 4));
    }
    let tint: toml::Value = toml::from_str("color = [0.0, 0.0, 1.0, 1.0]").unwrap();
    components::patch(&app.engine, owner, "boolean3d", &tint).unwrap();
    let world = app.engine.world();
    let renderable = world.get::<&Renderable3d>(owner).unwrap();
    assert!(
        crate::same(renderable.color, [0.0, 0.0, 1.0, 1.0]),
        "a patch re-tints at once"
    );
    assert_eq!(renderable.texture, "rock.png", "and keeps the rest");
}
