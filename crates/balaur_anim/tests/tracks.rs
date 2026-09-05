//! Component-property tracks and method tracks, driven headlessly.
//!
//! The point of this file is what `crates/balaur_anim/Cargo.toml` does *not*
//! say. A clip here animates `color/rgba` and `shape/radius`, which belong to
//! `balaur_render`, and `balaur_anim` has no dependency on that crate — every
//! one of these tracks goes through the component registry and
//! `balaur_core::components::patch`. A third-party plugin's components animate
//! the day they are registered, for the same reason and with no code.

mod common;

use balaur_anim::{AnimationPlugin, Playback};
use balaur_core::hecs::Entity;
use balaur_core::{App, AppConfig, components, scene};
use common::Calls;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
    balaur_plugin::load(&mut app, &mut balaur::RenderPlugin::default()).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

fn spawn(app: &App, name: &str) -> Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), name, root)
}

fn set(app: &App, entity: Entity, component: &str, params: &str) {
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, component, Some(&params)).unwrap();
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
}

fn property(app: &App, entity: Entity, component: &str, prop: &str) -> toml::Value {
    components::get(&app.engine, entity, component)
        .and_then(|table| table.get(prop).cloned())
        .unwrap_or_else(|| panic!("{component}.{prop} is not set on this node"))
}

fn numbers(value: &toml::Value) -> Vec<f64> {
    value
        .as_array()
        .expect("an array of numbers")
        .iter()
        .filter_map(balaur_core::components::as_f64)
        .collect()
}

#[test]
fn a_clip_animates_a_component_the_animation_crate_does_not_depend_on() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(
        &app,
        entity,
        "shape3d",
        "kind = \"cuboid\"\ncolor = [0.0, 0.0, 0.0, 1.0]",
    );
    set(
        &app,
        entity,
        "animation",
        r#"
[library]
length = 1.0

[[library.tracks]]
property = "shape3d/color"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0, 1.0] },
  { t = 1.0, value = [1.0, 0.5, 0.0, 1.0] },
]
"#,
    );
    balaur_anim::play(&app.engine, entity, "").unwrap();

    tick(&mut app, 31);

    let rgba = numbers(&property(&app, entity, "shape3d", "color"));
    assert!(
        (rgba[0] - 0.5).abs() < 0.05,
        "half a second in, red should be halfway: {rgba:?}"
    );
    assert!((rgba[1] - 0.25).abs() < 0.05, "{rgba:?}");
    assert!((rgba[3] - 1.0).abs() < 1e-6, "alpha was animated flat");
}

#[test]
fn animating_one_property_leaves_the_rest_of_its_component_alone() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(
        &app,
        entity,
        "shape3d",
        r#"kind = "cuboid"
half_extents = [2.0, 3.0, 4.0]"#,
    );
    set(
        &app,
        entity,
        "animation",
        r#"
[library]
length = 1.0

[[library.tracks]]
property = "shape3d/radius"
keys = [ { t = 0.0, value = 0.5 }, { t = 1.0, value = 2.0 } ]
"#,
    );
    balaur_anim::play(&app.engine, entity, "").unwrap();

    tick(&mut app, 30);

    assert_eq!(
        numbers(&property(&app, entity, "shape3d", "half_extents")),
        vec![2.0, 3.0, 4.0],
        "writing `radius` through the registry put `half_extents` back to its \
         schema default — which is what `components::patch` exists to prevent"
    );
}

