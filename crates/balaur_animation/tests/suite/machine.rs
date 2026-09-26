//! State machines: a start state, `auto` transitions on conditions and at a
//! clip's end, travel through the states between, and the fade each
//! transition asks for.

use balaur_animation::{AnimationPlugin, machine};
use balaur_core::hecs::Entity;
use balaur_core::{App, AppConfig, assets, components, scene};

fn app() -> App {
    let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures"))).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
}

/// A clip that holds the node at `x`, for `length` seconds.
fn hold(x: f64, length: f64, loop_mode: &str) -> toml::Value {
    toml::from_str(&format!(
        r#"
        length = {length}
        loop_mode = "{loop_mode}"
        [[tracks]]
        property = "position"
        interpolation = "linear"
        keys = [{{ time = 0.0, value = [{x}, 0.0, 0.0] }}, {{ time = {length}, value = [{x}, 0.0, 0.0] }}]
        "#
    ))
    .unwrap()
}

/// A node playing `idle` (x = 0, looping), `walk` (x = 1, looping) and
/// `jump` (x = 2, half a second, once), run by `machine`.
fn rig(app: &App, machine: &str) -> Entity {
    let eng = &app.engine;
    let root = eng.root();
    let entity = scene::spawn_node(&mut eng.world_mut(), "Hero", root);
    components::add(eng, entity, "animation", None).unwrap();
    balaur_animation::add_clip(eng, entity, "idle", hold(0.0, 1.0, "linear")).unwrap();
    balaur_animation::add_clip(eng, entity, "walk", hold(1.0, 1.0, "linear")).unwrap();
    balaur_animation::add_clip(eng, entity, "jump", hold(2.0, 0.5, "none")).unwrap();
    let body: toml::Value = toml::from_str(machine).unwrap();
    let reference = assets::define_inline(eng, machine::MACHINE_ASSET_TYPE, body).unwrap();
    let params = toml::Value::Table(toml::map::Map::from_iter([(
        "machine".to_string(),
        toml::Value::String(reference.to_string()),
    )]));
    components::add(eng, entity, "state_machine", Some(&params)).unwrap();
    entity
}

fn x(app: &App, entity: Entity) -> f32 {
    app.engine
        .world()
        .get::<&scene::Transform>(entity)
        .map_or(f32::NAN, |t| t.position.x)
}

const STATES: &str = r#"
start = "idle"
[states]
idle = "idle"
walk = "walk"
jump = "jump"
"#;

#[test]
fn a_machine_enters_its_start_and_crosses_when_a_condition_comes_on() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nadvance_mode = \"auto\"\ncondition = \"moving\"\nblend_time = 0.5\n"
        ),
    );
    tick(&mut app, 10);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("idle")
    );
    assert_eq!(
        balaur_animation::current_clip(&app.engine, hero).as_deref(),
        Some("idle")
    );
    assert!(x(&app, hero).abs() < 1e-4);

    machine::set_condition(&app.engine, hero, "moving", true).unwrap();
    tick(&mut app, 15);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk")
    );
    // A quarter of a second into a half-second fade: between the two.
    let mid = x(&app, hero);
    assert!(
        mid > 0.2 && mid < 0.8,
        "mid-fade the node sits between the clips, at {mid}"
    );
    tick(&mut app, 30);
    assert!((x(&app, hero) - 1.0).abs() < 1e-4, "the fade has run");
}

#[test]
fn an_at_end_transition_waits_for_the_clip_to_finish() {
    let mut app = app();
    let hero = rig(
        &app,
        &(STATES.replace("start = \"idle\"", "start = \"jump\"")
            + "\n[[transitions]]\nfrom = \"jump\"\nto = \"idle\"\nadvance_mode = \"auto\"\nswitch_mode = \"at_end\"\n"),
    );
    tick(&mut app, 20);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("jump")
    );
    tick(&mut app, 20);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("idle")
    );
    assert_eq!(
        balaur_animation::current_clip(&app.engine, hero).as_deref(),
        Some("idle")
    );
}

