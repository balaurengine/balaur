//! Tweens, headless, asserted on the scene tree they moved.
//!
//! A tween is a generated clip, so what these tests are really checking is
//! that the generation is right: which values a step starts from, when each
//! step begins, and that the whole thing rides the same fixed step and the
//! same sampler a clip does. Nothing here writes a second interpolator's
//! worth of expectations — the curves themselves are `tests/ease.rs`.

mod common;

use balaur_anim::{tween, AnimationPlugin, AnimationState};
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{components, App, AppConfig};
use common::Calls;
use glamx::Vec3;
use std::f32::consts::FRAC_PI_2;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures")))
    .unwrap();
    balaur_plugin::load(&mut app, &mut balaur::RenderPlugin::default()).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

fn spawn(app: &App, name: &str) -> Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), name, root)
}

fn child(app: &App, parent: Entity, name: &str) -> Entity {
    scene::spawn_node(&mut app.engine.world_mut(), name, parent)
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
}

fn spec(text: &str) -> toml::Value {
    toml::from_str(text).unwrap()
}

fn start(app: &App, entity: Entity, text: &str) -> u64 {
    tween::start(&app.engine, entity, &spec(text)).unwrap()
}

fn transform(app: &App, entity: Entity) -> Transform {
    *app.engine.world().get::<&Transform>(entity).unwrap()
}

fn height(app: &App, entity: Entity) -> f32 {
    transform(app, entity).position.y
}

fn place(app: &App, entity: Entity, at: Vec3) {
    app.engine
        .world_mut()
        .get::<&mut Transform>(entity)
        .unwrap()
        .position = at;
}

fn near(got: f32, want: f32, what: &str) {
    assert!((got - want).abs() < 0.06, "{what}: {got}, wanted {want}");
}

/// Position first, then scale: nothing about the second step until the first
/// has finished.
const IN_ORDER: &str = r#"
[[steps]]
property = "position"
to = [0.0, 10.0, 0.0]
duration = 0.5

[[steps]]
property = "scale"
to = [3.0, 3.0, 3.0]
duration = 0.5
"#;

#[test]
fn a_sequential_tween_lands_its_steps_in_order() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    start(&app, entity, IN_ORDER);

    tick(&mut app, 15);
    near(height(&app, entity), 5.0, "halfway through the first step");
    near(
        transform(&app, entity).scale.x,
        1.0,
        "the second step started early",
    );

    tick(&mut app, 30);
    near(
        height(&app, entity),
        10.0,
        "the first step should have landed",
    );
    near(
        transform(&app, entity).scale.x,
        2.0,
        "halfway through the second",
    );

    tick(&mut app, 30);
    near(
        transform(&app, entity).scale.x,
        3.0,
        "the second step landed",
    );
}

/// The scale step joins the position step; the second position step waits for
/// the longer of the two.
const JOINED: &str = r#"
[[steps]]
property = "position"
to = [0.0, 10.0, 0.0]
duration = 0.5

[[steps]]
property = "scale"
to = [3.0, 3.0, 3.0]
duration = 1.0
parallel = true

[[steps]]
property = "position"
to = [0.0, 0.0, 0.0]
duration = 0.5
"#;

#[test]
fn a_parallel_group_runs_together_and_the_next_step_waits_for_all_of_it() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    start(&app, entity, JOINED);

    tick(&mut app, 15);
    near(height(&app, entity), 5.0, "the joined steps run together");
    near(transform(&app, entity).scale.x, 1.5, "so does the scale");

    // Three quarters of a second: the position step ended at 0.5 and the
    // scale step runs to 1.0, so the third step has not started.
    tick(&mut app, 30);
    near(
        height(&app, entity),
        10.0,
        "the third step jumped the group",
    );
    near(
        transform(&app, entity).scale.x,
        2.5,
        "the joined step carries on",
    );

    // A quarter past the group's end, halfway down.
    tick(&mut app, 30);
    near(
        height(&app, entity),
        5.0,
        "the third step should be under way",
    );
    near(transform(&app, entity).scale.x, 3.0, "the group has landed");
}

