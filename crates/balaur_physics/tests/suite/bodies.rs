//! Everything a rigid body carries beyond its kind, in both dimensions.
//!
//! Driven from Rust rather than from a script so the assertions can read the
//! rapier world directly — the same reason `api.rs` gives.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{App, AppConfig, components};
use balaur_physics::{PhysicsPlugin, PhysicsState2d, PhysicsState3d};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
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
    let collider: toml::Value = toml::from_str("kind = \"sphere\"\nradius = 0.5").unwrap();
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
dominance = 7
solver_iterations = 3
lock_translation = ["y"]
lock_rotation = ["x", "z"]
continuous_collision = true
speculative_distance = 0.75
allow_fast_rotation = true
time_to_sleep = 1.5"#,
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
    assert!((f("speculative_distance") - 0.75).abs() < 1e-6);
    assert!((f("time_to_sleep") - 1.5).abs() < 1e-6);
    assert!(b("continuous_collision") && b("allow_fast_rotation") && b("enabled"));
    assert_eq!(flags("lock_translation"), ["y"]);
    assert_eq!(flags("lock_rotation"), ["x", "z"]);
}

/// Sleeping is held off by the thresholds, not by the timer, so the number
/// the author wrote survives being unable to sleep and comes back on.
#[test]
fn sleep_time_survives_a_body_that_cannot_sleep() {
    let app = app();
    let e = body_with(
        &app,
        "Awake",
        "kind = \"dynamic\"\ncan_sleep = false\ntime_to_sleep = 1.5",
    );
    let read = |app: &App| {
        components::get(&app.engine, e, "body3d")
            .and_then(|b| {
                b.get("time_to_sleep")
                    .and_then(balaur_core::components::as_f64)
            })
            .unwrap_or_default()
    };
    assert!(
        (read(&app) - 1.5).abs() < 1e-6,
        "a body that cannot sleep reported {}",
        read(&app)
    );

    let patch: toml::Value = toml::from_str("can_sleep = true").unwrap();
    components::patch(&app.engine, e, "body3d", &patch).unwrap();
    assert!(
        (read(&app) - 1.5).abs() < 1e-6,
        "turning sleeping back on lost the time: {}",
        read(&app)
    );
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
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&e]].linvel().y
    };
    assert!(before < -0.1, "the body should be falling by now");
    let params: toml::Value = toml::from_str("kind = \"dynamic\"\nlinear_damping = 0.1").unwrap();
    components::add(&app.engine, e, "body3d", Some(&params)).unwrap();
    let after = {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&e]].linvel().y
    };
    assert!(
        (after - before).abs() < 1e-6,
        "re-applying body3d changed the velocity from {before} to {after}"
    );
}

#[test]
fn changing_kind_keeps_the_body() {
    let app = app();
    let e = body_with(&app, "Switch", "kind = \"dynamic\"");
    let handle = {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.bodies[&e]
    };
    let params: toml::Value = toml::from_str("kind = \"kinematic_velocity\"").unwrap();
    components::add(&app.engine, e, "body3d", Some(&params)).unwrap();
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    assert_eq!(
        state.bodies[&e], handle,
        "the body was rebuilt, not written"
    );
    assert!(state.world.bodies[handle].is_kinematic());
}

