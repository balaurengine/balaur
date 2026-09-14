//! Drawing between two fixed steps: who opts in, what the renderer sees, and
//! what the tick still answers.

use balaur_core::interpolate;
use balaur_core::scene::{self, GlobalTransform, Transform, spawn_node};
use balaur_core::{App, AppConfig, settings};

/// An app with `[time] interpolate` on, and one node under the root.
fn app() -> (App, hecs::Entity) {
    let app = App::new(AppConfig::bare(".")).unwrap();
    settings::set(&app.engine, "time/interpolate", toml::Value::Boolean(true));
    interpolate::apply_setting(&app.engine);
    let root = app.engine.root();
    let node = spawn_node(&mut app.engine.world_mut(), "Body", root);
    balaur_core::transform::ensure(&app.engine, node);
    (app, node)
}

/// Put the node at `x` and take a fixed step's worth of capture.
fn step_to(app: &App, node: hecs::Entity, x: f32) {
    app.engine
        .world()
        .get::<&mut Transform>(node)
        .unwrap()
        .position
        .x = x;
    interpolate::capture(&app.engine);
}

#[test]
fn nothing_is_drawn_between_steps_until_the_project_asks() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let node = spawn_node(&mut app.engine.world_mut(), "Body", app.engine.root());
    interpolate::enable(&app.engine, node);
    assert!(!interpolate::is_on(&app.engine, node));
}

#[test]
fn a_node_between_two_steps_is_drawn_between_the_poses_they_left() {
    let (app, node) = app();
    interpolate::enable(&app.engine, node);
    step_to(&app, node, 0.0);
    step_to(&app, node, 10.0);

    let root = app.engine.root();
    scene::propagate_transforms_at(&mut app.engine.world_mut(), root, 0.25);
    let world = app.engine.world();
    let drawn = world.get::<&GlobalTransform>(node).unwrap().position.x;
    assert!(
        (drawn - 2.5).abs() < 1e-4,
        "a quarter of the way from 0 to 10, got {drawn}"
    );
}

#[test]
fn the_tick_still_answers_where_the_step_put_the_node() {
    let (app, node) = app();
    interpolate::enable(&app.engine, node);
    step_to(&app, node, 0.0);
    step_to(&app, node, 10.0);
    scene::propagate_transforms_at(&mut app.engine.world_mut(), app.engine.root(), 0.5);

    let world = app.engine.world();
    assert!(
        (world.get::<&Transform>(node).unwrap().position.x - 10.0).abs() < 1e-5,
        "a script reads the tick, never the frame it is drawn on"
    );
    assert!(
        (scene::composed_global(&world, node).position.x - 10.0).abs() < 1e-5,
        "and so does the pose composed on demand"
    );
}

#[test]
fn a_reset_stops_a_teleport_streaking_across_the_level() {
    let (app, node) = app();
    interpolate::enable(&app.engine, node);
    step_to(&app, node, 0.0);
    app.engine
        .world()
        .get::<&mut Transform>(node)
        .unwrap()
        .position
        .x = 500.0;
    interpolate::reset(&app.engine, node);

    scene::propagate_transforms_at(&mut app.engine.world_mut(), app.engine.root(), 0.5);
    let world = app.engine.world();
    assert!(
        (world.get::<&GlobalTransform>(node).unwrap().position.x - 500.0).abs() < 1e-5,
        "both kept poses are where the node now is"
    );
}

#[test]
fn the_interpolate_key_outranks_what_the_node_is_made_of() {
    let (app, node) = app();
    interpolate::set(&app.engine, node, Some(false));
    interpolate::enable(&app.engine, node);
    assert!(!interpolate::is_on(&app.engine, node), "off stays off");

    interpolate::set(&app.engine, node, Some(true));
    interpolate::disable(&app.engine, node);
    assert!(interpolate::is_on(&app.engine, node), "and on stays on");
}

#[test]
fn a_node_nobody_opted_in_is_drawn_where_the_frame_left_it() {
    let (app, node) = app();
    app.engine
        .world()
        .get::<&mut Transform>(node)
        .unwrap()
        .position
        .x = 7.0;
    scene::propagate_transforms_at(&mut app.engine.world_mut(), app.engine.root(), 0.0);
    let world = app.engine.world();
    assert!(
        (world.get::<&GlobalTransform>(node).unwrap().position.x - 7.0).abs() < 1e-5,
        "a node moved in update must not be dragged a step behind"
    );
}