#[test]
fn travel_passes_through_the_states_between_and_cuts_to_one_it_cannot_reach() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\n\n[[transitions]]\nfrom = \"walk\"\nto = \"jump\"\n"
        ),
    );
    tick(&mut app, 3);
    machine::travel(&app.engine, hero, "jump").unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk")
    );
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("jump")
    );

    // Nothing leads back from `jump`, so the machine cuts straight to `idle`.
    machine::travel(&app.engine, hero, "idle").unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("idle")
    );
}

#[test]
fn a_transition_to_a_state_the_machine_lacks_is_refused() {
    let body: toml::Value = toml::from_str(&format!(
        "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"swim\"\n"
    ))
    .unwrap();
    let why = machine::parse(&body).unwrap_err();
    assert!(format!("{why:#}").contains("swim"), "{why:#}");
}

/// A clip that carries the node from x = 0 to x = 1 over `length` seconds.
fn ramp(length: f64, loop_mode: &str) -> toml::Value {
    toml::from_str(&format!(
        r#"
        length = {length}
        loop_mode = "{loop_mode}"
        [[tracks]]
        property = "position"
        interpolation = "linear"
        keys = [{{ time = 0.0, value = [0.0, 0.0, 0.0] }}, {{ time = {length}, value = [1.0, 0.0, 0.0] }}]
        "#
    ))
    .unwrap()
}

/// [`rig`], with `walk` swapped for a clip of the test's own.
fn rig_walking(app: &App, walk: toml::Value, machine: &str) -> Entity {
    let hero = rig(app, machine);
    balaur_animation::add_clip(&app.engine, hero, "walk", walk).unwrap();
    hero
}

#[test]
fn a_transition_that_does_not_reset_resumes_the_state_where_it_was_left() {
    let mut app = app();
    let hero = rig_walking(
        &app,
        ramp(1.0, "linear"),
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nreset = false\n\n[[transitions]]\nfrom = \"walk\"\nto = \"idle\"\n"
        ),
    );
    tick(&mut app, 2);
    machine::travel(&app.engine, hero, "walk").unwrap();
    tick(&mut app, 31);
    let left_at = balaur_animation::time(&app.engine, hero);
    machine::travel(&app.engine, hero, "idle").unwrap();
    tick(&mut app, 5);
    machine::travel(&app.engine, hero, "walk").unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk")
    );
    let resumed = balaur_animation::time(&app.engine, hero);
    assert!(
        (resumed - left_at).abs() < 0.05,
        "`walk` picks up near {left_at}, not from the start: {resumed}"
    );
}

#[test]
fn the_lowest_priority_auto_transition_wins() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nadvance_mode = \"auto\"\npriority = 2\n\n[[transitions]]\nfrom = \"idle\"\nto = \"jump\"\nadvance_mode = \"auto\"\npriority = 1\n"
        ),
    );
    tick(&mut app, 2);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("jump")
    );
}

#[test]
fn travel_takes_the_cheapest_chain_by_priority() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"jump\"\npriority = 3\n\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\n\n[[transitions]]\nfrom = \"walk\"\nto = \"jump\"\n"
        ),
    );
    tick(&mut app, 2);
    machine::travel(&app.engine, hero, "jump").unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk"),
        "two hops at 1 cost less than one at 3"
    );
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("jump")
    );
}

#[test]
fn an_eased_fade_follows_its_curve() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nadvance_mode = \"auto\"\ncondition = \"moving\"\nblend_time = 0.5\nease = \"in_quad\"\n"
        ),
    );
    tick(&mut app, 2);
    machine::set_condition(&app.engine, hero, "moving", true).unwrap();
    tick(&mut app, 16);
    let mid = x(&app, hero);
    assert!(
        (mid - 0.25).abs() < 0.02,
        "halfway through an in_quad fade the weight is a quarter: {mid}"
    );
}

#[test]
fn an_unknown_fade_curve_is_refused() {
    let body: toml::Value = toml::from_str(&format!(
        "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nease = \"sideways\"\n"
    ))
    .unwrap();
    let why = machine::parse(&body).unwrap_err();
    assert!(format!("{why:#}").contains("sideways"), "{why:#}");
}

