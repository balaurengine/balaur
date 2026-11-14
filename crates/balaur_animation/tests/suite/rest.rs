//! What animation tells a loop about to sleep: pending while a clip plays, a
//! tween runs or a machine has a state to enter, and at rest after.

use balaur_animation::{AnimationPlugin, machine, tween};
use balaur_core::hecs::Entity;
use balaur_core::process::{self, ProcessMode};
use balaur_core::{App, AppConfig, assets, components, fixed_dt, rest, scene};

/// The name the plugin's probe answers to.
const ANIMATION: &str = "animation";

fn app() -> App {
    let mut app = App::new(AppConfig::bare("tests/fixtures")).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin).unwrap();
    app
}

fn pends(app: &App) -> bool {
    rest::pending_names(&app.engine).contains(&ANIMATION)
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(fixed_dt());
    }
}

/// A clip that lifts its node over `length` seconds.
fn rise(length: f64, loop_mode: &str) -> toml::Value {
    toml::from_str(&format!(
        r#"
        length = {length}
        loop_mode = "{loop_mode}"
        [[tracks]]
        property = "position"
        keys = [{{ time = 0.0, value = [0.0, 0.0, 0.0] }}, {{ time = {length}, value = [0.0, 1.0, 0.0] }}]
        "#
    ))
    .unwrap()
}

/// A node with a player holding `once` (a quarter second) and `round` (looping).
fn player(app: &App, name: &str) -> Entity {
    let eng = &app.engine;
    let entity = scene::spawn_node(&mut eng.world_mut(), name, eng.root());
    components::add(eng, entity, ANIMATION, None).unwrap();
    balaur_animation::add_clip(eng, entity, "once", rise(0.25, "none")).unwrap();
    balaur_animation::add_clip(eng, entity, "round", rise(0.25, "linear")).unwrap();
    entity
}

#[test]
fn a_clip_is_pending_while_it_plays_and_at_rest_once_it_ends() {
    let mut app = app();
    let hero = player(&app, "Hero");
    tick(&mut app, 1);
    assert!(!pends(&app), "a player with nothing playing is at rest");

    balaur_animation::play(&app.engine, hero, "once").unwrap();
    tick(&mut app, 5);
    assert!(pends(&app), "the clip is a third of the way through");

    tick(&mut app, 15);
    assert!(!pends(&app), "it ran out");
}

#[test]
fn a_looping_clip_is_pending_until_it_is_paused_or_stopped() {
    let mut app = app();
    let hero = player(&app, "Hero");
    balaur_animation::play(&app.engine, hero, "round").unwrap();
    tick(&mut app, 40);
    assert!(
        pends(&app),
        "a loop is something moving for as long as it plays"
    );

    balaur_animation::pause(&app.engine, hero);
    tick(&mut app, 1);
    assert!(!pends(&app), "a held playhead moves nothing");

    balaur_animation::resume(&app.engine, hero);
    tick(&mut app, 1);
    assert!(pends(&app));

    balaur_animation::stop(&app.engine, hero);
    tick(&mut app, 1);
    assert!(!pends(&app));
}

#[test]
fn a_tween_is_pending_until_its_last_step_lands() {
    let mut app = app();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "Box", app.engine.root());
    let spec: toml::Value = toml::from_str(
        "[[steps]]\nproperty = \"position\"\nto = [0.0, 2.0, 0.0]\nduration = 0.25\n",
    )
    .unwrap();
    tween::start(&app.engine, entity, &spec).unwrap();

    tick(&mut app, 5);
    assert!(pends(&app), "the tween is on its way");

    tick(&mut app, 15);
    assert!(!pends(&app), "it landed and was let go");
}

#[test]
fn a_machine_sent_travelling_is_pending_until_the_state_it_reaches_has_played_out() {
    let mut app = app();
    let hero = player(&app, "Hero");
    let states = r#"
start = "wait"
[states]
wait = "once"
go = "once"
[[transitions]]
from = "wait"
to = "go"
"#;
    let body: toml::Value = toml::from_str(states).unwrap();
    let reference = assets::define_inline(&app.engine, machine::MACHINE_ASSET_TYPE, body).unwrap();
    let mut params = toml::Table::new();
    params.insert("machine".into(), reference.to_string().into());
    components::add(&app.engine, hero, "state_machine", Some(&params.into())).unwrap();

    tick(&mut app, 2);
    assert!(pends(&app), "it entered its start, whose clip is playing");
    tick(&mut app, 30);
    assert!(!pends(&app), "the start's clip ended and nothing leads on");

    machine::travel(&app.engine, hero, "go").unwrap();
    tick(&mut app, 2);
    assert_eq!(
        machine::current_state(&app.engine, hero).as_deref(),
        Some("go")
    );
    assert!(pends(&app));
    tick(&mut app, 30);
    assert!(!pends(&app));
}

#[test]
fn a_pause_holds_a_playing_clip_at_rest_unless_its_node_runs_always() {
    let mut app = app();
    let hero = player(&app, "Hero");
    balaur_animation::play(&app.engine, hero, "round").unwrap();
    app.engine.set_paused(true);
    tick(&mut app, 1);
    assert!(!pends(&app), "the pause holds the clip");

    process::set(&mut app.engine.world_mut(), hero, ProcessMode::Always);
    tick(&mut app, 1);
    assert!(pends(&app), "an always node plays through it");
}

#[test]
fn a_clip_playing_in_a_game_a_tool_hosts_does_not_keep_the_tool_awake() {
    let mut app = app();
    let hero = player(&app, "Hero");
    balaur_animation::play(&app.engine, hero, "round").unwrap();
    app.engine.set_debug_scope(Some(hero));
    tick(&mut app, 1);
    assert!(!pends(&app));

    app.engine.set_debug_scope(None);
    tick(&mut app, 1);
    assert!(pends(&app));
}