#[test]
fn by_is_relative_to_the_value_at_the_start_of_its_step() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    // Not at the origin, and the first step moves it again: a `by` that meant
    // the authored origin would land on 3, and one that meant the tween's
    // own start value would land on 8.
    place(&app, entity, Vec3::new(0.0, 5.0, 0.0));
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "position"
to = [0.0, 10.0, 0.0]
duration = 0.25

[[steps]]
property = "position"
by = [0.0, -4.0, 0.0]
duration = 0.25
"#,
    );

    // 10 at the hand-over, then four less: a `by` read against the authored
    // origin would land on -4, and one read against the tween's own start on
    // 1.
    tick(&mut app, 40);
    near(
        height(&app, entity),
        6.0,
        "`by` is an offset from where it was",
    );
}

#[test]
fn by_on_its_own_offsets_where_the_node_already_is() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    place(&app, entity, Vec3::new(0.0, 5.0, 0.0));
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "position"
by = [0.0, 3.0, 0.0]
duration = 0.25
"#,
    );
    tick(&mut app, 20);
    near(height(&app, entity), 8.0, "5 plus 3, not 3");
}

#[test]
fn from_states_the_start_outright() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    place(&app, entity, Vec3::new(0.0, 5.0, 0.0));
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "position"
from = [0.0, 0.0, 0.0]
to = [0.0, 2.0, 0.0]
duration = 0.5
"#,
    );
    tick(&mut app, 15);
    near(height(&app, entity), 1.0, "halfway from the stated start");
}

#[test]
fn a_tween_dies_with_the_node_it_was_made_on() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    let id = start(&app, entity, IN_ORDER);
    tick(&mut app, 5);
    assert!(tween::is_running(&app.engine, id));

    app.engine.push_command(balaur_core::Command::Free(entity));
    // `queue_free` defers to `Stage::Last`, so the node survives this frame
    // and the tween notices on the next one.
    tick(&mut app, 3);

    assert!(
        !tween::is_running(&app.engine, id),
        "a tween outlived the node it belonged to"
    );
    let state = app.engine.resource::<AnimationState>();
    assert!(
        state.borrow().tweens.is_empty(),
        "a freed node left its tween behind"
    );
}

#[test]
fn a_tween_that_reaches_its_end_stops_being_a_tween() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    let id = start(
        &app,
        entity,
        r#"
[[steps]]
property = "position"
to = [0.0, 1.0, 0.0]
duration = 0.1
"#,
    );
    tick(&mut app, 20);
    assert!(!tween::is_running(&app.engine, id), "it should be over");
    let state = app.engine.resource::<AnimationState>();
    assert!(state.borrow().tweens.is_empty(), "a finished tween leaked");
    near(height(&app, entity), 1.0, "and it landed where it was sent");
}

#[test]
fn stop_takes_a_tween_handle_as_readily_as_a_node() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    let id = start(&app, entity, IN_ORDER);
    tick(&mut app, 15);
    tween::stop(&app.engine, id);
    let stopped = height(&app, entity);

    tick(&mut app, 30);
    assert_eq!(
        height(&app, entity).to_bits(),
        stopped.to_bits(),
        "a stopped tween kept moving the node"
    );
    assert!(!tween::is_running(&app.engine, id));
    // A handle that names nothing is not an error to stop again.
    tween::stop(&app.engine, id);
}

#[test]
fn two_identical_tweens_produce_bit_identical_results() {
    // Deliberately ragged frames: the fixed step is what the simulation sees,
    // so how the frames fell must not reach the result.
    let frames = [0.021, 0.004, 0.033, 0.011, 0.017, 0.05, 0.008];
    let run = || {
        let mut app = app();
        let entity = spawn(&app, "Box");
        place(&app, entity, Vec3::new(1.0, 5.0, -2.0));
        start(
            &app,
            entity,
            r#"
[[steps]]
property = "position"
by = [0.0, 4.0, 0.0]
duration = 0.4
ease = "out_elastic"

[[steps]]
property = "scale"
to = [2.0, 0.5, 3.0]
duration = 0.3
ease = "in_out_back"
parallel = true

[[steps]]
property = "position"
to = [0.0, 0.0, 0.0]
duration = 0.5
ease = "in_out_bounce"
"#,
        );
        let mut trail = Vec::new();
        for _ in 0..6 {
            for dt in frames {
                app.tick(dt);
                let t = transform(&app, entity);
                trail.push((
                    t.position.y.to_bits(),
                    t.scale.x.to_bits(),
                    t.scale.z.to_bits(),
                ));
            }
        }
        trail
    };
    assert_eq!(run(), run(), "two runs of one tween drifted apart");
}