#[test]
fn break_loop_holds_a_looping_clip_at_its_end_while_it_fades_out() {
    let pose_after_fade = |break_loop: bool| {
        let mut app = app();
        let hero = rig(
            &app,
            &format!(
                "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nblend_time = 1.0\nbreak_loop_at_end = {break_loop}\n"
            ),
        );
        balaur_animation::add_clip(&app.engine, hero, "idle", ramp(0.5, "linear")).unwrap();
        tick(&mut app, 16);
        machine::travel(&app.engine, hero, "walk").unwrap();
        tick(&mut app, 31);
        x(&app, hero)
    };
    let held = pose_after_fade(true);
    assert!(
        (held - 1.0).abs() < 1e-3,
        "`idle` stops at its end, where `walk` also is: {held}"
    );
    let wrapped = pose_after_fade(false);
    assert!(
        wrapped < 0.9,
        "without it `idle` wraps back towards 0 mid-fade: {wrapped}"
    );
}

#[test]
fn the_machine_says_which_state_it_left_and_which_it_entered() {
    let mut app = app();
    let calls = std::rc::Rc::new(crate::common::Calls::default());
    app.engine.set_script_host(calls.clone());
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nadvance_mode = \"auto\"\ncondition = \"moving\"\n"
        ),
    );
    // The clips the states play announce too; this is about the states.
    let states = || -> Vec<String> {
        let order = calls.order(hero).into_iter();
        order.filter(|m| m.starts_with("on_state_")).collect()
    };
    tick(&mut app, 2);
    assert_eq!(states(), ["on_state_started"]);
    machine::set_condition(&app.engine, hero, "moving", true).unwrap();
    tick(&mut app, 1);
    assert_eq!(
        states(),
        ["on_state_started", "on_state_finished", "on_state_started"]
    );
    let text = |s: &str| Some(vec![balaur_script::Value::Str(s.to_string())]);
    assert_eq!(calls.args(hero, "on_state_finished"), text("idle"));
}

#[test]
fn travel_on_the_frame_a_machine_is_turned_on_waits_for_it_to_load() {
    let mut app = app();
    let hero = rig(&app, STATES);
    let active = |on: bool| {
        toml::Value::Table(toml::map::Map::from_iter([(
            "enabled".to_string(),
            on.into(),
        )]))
    };
    // Never on, so it has not loaded its asset.
    components::patch(&app.engine, hero, "state_machine", &active(false)).unwrap();
    tick(&mut app, 2);

    components::patch(&app.engine, hero, "state_machine", &active(true)).unwrap();
    machine::travel(&app.engine, hero, "walk").unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk")
    );
}

#[test]
fn among_equal_priorities_a_transition_ready_now_goes_first() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"jump\"\nadvance_mode = \"auto\"\nswitch_mode = \"at_end\"\n\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nadvance_mode = \"auto\"\ncondition = \"moving\"\n"
        ),
    );
    tick(&mut app, 2);
    machine::set_condition(&app.engine, hero, "moving", true).unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk"),
        "the at_end one authored first does not hold the ready one back"
    );
}

#[test]
fn reaching_end_stops_the_machine_until_a_travel() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"end\"\nadvance_mode = \"auto\"\ncondition = \"done\"\n"
        ),
    );
    tick(&mut app, 2);
    machine::set_condition(&app.engine, hero, "done", true).unwrap();
    tick(&mut app, 1);
    assert_eq!(machine::current_state(&app.engine, hero), None);
    assert!(
        !balaur_animation::is_playing(&app.engine, hero),
        "the clip holds"
    );
    tick(&mut app, 10);
    assert_eq!(
        machine::current_state(&app.engine, hero),
        None,
        "an ended machine does not start over on its own"
    );

    machine::travel(&app.engine, hero, "walk").unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk")
    );
}

/// `move` is a machine of its own: `walk`, and `jump` on `hop`.
const NESTED: &str = r#"
start = "idle"
[states]
idle = ""
[states.move]
start = "walk"
[states.move.states]
walk = ""
jump = ""
[[states.move.transitions]]
from = "walk"
to = "jump"
advance_mode = "auto"
condition = "hop"

