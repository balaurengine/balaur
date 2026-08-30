//! The physics script API, driven from Rust so the assertions can read the
//! simulation directly rather than through a language.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{components, App, AppConfig};
use balaur_physics::PhysicsPlugin;

fn app() -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.add_plugin(PhysicsPlugin).unwrap();
    app
}

/// A body with a collider: rapier derives mass from colliders, so a body
/// without one has none and gravity does nothing to it.
fn body(app: &App, kind: &str) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), "B", root);
    let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
    components::add(&app.engine, e, "body", Some(&params)).unwrap();
    let collider: toml::Value = toml::from_str("kind = \"ball\"\nradius = 0.5").unwrap();
    components::add(&app.engine, e, "collider", Some(&collider)).unwrap();
    e
}

#[test]
fn a_dynamic_body_falls_and_a_static_one_does_not() {
    let mut app = app();
    let falling = body(&app, "dynamic");
    let held = body(&app, "static");
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    let world = app.engine.world();
    assert!(
        world.get::<&Transform>(falling).unwrap().position.y < -0.01,
        "dynamic body did not fall"
    );
    assert!(
        world.get::<&Transform>(held).unwrap().position.y.abs() < 1e-4,
        "static body moved"
    );
}

#[test]
fn every_declared_body_kind_is_accepted() {
    let app = app();
    for (_, kind) in balaur_physics::BODY_KINDS {
        let root = app.engine.root();
        let e = scene::spawn_node(&mut app.engine.world_mut(), kind, root);
        let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
        components::add(&app.engine, e, "body", Some(&params))
            .unwrap_or_else(|e| panic!("body kind `{kind}` was rejected: {e}"));
    }
}

#[test]
fn every_declared_shape_is_accepted() {
    let app = app();
    for (component, shapes) in [
        ("collider", balaur_physics::SHAPE_KINDS),
        ("collider2d", balaur_physics::SHAPE_KINDS_2D),
    ] {
        for (_, shape) in shapes {
            let root = app.engine.root();
            let e = scene::spawn_node(&mut app.engine.world_mut(), shape, root);
            let params: toml::Value = toml::from_str(&format!("kind = \"{shape}\"")).unwrap();
            components::add(&app.engine, e, component, Some(&params))
                .unwrap_or_else(|e| panic!("{component} shape `{shape}` was rejected: {e}"));
        }
    }
}

#[test]
fn an_unknown_body_kind_is_rejected_with_its_name() {
    let app = app();
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), "B", root);
    let params = toml::toml! { kind = "levitating" };
    let err = components::add(&app.engine, e, "body", Some(&params.into())).unwrap_err();
    // anyhow's Display shows only the outermost context; {:#} walks the chain.
    assert!(
        format!("{err:#}").contains("levitating"),
        "unhelpful: {err:#}"
    );
}

/// Pausing must stop the simulation without dropping the bodies, or a paused
/// editor loses the scene.
#[test]
fn pausing_stops_the_simulation_and_resuming_continues_it() {
    let mut app = app();
    let e = body(&app, "dynamic");
    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    let moved = app.engine.world().get::<&Transform>(e).unwrap().position.y;
    assert!(moved < 0.0);

    app.engine
        .resource::<balaur_physics::PhysicsState>()
        .borrow_mut()
        .paused = true;
    assert!(
        app.engine
            .resource::<balaur_physics::PhysicsState>()
            .borrow()
            .paused
    );
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    let paused_at = app.engine.world().get::<&Transform>(e).unwrap().position.y;
    assert!(
        (paused_at - moved).abs() < 1e-5,
        "the body moved while paused"
    );

    app.engine
        .resource::<balaur_physics::PhysicsState>()
        .borrow_mut()
        .paused = false;
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    assert!(
        app.engine.world().get::<&Transform>(e).unwrap().position.y < paused_at - 1e-4,
        "the body did not resume"
    );
}

#[test]
fn removing_a_body_stops_it_being_simulated() {
    let mut app = app();
    let e = body(&app, "dynamic");
    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    components::remove(&app.engine, e, "body").unwrap();
    let at = app.engine.world().get::<&Transform>(e).unwrap().position.y;
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    assert!(
        (app.engine.world().get::<&Transform>(e).unwrap().position.y - at).abs() < 1e-5,
        "a removed body kept falling"
    );
}

/// Two runs from the same setup must agree bit for bit; the whole engine is
/// built on that promise.
#[test]
fn the_same_setup_simulates_identically_twice() {
    let run = || {
        let mut app = app();
        let e = body(&app, "dynamic");
        for _ in 0..60 {
            app.tick(1.0 / 60.0);
        }
        let p = app.engine.world().get::<&Transform>(e).unwrap().position;
        (p.x.to_bits(), p.y.to_bits(), p.z.to_bits())
    };
    assert_eq!(run(), run());
}