#[test]
fn a_component_track_can_drive_a_child_node() {
    let mut app = app();
    let parent = spawn(&app, "Player");
    let child = scene::spawn_node(&mut app.engine.world_mut(), "Halo", parent);
    set(
        &app,
        child,
        "shape3d",
        "kind = \"ball\"\ncolor = [0.0, 0.0, 0.0, 1.0]",
    );
    set(
        &app,
        parent,
        "animation",
        r#"
[library]
length = 1.0
interp = "linear"

[[library.tracks]]
target = "Halo"
property = "shape3d/color"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0, 1.0] },
  { t = 1.0, value = [0.0, 0.0, 1.0, 1.0] },
]
"#,
    );
    balaur_anim::play(&app.engine, parent, "").unwrap();

    tick(&mut app, 61);

    let rgba = numbers(&property(&app, child, "shape3d", "color"));
    assert!(
        (rgba[2] - 1.0).abs() < 1e-3,
        "the child was not driven: {rgba:?}"
    );
}

#[test]
fn a_track_naming_a_component_nothing_registered_leaves_the_node_alone() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(&app, entity, "shape3d", r#"kind = "ball""#);
    set(
        &app,
        entity,
        "animation",
        r#"
[library]
length = 1.0

[[library.tracks]]
property = "wobbler/amount"
keys = [ { t = 0.0, value = 0.0 }, { t = 1.0, value = 1.0 } ]
"#,
    );
    balaur_anim::play(&app.engine, entity, "").unwrap();

    tick(&mut app, 30);

    assert!(
        balaur_anim::is_playing(&app.engine, entity),
        "a track nothing handles should be skipped, not fatal"
    );
    assert!(components::get(&app.engine, entity, "wobbler").is_none());
}

fn app_recording_calls() -> (App, std::rc::Rc<Calls>) {
    let app = app();
    let calls = std::rc::Rc::new(Calls::default());
    app.engine.set_script_host(calls.clone());
    (app, calls)
}

const FOOTSTEPS: &str = r#"
[library]
length = 1.0
loop = "loop"

[[library.tracks]]
keys = [ { t = 0.5, call = "on_footstep" } ]
"#;

#[test]
fn a_method_key_fires_once_per_loop() {
    let (mut app, calls) = app_recording_calls();
    let entity = spawn(&app, "Walker");
    set(&app, entity, "animation", FOOTSTEPS);
    balaur_anim::play(&app.engine, entity, "").unwrap();

    tick(&mut app, 60);
    assert_eq!(calls.count(entity, "on_footstep"), 1, "one loop, one step");
    tick(&mut app, 60);
    assert_eq!(calls.count(entity, "on_footstep"), 2, "the wrap re-arms it");
}

#[test]
fn a_seek_does_not_fire_the_keys_it_skipped() {
    let (mut app, calls) = app_recording_calls();
    let entity = spawn(&app, "Walker");
    set(&app, entity, "animation", FOOTSTEPS);
    balaur_anim::play(&app.engine, entity, "").unwrap();

    tick(&mut app, 6);
    balaur_anim::seek(&app.engine, entity, 0.9);
    tick(&mut app, 3);

    assert_eq!(
        calls.count(entity, "on_footstep"),
        0,
        "a seek jumped over the key and fired it anyway"
    );
}

#[test]
fn a_method_key_fires_on_the_node_its_track_targets() {
    let (mut app, calls) = app_recording_calls();
    let parent = spawn(&app, "Player");
    let child = scene::spawn_node(&mut app.engine.world_mut(), "Feet", parent);
    set(
        &app,
        parent,
        "animation",
        r#"
[library]
length = 1.0

[[library.tracks]]
target = "Feet"
keys = [ { t = 0.5, call = "on_footstep" } ]
"#,
    );
    balaur_anim::play(&app.engine, parent, "").unwrap();

    tick(&mut app, 40);

    assert_eq!(calls.count(child, "on_footstep"), 1);
    assert_eq!(calls.count(parent, "on_footstep"), 0);
}

