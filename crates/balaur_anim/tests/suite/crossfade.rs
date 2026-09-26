//! Crossfades: every track either clip keys is posed through the fade, a fade
//! started mid-fade carries on from where the blend was, and a cut drops it.

use balaur_anim::ease::Easing;
use balaur_anim::{AnimationPlugin, AnimationState};
use balaur_core::hecs::Entity;
use balaur_core::{App, AppConfig, components, scene};

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

/// A looping one-second clip holding the node at `x`, plus `extra` tracks.
fn clip(x: f64, extra: &str) -> toml::Value {
    toml::from_str(&format!(
        r#"
        length = 1.0
        loop_mode = "linear"
        [[tracks]]
        property = "position"
        interpolation = "linear"
        keys = [{{ time = 0.0, value = [{x}, 0.0, 0.0] }}, {{ time = 1.0, value = [{x}, 0.0, 0.0] }}]
        {extra}
        "#
    ))
    .unwrap()
}

/// Scale from 1 to 3 over the clip's second.
const GROW: &str = r#"
        [[tracks]]
        property = "scale"
        interpolation = "linear"
        keys = [{ time = 0.0, value = [1.0, 1.0, 1.0] }, { time = 1.0, value = [3.0, 3.0, 3.0] }]
"#;

/// A node with `left` (x = 0, growing), `right` (x = 1) and `far` (x = 2).
fn hero(app: &App) -> Entity {
    let eng = &app.engine;
    let root = eng.root();
    let entity = scene::spawn_node(&mut eng.world_mut(), "Hero", root);
    components::add(eng, entity, "animation", None).unwrap();
    balaur_anim::add_clip(eng, entity, "left", clip(0.0, GROW)).unwrap();
    balaur_anim::add_clip(eng, entity, "right", clip(1.0, "")).unwrap();
    balaur_anim::add_clip(eng, entity, "far", clip(2.0, "")).unwrap();
    entity
}

fn transform(app: &App, entity: Entity) -> scene::Transform {
    *app.engine.world().get::<&scene::Transform>(entity).unwrap()
}

fn fades(app: &App, entity: Entity) -> usize {
    app.engine.resource::<AnimationState>().borrow().players[&entity]
        .fades
        .len()
}

#[test]
fn a_track_only_the_outgoing_clip_keys_keeps_playing_through_the_fade() {
    let mut app = app();
    let hero = hero(&app);
    balaur_anim::play(&app.engine, hero, "left").unwrap();
    tick(&mut app, 15);
    balaur_anim::player::play_blended(&app.engine, hero, "right", 1.0, Easing::LINEAR, true).unwrap();
    tick(&mut app, 30);

    let at = transform(&app, hero);
    assert!(
        (at.position.x - 0.5).abs() < 1e-3,
        "halfway through the fade: {}",
        at.position.x
    );
    assert!(
        (at.scale.x - 2.5).abs() < 1e-3,
        "`right` keys no scale, so `left` goes on growing it: {}",
        at.scale.x
    );
}

#[test]
fn a_fade_started_mid_fade_carries_on_from_where_the_blend_was() {
    let mut app = app();
    let hero = hero(&app);
    balaur_anim::play(&app.engine, hero, "left").unwrap();
    tick(&mut app, 5);
    balaur_anim::player::play_blended(&app.engine, hero, "right", 0.5, Easing::LINEAR, true).unwrap();
    tick(&mut app, 15);
    let before = transform(&app, hero).position.x;
    balaur_anim::player::play_blended(&app.engine, hero, "far", 0.5, Easing::LINEAR, true).unwrap();
    tick(&mut app, 1);
    let after = transform(&app, hero).position.x;
    assert!(
        (after - before).abs() < 0.1,
        "one step moves the node a step, not to `right`: {before} then {after}"
    );
    assert_eq!(fades(&app, hero), 2);

    tick(&mut app, 30);
    assert!((transform(&app, hero).position.x - 2.0).abs() < 1e-5);
    assert_eq!(fades(&app, hero), 0, "a fade that has run is dropped");
}

#[test]
fn a_cut_drops_a_fade_in_progress() {
    let mut app = app();
    let hero = hero(&app);
    balaur_anim::play(&app.engine, hero, "left").unwrap();
    balaur_anim::player::play_blended(&app.engine, hero, "right", 1.0, Easing::LINEAR, true).unwrap();
    tick(&mut app, 10);
    balaur_anim::play(&app.engine, hero, "far").unwrap();
    tick(&mut app, 1);
    assert_eq!(fades(&app, hero), 0);
    assert!((transform(&app, hero).position.x - 2.0).abs() < 1e-6);
}

#[test]
fn a_clip_that_ends_mid_fade_leaves_nothing_to_blend_later() {
    let mut app = app();
    let hero = hero(&app);
    let once: toml::Value = toml::from_str(
        r#"
        length = 0.25
        [[tracks]]
        property = "position"
        keys = [{ time = 0.0, value = [3.0, 0.0, 0.0] }, { time = 0.25, value = [3.0, 0.0, 0.0] }]
        "#,
    )
    .unwrap();
    balaur_anim::add_clip(&app.engine, hero, "once", once).unwrap();
    balaur_anim::play(&app.engine, hero, "left").unwrap();
    balaur_anim::player::play_blended(&app.engine, hero, "once", 1.0, Easing::LINEAR, true).unwrap();
    tick(&mut app, 30);
    assert_eq!(
        balaur_anim::current_clip(&app.engine, hero),
        None,
        "`once` ended"
    );
    assert_eq!(fades(&app, hero), 0);

    balaur_anim::player::play_blended(&app.engine, hero, "far", 0.5, Easing::LINEAR, true).unwrap();
    tick(&mut app, 30);
    assert!(
        (transform(&app, hero).position.x - 2.0).abs() < 1e-5,
        "`left` does not come back into the blend"
    );
}
