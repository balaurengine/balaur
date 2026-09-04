//! Animation driven from Rust, headless, so the assertions read the scene
//! tree directly rather than through a language.
//!
//! Every clip here is either written inline on the component — the asset
//! layer's "a table is a definition" rule — or addressed in
//! `tests/fixtures/animations/hero.toml`, which is a real library file read
//! off disk exactly as a shipped game reads one.

use balaur_anim::{AnimationPlugin, AnimationState};
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{components, project, App, AppConfig};
use glamx::{Quat, Vec3};

fn app() -> App {
    let mut app = App::new(AppConfig {
        // Asset references resolve against the project root when no script
        // host is running, which is every Rust-only app and every test.
        project_root: std::path::PathBuf::from("tests/fixtures"),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

/// A clip that lifts a node ten units over one second, written inline.
fn rise(wrap: &str) -> String {
    format!(
        r#"
[library]
length = 1.0
loop = "{wrap}"

[[library.tracks]]
property = "position"
keys = [
  {{ t = 0.0, value = [0.0, 0.0, 0.0] }},
  {{ t = 1.0, value = [0.0, 10.0, 0.0] }},
]
"#
    )
}

fn animated(app: &App, name: &str, params: &str) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "animation", Some(&params)).unwrap();
    entity
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
}

fn transform(app: &App, entity: Entity) -> Transform {
    *app.engine.world().get::<&Transform>(entity).unwrap()
}

fn height(app: &App, entity: Entity) -> f32 {
    transform(app, entity).position.y
}

#[test]
fn a_clip_drives_a_nodes_position_over_time() {
    let mut app = app();
    let entity = animated(&app, "Box", &rise("none"));
    assert_eq!(
        height(&app, entity).to_bits(),
        0.0f32.to_bits(),
        "a clip moved a node before playing"
    );
    balaur_anim::play(&app.engine, entity, "").unwrap();

    tick(&mut app, 30);
    assert!(
        (height(&app, entity) - 5.0).abs() < 0.05,
        "half a second into a one second clip is halfway, not {}",
        height(&app, entity)
    );
    tick(&mut app, 30);
    assert!((height(&app, entity) - 10.0).abs() < 0.05);
}

#[test]
fn a_looping_clip_wraps_back_to_the_start() {
    let mut app = app();
    let entity = animated(&app, "Box", &rise("loop"));
    balaur_anim::play(&app.engine, entity, "").unwrap();
    // 1.25 seconds into a one second clip: a quarter of the way through the
    // second pass.
    tick(&mut app, 75);
    assert!(
        (height(&app, entity) - 2.5).abs() < 0.05,
        "a looping clip did not wrap: {}",
        height(&app, entity)
    );
    assert!(balaur_anim::is_playing(&app.engine, entity));
}

#[test]
fn a_pingpong_clip_reverses_at_the_end() {
    let mut app = app();
    let entity = animated(&app, "Box", &rise("pingpong"));
    balaur_anim::play(&app.engine, entity, "").unwrap();
    // The same 1.25 seconds, played backwards from the end instead of
    // restarting: three quarters up rather than one quarter.
    tick(&mut app, 75);
    assert!(
        (height(&app, entity) - 7.5).abs() < 0.05,
        "a pingpong clip did not reverse: {}",
        height(&app, entity)
    );
    assert!(balaur_anim::is_playing(&app.engine, entity));
}

#[test]
fn a_clip_that_does_not_loop_holds_its_last_key_and_stops() {
    let mut app = app();
    let entity = animated(&app, "Box", &rise("none"));
    balaur_anim::play(&app.engine, entity, "").unwrap();
    tick(&mut app, 120);
    assert!(
        (height(&app, entity) - 10.0).abs() < 1e-4,
        "the last key was not held"
    );
    assert!(
        !balaur_anim::is_playing(&app.engine, entity),
        "a clip that ran off its end is still playing"
    );
}

#[test]
fn autoplay_starts_the_named_clip_when_the_scene_loads() {
    let mut app = app();
    let source = r#"
[[nodes]]
name = "Box"

[nodes.animation]
library = "animations/hero.toml"
autoplay = "idle"
"#;
    let root = app.engine.root();
    project::instantiate_scene(&app.engine, source, root, false).unwrap();
    let entity = scene::find_node(&app.engine.world(), root, "Box").unwrap();

    tick(&mut app, 30);
    assert!(
        (height(&app, entity) - 5.0).abs() < 0.05,
        "autoplay did not start the clip: {}",
        height(&app, entity)
    );
}

#[test]
fn a_saved_animation_node_no_longer_warns_that_nothing_handles_it() {
    balaur_core::logbuf::capture_for_test();
    let app = app();
    let source = r#"
[[nodes]]
name = "Box"

[nodes.animation]
library = "animations/hero.toml"
autoplay = "idle"
"#;
    let root = app.engine.root();
    project::instantiate_scene(&app.engine, source, root, false).unwrap();
    let unhandled: Vec<String> = balaur_core::logbuf::recent(200)
        .into_iter()
        .map(|entry| entry.message)
        .filter(|message| message.contains("no registered handler"))
        .collect();
    assert!(
        unhandled.is_empty(),
        "the animation scene key is still unhandled: {unhandled:?}"
    );
}

#[test]
fn a_rotation_crossing_180_degrees_interpolates_the_short_way() {
    let mut app = app();
    // +170° to -170° about z. The short way is 20° through 180°; lerping the
    // euler angles instead travels 340° the other way and passes through 0°.
    let entity = animated(
        &app,
        "Box",
        r#"
[library]
length = 1.0

[[library.tracks]]
property = "rotation_euler"
keys = [
  { t = 0.0, value = [0.0, 0.0, 2.9670597] },
  { t = 1.0, value = [0.0, 0.0, -2.9670597] },
]
"#,
    );
    balaur_anim::play(&app.engine, entity, "").unwrap();
    tick(&mut app, 30);

    let facing = transform(&app, entity).rotation * Vec3::X;
    assert!(
        facing.x < -0.99,
        "halfway across ±180° should face -x; euler lerp would face +x. Got {facing:?}"
    );
}

#[test]
fn a_track_targets_a_child_node_by_path() {
    let mut app = app();
    let entity = animated(
        &app,
        "Rig",
        r#"
[library]
length = 1.0

[[library.tracks]]
target = "Arm"
property = "position"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 1.0, value = [0.0, 10.0, 0.0] },
]
"#,
    );
    let arm = scene::spawn_node(&mut app.engine.world_mut(), "Arm", entity);
    balaur_anim::play(&app.engine, entity, "").unwrap();
    tick(&mut app, 30);

    assert!(
        (height(&app, arm) - 5.0).abs() < 0.05,
        "the child was not animated"
    );
    assert_eq!(
        height(&app, entity).to_bits(),
        0.0f32.to_bits(),
        "the player itself moved"
    );
}