[[transitions]]
from = "idle"
to = "move"
advance_mode = "auto"
condition = "go"
[[transitions]]
from = "move"
to = "idle"
advance_mode = "auto"
condition = "stop"
"#;

#[test]
fn a_nested_machine_is_entered_at_its_start_and_left_from_any_state_in_it() {
    let mut app = app();
    let hero = rig(&app, NESTED);
    tick(&mut app, 2);
    machine::set_condition(&app.engine, hero, "go", true).unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("move/walk")
    );
    assert_eq!(
        balaur_animation::current_clip(&app.engine, hero).as_deref(),
        Some("walk"),
        "a nested state plays the clip of its own name"
    );
    machine::set_condition(&app.engine, hero, "hop", true).unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("move/jump")
    );
    machine::set_condition(&app.engine, hero, "go", false).unwrap();
    machine::set_condition(&app.engine, hero, "stop", true).unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("idle")
    );

    machine::set_condition(&app.engine, hero, "stop", false).unwrap();
    machine::travel(&app.engine, hero, "move").unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("move/walk"),
        "travelling to a group lands on its start"
    );
}

#[test]
fn a_check_holds_an_auto_transition_until_the_script_answers_true() {
    let mut app = app();
    let calls = std::rc::Rc::new(crate::common::Calls::default());
    app.engine.set_script_host(calls.clone());
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nadvance_mode = \"auto\"\ncheck = \"can_walk\"\n"
        ),
    );
    tick(&mut app, 3);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("idle")
    );
    assert!(calls.count(hero, "can_walk") > 0, "the script was asked");

    calls.answer("can_walk", balaur_script::Value::Bool(true));
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk")
    );
}

#[test]
fn a_fade_curve_shapes_the_fade_in_place_of_its_ease() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"walk\"\nadvance_mode = \"auto\"\ncondition = \"moving\"\nblend_time = 0.5\nblend_curve = [[0.0, 0.0], [0.5, 0.9], [1.0, 1.0]]\n"
        ),
    );
    tick(&mut app, 2);
    machine::set_condition(&app.engine, hero, "moving", true).unwrap();
    tick(&mut app, 16);
    let mid = x(&app, hero);
    assert!(
        (mid - 0.9).abs() < 0.02,
        "halfway through, the curve says 0.9: {mid}"
    );
}

#[test]
fn a_machine_refuses_what_end_and_nesting_cannot_mean() {
    let refused = |body: &str| {
        let body: toml::Value = toml::from_str(body).unwrap();
        machine::parse(&body).is_err()
    };
    assert!(
        refused("[states]\nend = \"\"\n"),
        "`end` is not a state name"
    );
    assert!(refused(&format!(
        "{STATES}\n[[transitions]]\nfrom = \"end\"\nto = \"idle\"\n"
    )));
    assert!(
        refused(&NESTED.replace(
            "[[states.move.transitions]]\nfrom = \"walk\"\nto = \"jump\"",
            "[[states.move.transitions]]\nfrom = \"walk\"\nto = \"end\""
        )),
        "a nested machine is left from its group, not ended"
    );
    assert!(
        refused("[states.move]\n[states.move.states]\nwalk = \"\"\n"),
        "a nested machine needs a start"
    );
}

#[test]
fn an_ended_machine_stays_ended_across_a_rollback() {
    let mut app = app();
    let hero = rig(
        &app,
        &format!(
            "{STATES}\n[[transitions]]\nfrom = \"idle\"\nto = \"end\"\nadvance_mode = \"auto\"\n"
        ),
    );
    tick(&mut app, 3);
    assert_eq!(machine::current_state(&app.engine, hero), None);
    let frame = balaur_core::snapshot::capture(&app.engine);

    machine::travel(&app.engine, hero, "walk").unwrap();
    tick(&mut app, 1);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("walk")
    );

    balaur_core::snapshot::restore(&app.engine, &frame);
    tick(&mut app, 5);
    assert_eq!(
        machine::current_state(&app.engine, hero),
        None,
        "restored, it is ended again rather than back at its start"
    );
}
