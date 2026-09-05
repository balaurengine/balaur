//! The physics script API, driven from Rust so the assertions can read the
//! simulation directly rather than through a language.

use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{components, App, AppConfig};
use balaur_physics::PhysicsPlugin;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PhysicsPlugin::default()).unwrap();
    app
}

/// A body with a collider: rapier derives mass from colliders, so a body
/// without one has none and gravity does nothing to it.
fn body(app: &App, kind: &str) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), "B", root);
    let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
    components::add(&app.engine, e, "body3d", Some(&params)).unwrap();
    let collider: toml::Value = toml::from_str("kind = \"ball\"\nradius = 0.5").unwrap();
    components::add(&app.engine, e, "collider3d", Some(&collider)).unwrap();
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
        components::add(&app.engine, e, "body3d", Some(&params))
            .unwrap_or_else(|e| panic!("body kind `{kind}` was rejected: {e}"));
    }
}

#[test]
fn every_declared_shape_is_accepted() {
    let app = app();
    for (component, shapes) in [
        ("collider3d", balaur_physics::SHAPE_KINDS),
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
    let err = components::add(&app.engine, e, "body3d", Some(&params.into())).unwrap_err();
    // anyhow's Display shows only the outermost context; {:#} walks the chain.
    assert!(
        format!("{err:#}").contains("levitating"),
        "unhelpful: {err:#}"
    );
}

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
    components::remove(&app.engine, e, "body3d").unwrap();
    let at = app.engine.world().get::<&Transform>(e).unwrap().position.y;
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    assert!(
        (app.engine.world().get::<&Transform>(e).unwrap().position.y - at).abs() < 1e-5,
        "a removed body kept falling"
    );
}

#[test]
fn a_3d_collider_takes_friction_restitution_and_density() {
    let mut app = app();
    let root = app.engine.root();
    let ground = scene::spawn_node(&mut app.engine.world_mut(), "Ground", root);
    let flat: toml::Value = toml::from_str("kind = \"cuboid\"\nhalf_extents = [10.0, 0.5, 10.0]")
        .expect("literal collider params parse");
    components::add(&app.engine, ground, "collider3d", Some(&flat)).unwrap();

    let ball = scene::spawn_node(&mut app.engine.world_mut(), "Ball", root);
    app.engine
        .world()
        .get::<&mut Transform>(ball)
        .unwrap()
        .position
        .y = 2.0;
    let body: toml::Value =
        toml::from_str("kind = \"dynamic\"").expect("literal body params parse");
    components::add(&app.engine, ball, "body3d", Some(&body)).unwrap();
    let bouncy: toml::Value = toml::from_str(
        "kind = \"ball\"\nradius = 0.5\nrestitution = 0.9\nfriction = 0.2\ndensity = 3.0",
    )
    .expect("literal collider params parse");
    components::add(&app.engine, ball, "collider3d", Some(&bouncy)).unwrap();

    // Read back through the live rapier collider, not stored params.
    let got = components::get(&app.engine, ball, "collider3d").unwrap();
    let f = |key: &str| {
        got.get(key)
            .and_then(components::as_f64)
            .unwrap_or_else(|| panic!("collider reports no `{key}`: {got:?}"))
    };
    assert!((f("restitution") - 0.9).abs() < 1e-6);
    assert!((f("friction") - 0.2).abs() < 1e-6);
    assert!((f("density") - 3.0).abs() < 1e-6);

    let mut min_y = f32::MAX;
    let mut rebounded = false;
    for _ in 0..180 {
        app.tick(1.0 / 60.0);
        let y = app
            .engine
            .world()
            .get::<&Transform>(ball)
            .unwrap()
            .position
            .y;
        min_y = min_y.min(y);
        if y > min_y + 0.05 {
            rebounded = true;
        }
    }
    assert!(min_y < 1.9, "the ball never fell");
    assert!(rebounded, "a restitution 0.9 ball did not bounce");
}

#[test]
fn a_sensor_reports_overlap_without_collision_response() {
    let mut app = app();
    let root = app.engine.root();
    let sensor = scene::spawn_node(&mut app.engine.world_mut(), "Sensor", root);
    let gate: toml::Value =
        toml::from_str("kind = \"rect\"\nhalf_extents = [2.0, 0.5]\nsensor = true")
            .expect("literal collider params parse");
    components::add(&app.engine, sensor, "collider2d", Some(&gate)).unwrap();

    let ball = scene::spawn_node(&mut app.engine.world_mut(), "Ball", root);
    app.engine
        .world()
        .get::<&mut Transform>(ball)
        .unwrap()
        .position
        .y = 3.0;
    let body: toml::Value =
        toml::from_str("kind = \"dynamic\"").expect("literal body params parse");
    components::add(&app.engine, ball, "body2d", Some(&body)).unwrap();
    let shape: toml::Value =
        toml::from_str("kind = \"circle\"\nradius = 0.5").expect("literal collider params parse");
    components::add(&app.engine, ball, "collider2d", Some(&shape)).unwrap();

    let mut seen_from_ball = false;
    let mut seen_from_sensor = false;
    for _ in 0..240 {
        app.tick(1.0 / 60.0);
        seen_from_ball |= balaur_physics::dim2::overlaps(&app.engine, ball).contains(&sensor);
        seen_from_sensor |= balaur_physics::dim2::overlaps(&app.engine, sensor).contains(&ball);
    }
    let final_y = app
        .engine
        .world()
        .get::<&Transform>(ball)
        .unwrap()
        .position
        .y;
    assert!(
        final_y < -1.0,
        "the ball did not pass through the sensor: {final_y}"
    );
    assert!(seen_from_ball, "overlaps(ball) never reported the sensor");
    assert!(seen_from_sensor, "overlaps(sensor) never reported the ball");
}

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