#[test]
fn the_speed_property_scales_playback() {
    let mut app = app();
    let params = format!("speed = 2.0\n{}", rise("none"));
    let entity = animated(&app, "Box", &params);
    balaur_anim::play(&app.engine, entity, "").unwrap();
    // Quarter of a second at double speed is half the clip.
    tick(&mut app, 15);
    assert!(
        (height(&app, entity) - 5.0).abs() < 0.05,
        "speed did not scale playback: {}",
        height(&app, entity)
    );
}

#[test]
fn a_library_file_addresses_its_clips_by_name() {
    let mut app = app();
    let entity = animated(&app, "Hero", "library = \"animations/hero.toml\"");

    balaur_anim::play(&app.engine, entity, "idle").unwrap();
    tick(&mut app, 30);
    assert!(
        (height(&app, entity) - 5.0).abs() < 0.05,
        "clip 'idle' did not play"
    );

    balaur_anim::play(&app.engine, entity, "spin").unwrap();
    tick(&mut app, 30);
    let turned = transform(&app, entity).rotation * Vec3::X;
    assert!(
        turned.y > 0.6,
        "clip 'spin' did not rotate the node: {turned:?}"
    );
}

#[test]
fn a_clip_the_library_does_not_have_fails_with_the_reference_it_asked_for() {
    let app = app();
    let entity = animated(&app, "Hero", "library = \"animations/hero.toml\"");
    let err = balaur_anim::play(&app.engine, entity, "sprint").unwrap_err();
    assert!(
        format!("{err:#}").contains("animations/hero.toml#sprint"),
        "unhelpful: {err:#}"
    );
}

