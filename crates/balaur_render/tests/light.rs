//! `light2d`, `occluder2d` and the camera's `ambient`, without a window.
//!
//! The light map itself needs a GPU; everything it is built from — the world
//! space lights, the outlines that occlude them, and the polygon an edge
//! casts — is resolved here and asserted on headless.

use balaur_core::glamx::{Quat, Vec2, Vec3};
use balaur_core::{components, scene, App, AppConfig, Transform};
use balaur_render::light::{lights, occluder_edges, outline, shadow_quad, LightKind2d, LitLight2d};
use balaur_render::{CameraConfig2d, Occluder2d, RenderPlugin};

fn app() -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.add_plugin(RenderPlugin).unwrap();
    app
}

fn physics_app() -> App {
    let mut app = app();
    app.add_plugin(balaur::PhysicsPlugin).unwrap();
    app
}

fn node(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

fn add(app: &App, entity: balaur_core::hecs::Entity, key: &str, params: &str) {
    let table: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, key, Some(&table)).unwrap();
}

fn place(app: &App, entity: balaur_core::hecs::Entity, position: Vec3, rotation: Quat) {
    let mut world = app.engine.world_mut();
    let mut transform = world.get::<&mut Transform>(entity).unwrap();
    transform.position = position;
    transform.rotation = rotation;
}

