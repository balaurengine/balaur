//! The `camera` component, without a window: it writes the same
//! `CameraConfig3d` / `CameraConfig2d` resources a windowed backend applies.

use balaur_core::glamx::Vec3;
use balaur_core::{App, AppConfig, Transform, components, scene};
use balaur_render::{CameraConfig2d, CameraConfig3d, PostConfig, RenderPlugin};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
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

fn add_camera_3d(app: &App, entity: balaur_core::hecs::Entity, params: &str) {
    let table: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "camera3d", Some(&table)).unwrap();
}

fn add_camera_2d(app: &App, entity: balaur_core::hecs::Entity, params: &str) {
    let table: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "camera2d", Some(&table)).unwrap();
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
    add_camera_3d(&app, cam, "look_at = [1.0, 2.0, 3.0]");
    {
        // Control: the boot default must differ from the node, and the boot
        // `changed` is cleared as a backend would after applying it.
        let config = app.engine.resource::<CameraConfig3d>();
        let mut config = config.borrow_mut();
        assert!((config.eye - Vec3::new(3.0, 4.0, 5.0)).length() > 1e-3);
        config.changed = false;
    }
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<CameraConfig3d>();
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
    add_camera_2d(&app, cam, "pixels_per_unit = 30.0");
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
    add_camera_3d(&app, cam, "current = false");
    let before = app.engine.resource::<CameraConfig3d>().borrow().eye;
    app.tick(1.0 / 60.0);
    assert_close(app.engine.resource::<CameraConfig3d>().borrow().eye, before);
}

/// A backend clears `changed` once it applies a pose; a camera that has not
/// moved must not raise it again, or interactive orbit/pan controls die.
#[test]
fn an_unmoved_camera_does_not_reassert_itself() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::new(3.0, 4.0, 5.0));
    add_camera_3d(&app, cam, "");
    let cam_2d = node_at(&app, app.engine.root(), Vec3::new(1.0, 1.0, 0.0));
    add_camera_2d(&app, cam_2d, "");
    app.tick(1.0 / 60.0);
    {
        let config = app.engine.resource::<CameraConfig3d>();
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
        !app.engine.resource::<CameraConfig3d>().borrow().changed,
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
    add_camera_2d(&app, cam, "");
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
    add_camera_3d(
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
    add_camera_3d(&app, cam, "post = [\"bloom\"]");
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
    add_camera_2d(&app, cam, "current = false\npixels_per_unit = 25.0");
    let saved = components::get(&app.engine, cam, "camera2d").unwrap();
    let table = saved.as_table().unwrap();
    assert!(!table["current"].as_bool().unwrap());
    assert!((table["pixels_per_unit"].as_float().unwrap() - 25.0).abs() < 1e-6);
    // The split is what keeps this out: a flat camera has nothing to aim.
    assert!(
        !table.contains_key("look_at"),
        "2D camera reports a look_at"
    );
}

#[test]
fn the_post_list_round_trips() {
    let app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_3d(
        &app,
        cam,
        "post = [\"ssr\", \"bloom\"]\nbloom_intensity = 0.25",
    );
    let saved = components::get(&app.engine, cam, "camera3d").unwrap();
    let table = saved.as_table().unwrap();
    let post: Vec<&str> = table["post"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // The order the scene wrote, because the order is what the list means.
    assert_eq!(post, ["ssr", "bloom"]);
    assert!((table["bloom_intensity"].as_float().unwrap() - 0.25).abs() < 1e-6);
}

#[test]
fn a_name_the_engine_does_not_know_is_a_material() {
    let app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_3d(
        &app,
        cam,
        "post = [\"ssao\", \"materials/fog.toml\", \"tonemap\", \"materials/grade.toml\"]",
    );
    let saved = components::get(&app.engine, cam, "camera3d").unwrap();
    let post: Vec<&str> = saved.as_table().unwrap()["post"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        post,
        [
            "ssao",
            "materials/fog.toml",
            "tonemap",
            "materials/grade.toml"
        ],
        "a material rides in the list beside the engine's own names"
    );
}

#[test]
fn the_tonemap_is_which_side_of_it_a_material_falls() {
    let split = |list: &str| {
        let post = balaur_render::Post {
            passes: list
                .split_whitespace()
                .map(|name| match name {
                    "bloom" => balaur_render::PostPass::Bloom,
                    "tonemap" => balaur_render::PostPass::Tonemap,
                    other => balaur_render::PostPass::Material(other.to_string()),
                })
                .collect(),
            ..balaur_render::Post::default()
        };
        post.materials()
    };
    let (film, screen) = split("fog tonemap grade");
    assert_eq!(film, ["fog"], "before it, a pass works in linear light");
    assert_eq!(screen, ["grade"], "after it, on the finished picture");
    let (film, screen) = split("grade vignette");
    assert!(
        film.is_empty(),
        "a list that never names the tonemap has it at the head"
    );
    assert_eq!(screen, ["grade", "vignette"], "in the order it was written");
    let (film, screen) = split("tonemap bloom grade");
    assert!(film.is_empty());
    assert_eq!(screen, ["grade"], "and an engine pass is not a material");
}

#[test]
fn the_camera_hands_its_two_chains_to_the_backend() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_3d(
        &app,
        cam,
        "post = [\"materials/fog.toml\", \"tonemap\", \"bloom\", \"materials/grade.toml\"]",
    );
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<PostConfig>();
    let config = config.borrow();
    assert_eq!(config.film, ["materials/fog.toml"]);
    assert_eq!(config.screen, ["materials/grade.toml"]);
    assert!(config.bloom, "and the engine's own is still switched on");
}

#[test]
fn patching_one_camera_property_leaves_the_rest_alone() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_3d(
        &app,
        cam,
        "post = [\"ssao\", \"vignette\"]\nssao_bias = 0.25\nssao_radius = 0.6\nvignette_amount = 0.32",
    );
    // What a script driving the view does every frame.
    let aim: toml::Value = toml::from_str("look_at = [1.0, 2.0, 3.0]").unwrap();
    components::patch(&app.engine, cam, "camera3d", &aim).unwrap();
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<PostConfig>();
    let config = config.borrow();
    assert!(
        (config.occlusion.bias - 0.25).abs() < 1e-6,
        "a patched aim reset the occlusion bias to {}",
        config.occlusion.bias
    );
    assert!((config.occlusion.radius - 0.6).abs() < 1e-6);
    assert!((config.finish.vignette_amount - 0.32).abs() < 1e-6);
}