/// Extra mass is extra: a heavier body pushes a lighter one, not the reverse.
#[test]
fn mass_is_the_total_and_zero_sums_the_colliders() {
    let mut app = app();
    let summed = body_with(&app, "Summed", "kind = \"dynamic\"");
    let light = body_with(&app, "Light", "kind = \"dynamic\"\nmass = 0.1");
    let heavy = body_with(&app, "Heavy", "kind = \"dynamic\"\nmass = 100.0");
    app.tick(1.0 / 60.0);
    let mass_of = |app: &App, e: Entity| {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&e]].mass()
    };
    let sphere = 4.0 / 3.0 * std::f32::consts::PI * 0.125;
    assert!(
        (mass_of(&app, summed) - sphere).abs() < 1e-3,
        "{}",
        mass_of(&app, summed)
    );
    assert!(
        (mass_of(&app, light) - 0.1).abs() < 1e-5,
        "{}",
        mass_of(&app, light)
    );
    assert!(
        (mass_of(&app, heavy) - 100.0).abs() < 1e-3,
        "{}",
        mass_of(&app, heavy)
    );
    let collider = components::get(&app.engine, heavy, "collider3d").unwrap();
    assert_eq!(
        collider
            .get("density")
            .and_then(balaur_core::components::as_f64),
        Some(1.0),
        "the collider still reports the density it was given"
    );
    components::patch(
        &app.engine,
        heavy,
        "body3d",
        &toml::from_str("mass = 0.0").unwrap(),
    )
    .unwrap();
    app.tick(1.0 / 60.0);
    assert!(
        (mass_of(&app, heavy) - sphere).abs() < 1e-3,
        "clearing mass left the colliders weightless: {}",
        mass_of(&app, heavy)
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
        Some(&toml::from_str("kind = \"box\"\nsize = [20.0, 1.0, 20.0]").unwrap()),
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
    let state = app.engine.resource::<PhysicsState3d>();
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

/// `mass`, `inertia` and `center_of_mass` are authored and were reported by
/// neither dimension, so an inspector save dropped them.
#[test]
fn a_body_reports_the_mass_properties_it_was_given() {
    let app = app();
    let e = body_with(
        &app,
        "Heavy",
        r#"kind = "dynamic"
mass = 5.0
inertia = [1.0, 2.0, 3.0]
center_of_mass = [0.0, 0.5, 0.0]
gyroscopic_forces = true"#,
    );
    let back = components::get(&app.engine, e, "body3d").expect("body3d reports itself");
    let f = |key: &str| {
        back.get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or_default()
    };
    assert!(
        (f("mass") - 5.0).abs() < 1e-5,
        "mass came back as {}",
        f("mass")
    );
    assert_eq!(back.get("gyroscopic_forces").unwrap().as_bool(), Some(true));
    let inertia = back.get("inertia").unwrap().as_array().unwrap();
    assert!((inertia[1].as_float().unwrap() - 2.0).abs() < 1e-5);
    let com = back.get("center_of_mass").unwrap().as_array().unwrap();
    assert!((com[1].as_float().unwrap() - 0.5).abs() < 1e-5);
}

/// The 2D twin. `inertia` is one number there, and there is no `gyroscopic`:
/// rapier2d gates it behind its 3D build.
#[test]
fn a_2d_body_reports_the_mass_properties_it_was_given() {
    let app = app();
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), "Heavy", root);
    let params: toml::Value = toml::from_str(
        r#"kind = "dynamic"
mass = 4.0
inertia = 2.5
center_of_mass = [0.25, 0.0]"#,
    )
    .unwrap();
    components::add(&app.engine, e, "body2d", Some(&params)).unwrap();
    let back = components::get(&app.engine, e, "body2d").expect("body2d reports itself");
    let f = |key: &str| {
        back.get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or_default()
    };
    assert!(
        (f("mass") - 4.0).abs() < 1e-5,
        "mass came back as {}",
        f("mass")
    );
    assert!((f("inertia") - 2.5).abs() < 1e-5);
    let com = back.get("center_of_mass").unwrap().as_array().unwrap();
    assert!((com[0].as_float().unwrap() - 0.25).abs() < 1e-5);
    assert!(
        back.get("gyroscopic_forces").is_none(),
        "2D cannot apply gyroscopic"
    );
}

