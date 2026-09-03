//! Phase 2 of `docs/PLAN-rapier.md`: everything a rigid body carries beyond
//! its kind, in both dimensions.
//!
//! Driven from Rust rather than from a script so the assertions can read the
//! rapier world directly — the same reason `api.rs` gives.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{components, App, AppConfig};
use balaur_physics::{PhysicsPlugin, PhysicsState, PhysicsState2d};

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

fn node(app: &App, name: &str) -> Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), name, root)
}

fn body_at(app: &App, name: &str, x: f32, params: &str) -> Entity {
    let e = node(app, name);
    {
        let world = app.engine.world();
        world.get::<&mut Transform>(e).unwrap().position.x = x;
    }
    with_body_params(app, e, params)
}

fn body_with(app: &App, name: &str, params: &str) -> Entity {
    let e = node(app, name);
    with_body_params(app, e, params)
}

/// A body's pose is read from the node when the body is made, so a test that
/// wants one somewhere else places the node first.
fn with_body_params(app: &App, e: Entity, params: &str) -> Entity {
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, e, "body3d", Some(&params)).unwrap();
    let collider: toml::Value = toml::from_str("kind = \"ball\"\nradius = 0.5").unwrap();
    components::add(&app.engine, e, "collider3d", Some(&collider)).unwrap();
    e
}

/// Every property the body schema declares, written and read back.
#[test]
fn body_properties_round_trip() {
    let app = app();
    let e = body_with(
        &app,
        "Tuned",
        r#"kind = "dynamic"
linear_damping = 0.25
angular_damping = 0.5
gravity_scale = 2.0
dominance = 7.0
solver_iterations = 3.0
lock_translation = ["y"]
lock_rotation = ["x", "z"]
ccd = true
soft_ccd = 0.75
fast_rotation = true
sleep_time = 1.5"#,
    );
    let back = components::get(&app.engine, e, "body3d").expect("body3d reports itself");
    let f = |key: &str| {
        back.get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or_default()
    };
    let b = |key: &str| back.get(key).and_then(toml::Value::as_bool).unwrap();
    let flags = |key: &str| {
        balaur_core::components::as_flags(back.get(key))
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert!((f("linear_damping") - 0.25).abs() < 1e-6);
    assert!((f("angular_damping") - 0.5).abs() < 1e-6);
    assert!((f("gravity_scale") - 2.0).abs() < 1e-6);
    assert!((f("dominance") - 7.0).abs() < 1e-6);
    assert!((f("solver_iterations") - 3.0).abs() < 1e-6);
    assert!((f("soft_ccd") - 0.75).abs() < 1e-6);
    assert!((f("sleep_time") - 1.5).abs() < 1e-6);
    assert!(b("ccd") && b("fast_rotation") && b("enabled"));
    assert_eq!(flags("lock_translation"), ["y"]);
    assert_eq!(flags("lock_rotation"), ["x", "z"]);
}

/// The point of `lock_*`: a locked axis does not move, and its neighbours do.
#[test]
fn a_locked_axis_holds_still() {
    let mut app = app();
    let e = body_with(
        &app,
        "Held",
        "kind = \"dynamic\"\nlock_translation = [\"y\"]",
    );
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    let world = app.engine.world();
    let position = world.get::<&Transform>(e).unwrap().position;
    assert!(
        position.y.abs() < 1e-5,
        "a body with y locked fell to {}",
        position.y
    );
}

/// `gravity_scale = 0` is the floating platform, and it must not need a
/// static body to hold still.
#[test]
fn gravity_scale_zero_hangs_in_the_air() {
    let mut app = app();
    let e = body_with(&app, "Platform", "kind = \"dynamic\"\ngravity_scale = 0.0");
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    let world = app.engine.world();
    assert!(world.get::<&Transform>(e).unwrap().position.y.abs() < 1e-5);
}

/// Applying the component again must not throw the body's velocity away —
/// which is what rebuilding it used to do, and why `write_body` exists.
#[test]
fn re_applying_a_body_keeps_its_velocity() {
    let mut app = app();
    let e = body_with(&app, "Moving", "kind = \"dynamic\"");
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    let before = {
        let state = app.engine.resource::<PhysicsState>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&e]].linvel().y
    };
    assert!(before < -0.1, "the body should be falling by now");
    let params: toml::Value = toml::from_str("kind = \"dynamic\"\nlinear_damping = 0.1").unwrap();
    components::add(&app.engine, e, "body3d", Some(&params)).unwrap();
    let after = {
        let state = app.engine.resource::<PhysicsState>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&e]].linvel().y
    };
    assert!(
        (after - before).abs() < 1e-6,
        "re-applying body3d changed the velocity from {before} to {after}"
    );
}

