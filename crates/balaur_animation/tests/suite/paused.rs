//! A paused game holds what is playing, unless the node says otherwise.

use balaur_animation::AnimationPlugin;
use balaur_core::hecs::Entity;
use balaur_core::process::{self, ProcessMode};
use balaur_core::scene::{self, Transform};
use balaur_core::{App, AppConfig, components};

fn app() -> App {
    let mut app = App::new(AppConfig::bare("tests/fixtures")).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin).unwrap();
    app
}

/// A node playing a clip that lifts it ten units over one second.
fn rising(app: &App, name: &str) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    let params: toml::Value = toml::from_str(
        r#"
autoplay = "rise"

[library.rise]
length = 1.0

[[library.rise.tracks]]
property = "position"
keys = [
  { time = 0.0, value = [0.0, 0.0, 0.0] },
  { time = 1.0, value = [0.0, 10.0, 0.0] },
]
"#,
    )
    .unwrap();
    components::add(&app.engine, entity, "animation", Some(&params)).unwrap();
    entity
}

fn height(app: &App, entity: Entity) -> f32 {
    app.engine
        .world()
        .get::<&Transform>(entity)
        .unwrap()
        .position
        .y
}

#[test]
fn a_pause_holds_a_clip_and_an_always_node_keeps_playing() {
    let mut app = app();
    let game = rising(&app, "Hero");
    let menu = rising(&app, "Menu");
    process::set(&mut app.engine.world_mut(), menu, ProcessMode::Always);

    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    let held_at = height(&app, game);
    assert!(held_at > 0.0, "it was playing before the pause");

    app.engine.set_paused(true);
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }
    assert!(
        (height(&app, game) - held_at).abs() < f32::EPSILON,
        "a held clip does not advance"
    );
    assert!(
        height(&app, menu) > held_at,
        "and one on an always node does"
    );

    app.engine.set_paused(false);
    app.tick(1.0 / 60.0);
    assert!(height(&app, game) > held_at, "and it plays again");
}

/// A tween that lifts its node ten units over half a second.
const LIFT: &str = r#"
[[steps]]
property = "position"
to = [0.0, 10.0, 0.0]
duration = 0.5
"#;

// A debugger stop or a paused replay holds the game a tool hosts, and only
// it: the tool's own clips and tweens, outside the held subtree, play on.
#[test]
fn a_hold_freezes_the_held_game_and_the_tool_around_it_plays_on() {
    let mut app = app();
    let game = rising(&app, "Game");
    let shell = rising(&app, "Shell");
    let held_tween = scene::spawn_node(&mut app.engine.world_mut(), "Held", game);
    let free_tween = scene::spawn_node(&mut app.engine.world_mut(), "Free", shell);
    app.engine.set_debug_scope(Some(game));
    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
    let lift: toml::Value = toml::from_str(LIFT).unwrap();
    let held = balaur_animation::tween::start(&app.engine, held_tween, &lift).unwrap();
    let free = balaur_animation::tween::start(&app.engine, free_tween, &lift).unwrap();
    let game_at = height(&app, game);
    let shell_at = height(&app, shell);
    assert!(game_at > 0.0, "the control: the game's clip was playing");

    for hold in [true, false] {
        // The debugger's hold, then a paused replay's.
        if hold {
            app.engine.set_frozen(true);
        } else {
            app.engine.set_replay_hold(true);
        }
        for _ in 0..40 {
            app.tick(1.0 / 60.0);
        }
        app.engine.set_frozen(false);
        app.engine.set_replay_hold(false);
    }
    assert!(
        (height(&app, game) - game_at).abs() < f32::EPSILON,
        "the held game's clip stood still"
    );
    assert!(
        height(&app, held_tween).abs() < f32::EPSILON,
        "and so did its tween"
    );
    assert!(
        balaur_animation::tween::is_running(&app.engine, held),
        "which is still to run"
    );
    assert!(height(&app, shell) > shell_at, "the tool's clip played on");
    assert!(
        (height(&app, free_tween) - 10.0).abs() < 1e-3,
        "and its tween finished: {}",
        height(&app, free_tween)
    );
    assert!(!balaur_animation::tween::is_running(&app.engine, free));

    app.tick(1.0 / 60.0);
    assert!(height(&app, game) > game_at, "let go, the game plays again");
}
