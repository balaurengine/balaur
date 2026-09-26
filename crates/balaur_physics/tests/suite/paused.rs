//! A paused game holds every body, and a moving body draws between steps.

use balaur_core::hecs::Entity;
use balaur_core::interpolate;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, AppConfig, components, fixed_dt, settings};
use balaur_physics::PhysicsPlugin;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

fn falling_body(app: &App, name: &str, kind: &str) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
    components::add(&app.engine, e, "body3d", Some(&params)).unwrap();
    let collider: toml::Value = toml::from_str("kind = \"sphere\"\nradius = 0.5").unwrap();
    components::add(&app.engine, e, "collider3d", Some(&collider)).unwrap();
    e
}

fn height(app: &App, e: Entity) -> f32 {
    app.engine.world().get::<&Transform>(e).unwrap().position.y
}

#[test]
fn a_paused_game_holds_every_body_even_under_an_always_node() {
    let mut app = app();
    let ball = falling_body(&app, "Ball", "dynamic");
    balaur_core::process::set(
        &mut app.engine.world_mut(),
        ball,
        balaur_core::process::ProcessMode::Always,
    );

    app.engine.set_paused(true);
    app.tick(fixed_dt() * 10.0);
    assert!(
        height(&app, ball).abs() < f32::EPSILON,
        "physics is one world; a body that kept falling inside a held one \
         would collide with a body that did not"
    );

    app.engine.set_paused(false);
    app.tick(fixed_dt() * 10.0);
    assert!(height(&app, ball) < 0.0, "and it falls again");
}

#[test]
fn a_paused_game_holds_a_2d_body_too() {
    let mut app = app();
    let root = app.engine.root();
    let ball = scene::spawn_node(&mut app.engine.world_mut(), "Ball2d", root);
    let params: toml::Value = toml::from_str("kind = \"dynamic\"").unwrap();
    components::add(&app.engine, ball, "body2d", Some(&params)).unwrap();
    let collider: toml::Value = toml::from_str("kind = \"circle\"\nradius = 0.5").unwrap();
    components::add(&app.engine, ball, "collider2d", Some(&collider)).unwrap();

    app.engine.set_paused(true);
    app.tick(fixed_dt() * 10.0);
    assert!(height(&app, ball).abs() < f32::EPSILON);

    app.engine.set_paused(false);
    app.tick(fixed_dt() * 10.0);
    assert!(height(&app, ball) < 0.0);
}

#[test]
fn a_moving_body_asks_to_be_drawn_between_steps_and_a_static_one_does_not() {
    let app = app();
    settings::set(&app.engine, "time/interpolate", toml::Value::Boolean(true));
    interpolate::apply_setting(&app.engine);

    let ball = falling_body(&app, "Ball", "dynamic");
    let mover = falling_body(&app, "Platform", "kinematic");
    let ground = falling_body(&app, "Ground", "static");

    assert!(interpolate::is_on(&app.engine, ball));
    assert!(interpolate::is_on(&app.engine, mover));
    assert!(
        !interpolate::is_on(&app.engine, ground),
        "a body that never moves has nothing to blend"
    );
}

#[test]
fn a_teleport_starts_the_blend_again_where_the_body_now_is() {
    let mut app = app();
    settings::set(&app.engine, "time/interpolate", toml::Value::Boolean(true));
    interpolate::apply_setting(&app.engine);
    let ball = falling_body(&app, "Ball", "dynamic");

    app.tick(fixed_dt() * 4.0);
    {
        let world = app.engine.world();
        let kept = world.get::<&interpolate::Interpolation>(ball).unwrap();
        assert!(
            (kept.previous.position.y - kept.current.position.y).abs() > 1e-5,
            "a falling body has two poses to blend"
        );
    }

    app.engine
        .world()
        .get::<&mut Transform>(ball)
        .unwrap()
        .position
        .y = 100.0;
    interpolate::reset(&app.engine, ball);
    let world = app.engine.world();
    let kept = world.get::<&interpolate::Interpolation>(ball).unwrap();
    assert!((kept.previous.position.y - 100.0).abs() < 1e-5);
    assert!((kept.current.position.y - 100.0).abs() < 1e-5);
}