/// The inspector shows a component through its `get` hook, so a property the
/// hook leaves out is a property nothing displays. `patch` no longer depends on
/// this -- it keeps what was asked for -- but a reader still does.
#[test]
fn every_component_reports_every_property_it_holds() {
    // These report what is in effect rather than every property: a ball has no
    // `tube_radius`, and a size or a tile a sheet derived is not the
    // component's to state. Their omissions are the design.
    const DERIVED: &[&str] = &["shape3d", "shape2d", "sprite", "tilemap"];
    let app = app();
    let root = app.engine.root();
    let mut offenders = Vec::new();
    let mut checked = 0;
    for (name, schema) in components::schemas(&app.engine) {
        let entity = scene::spawn_node(&mut app.engine.world_mut(), "N", root);
        if components::add(&app.engine, entity, &name, None).is_err() {
            continue;
        }
        let Some(read) = components::get(&app.engine, entity, &name) else {
            continue;
        };
        let (Some(want), Some(got)) = (schema.as_table(), read.as_table()) else {
            continue;
        };
        checked += 1;
        let missing: Vec<&str> = want
            .keys()
            .filter(|key| !got.contains_key(*key))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() && !DERIVED.contains(&name.as_str()) {
            offenders.push(format!("{name} does not report {missing:?}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "patching any other property resets these: {offenders:?}"
    );
    assert!(checked > 5, "only {checked} components were reachable");
}

#[test]
fn a_patch_keeps_what_was_asked_for_even_where_get_is_silent() {
    let app = app();
    let node = node_at(&app, app.engine.root(), Vec3::ZERO);
    // A sphere has no size, so `shape3d` does not report the one asked
    // for here: they are only in the table the scene handed over.
    let asked: toml::Value =
        toml::from_str("kind = \"sphere\"\nradius = 0.7\nsize = [4.0, 2.0, 4.0]").unwrap();
    components::add(&app.engine, node, "shape3d", Some(&asked)).unwrap();
    let becomes: toml::Value = toml::from_str("kind = \"box\"").unwrap();
    components::patch(&app.engine, node, "shape3d", &becomes).unwrap();
    let read = components::get(&app.engine, node, "shape3d").unwrap();
    let size = read["size"].as_array().unwrap();
    let sizes: Vec<f64> = size.iter().map(|v| v.as_float().unwrap()).collect();
    assert_eq!(
        sizes,
        vec![4.0, 2.0, 4.0],
        "the patch fell back to the schema default instead of what was asked for"
    );
}

/// Every `current_changed` a camera heard over `frames` ticks, in order.
fn current_changes(app: &mut App, camera: balaur_core::hecs::Entity, frames: u32) -> Vec<bool> {
    let mut heard = Vec::new();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
        for value in balaur_core::events::delivered_from(&app.engine, camera, "current_changed") {
            if let balaur_script::Value::Bool(current) = value {
                heard.push(current);
            }
        }
    }
    heard
}

#[test]
fn a_camera_says_when_it_becomes_the_one_drawn_from_and_when_it_stops() {
    let mut app = app();
    let root = app.engine.root();
    let first = node_at(&app, root, Vec3::ZERO);
    let second = node_at(&app, root, Vec3::ZERO);
    add_camera_2d(&app, first, "current = true");
    add_camera_2d(&app, second, "current = false");
    assert_eq!(current_changes(&mut app, first, 3), vec![true]);
    add_camera_2d(&app, first, "current = false");
    add_camera_2d(&app, second, "current = true");
    app.tick(1.0 / 60.0);
    let heard = |app: &App, camera| {
        balaur_core::events::delivered_from(&app.engine, camera, "current_changed")
    };
    app.tick(1.0 / 60.0);
    assert_eq!(heard(&app, first), vec![balaur_script::Value::Bool(false)]);
    assert_eq!(heard(&app, second), vec![balaur_script::Value::Bool(true)]);
}

/// The current camera's lens is what the backend builds kiss3d's camera
/// from, and with no current camera the defaults come back: a game's lens
/// does not outlive its play in the editor.
#[test]
fn the_current_cameras_lens_reaches_the_backend_and_leaves_with_it() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::new(0.0, 2.0, 6.0));
    add_camera_3d(
        &app,
        cam,
        "fov_degrees = 70.0\nnear = 0.5\nfar = 80.0\nprojection = \"orthographic\"\nrender_layers = 2",
    );
    app.engine
        .resource::<CameraConfig3d>()
        .borrow_mut()
        .lens_changed = false;
    app.tick(1.0 / 60.0);
    {
        let config = app.engine.resource::<CameraConfig3d>();
        let config = config.borrow();
        assert!(
            config.lens_changed,
            "the backend was never told the lens moved"
        );
        assert!((config.lens.fov_degrees - 70.0).abs() < 1e-6);
        assert!((config.lens.near - 0.5).abs() < 1e-6 && (config.lens.far - 80.0).abs() < 1e-6);
        assert!(config.lens.orthographic);
        assert_eq!(config.lens.render_layers, 2);
    }
    app.engine
        .resource::<CameraConfig3d>()
        .borrow_mut()
        .lens_changed = false;
    let off: toml::Value = toml::from_str("current = false").unwrap();
    components::patch(&app.engine, cam, "camera3d", &off).unwrap();
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<CameraConfig3d>();
    let config = config.borrow();
    assert!(config.lens_changed);
    assert_eq!(
        config.lens,
        balaur_render::Lens3d::default(),
        "the game's lens stayed"
    );
}

