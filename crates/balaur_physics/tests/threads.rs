//! The threads the simulation runs on, and a resting control beside them.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{components, App, AppConfig};
use balaur_physics::{PhysicsPlugin, PhysicsState};

fn app() -> App {
    let mut app = App::new(AppConfig::bare("."))
    .unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

/// A ball resting on a floor, both `x` units from the origin.
fn resting_ball_at(app: &App, x: f32) -> Entity {
    let root = app.engine.root();
    let ground = scene::spawn_node(&mut app.engine.world_mut(), "Ground", root);
    {
        let world = app.engine.world();
        let mut transform = world.get::<&mut Transform>(ground).unwrap();
        transform.position.x = x;
        transform.position.y = -1.0;
    }
    components::add(
        &app.engine,
        ground,
        "collider3d",
        Some(&toml::from_str("kind = \"cuboid\"\nhalf_extents = [50.0, 0.5, 50.0]").unwrap()),
    )
    .unwrap();

    let ball = scene::spawn_node(&mut app.engine.world_mut(), "Ball", root);
    {
        let world = app.engine.world();
        world.get::<&mut Transform>(ball).unwrap().position.x = x;
    }
    components::add(
        &app.engine,
        ball,
        "body3d",
        Some(&toml::from_str("kind = \"dynamic\"\ncan_sleep = false").unwrap()),
    )
    .unwrap();
    components::add(
        &app.engine,
        ball,
        "collider3d",
        Some(&toml::from_str("kind = \"ball\"\nradius = 0.5").unwrap()),
    )
    .unwrap();
    ball
}

/// How fast the ball is still moving after it should have settled.
fn residual_speed(x: f32) -> f32 {
    let mut app = app();
    let ball = resting_ball_at(&app, x);
    for _ in 0..240 {
        app.tick(1.0 / 60.0);
    }
    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    let velocity = state.world.bodies[state.bodies[&ball]].linvel();
    velocity.length()
}

/// A body on a floor settles rather than creeping.
#[test]
fn a_body_rests_near_the_origin() {
    assert!(
        residual_speed(0.0) < 0.01,
        "a resting ball at the origin still moves at {}",
        residual_speed(0.0)
    );
}

/// The claim a threaded solver rests on: the thread count does not change
/// results. Rapier's solver is coloured and staged, so this holds it to that
/// rather than trusting it.
#[test]
fn the_thread_count_does_not_change_the_simulation() {
    use balaur_core::digest;

    let digest_at = |threads: usize| {
        let mut app = app();
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global();
        resting_ball_at(&app, 0.0);
        for _ in 0..120 {
            app.tick(1.0 / 60.0);
        }
        digest::digest(&app.engine).0
    };
    // The pool is global and set once per process, so the comparison is
    // between whatever it was and what one thread gives.
    let a = digest_at(8);
    let b = digest_at(1);
    assert_eq!(a, b, "the simulation depends on the thread count");
}