#[test]
fn a_clip_that_ends_calls_on_animation_finished_once() {
    let (mut app, calls) = app_recording_calls();
    let entity = spawn(&app, "Box");
    set(
        &app,
        entity,
        "animation",
        r#"
[library]
length = 0.5

[[library.tracks]]
property = "position"
keys = [ { t = 0.0, value = [0.0, 0.0, 0.0] }, { t = 0.5, value = [0.0, 1.0, 0.0] } ]
"#,
    );
    balaur_anim::play(&app.engine, entity, "").unwrap();

    tick(&mut app, 120);

    assert_eq!(
        calls.count(entity, "on_animation_finished"),
        1,
        "the signal is one event, not one a frame after the clip ended"
    );
    assert_eq!(
        calls.args(entity, "on_animation_finished"),
        Some(vec![balaur_script::Value::Str(String::new())]),
        "the handler is told which clip ended; this one was addressed by the \
         library reference itself, so its name is empty"
    );
}

#[test]
fn just_finished_answers_for_one_frame_and_names_the_clip() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(
        &app,
        entity,
        "animation",
        r#"library = "animations/hero.toml"
autoplay = "spin""#,
    );

    tick(&mut app, 61);
    assert_eq!(
        balaur_anim::just_finished(&app.engine, entity).as_deref(),
        Some("spin"),
        "the frame a clip ends, `just_finished` names it"
    );
    tick(&mut app, 1);
    assert_eq!(
        balaur_anim::just_finished(&app.engine, entity),
        None,
        "and the frame after, it does not"
    );
}

#[test]
fn a_queued_clip_starts_when_the_one_before_it_ends() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(
        &app,
        entity,
        "animation",
        r#"library = "animations/hero.toml"
autoplay = "spin""#,
    );
    balaur_anim::queue(&app.engine, entity, "idle");

    tick(&mut app, 30);
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("spin"),
        "a queued clip must wait its turn"
    );

    tick(&mut app, 40);
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("idle")
    );
    assert!(balaur_anim::is_playing(&app.engine, entity));
}

#[test]
fn pause_holds_the_playhead_and_resume_carries_on_from_it() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(
        &app,
        entity,
        "animation",
        r#"library = "animations/hero.toml"
autoplay = "idle""#,
    );

    tick(&mut app, 15);
    balaur_anim::pause(&app.engine, entity);
    let held = balaur_anim::time(&app.engine, entity);
    tick(&mut app, 30);

    assert!(!balaur_anim::is_playing(&app.engine, entity));
    assert_eq!(
        balaur_anim::time(&app.engine, entity).to_bits(),
        held.to_bits()
    );
    assert_eq!(
        balaur_anim::current(&app.engine, entity).as_deref(),
        Some("idle"),
        "a paused clip is still the current one"
    );

    balaur_anim::resume(&app.engine, entity);
    tick(&mut app, 6);
    assert!(balaur_anim::is_playing(&app.engine, entity));
    assert!(balaur_anim::time(&app.engine, entity) > held);
}

#[test]
fn stop_ends_the_clip_where_pause_only_holds_it() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(
        &app,
        entity,
        "animation",
        r#"library = "animations/hero.toml"
autoplay = "idle""#,
    );
    tick(&mut app, 10);

    balaur_anim::stop(&app.engine, entity);
    balaur_anim::resume(&app.engine, entity);

    assert!(!balaur_anim::is_playing(&app.engine, entity));
    assert_eq!(
        balaur_anim::current(&app.engine, entity),
        None,
        "`resume` brought back a clip that `stop` had ended"
    );
}

#[test]
fn a_clip_defined_at_run_time_plays_by_the_name_it_was_given() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(&app, entity, "animation", "");
    let body: toml::Value = toml::from_str(
        r#"length = 1.0
tracks = [ { property = "position", keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 1.0, value = [0.0, 6.0, 0.0] },
] } ]"#,
    )
    .unwrap();
    balaur_anim::define(&app.engine, entity, "hurt", body).unwrap();

    balaur_anim::play(&app.engine, entity, "hurt").unwrap();
    tick(&mut app, 61);

    let world = app.engine.world();
    let transform = world.get::<&scene::Transform>(entity).unwrap();
    assert!(
        (transform.position.y - 6.0).abs() < 1e-3,
        "a defined clip did not drive the node: {:?}",
        transform.position
    );
}