fn node_at(app: &App) -> Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

/// Every parametric collider kind the schema offers has to build. rapier
/// measures capsules, cylinders and cones from the centre while the schema
/// states the whole straight part, so a wrong conversion halves the shape
/// silently — applying without error is the floor, not the ceiling.
#[test]
fn every_parametric_collider_kind_applies() {
    for source in [
        "kind = \"ball\"\nradius = 0.5",
        "kind = \"cuboid\"",
        "kind = \"capsule\"\nradius = 0.4\nheight = 2.0",
        "kind = \"cylinder\"\nradius = 0.4\nheight = 2.0",
        "kind = \"cone\"\nradius = 0.4\nheight = 2.0",
        "kind = \"triangle\"",
    ] {
        let app = app();
        let e = node_at(&app);
        let params: toml::Value = toml::from_str(source).unwrap();
        components::add(&app.engine, e, "collider3d", Some(&params))
            .unwrap_or_else(|why| panic!("{source} did not apply: {why:#}"));
    }
}

#[test]
fn a_2d_capsule_collider_applies() {
    let app = app();
    let e = node_at(&app);
    let params: toml::Value =
        toml::from_str("kind = \"capsule\"\nradius = 0.4\nheight = 2.0").unwrap();
    components::add(&app.engine, e, "collider2d", Some(&params)).unwrap();
}

/// A mesh-backed collider without its asset says which asset it wanted,
/// rather than failing somewhere later with no name attached.
#[test]
fn a_mesh_collider_without_its_asset_says_so() {
    for kind in ["trimesh", "convex_hull", "polyline"] {
        let app = app();
        let e = node_at(&app);
        let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
        let err = format!(
            "{:#}",
            components::add(&app.engine, e, "collider3d", Some(&params))
                .expect_err("a mesh collider with no mesh must not apply")
        );
        assert!(
            err.contains("mesh"),
            "{kind} should name the missing asset: {err}"
        );
    }
}

#[test]
fn a_heightfield_collider_without_its_asset_says_so() {
    let app = app();
    let e = node_at(&app);
    let params: toml::Value = toml::from_str("kind = \"heightfield\"").unwrap();
    let err = format!(
        "{:#}",
        components::add(&app.engine, e, "collider3d", Some(&params))
            .expect_err("a heightfield with no grid must not apply")
    );
    assert!(err.contains("heightfield"), "{err}");
}

#[test]
fn an_unknown_collider_kind_is_refused_by_name() {
    let app = app();
    let e = node_at(&app);
    let params: toml::Value = toml::from_str("kind = \"blancmange\"").unwrap();
    let err = format!(
        "{:#}",
        components::add(&app.engine, e, "collider3d", Some(&params))
            .expect_err("an unknown kind must not apply")
    );
    assert!(err.contains("blancmange"), "{err}");
}

/// A body with a collider under its own name, so a joint can point at it.
fn named_body(app: &App, name: &str, kind: &str) -> Entity {
    let root = app.engine.root();
    let e = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    let params: toml::Value = toml::from_str(&format!("kind = \"{kind}\"")).unwrap();
    components::add(&app.engine, e, "body3d", Some(&params)).unwrap();
    let collider: toml::Value = toml::from_str("kind = \"ball\"\nradius = 0.5").unwrap();
    components::add(&app.engine, e, "collider3d", Some(&collider)).unwrap();
    e
}

fn joint_count(app: &App) -> usize {
    let state = app.engine.resource::<balaur_physics::PhysicsState>();
    let n = state.borrow().joints.len();
    n
}

/// Rapier drops a joint with either end's body, so the map must drop the
/// handle too: otherwise the joint silently disappears and never comes back.
#[test]
fn a_joint_is_remade_when_the_body_it_lost_returns() {
    let mut app = app();
    let anchor = named_body(&app, "Anchor", "static");
    let hanging = named_body(&app, "Hanging", "dynamic");
    components::add(
        &app.engine,
        hanging,
        "joint3d",
        Some(&toml::from_str("kind = \"revolute\"\nbody = \"/Anchor\"").unwrap()),
    )
    .unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(joint_count(&app), 1, "the joint was never made");

    components::remove(&app.engine, anchor, "body3d").unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(joint_count(&app), 0, "the handle outlived the body");

    components::add(
        &app.engine,
        anchor,
        "body3d",
        Some(&toml::from_str("kind = \"static\"").unwrap()),
    )
    .unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(joint_count(&app), 1, "the authored joint never came back");
}

/// A joint switched off has params and no handle for ever, so the retry list
/// would hold it and re-apply the whole component every step.
#[test]
fn a_disabled_joint_is_not_retried_every_step() {
    let mut app = app();
    let _anchor = named_body(&app, "Anchor", "static");
    let hanging = named_body(&app, "Hanging", "dynamic");
    components::add(
        &app.engine,
        hanging,
        "joint3d",
        Some(&toml::from_str("kind = \"revolute\"\nbody = \"/Anchor\"\nenabled = false").unwrap()),
    )
    .unwrap();
    for _ in 0..5 {
        app.tick(1.0 / 60.0);
    }
    let state = app.engine.resource::<balaur_physics::PhysicsState>();
    let state = state.borrow();
    assert!(state.joints.is_empty(), "a disabled joint was made anyway");
    assert!(
        balaur_physics::joint::pending(&state).is_empty(),
        "a disabled joint is on the retry list, so it re-applies every step"
    );
}