#[test]
fn an_interval_and_a_call_sequence_among_the_property_steps() {
    let mut app = app();
    let calls = std::rc::Rc::new(Calls::default());
    app.engine.set_script_host(calls.clone());
    let entity = spawn(&app, "Box");
    start(
        &app,
        entity,
        r#"
[[steps]]
call = "on_started"

[[steps]]
property = "position"
to = [0.0, 10.0, 0.0]
duration = 0.5

[[steps]]
interval = 0.25

[[steps]]
call = "on_landed"

[[steps]]
property = "position"
to = [0.0, 0.0, 0.0]
duration = 0.5
"#,
    );

    tick(&mut app, 1);
    assert_eq!(
        calls.count(entity, "on_started"),
        1,
        "a call at the head of a tween has to fire on the first step"
    );

    tick(&mut app, 34);
    near(height(&app, entity), 10.0, "the property step ran");
    assert_eq!(
        calls.count(entity, "on_landed"),
        0,
        "the interval was not waited out"
    );

    tick(&mut app, 12);
    assert_eq!(calls.count(entity, "on_landed"), 1, "the call is late");
    assert_eq!(
        calls.order(entity),
        vec!["on_started".to_string(), "on_landed".to_string()],
        "the calls came out of order"
    );

    tick(&mut app, 40);
    near(height(&app, entity), 0.0, "the step after the call ran");
    assert_eq!(calls.count(entity, "on_landed"), 1, "it fired twice");
}

#[test]
fn a_call_step_takes_no_time_of_its_own() {
    let mut app = app();
    let calls = std::rc::Rc::new(Calls::default());
    app.engine.set_script_host(calls.clone());
    let entity = spawn(&app, "Box");
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "position"
to = [0.0, 1.0, 0.0]
duration = 0.25

[[steps]]
call = "on_half"

[[steps]]
property = "position"
to = [0.0, 2.0, 0.0]
duration = 0.25
"#,
    );
    tick(&mut app, 31);
    assert_eq!(calls.count(entity, "on_half"), 1);
    near(
        height(&app, entity),
        2.0,
        "the whole tween is half a second",
    );
}

#[test]
fn a_step_can_tween_another_node_from_the_same_call() {
    let mut app = app();
    let entity = spawn(&app, "Player");
    let shadow = child(&app, entity, "Shadow");
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "position"
to = [0.0, 4.0, 0.0]
duration = 0.5

[[steps]]
target = "Shadow"
property = "scale"
to = [2.0, 2.0, 2.0]
duration = 0.5
parallel = true
"#,
    );
    tick(&mut app, 31);
    near(height(&app, entity), 4.0, "the player moved");
    near(transform(&app, shadow).scale.x, 2.0, "the shadow scaled");
    near(height(&app, shadow), 0.0, "the shadow was not moved");
}

#[test]
fn a_tween_drives_a_component_property_the_animation_crate_does_not_depend_on() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    let params: toml::Value =
        toml::from_str("kind = \"cuboid\"\ncolor = [0.0, 0.0, 0.0, 1.0]").unwrap();
    components::add(&app.engine, entity, "shape3d", Some(&params)).unwrap();
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "shape3d/color"
to = [1.0, 0.0, 0.0, 1.0]
duration = 0.5
"#,
    );
    tick(&mut app, 31);
    let rgba = components::get(&app.engine, entity, "shape3d")
        .and_then(|table| table.get("color").cloned())
        .unwrap();
    let channels: Vec<f64> = rgba
        .as_array()
        .unwrap()
        .iter()
        .filter_map(balaur_core::components::as_f64)
        .collect();
    assert!(
        (channels[0] - 1.0).abs() < 0.05 && channels[1] < 0.05,
        "the colour did not arrive: {channels:?}"
    );
}