#[track_caller]
fn assert_close(actual: Vec2, expected: Vec2) {
    assert!(
        (actual - expected).length() < 1e-5,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn the_plugin_registers_the_lighting_components() {
    let app = app();
    let names = components::names(&app.engine);
    for expected in ["light2d", "occluder2d"] {
        assert!(names.iter().any(|n| n == expected), "{expected} is missing");
    }
}

/// The light is collected at its *global* pose, so a light parented to a
/// moving lantern moves with it.
#[test]
fn a_light_is_collected_where_the_scene_tree_puts_it() {
    let mut app = app();
    let rig = node(&app);
    place(&app, rig, Vec3::new(3.0, 1.0, 0.0), Quat::IDENTITY);
    let lamp = scene::spawn_node(&mut app.engine.world_mut(), "Lamp", rig);
    place(&app, lamp, Vec3::new(0.0, 2.0, 0.0), Quat::IDENTITY);
    add(&app, lamp, "light2d", "radius = 4.0\nintensity = 2.0");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let collected = lights(&world, app.engine.root());
    assert_eq!(collected.len(), 1);
    assert_close(collected[0].position, Vec2::new(3.0, 3.0));
    assert!((collected[0].radius - 4.0).abs() < 1e-6);
    assert!((collected[0].intensity - 2.0).abs() < 1e-6);
}

#[test]
fn a_scene_with_no_light2d_collects_nothing() {
    let mut app = app();
    let bare = node(&app);
    add(&app, bare, "shape2d", "kind = \"rect\"");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    assert!(lights(&world, app.engine.root()).is_empty());
}

/// At rest a directional light shines straight down; the node's z rotation
/// turns it, which is the only thing that aims one.
#[test]
fn a_directional_light_is_aimed_by_the_nodes_rotation() {
    let mut app = app();
    let sun = node(&app);
    place(
        &app,
        sun,
        Vec3::ZERO,
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
    );
    add(&app, sun, "light2d", "kind = \"directional\"");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let collected = lights(&world, app.engine.root());
    assert_eq!(collected[0].kind, LightKind2d::Directional);
    assert_close(collected[0].direction, Vec2::new(1.0, 0.0));
}

/// The plan's default: an occluder with nothing authored takes the outline
/// of the collider the node already has.
#[test]
fn an_occluder_defaults_to_the_nodes_collider_outline() {
    let mut app = physics_app();
    let wall = node(&app);
    add(
        &app,
        wall,
        "collider2d",
        "kind = \"rect\"\nhalf_extents = [2.0, 0.5]",
    );
    add(&app, wall, "occluder2d", "");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let points = world.get::<&Occluder2d>(wall).unwrap().points.clone();
    assert_eq!(points.len(), 4, "a rect traces four corners: {points:?}");
    assert!(points.contains(&Vec2::new(2.0, 0.5)), "{points:?}");
    assert!(points.contains(&Vec2::new(-2.0, -0.5)), "{points:?}");
}

/// A collider added *after* the occluder still fills it in: scene files
/// apply component keys in the order they are written.
#[test]
fn an_occluder_declared_before_its_collider_still_resolves() {
    let mut app = physics_app();
    let wall = node(&app);
    add(&app, wall, "occluder2d", "");
    app.tick(1.0 / 60.0);
    assert!(
        app.engine
            .world()
            .get::<&Occluder2d>(wall)
            .unwrap()
            .points
            .is_empty(),
        "control: nothing to trace yet"
    );
    add(&app, wall, "collider2d", "kind = \"circle\"\nradius = 1.0");
    app.tick(1.0 / 60.0);
    assert_eq!(
        app.engine
            .world()
            .get::<&Occluder2d>(wall)
            .unwrap()
            .points
            .len(),
        16
    );
}

#[test]
fn an_occluder_falls_back_to_the_nodes_2d_shape() {
    let mut app = app();
    let crate_node = node(&app);
    add(
        &app,
        crate_node,
        "shape2d",
        "kind = \"rect\"\nhalf_extents = [1.0, 1.0]",
    );
    add(&app, crate_node, "occluder2d", "");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let points = world.get::<&Occluder2d>(crate_node).unwrap().points.clone();
    assert_eq!(points.len(), 4, "{points:?}");
    assert!(points.contains(&Vec2::new(1.0, 1.0)), "{points:?}");
}

/// A closed outline's last edge joins back to its first point, so a box
/// casts a shadow on every side rather than three.
#[test]
fn a_closed_occluder_edge_list_wraps_around() {
    let mut app = app();
    let crate_node = node(&app);
    place(&app, crate_node, Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY);
    add(
        &app,
        crate_node,
        "shape2d",
        "kind = \"rect\"\nhalf_extents = [1.0, 1.0]",
    );
    add(&app, crate_node, "occluder2d", "");
    app.tick(1.0 / 60.0);
    let edges = {
        let world = app.engine.world();
        occluder_edges(&world, app.engine.root())
    };
    assert_eq!(edges.len(), 4, "{edges:?}");
    // The node's own transform is applied: the outline is in world space.
    for edge in &edges {
        for point in edge {
            assert!((point.x - 5.0).abs() <= 1.0 + 1e-5, "{edges:?}");
        }
    }
    add(&app, crate_node, "occluder2d", "closed = false");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let edges = occluder_edges(&world, app.engine.root());
    assert_eq!(edges.len(), 3, "an open outline is a chain: {edges:?}");
}

/// What the editor's gizmo draws: the outline in world space, closed, with
/// the node's rotation and scale already in it.
#[test]
fn an_outline_comes_back_in_world_space_and_closed() {
    let mut app = app();
    let crate_node = node(&app);
    place(
        &app,
        crate_node,
        Vec3::new(2.0, 0.0, 0.0),
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
    );
    {
        let mut world = app.engine.world_mut();
        world.get::<&mut Transform>(crate_node).unwrap().scale = Vec3::new(2.0, 1.0, 1.0);
    }
    add(
        &app,
        crate_node,
        "shape2d",
        "kind = \"rect\"\nhalf_extents = [1.0, 1.0]",
    );
    add(&app, crate_node, "occluder2d", "");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let points = outline(&world, crate_node);
    assert_eq!(
        points.len(),
        5,
        "closed repeats the first point: {points:?}"
    );
    assert_close(points[0], points[4]);
    // (-1, -1) scaled to (-2, -1), turned a quarter turn to (1, -2), moved.
    assert_close(points[0], Vec2::new(3.0, -2.0));
}

#[test]
fn a_node_without_an_occluder_has_no_outline() {
    let mut app = app();
    let bare = node(&app);
    add(&app, bare, "shape2d", "kind = \"rect\"");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    assert!(outline(&world, bare).is_empty());
}

fn point_light(position: Vec2, radius: f32) -> LitLight2d {
    LitLight2d {
        position,
        direction: Vec2::NEG_Y,
        color: [1.0; 3],
        radius,
        intensity: 1.0,
        shadows: true,
        kind: LightKind2d::Point,
    }
}

/// The far edge of a shadow has to clear the light's reach, or the light
/// leaks back in past the end of its own polygon.
#[test]
fn a_shadow_quad_reaches_past_the_lights_radius() {
    let light = point_light(Vec2::ZERO, 10.0);
    let quad = shadow_quad(
        [Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0)],
        &light,
        light.radius,
    );
    assert_close(quad[0], Vec2::new(1.0, -1.0));
    assert_close(quad[1], Vec2::new(1.0, 1.0));
    for far in [quad[2], quad[3]] {
        assert!(
            (far - light.position).length() > light.radius,
            "{far} is still inside the light"
        );
    }
}

/// Both ends of a directional light's shadow travel the same way, so the
/// polygon is a strip rather than a fan.
#[test]
fn a_directional_shadow_quad_is_a_parallel_strip() {
    let light = LitLight2d {
        direction: Vec2::new(0.0, -1.0),
        kind: LightKind2d::Directional,
        ..point_light(Vec2::ZERO, 0.0)
    };
    let quad = shadow_quad([Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0)], &light, 20.0);
    assert_close(quad[2], Vec2::new(1.0, -20.0));
    assert_close(quad[3], Vec2::new(-1.0, -20.0));
}

