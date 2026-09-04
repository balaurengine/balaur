//! The `camera` component, without a window: it writes the same
//! `CameraConfig` / `CameraConfig2d` resources a windowed backend applies.

use balaur_core::glamx::Vec3;
use balaur_core::{components, scene, App, AppConfig, Transform};
use balaur_render::{CameraConfig, CameraConfig2d, PostConfig, RenderPlugin};

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

fn node_at(
    app: &App,
    parent: balaur_core::hecs::Entity,
    position: Vec3,
) -> balaur_core::hecs::Entity {
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "N", parent);
    app.engine
        .world_mut()
        .get::<&mut Transform>(entity)
        .unwrap()
        .position = position;
    entity
}

fn add_camera(app: &App, entity: balaur_core::hecs::Entity, params: &str) {
    let table: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "camera", Some(&table)).unwrap();
}

#[track_caller]
fn assert_close(actual: Vec3, expected: Vec3) {
    assert!(
        (actual - expected).length() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

/// The eye is the node's *global* position: the camera sits under a parent,
/// so the test also proves the system runs after transform propagation.
#[test]
fn a_current_camera_component_drives_the_camera_config() {
    let mut app = app();
    let rig = node_at(&app, app.engine.root(), Vec3::new(1.0, 0.0, 0.0));
    let cam = node_at(&app, rig, Vec3::new(2.0, 4.0, 5.0));
    add_camera(&app, cam, "look_at = [1.0, 2.0, 3.0]");
    {
        // Control: the boot default must differ from the node, and the boot
        // `changed` is cleared as a backend would after applying it.
        let config = app.engine.resource::<CameraConfig>();
        let mut config = config.borrow_mut();
        assert!((config.eye - Vec3::new(3.0, 4.0, 5.0)).length() > 1e-3);
        config.changed = false;
    }
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<CameraConfig>();
    let config = config.borrow();
    assert_close(config.eye, Vec3::new(3.0, 4.0, 5.0));
    assert_close(config.target, Vec3::new(1.0, 2.0, 3.0));
    assert!(
        config.changed,
        "the backend was never told to apply the pose"
    );
}

#[test]
fn a_2d_camera_component_drives_center_and_zoom() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::new(7.0, -2.0, 0.0));
    add_camera(&app, cam, "kind = \"2d\"\nzoom = 30.0");
    {
        // Control: the boot default must differ from the node, and the boot
        // `changed` is cleared as a backend would after applying it.
        let config = app.engine.resource::<CameraConfig2d>();
        let mut config = config.borrow_mut();
        assert!((config.zoom - 30.0).abs() > 1e-3);
        config.changed = false;
    }
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<CameraConfig2d>();
    let config = config.borrow();
    assert!(
        (config.center[0] - 7.0).abs() < 1e-6,
        "x: {}",
        config.center[0]
    );
    assert!(
        (config.center[1] + 2.0).abs() < 1e-6,
        "y: {}",
        config.center[1]
    );
    assert!((config.zoom - 30.0).abs() < 1e-6, "zoom: {}", config.zoom);
    assert!(config.changed);
}

#[test]
fn a_camera_that_is_not_current_leaves_the_config_alone() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::new(9.0, 9.0, 9.0));
    add_camera(&app, cam, "current = false");
    let before = app.engine.resource::<CameraConfig>().borrow().eye;
    app.tick(1.0 / 60.0);
    assert_close(app.engine.resource::<CameraConfig>().borrow().eye, before);
}

/// A backend clears `changed` once it applies a pose; a camera that has not
/// moved must not raise it again, or interactive orbit/pan controls die.
#[test]
fn an_unmoved_camera_does_not_reassert_itself() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::new(3.0, 4.0, 5.0));
    add_camera(&app, cam, "");
    let cam_2d = node_at(&app, app.engine.root(), Vec3::new(1.0, 1.0, 0.0));
    add_camera(&app, cam_2d, "kind = \"2d\"");
    app.tick(1.0 / 60.0);
    {
        let config = app.engine.resource::<CameraConfig>();
        assert!(
            config.borrow().changed,
            "control: the first tick must write"
        );
        config.borrow_mut().changed = false;
        let config_2d = app.engine.resource::<CameraConfig2d>();
        assert!(
            config_2d.borrow().changed,
            "control: the first tick must write"
        );
        config_2d.borrow_mut().changed = false;
    }
    app.tick(1.0 / 60.0);
    assert!(
        !app.engine.resource::<CameraConfig>().borrow().changed,
        "a still 3D camera re-asserted itself"
    );
    assert!(
        !app.engine.resource::<CameraConfig2d>().borrow().changed,
        "a still 2D camera re-asserted itself"
    );
}

/// A camera whose values happen to equal `CameraConfig2d::default()` still
/// has to reach the backend: the backend starts at its own zoom, so a scene
/// writing the schema's default of 60 must not read as "nothing to do".
/// Before this, such a scene drew at the backend's zoom and looked tiny.
#[test]
fn a_2d_camera_matching_the_defaults_still_reaches_the_backend() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera(&app, cam, "kind = \"2d\"");
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<CameraConfig2d>();
    let config = config.borrow();
    assert!((config.zoom - 60.0).abs() < 1e-6, "zoom: {}", config.zoom);
    assert!(
        config.changed,
        "a backend would never apply this camera, and the scene draws at its zoom"
    );
}

/// `post` is not per-dimension: the effects run over the whole film, so a 3D
/// camera drives them too.
#[test]
fn a_cameras_post_effects_reach_the_config() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera(
        &app,
        cam,
        "post = [\"bloom\", \"dof\"]\nbloom_threshold = 0.8",
    );
    {
        let config = app.engine.resource::<PostConfig>();
        let config = config.borrow();
        assert!(!config.bloom, "control: nothing has driven post yet");
        assert!(!config.changed);
    }
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<PostConfig>();
    let config = config.borrow();
    assert!(config.bloom);
    assert!(config.dof);
    assert!(!config.ssao, "an effect the list left out must stay off");
    assert!((config.bloom_threshold - 0.8).abs() < 1e-6);
    assert!(config.changed, "the backend was never told to apply them");
}

/// A backend clears `changed` once it has rebuilt its post chain; a camera
/// that has not changed must not make it rebuild again.
#[test]
fn unchanged_post_effects_do_not_reassert_themselves() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera(&app, cam, "post = [\"bloom\"]");
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<PostConfig>();
    assert!(
        config.borrow().changed,
        "control: the first tick must write"
    );
    config.borrow_mut().changed = false;
    app.tick(1.0 / 60.0);
    assert!(!config.borrow().changed);
}

#[test]
fn the_component_round_trips() {
    let app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera(&app, cam, "kind = \"2d\"\ncurrent = false\nzoom = 25.0");
    let saved = components::get(&app.engine, cam, "camera").unwrap();
    let table = saved.as_table().unwrap();
    assert_eq!(table["kind"].as_str().unwrap(), "2d");
    assert!(!table["current"].as_bool().unwrap());
    assert!((table["zoom"].as_float().unwrap() - 25.0).abs() < 1e-6);
}

#[test]
fn the_post_list_round_trips() {
    let app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera(
        &app,
        cam,
        "post = [\"ssr\", \"bloom\"]\nbloom_intensity = 0.25",
    );
    let saved = components::get(&app.engine, cam, "camera").unwrap();
    let table = saved.as_table().unwrap();
    let post: Vec<&str> = table["post"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // Schema order, not the order the scene happened to write them in.
    assert_eq!(post, ["bloom", "ssr"]);
    assert!((table["bloom_intensity"].as_float().unwrap() - 0.25).abs() < 1e-6);
}