#[test]
fn a_tween_captures_the_component_value_it_starts_from() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    let params: toml::Value =
        toml::from_str("kind = \"cuboid\"\ncolor = [0.0, 1.0, 0.0, 1.0]").unwrap();
    components::add(&app.engine, entity, "shape3d", Some(&params)).unwrap();
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "shape3d/color"
by = [1.0, -1.0, 0.0, 0.0]
duration = 0.5
"#,
    );
    tick(&mut app, 31);
    let rgba = components::get(&app.engine, entity, "shape3d")
        .and_then(|table| table.get("color").cloned())
        .unwrap();
    let channels: Vec<f64> = rgba
        .as_array()
        .unwrap()
        .iter()
        .filter_map(balaur_core::components::as_f64)
        .collect();
    assert!(
        (channels[0] - 1.0).abs() < 0.05 && channels[1] < 0.05,
        "green should have been offset away, not replaced: {channels:?}"
    );
}

#[test]
fn a_looping_tween_plays_its_sequence_that_many_times() {
    let mut app = app();
    let calls = std::rc::Rc::new(Calls::default());
    app.engine.set_script_host(calls.clone());
    let entity = spawn(&app, "Box");
    let id = start(
        &app,
        entity,
        r#"
loops = 3

[[steps]]
property = "position"
to = [0.0, 1.0, 0.0]
duration = 0.2

[[steps]]
call = "on_round"
"#,
    );
    tick(&mut app, 12 * 3 + 6);
    assert_eq!(
        calls.count(entity, "on_round"),
        3,
        "three rounds, three calls"
    );
    assert!(
        !tween::is_running(&app.engine, id),
        "a tween with a loop count should end when it runs out"
    );
}

#[test]
fn speed_scales_the_whole_sequence() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    start(
        &app,
        entity,
        r#"
speed = 2.0

[[steps]]
property = "position"
to = [0.0, 10.0, 0.0]
duration = 1.0
"#,
    );
    tick(&mut app, 15);
    near(height(&app, entity), 5.0, "half a second at double speed");
}

#[test]
fn tween_to_is_the_same_tween_spelled_shorter() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    let to = toml::Value::Array(vec![0.0.into(), 3.0.into(), 0.0.into()]);
    tween::start_to(&app.engine, entity, "position", &to, 0.5, Some("out_back")).unwrap();
    tick(&mut app, 31);
    near(height(&app, entity), 3.0, "the sugar tween landed");
}

#[test]
fn an_eased_step_is_not_where_a_straight_one_would_be() {
    let mut app = app();
    let straight = spawn(&app, "Straight");
    let eased = spawn(&app, "Eased");
    let step = |ease: &str| {
        format!("[[steps]]\nproperty = \"position\"\nto = [0.0, 10.0, 0.0]\nduration = 0.5{ease}\n")
    };
    start(&app, straight, &step(""));
    start(&app, eased, &step("\nease = \"in_quad\""));

    tick(&mut app, 15);
    near(height(&app, straight), 5.0, "the straight line");
    near(
        height(&app, eased),
        2.5,
        "in_quad is a quarter of the way at half the time",
    );

    tick(&mut app, 20);
    near(height(&app, straight), 10.0, "both still arrive");
    near(height(&app, eased), 10.0, "both still arrive");
}

#[test]
fn a_step_that_says_nothing_is_rejected() {
    let app = app();
    let entity = spawn(&app, "Box");
    let why = tween::start(&app.engine, entity, &spec("[[steps]]\nduration = 0.5")).unwrap_err();
    let why = format!("{why:#}");
    assert!(why.contains("property"), "{why}");
    assert!(why.contains("interval") && why.contains("call"), "{why}");
}

#[test]
fn a_step_that_goes_both_to_and_by_is_rejected() {
    let app = app();
    let entity = spawn(&app, "Box");
    let why = tween::start(
        &app.engine,
        entity,
        &spec(
            "[[steps]]\nproperty = \"position\"\nto = [0.0,1.0,0.0]\nby = [0.0,1.0,0.0]\nduration = 0.5",
        ),
    )
    .unwrap_err();
    let why = format!("{why:#}");
    assert!(why.contains("`to`") && why.contains("`by`"), "{why}");
}