/// An edge point sitting exactly on the light has no direction to be pushed
/// in; it must stay put rather than come out NaN.
#[test]
fn an_edge_through_the_light_casts_no_infinity() {
    let light = point_light(Vec2::ZERO, 5.0);
    let quad = shadow_quad([Vec2::ZERO, Vec2::new(1.0, 0.0)], &light, 5.0);
    for corner in quad {
        assert!(corner.is_finite(), "{quad:?}");
    }
    assert_close(quad[3], Vec2::ZERO);
}

#[test]
fn the_cameras_ambient_reaches_the_2d_config() {
    let mut app = app();
    let cam = node(&app);
    add(&app, cam, "camera", "kind = \"2d\"\nambient = \"#402010\"");
    assert_eq!(
        app.engine.resource::<CameraConfig2d>().borrow().ambient,
        [0.0; 3],
        "control: nothing has driven the camera yet"
    );
    app.tick(1.0 / 60.0);
    let ambient = app.engine.resource::<CameraConfig2d>().borrow().ambient;
    assert!((ambient[0] - 0.25).abs() < 0.01, "{ambient:?}");
    assert!((ambient[1] - 0.125).abs() < 0.01, "{ambient:?}");
    assert!((ambient[2] - 0.0625).abs() < 0.01, "{ambient:?}");
}

#[test]
fn the_components_round_trip() {
    let app = app();
    let lamp = node(&app);
    add(
        &app,
        lamp,
        "light2d",
        "kind = \"directional\"\ncolor = \"#ffd28a\"\nradius = 6.0\nintensity = 1.2\nshadows = false",
    );
    let saved = components::get(&app.engine, lamp, "light2d").unwrap();
    let table = saved.as_table().unwrap();
    assert_eq!(table["kind"].as_str().unwrap(), "directional");
    assert!((table["intensity"].as_float().unwrap() - 1.2).abs() < 1e-6);
    assert!(!table["shadows"].as_bool().unwrap());

    add(&app, lamp, "occluder2d", "closed = false");
    let saved = components::get(&app.engine, lamp, "occluder2d").unwrap();
    let table = saved.as_table().unwrap();
    assert!(!table["closed"].as_bool().unwrap());
    assert_eq!(table["mesh"].as_str().unwrap(), "");

    let cam = node(&app);
    add(
        &app,
        cam,
        "camera",
        "kind = \"2d\"\nambient = [0.1, 0.2, 0.3, 1.0]",
    );
    let saved = components::get(&app.engine, cam, "camera").unwrap();
    let ambient = saved.as_table().unwrap()["ambient"].as_array().unwrap();
    assert!((ambient[1].as_float().unwrap() - 0.2).abs() < 1e-6);
}

#[test]
fn an_unknown_light_kind_is_an_error_not_a_panic() {
    let app = app();
    let lamp = node(&app);
    let table: toml::Value = toml::from_str("kind = \"spot\"").unwrap();
    let err = components::add(&app.engine, lamp, "light2d", Some(&table))
        .err()
        .expect("an unknown kind must be refused");
    assert!(format!("{err:#}").contains("spot"), "{err:#}");
}