#[test]
fn removing_the_component_stops_the_node_being_animated() {
    let mut app = app();
    let entity = animated(&app, "Box", &rise("loop"));
    balaur_anim::play(&app.engine, entity, "").unwrap();
    tick(&mut app, 30);
    let stopped_at = height(&app, entity);

    components::remove(&app.engine, entity, "animation").unwrap();
    tick(&mut app, 30);
    assert_eq!(
        height(&app, entity).to_bits(),
        stopped_at.to_bits(),
        "a removed player kept animating"
    );
    assert!(app
        .engine
        .resource::<AnimationState>()
        .borrow()
        .players
        .is_empty());
}

#[test]
fn what_the_component_reports_back_is_what_the_scene_set() {
    let app = app();
    let entity = animated(
        &app,
        "Hero",
        "library = \"animations/hero.toml\"\nautoplay = \"idle\"\nspeed = 2.0\nroot = \"Rig\"",
    );
    let reported = components::get(&app.engine, entity, "animation").unwrap();
    assert_eq!(
        reported.get("library").and_then(toml::Value::as_str),
        Some("animations/hero.toml")
    );
    assert_eq!(
        reported.get("autoplay").and_then(toml::Value::as_str),
        Some("idle")
    );
    assert_eq!(
        reported.get("speed").and_then(toml::Value::as_float),
        Some(2.0)
    );
    assert_eq!(
        reported.get("root").and_then(toml::Value::as_str),
        Some("Rig")
    );
}

#[test]
fn the_same_setup_animates_identically_twice() {
    let run = || {
        let mut app = app();
        let entity = animated(
            &app,
            "Box",
            r#"
[library]
length = 1.3
loop = "pingpong"

[[library.tracks]]
property = "position"
interp = "cubic"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 0.4, value = [1.0, 3.0, -2.0] },
  { t = 0.9, value = [-4.0, 1.0, 5.0] },
  { t = 1.3, value = [2.0, -1.0, 0.5] },
]

[[library.tracks]]
property = "rotation_euler"
keys = [
  { t = 0.0, value = [0.0, 0.0, 2.9670597] },
  { t = 0.7, value = [1.1, -0.4, -2.9670597] },
  { t = 1.3, value = [-0.3, 2.2, 0.8] },
]

[[library.tracks]]
property = "scale"
keys = [
  { t = 0.0, value = [1.0, 1.0, 1.0] },
  { t = 1.3, value = [0.25, 3.5, 1.75] },
]
"#,
        );
        balaur_anim::play(&app.engine, entity, "").unwrap();
        for frame in 0..97 {
            // A deliberately ragged frame time: the accumulator is what makes
            // the simulation identical anyway.
            app.tick(if frame % 3 == 0 {
                1.0 / 45.0
            } else {
                1.0 / 90.0
            });
        }
        let t = transform(&app, entity);
        (
            t.position.to_array().map(f32::to_bits),
            Quat::to_array(&t.rotation).map(f32::to_bits),
            t.scale.to_array().map(f32::to_bits),
        )
    };
    assert_eq!(run(), run());
}

#[test]
fn a_non_looping_clip_run_backwards_finishes_at_the_start() {
    let mut app = app();
    let entity = animated(&app, "Box", &rise("none"));
    balaur_anim::play(&app.engine, entity, "").unwrap();
    balaur_anim::seek(&app.engine, entity, 1.0);
    balaur_anim::set_speed(&app.engine, entity, -1.0);

    tick(&mut app, 30);
    assert!(
        balaur_anim::is_playing(&app.engine, entity),
        "half a second back is still inside the clip"
    );

    tick(&mut app, 40);
    assert!(
        !balaur_anim::is_playing(&app.engine, entity),
        "running off the start ends a clip, or a negative speed plays forever"
    );
    assert!(height(&app, entity).abs() < 1e-5);
}

#[test]
fn a_clip_run_backwards_names_itself_as_the_one_that_finished() {
    let mut app = app();
    let entity = animated(&app, "Box", "library = \"animations/hero.toml\"\n");
    balaur_anim::play(&app.engine, entity, "spin").unwrap();
    balaur_anim::seek(&app.engine, entity, 0.1);
    balaur_anim::set_speed(&app.engine, entity, -1.0);
    tick(&mut app, 7);
    assert_eq!(
        balaur_anim::just_finished(&app.engine, entity).as_deref(),
        Some("spin")
    );
}