#[test]
fn a_definition_that_is_not_a_clip_is_refused_where_it_was_written() {
    let app = app();
    let entity = spawn(&app, "Box");
    set(&app, entity, "animation", "");
    let body: toml::Value =
        toml::from_str("length = 1.0\ntracks = [ { property = \"jiggle\" } ]").unwrap();
    let why = format!(
        "{:#}",
        balaur_anim::define(&app.engine, entity, "hurt", body).unwrap_err()
    );
    assert!(why.contains("hurt"), "the message owes the name: {why}");
    assert!(why.contains("jiggle"), "unhelpful: {why}");
}

#[test]
fn play_can_pick_the_current_clip_back_up_where_it_left_off() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    set(
        &app,
        entity,
        "animation",
        r#"library = "animations/hero.toml"
autoplay = "idle""#,
    );
    tick(&mut app, 20);
    let reached = balaur_anim::time(&app.engine, entity);

    balaur_anim::play_from(&app.engine, entity, "idle", false).unwrap();
    assert_eq!(
        balaur_anim::time(&app.engine, entity).to_bits(),
        reached.to_bits()
    );

    balaur_anim::play(&app.engine, entity, "idle").unwrap();
    assert_eq!(
        balaur_anim::time(&app.engine, entity).to_bits(),
        0.0f32.to_bits()
    );
}

#[test]
fn every_player_advances_in_the_same_order_on_every_run() {
    let run = || {
        let mut app = app();
        for name in ["A", "B", "C", "D"] {
            let entity = spawn(&app, name);
            set(
                &app,
                entity,
                "animation",
                r#"library = "animations/hero.toml"
autoplay = "idle""#,
            );
        }
        tick(&mut app, 17);
        let state = app.engine.resource::<balaur_anim::AnimationState>();
        let order: Vec<f32> = state
            .borrow()
            .players
            .values()
            .map(|playback: &Playback| playback.time)
            .collect();
        order
    };
    assert_eq!(
        run(),
        run(),
        "playback order or timing drifted between runs"
    );
}

#[test]
fn animating_a_render_component_costs_no_dependency_on_the_render_crate() {
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .unwrap();
    let shipped = manifest
        .split("[dev-dependencies]")
        .next()
        .expect("the manifest declares its dependencies before its dev-dependencies");
    for plugin in [
        "balaur_render",
        "balaur_ui",
        "balaur_physics",
        "balaur_input",
    ] {
        assert!(
            !shipped.contains(plugin),
            "`{plugin}` reached balaur_anim's shipped dependencies. Component tracks go through \
             the component registry precisely so they do not have to — a dependency here is the \
             design being given up, not a build fix."
        );
    }
}

#[test]
fn a_rotation_track_takes_quaternion_keys_and_slerps_between_them() {
    let mut app = app();
    let entity = spawn(&app, "Turn");
    let half = std::f32::consts::FRAC_1_SQRT_2;
    set(
        &app,
        entity,
        "animation",
        &format!(
            r#"
[library]
length = 1.0

[[library.tracks]]
property = "rotation"
keys = [
  {{ t = 0.0, value = [0, 0, 0, 1] }},
  {{ t = 1.0, value = [0, 0, {half}, {half}] }},
]
"#
        ),
    );
    balaur_anim::play(&app.engine, entity, "").unwrap();

    tick(&mut app, 31);

    let rotation = app
        .engine
        .world()
        .get::<&scene::Transform>(entity)
        .unwrap()
        .rotation;
    let angle = balaur_core::skeleton::angle_about_z(rotation);
    assert!(
        (angle - std::f32::consts::FRAC_PI_4).abs() < 0.03,
        "half way to a quarter turn is an eighth, not {angle}"
    );
}
