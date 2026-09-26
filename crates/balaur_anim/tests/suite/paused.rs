//! A paused game holds what is playing, unless the node says otherwise.

use balaur_anim::AnimationPlugin;
use balaur_core::hecs::Entity;
use balaur_core::process::{self, ProcessMode};
use balaur_core::scene::{self, Transform};
use balaur_core::{App, AppConfig, components};

fn app() -> App {
    let mut app = App::new(AppConfig::bare("tests/fixtures")).unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
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