#[test]
fn a_tween_of_a_component_the_node_does_not_have_says_so() {
    let app = app();
    let entity = spawn(&app, "Box");
    let why = tween::start(
        &app.engine,
        entity,
        &spec("[[steps]]\nproperty = \"color/rgba\"\nto = [1.0,0.0,0.0,1.0]\nduration = 0.5"),
    )
    .unwrap_err();
    let why = format!("{why:#}");
    assert!(why.contains("color") && why.contains("rgba"), "{why}");
}

#[test]
fn a_step_targeting_a_node_that_is_not_there_says_which_one() {
    let app = app();
    let entity = spawn(&app, "Box");
    let why = tween::start(
        &app.engine,
        entity,
        &spec(
            "[[steps]]\ntarget = \"Ghost\"\nproperty = \"position\"\nto = [0.0,1.0,0.0]\nduration = 0.5",
        ),
    )
    .unwrap_err();
    let why = format!("{why:#}");
    assert!(why.contains("Ghost"), "{why}");
}

#[test]
fn a_transform_step_given_the_wrong_number_of_channels_says_how_many_it_wanted() {
    let app = app();
    let entity = spawn(&app, "Box");
    let why = tween::start(
        &app.engine,
        entity,
        &spec("[[steps]]\nproperty = \"position\"\nto = [1.0, 2.0]\nduration = 0.5"),
    )
    .unwrap_err();
    let why = format!("{why:#}");
    assert!(why.contains("position") && why.contains('3'), "{why}");
}

#[test]
fn a_tween_that_would_run_backwards_forever_is_rejected() {
    let app = app();
    let entity = spawn(&app, "Box");
    let why = tween::start(
        &app.engine,
        entity,
        &spec(
            "speed = -1.0\n[[steps]]\nproperty = \"position\"\nto = [0.0,1.0,0.0]\nduration = 0.5",
        ),
    )
    .unwrap_err();
    let why = format!("{why:#}");
    assert!(why.contains("positive"), "{why}");
}

#[test]
fn a_tween_rotates_from_the_rotation_the_node_already_had() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    app.engine
        .world_mut()
        .get::<&mut Transform>(entity)
        .unwrap()
        .rotation = balaur_anim::sampler::quat_from_euler(Vec3::new(0.0, 0.3, 0.0));
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "rotation_euler"
by = [0.0, 0.4, 0.0]
duration = 0.5
"#,
    );
    tick(&mut app, 31);
    let angles = balaur_anim::sampler::euler_from_quat(transform(&app, entity).rotation);
    near(
        angles.y,
        0.7,
        "four tenths of a radian on from where it was",
    );
}

#[test]
fn a_by_step_on_a_quaternion_rotation_composes_the_turn() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    app.engine
        .world_mut()
        .get::<&mut Transform>(entity)
        .unwrap()
        .rotation = balaur_anim::sampler::quat_from_euler(Vec3::new(0.0, 0.0, 0.5));
    let quarter = balaur_anim::sampler::quat_from_euler(Vec3::new(0.0, 0.0, FRAC_PI_2));
    start(
        &app,
        entity,
        &format!(
            r#"
[[steps]]
property = "rotation"
by = [{}, {}, {}, {}]
duration = 0.5
"#,
            quarter.x, quarter.y, quarter.z, quarter.w
        ),
    );
    tick(&mut app, 31);
    let angles = balaur_anim::sampler::euler_from_quat(transform(&app, entity).rotation);
    near(
        angles.z,
        0.5 + FRAC_PI_2,
        "a quaternion `by` turns by that much, rather than adding four numbers",
    );
}

/// The limit `rotation_euler` inherits from being slerped: a `by` bigger than
/// half a turn arrives by the short way, and a whole turn arrives nowhere.
#[test]
fn a_full_turn_by_on_euler_lands_where_it_started() {
    let mut app = app();
    let entity = spawn(&app, "Box");
    start(
        &app,
        entity,
        r#"
[[steps]]
property = "rotation_euler"
by = [0.0, 0.0, 6.2831853]
duration = 0.5
"#,
    );
    tick(&mut app, 15);
    let half_way = balaur_anim::sampler::euler_from_quat(transform(&app, entity).rotation);
    near(half_way.z, 0.0, "the two ends are the same rotation");
    tick(&mut app, 16);
    let angles = balaur_anim::sampler::euler_from_quat(transform(&app, entity).rotation);
    near(angles.z, 0.0, "and so is the end");
}