#[test]
fn a_2d_body_mass_is_the_total_too() {
    let mut app = app();
    let e = node(&app, "Crate");
    let body: toml::Value = toml::from_str("kind = \"dynamic\"\nmass = 3.0").unwrap();
    components::add(&app.engine, e, "body2d", Some(&body)).unwrap();
    let collider: toml::Value = toml::from_str("kind = \"rectangle\"\nsize = [2.0, 2.0]").unwrap();
    components::add(&app.engine, e, "collider2d", Some(&collider)).unwrap();
    app.tick(1.0 / 60.0);
    let state = app.engine.resource::<PhysicsState2d>();
    let state = state.borrow();
    let mass = state.world.bodies[state.bodies[&e]].mass();
    assert!(
        (mass - 3.0).abs() < 1e-5,
        "a 2x2 box of density 1 under mass = 3 weighs {mass}"
    );
}

/// The principal inertia of a body's whole mass, smallest first.
fn principal_inertia_3d(app: &App, e: Entity) -> [f32; 3] {
    let state = app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    let inertia = state.world.bodies[state.bodies[&e]]
        .mass_properties()
        .local_mprops
        .principal_inertia();
    let mut sorted = [inertia.x, inertia.y, inertia.z];
    sorted.sort_by(f32::total_cmp);
    sorted
}

/// Rapier derives no inertia for stated mass properties, so Balaur fits one.
#[test]
fn a_body_with_a_centre_of_mass_and_no_inertia_still_turns() {
    let app = app();
    let e = node(&app, "Lopsided");
    let body: toml::Value =
        toml::from_str("kind = \"dynamic\"\nmass = 2.0\ncenter_of_mass = [0.0, 0.5, 0.0]").unwrap();
    components::add(&app.engine, e, "body3d", Some(&body)).unwrap();
    let cube: toml::Value = toml::from_str("kind = \"box\"\nsize = [1.0, 1.0, 1.0]").unwrap();
    components::add(&app.engine, e, "collider3d", Some(&cube)).unwrap();
    // A unit cube of mass 2 turns about its centre with 2/6 on every axis; half
    // a unit up, the two axes across the offset gain 2 * 0.5^2.
    let [low, mid, high] = principal_inertia_3d(&app, e);
    let side = 2.0 / 6.0;
    assert!((low - side).abs() < 1e-4, "about y: {low}");
    assert!((mid - (side + 0.5)).abs() < 1e-4, "across: {mid}");
    assert!((high - (side + 0.5)).abs() < 1e-4, "across: {high}");

    let back = components::get(&app.engine, e, "body3d").unwrap();
    let inertia = back.get("inertia").unwrap().as_array().unwrap();
    assert!(
        inertia.iter().all(|n| n.as_float() == Some(0.0)),
        "a derived inertia reads back as the 0 that asked for it: {inertia:?}"
    );
    let com = back.get("center_of_mass").unwrap().as_array().unwrap();
    assert!((com[1].as_float().unwrap() - 0.5).abs() < 1e-5);
}

#[test]
fn a_2d_body_with_a_centre_of_mass_and_no_inertia_still_turns() {
    let app = app();
    let e = node(&app, "Lopsided");
    let body: toml::Value =
        toml::from_str("kind = \"dynamic\"\nmass = 2.0\ncenter_of_mass = [0.5, 0.0]").unwrap();
    components::add(&app.engine, e, "body2d", Some(&body)).unwrap();
    let square: toml::Value = toml::from_str("kind = \"rectangle\"\nsize = [1.0, 1.0]").unwrap();
    components::add(&app.engine, e, "collider2d", Some(&square)).unwrap();
    let inertia = {
        let state = app.engine.resource::<PhysicsState2d>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&e]]
            .mass_properties()
            .local_mprops
            .principal_inertia()
    };
    assert!(
        (inertia - (2.0 / 6.0 + 0.5)).abs() < 1e-4,
        "a unit square of mass 2 half a unit off its centre has inertia {inertia}"
    );
    let back = components::get(&app.engine, e, "body2d").unwrap();
    assert_eq!(
        back.get("inertia").and_then(toml::Value::as_float),
        Some(0.0),
        "a derived inertia reads back as its 0"
    );
}