/// A lens kiss3d could not draw is refused at the component, naming why.
#[test]
fn a_camera_refuses_a_far_plane_nearer_than_its_near_one() {
    let app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    let table: toml::Value = toml::from_str("near = 10.0\nfar = 1.0").unwrap();
    let err = components::add(&app.engine, cam, "camera3d", Some(&table)).unwrap_err();
    assert!(format!("{err:#}").contains("past `near`"), "{err:#}");
}

/// Every knob the fork's own passes take reaches the backend's config and
/// reads back, on both cameras: the two share one spelling.
#[test]
fn the_post_pass_knobs_reach_the_config_and_read_back() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_3d(
        &app,
        cam,
        r#"post = ["ssr", "dof", "crt", "loupe"]
bloom_knee = 0.25
ssr_max_steps = 12
ssr_thickness = 0.75
ssr_intensity = 0.5
dof_mode = "gaussian"
dof_focus_distance = 4.0
dof_taps = 16
fxaa_edge_threshold = 0.2
sharpen_amount = 0.8
crt_curvature = 0.3
crt_scanline_count = 240.0
loupe_zoom = 4.0
loupe_focus = [0.25, 0.75]
loupe_corner = "top_left"
loupe_border_color = [0.0, 1.0, 0.0, 1.0]"#,
    );
    app.tick(1.0 / 60.0);
    {
        let config = app.engine.resource::<PostConfig>();
        let config = config.borrow();
        assert!(config.changed && config.ssr && config.dof);
        assert!((config.bloom_knee - 0.25).abs() < 1e-6);
        assert_eq!(config.reflections.max_steps, 12);
        assert!((config.reflections.intensity - 0.5).abs() < 1e-6);
        assert_eq!(
            config.depth_of_field.mode,
            balaur_render::FocusBlur::Gaussian
        );
        assert_eq!(config.depth_of_field.taps, 16);
        assert!((config.effects.crt_curvature - 0.3).abs() < 1e-6);
        assert_eq!(
            config.effects.loupe_corner,
            balaur_render::LoupeCorner::TopLeft
        );
        assert!(crate::same(config.effects.loupe_focus, [0.25, 0.75]));
        assert!(crate::same(
            config.effects.loupe_border_color,
            [0.0, 1.0, 0.0]
        ));
    }
    let read = components::get(&app.engine, cam, "camera3d").unwrap();
    assert_eq!(read["ssr_max_steps"].as_integer(), Some(12));
    assert_eq!(read["dof_mode"].as_str(), Some("gaussian"));
    assert_eq!(read["loupe_corner"].as_str(), Some("top_left"));
    assert!((read["sharpen_amount"].as_float().unwrap() - 0.8).abs() < 1e-6);
    let flat = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_2d(&app, flat, "crt_curvature = 0.4\nloupe_zoom = 2.0");
    let read = components::get(&app.engine, flat, "camera2d").unwrap();
    assert!((read["crt_curvature"].as_float().unwrap() - 0.4).abs() < 1e-6);
}