#[test]
fn a_delay_holds_the_start_before_the_first_step() {
    let mut app = app();
    let entity = spawn(&app, "Held");
    start(
        &app,
        entity,
        r#"
delay = 0.5
[[steps]]
property = "position"
to = [0.0, 10.0, 0.0]
duration = 0.5
"#,
    );
    tick(&mut app, 30);
    near(
        height(&app, entity),
        0.0,
        "still held at the end of the delay",
    );
    tick(&mut app, 15);
    near(height(&app, entity), 5.0, "halfway up, a quarter second in");
    tick(&mut app, 15);
    near(height(&app, entity), 10.0, "there");
}

#[test]
fn then_waits_for_the_other_tween_and_starts_from_where_it_left_the_node() {
    let mut app = app();
    let entity = spawn(&app, "Chained");
    let first = start(
        &app,
        entity,
        r#"
[[steps]]
property = "position"
to = [0.0, 10.0, 0.0]
duration = 0.5
"#,
    );
    let second = start(
        &app,
        entity,
        &format!(
            r#"
then = {first}
[[steps]]
property = "position"
by = [0.0, 5.0, 0.0]
duration = 0.5
"#
        ),
    );
    tick(&mut app, 15);
    near(height(&app, entity), 5.0, "only the first one moves it");
    assert!(
        tween::is_running(&app.engine, second),
        "waiting is still alive"
    );
    tick(&mut app, 15);
    near(height(&app, entity), 10.0, "the first one lands");
    tick(&mut app, 15);
    near(
        height(&app, entity),
        12.5,
        "`by` counts from where the first one left it",
    );
    tick(&mut app, 15);
    near(height(&app, entity), 15.0, "and the second one lands");
    assert!(!tween::is_running(&app.engine, second));
}

#[test]
fn then_on_a_tween_already_over_starts_at_once() {
    let mut app = app();
    let entity = spawn(&app, "Eager");
    let gone = start(&app, entity, "[[steps]]\ninterval = 0.01\n");
    tick(&mut app, 5);
    assert!(!tween::is_running(&app.engine, gone));
    start(
        &app,
        entity,
        &format!("then = {gone}\n[[steps]]\nproperty = \"position\"\nto = [0.0, 6.0, 0.0]\nduration = 0.5\n"),
    );
    tick(&mut app, 15);
    near(
        height(&app, entity),
        3.0,
        "no waiting on a handle that names nothing",
    );
}

#[test]
fn a_value_tween_is_read_not_written() {
    let mut app = app();
    let from = toml::Value::Float(0.0);
    let to = toml::Value::Float(100.0);
    let id = tween::start_value(&app.engine, &from, &to, 0.5, Some("linear")).unwrap();
    assert_eq!(
        tween::value_of(&app.engine, id),
        Some(balaur_script::Value::Num(0.0))
    );
    tick(&mut app, 15);
    let Some(balaur_script::Value::Num(half)) = tween::value_of(&app.engine, id) else {
        panic!("a number")
    };
    near(half as f32, 50.0, "halfway");
    tick(&mut app, 15);
    assert_eq!(
        tween::value_of(&app.engine, id),
        Some(balaur_script::Value::Num(100.0)),
        "over, and readable for the tick it landed on"
    );
    assert!(!tween::is_running(&app.engine, id));
    tick(&mut app, 1);
    assert_eq!(
        tween::value_of(&app.engine, id),
        None,
        "and gone the tick after"
    );

    let pair = toml::Value::Array(vec![toml::Value::Float(0.0), toml::Value::Float(2.0)]);
    let far = toml::Value::Array(vec![toml::Value::Float(4.0), toml::Value::Float(2.0)]);
    let id = tween::start_value(&app.engine, &pair, &far, 1.0, None).unwrap();
    tick(&mut app, 30);
    let Some(balaur_script::Value::List(mid)) = tween::value_of(&app.engine, id) else {
        panic!("a list")
    };
    assert_eq!(mid.len(), 2, "the shape it was started with");
    let balaur_script::Value::Num(x) = mid[0] else {
        panic!()
    };
    near(x as f32, 2.0, "x halfway");
    assert!(
        tween::start_value(&app.engine, &from, &pair, 1.0, None).is_err(),
        "widths differ"
    );
}
