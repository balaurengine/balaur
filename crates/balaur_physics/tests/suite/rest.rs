//! What the physics worlds tell a loop about to sleep: pending while a body
//! moves under the step, at rest once every one of them sleeps or is held.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, AppConfig, components, fixed_dt, rest};
use balaur_physics::{PhysicsPlugin, PhysicsState2d, PhysicsState3d};

/// The name the plugin's probe answers to.
const PHYSICS: &str = "physics";

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin).unwrap();
    app
}

fn pends(app: &App) -> bool {
    rest::pending_names(&app.engine).contains(&PHYSICS)
}

/// A node at height `y` carrying `component` and its collider, each from TOML.
fn node_with(app: &App, name: &str, y: f32, parts: &[(&str, &str)]) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    app.engine
        .world()
        .get::<&mut Transform>(e)
        .unwrap()
        .position
        .y = y;
    for (component, params) in parts {
        let params: toml::Value = toml::from_str(params).unwrap();
        components::add(&app.engine, e, component, Some(&params)).unwrap();
    }
    e
}

const BALL_3D: &str = "shape = { kind = \"sphere\", radius = 0.5 }";
const FLOOR_3D: &str = "shape = { kind = \"box\", size = [20.0, 1.0, 20.0] }";
const BALL_2D: &str = "shape = { kind = \"circle\", radius = 0.5 }";
const FLOOR_2D: &str = "shape = { kind = \"rectangle\", size = [20.0, 1.0] }";
const DYNAMIC: &str = "kind = \"dynamic\"";
const KINEMATIC: &str = "kind = \"kinematic\"";

/// Tick until the probe answers at rest, and say how many steps that took.
fn steps_until_rest(app: &mut App, most: u32) -> Option<u32> {
    (1..=most).find(|_| {
        app.tick(fixed_dt());
        !pends(app)
    })
}

#[test]
fn a_falling_body_is_pending_until_it_lands_and_falls_asleep() {
    let mut app = app();
    node_with(&app, "Floor", -2.0, &[("collider3d", FLOOR_3D)]);
    let ball = node_with(
        &app,
        "Ball",
        0.0,
        &[("body3d", DYNAMIC), ("collider3d", BALL_3D)],
    );
    assert!(!pends(&app), "no step has counted the body yet");

    app.tick(fixed_dt());
    assert!(pends(&app), "it is falling");

    let slept = steps_until_rest(&mut app, 900).expect("a ball on a floor never slept");
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    assert!(state.world.bodies[state.bodies[&ball]].is_sleeping());
    assert!(
        slept > 30,
        "at rest while it was still in the air: {slept} steps"
    );
}

#[test]
fn a_falling_2d_body_is_pending_until_it_lands_and_falls_asleep() {
    let mut app = app();
    node_with(&app, "Floor", -2.0, &[("collider2d", FLOOR_2D)]);
    let ball = node_with(
        &app,
        "Ball",
        0.0,
        &[("body2d", DYNAMIC), ("collider2d", BALL_2D)],
    );

    app.tick(fixed_dt());
    assert!(pends(&app), "it is falling");

    steps_until_rest(&mut app, 900).expect("a ball on a floor never slept");
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    assert!(state.world.bodies[state.bodies[&ball]].is_sleeping());
}

#[test]
fn a_world_of_static_colliders_is_at_rest() {
    let mut app = app();
    node_with(&app, "Floor", -2.0, &[("collider3d", FLOOR_3D)]);
    node_with(
        &app,
        "Wall",
        0.0,
        &[("body3d", "kind = \"static\""), ("collider3d", BALL_3D)],
    );
    node_with(&app, "Floor2d", -2.0, &[("collider2d", FLOOR_2D)]);

    app.tick(fixed_dt());
    assert!(!pends(&app));
}

#[test]
fn a_kinematic_body_is_pending_while_it_is_moved_and_at_rest_once_it_stops() {
    let mut app = app();
    let lift = node_with(
        &app,
        "Lift",
        0.0,
        &[("body3d", KINEMATIC), ("collider3d", BALL_3D)],
    );
    app.tick(fixed_dt());
    assert!(!pends(&app), "a platform nobody moves is at rest");

    for _ in 0..3 {
        app.engine
            .world()
            .get::<&mut Transform>(lift)
            .unwrap()
            .position
            .y += 0.1;
        app.tick(fixed_dt());
        assert!(pends(&app), "it is being driven");
    }

    app.tick(fixed_dt());
    assert!(!pends(&app), "the step after its last move finds it still");
}

#[test]
fn a_paused_game_and_a_paused_world_hold_a_falling_body_at_rest() {
    let mut app = app();
    node_with(
        &app,
        "Ball",
        0.0,
        &[("body3d", DYNAMIC), ("collider3d", BALL_3D)],
    );
    node_with(
        &app,
        "Ball2d",
        0.0,
        &[("body2d", DYNAMIC), ("collider2d", BALL_2D)],
    );
    app.tick(fixed_dt());
    assert!(pends(&app));

    app.engine.set_paused(true);
    assert!(!pends(&app), "a paused game holds both worlds");
    app.engine.set_paused(false);
    assert!(pends(&app));

    app.engine.resource::<PhysicsState3d>().borrow_mut().paused = true;
    app.engine.resource::<PhysicsState2d>().borrow_mut().paused = true;
    assert!(
        !pends(&app),
        "and so does `physics::set_paused`, as an editor does"
    );
}

#[test]
fn a_soft_body_is_pending_while_it_is_awake_and_at_rest_once_it_is_taken_out() {
    let script = "pub class Scenario { node: Node }\nimpl Scenario { fn init() {} }\n";
    let (_dir, mut app) = crate::soft_bodies::boot(script, 2);
    assert!(pends(&app), "two soft bodies are falling, and no rigid one");

    let off: toml::Value = toml::from_str("enabled = false").unwrap();
    let world = app.engine.world();
    let blobs: Vec<Entity> = ["Blob", "Blob/Blob2d"]
        .iter()
        .filter_map(|path| scene::find_node(&world, app.engine.root(), path))
        .collect();
    drop(world);
    assert_eq!(blobs.len(), 2);
    components::patch(&app.engine, blobs[0], "softbody3d", &off).unwrap();
    components::patch(&app.engine, blobs[1], "softbody2d", &off).unwrap();
    app.tick(fixed_dt());
    assert!(!pends(&app), "a disabled soft body is not simulated");
}