/// `crt`, `grayscale`, `waves`, `loupe` and `stereo` are passes the engine
/// draws, in the order written, not material assets to load.
#[test]
fn the_fork_passes_are_drawn_where_the_list_puts_them() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_2d(
        &app,
        cam,
        r#"post = ["grayscale", "tonemap", "crt", "waves", "loupe", "stereo"]"#,
    );
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<PostConfig>();
    let config = config.borrow();
    assert_eq!(
        config.film,
        ["grayscale"],
        "before the tonemap, on the film"
    );
    assert_eq!(config.screen, ["crt", "waves", "loupe", "stereo"]);
}

/// A knob a pass is built with rebuilds the chain, so it has to raise the
/// change a backend rebuilds on.
#[test]
fn turning_a_pass_knob_raises_a_post_change() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_3d(&app, cam, "post = [\"crt\"]");
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<PostConfig>();
    config.borrow_mut().changed = false;
    let turned: toml::Value = toml::from_str("crt_vignette = 0.9").unwrap();
    components::patch(&app.engine, cam, "camera3d", &turned).unwrap();
    app.tick(1.0 / 60.0);
    assert!(config.borrow().changed);
    assert!((config.borrow().effects.crt_vignette - 0.9).abs() < 1e-6);
}

#[test]
fn a_2d_camera_off_hidpi_tells_the_backend() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_2d(&app, cam, "hidpi = false");
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<CameraConfig2d>();
    assert!(!config.borrow().hidpi);
    assert!(config.borrow().changed);
    let read = components::get(&app.engine, cam, "camera2d").unwrap();
    assert_eq!(read["hidpi"].as_bool(), Some(false));
}