#[test]
fn can_sleep_reads_back_what_was_authored_whatever_the_world_allows() {
    let app = app();
    let sleeper = body_with(&app, "Sleeper", "kind = \"dynamic\"\ntime_to_sleep = 0.75");
    let awake = body_with(
        &app,
        "Awake",
        "kind = \"dynamic\"\ncan_sleep = false\ntime_to_sleep = 1.5",
    );
    let read = |e: Entity| {
        let back = components::get(&app.engine, e, "body3d").unwrap();
        let can_sleep = back
            .get("can_sleep")
            .and_then(toml::Value::as_bool)
            .unwrap();
        let time = back
            .get("time_to_sleep")
            .and_then(balaur_core::components::as_f64)
            .unwrap();
        (can_sleep, time)
    };
    let held_awake = |e: Entity| {
        let state = app.engine.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.world.bodies[state.bodies[&e]]
            .activation()
            .normalized_linear_threshold
            < 0.0
    };

    balaur_physics::set_sleeping_allowed(&app.engine, false);
    let late = body_with(&app, "Late", "kind = \"dynamic\"");
    assert!(
        read(sleeper).0,
        "world sleep off hid the sleeper's can_sleep"
    );
    assert!(
        read(late).0,
        "a body added while sleep is off reads back false"
    );
    assert!(
        held_awake(sleeper) && held_awake(late),
        "the world switch holds every body awake"
    );

    balaur_physics::set_sleeping_allowed(&app.engine, true);
    assert_eq!(
        read(awake),
        (false, 1.5),
        "the switch wiped the awake body's settings"
    );
    assert!(
        (read(sleeper).1 - 0.75).abs() < 1e-6,
        "the switch wiped time_to_sleep"
    );
    assert!(
        held_awake(awake),
        "can_sleep = false came back able to sleep"
    );
    assert!(
        !held_awake(sleeper) && !held_awake(late),
        "the switch left bodies held awake"
    );
}

#[test]
fn can_sleep_reads_back_what_was_authored_in_2d_too() {
    let app = app();
    let e = node(&app, "Awake");
    let params: toml::Value =
        toml::from_str("kind = \"dynamic\"\ncan_sleep = false\ntime_to_sleep = 1.5").unwrap();
    components::add(&app.engine, e, "body2d", Some(&params)).unwrap();
    let other = node(&app, "Sleeper");
    let params: toml::Value = toml::from_str("kind = \"dynamic\"").unwrap();
    components::add(&app.engine, other, "body2d", Some(&params)).unwrap();
    let can_sleep = |e: Entity| {
        components::get(&app.engine, e, "body2d")
            .and_then(|b| b.get("can_sleep").and_then(toml::Value::as_bool))
            .unwrap()
    };
    balaur_physics::dim2::set_sleeping_allowed(&app.engine, false);
    assert!(can_sleep(other), "world sleep off hid the body's can_sleep");
    balaur_physics::dim2::set_sleeping_allowed(&app.engine, true);
    assert!(
        !can_sleep(e),
        "the switch coming back on overwrote can_sleep = false"
    );
    let time = components::get(&app.engine, e, "body2d")
        .and_then(|b| {
            b.get("time_to_sleep")
                .and_then(balaur_core::components::as_f64)
        })
        .unwrap();
    assert!(
        (time - 1.5).abs() < 1e-6,
        "the switch wiped time_to_sleep: {time}"
    );
}

#[test]
fn a_restore_puts_back_what_the_author_wrote() {
    let app = app();
    let e = body_with(&app, "Awake", "kind = \"dynamic\"\ncan_sleep = false");
    let taken = balaur_core::snapshot::capture(&app.engine);
    let patch: toml::Value = toml::from_str("can_sleep = true").unwrap();
    components::patch(&app.engine, e, "body3d", &patch).unwrap();
    balaur_core::snapshot::restore(&app.engine, &taken);
    let can_sleep = components::get(&app.engine, e, "body3d")
        .and_then(|b| b.get("can_sleep").and_then(toml::Value::as_bool))
        .unwrap();
    assert!(
        !can_sleep,
        "the restored body reads back the patch made after the snapshot"
    );
}