/// Changing kind in place is the same promise, one property over.
#[test]
fn changing_kind_keeps_the_body() {
    let app = app();
    let e = body_with(&app, "Switch", "kind = \"dynamic\"");
    let handle = {
        let state = app.engine.resource::<PhysicsState>();
        let state = state.borrow();
        state.bodies[&e]
    };
    let params: toml::Value = toml::from_str("kind = \"kinematic_velocity\"").unwrap();
    components::add(&app.engine, e, "body3d", Some(&params)).unwrap();
    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    assert_eq!(
        state.bodies[&e], handle,
        "the body was rebuilt, not written"
    );
    assert!(state.world.bodies[handle].is_kinematic());
}

/// Extra mass is extra: a heavier body pushes a lighter one, not the reverse.
#[test]
fn mass_is_additional() {
    let app = app();
    let light = body_with(&app, "Light", "kind = \"dynamic\"");
    let heavy = body_with(&app, "Heavy", "kind = \"dynamic\"\nmass = 100.0");
    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    let mass_of = |e: Entity| state.world.bodies[state.bodies[&e]].mass();
    assert!(
        mass_of(heavy) > mass_of(light) + 99.0,
        "mass = 100 added {}",
        mass_of(heavy) - mass_of(light)
    );
}

/// A body that cannot sleep keeps being simulated, which is what a networked
/// game and a rollback ring both depend on.
#[test]
fn can_sleep_false_keeps_a_body_awake() {
    let mut app = app();
    // Ground to come to rest on: rapier sleeps a body that has held still,
    // and a body falling forever never holds still.
    let ground = node(&app, "Ground");
    {
        let world = app.engine.world();
        world.get::<&mut Transform>(ground).unwrap().position.y = -2.0;
    }
    components::add(
        &app.engine,
        ground,
        "collider3d",
        Some(&toml::from_str("kind = \"cuboid\"\nhalf_extents = [10.0, 0.5, 10.0]").unwrap()),
    )
    .unwrap();
    // Apart, because sleeping is decided per island: two bodies that touch
    // sleep or stay awake together, and the one that cannot sleep would hold
    // the other one up.
    let sleeper = body_at(&app, "Sleeper", 0.0, "kind = \"dynamic\"");
    let awake = body_at(&app, "Awake", 5.0, "kind = \"dynamic\"\ncan_sleep = false");
    for _ in 0..400 {
        app.tick(1.0 / 60.0);
    }
    let state = app.engine.resource::<PhysicsState>();
    let state = state.borrow();
    assert!(
        state.world.bodies[state.bodies[&sleeper]].is_sleeping(),
        "a body resting on the ground never slept"
    );
    assert!(
        !state.world.bodies[state.bodies[&awake]].is_sleeping(),
        "can_sleep = false slept anyway"
    );
}

/// 2D carries the same properties, spelled for two dimensions.
#[test]
fn the_2d_body_carries_the_same_properties() {
    let app = app();
    let e = node(&app, "Flat");
    let params: toml::Value = toml::from_str(
        r#"kind = "dynamic"
linear_damping = 0.25
gravity_scale = 3.0
lock_translation = ["x"]
lock_rotation = true
inertia = 2.0
mass = 5.0"#,
    )
    .unwrap();
    components::add(&app.engine, e, "body2d", Some(&params)).unwrap();
    let back = components::get(&app.engine, e, "body2d").expect("body2d reports itself");
    assert_eq!(
        balaur_core::components::as_flags(back.get("lock_translation")),
        ["x"]
    );
    assert_eq!(back.get("lock_rotation").unwrap().as_bool(), Some(true));
    assert!(
        (back
            .get("gravity_scale")
            .and_then(balaur_core::components::as_f64)
            .unwrap()
            - 3.0)
            .abs()
            < 1e-6
    );
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    assert!(state.world.bodies[state.bodies[&e]].mass() >= 5.0);
}