/// The two window settings that apply while the game runs say so, which is
/// what lets the editor apply them without asking for a restart.
#[test]
fn msaa_and_vsync_apply_without_a_restart() {
    let app = app();
    for path in ["window/msaa", "window/vsync"] {
        let def = balaur_core::settings::def(&app.engine, path).expect("a defined setting");
        assert!(def.applies_now(), "{path} still asks for a restart");
    }
    let width = balaur_core::settings::def(&app.engine, "window/width").unwrap();
    assert!(!width.applies_now(), "control: the window size still does");
}

/// A fixed orthographic height reaches the lens the backend builds its camera
/// from, and reads back as written.
#[test]
fn an_orthographic_height_reaches_the_config_and_reads_back() {
    let mut app = app();
    let spatial = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_3d(
        &app,
        spatial,
        "projection = \"orthographic\"\northographic_height = 8.0\nbloom_mips = 9",
    );
    app.tick(1.0 / 60.0);
    {
        let config = app.engine.resource::<CameraConfig3d>();
        assert!((config.borrow().lens.orthographic_height - 8.0).abs() < 1e-6);
    }
    let read = components::get(&app.engine, spatial, "camera3d").unwrap();
    assert!((read["orthographic_height"].as_float().unwrap() - 8.0).abs() < 1e-6);
    assert_eq!(read["bloom_mips"].as_integer(), Some(9));
}

/// A camera answers to its node alone: the mouse controls kiss3d's cameras
/// carry are no keys of either component.
#[test]
fn neither_camera_takes_mouse_controls() {
    let app = app();
    for (component, key) in [
        ("camera3d", "orbit_button"),
        ("camera3d", "reset_key"),
        ("camera2d", "zoom_step"),
        ("camera2d", "pan_button"),
    ] {
        let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
        let table: toml::Value = toml::from_str(&format!("{key} = 1")).unwrap();
        assert!(
            components::add(&app.engine, cam, component, Some(&table)).is_err(),
            "`{component}` took `{key}`"
        );
    }
}

/// The bloom chain's length rides `post` like its other knobs, so the last
/// current camera of either dimension sets it.
#[test]
fn a_cameras_bloom_levels_reach_the_post_config() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_2d(&app, cam, "post = [\"bloom\"]\nbloom_mips = 3");
    app.tick(1.0 / 60.0);
    let config = app.engine.resource::<PostConfig>();
    assert_eq!(config.borrow().bloom_mips, 3);
}

#[test]
fn an_eye_separation_makes_a_stereo_pair_and_reads_back() {
    let mut app = app();
    let cam = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_3d(&app, cam, "eye_separation = 0.2");
    app.tick(1.0 / 60.0);
    {
        let config = app.engine.resource::<CameraConfig3d>();
        assert!((config.borrow().lens.eye_separation - 0.2).abs() < 1e-6);
    }
    let read = components::get(&app.engine, cam, "camera3d").unwrap();
    assert!((read["eye_separation"].as_float().unwrap() - 0.2).abs() < 1e-6);
    assert!(read.get("kind").is_none(), "a camera has no kind to pick");
}

#[test]
fn a_2d_cameras_gi_pass_and_its_knobs_reach_the_post_config() {
    let mut app = app();
    let flat = node_at(&app, app.engine.root(), Vec3::ZERO);
    add_camera_2d(
        &app,
        flat,
        "post = [\"gi\"]\ngi_rays = 4\ngi_solver = \"cascades\"\ngi_probe_spacing = 4\ngi_screen_occluders = true",
    );
    app.tick(1.0 / 60.0);
    {
        let config = app.engine.resource::<PostConfig>();
        let config = config.borrow();
        assert_eq!(
            config.screen,
            ["gi"],
            "a list naming no tonemap has it at the head"
        );
        assert_eq!(config.effects.gi.rays, 4);
        assert!(config.effects.gi.cascades && config.effects.gi.screen_occluders);
    }
    let read = components::get(&app.engine, flat, "camera2d").unwrap();
    assert_eq!(read["gi_solver"].as_str(), Some("cascades"));
    assert_eq!(read["gi_probe_spacing"].as_integer(), Some(4));
    assert_eq!(read["gi_max_steps"].as_integer(), Some(32));
}
