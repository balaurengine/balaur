//! What a rollback and a desync check see of the animation player.
//!
//! A restored world has to restore the playhead that posed it: rewinding a
//! kinematic platform's transform and leaving its clip where it was makes the
//! re-simulation animate from the wrong time, and every body it pushes
//! diverges from there.

use balaur_anim::{AnimationPlugin, AnimationState};
use balaur_core::components::StableId;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::{components, digest, snapshot, App, AppConfig};

fn app() -> App {
    let mut app = App::new(AppConfig {
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

/// A node with a stable id, which is what the snapshot keys on.
fn animated(app: &App, id: &str, params: &str) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), id, root);
    app.engine
        .world_mut()
        .insert_one(entity, StableId(id.to_string()))
        .unwrap();
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "animation", Some(&params)).unwrap();
    entity
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
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
fn a_rollback_across_a_playing_clip_restores_the_playhead() {
    let mut app = app();
    let node = animated(&app, "n_platform", &rise("loop"));
    tick(&mut app, 10);
    let at = balaur_anim::time(&app.engine, node);
    let frame = snapshot::capture(&app.engine);

    tick(&mut app, 20);
    assert_ne!(
        balaur_anim::time(&app.engine, node).to_bits(),
        at.to_bits(),
        "twenty more steps have to move the playhead, or the test proves nothing"
    );

    snapshot::restore(&app.engine, &frame);
    assert_eq!(balaur_anim::time(&app.engine, node).to_bits(), at.to_bits());
    assert!(balaur_anim::is_playing(&app.engine, node));
}

#[test]
fn re_simulating_from_a_snapshot_reaches_the_same_pose() {
    let mut app = app();
    let node = animated(&app, "n_platform", &rise("loop"));
    tick(&mut app, 10);
    let frame = snapshot::capture(&app.engine);
    tick(&mut app, 20);
    let expected = height(&app, node).to_bits();

    snapshot::restore(&app.engine, &frame);
    tick(&mut app, 20);

    assert_eq!(
        height(&app, node).to_bits(),
        expected,
        "the same twenty steps from the same state have to land on the same bits"
    );
}

#[test]
fn a_paused_player_digests_differently_from_a_playing_one() {
    let mut app = app();
    let node = animated(&app, "n_platform", &rise("loop"));
    tick(&mut app, 10);
    let playing = digest::digest(&app.engine);
    let pose = height(&app, node).to_bits();

    balaur_anim::pause(&app.engine, node);

    assert_eq!(
        height(&app, node).to_bits(),
        pose,
        "pausing moves nothing, so only the player's own state can differ"
    );
    assert_ne!(digest::digest(&app.engine).0, playing.0);
}

#[test]
fn a_stopped_clip_and_a_played_one_at_the_same_pose_digest_differently() {
    let mut app = app();
    let node = animated(&app, "n_platform", &rise("loop"));
    tick(&mut app, 10);
    let before = digest::entries(&app.engine);
    balaur_anim::stop(&app.engine, node);
    let after = digest::entries(&app.engine);

    let divergence = digest::first_divergence(&before, &after).expect("the player's row changed");
    assert!(
        divergence.starts_with("n_platform/animation"),
        "the row that moved should be the player's, not the transform's: {divergence}"
    );
}

#[test]
fn the_snapshot_carries_the_fixed_step_residual() {
    let mut app = app();
    animated(&app, "n_platform", &rise("loop"));
    // A frame that does not divide the fixed step leaves time owed; a replay
    // or a rollback that dropped it would take its steps on other frames.
    app.tick(0.03);
    let frame = snapshot::capture(&app.engine);
    let residual = frame.0["animation"]["accumulator"].as_f64().unwrap();
    assert!(residual > 0.0, "0.03 is not a whole step: {residual}");

    app.tick(0.03);
    snapshot::restore(&app.engine, &frame);
    let restored = snapshot::capture(&app.engine).0["animation"]["accumulator"]
        .as_f64()
        .unwrap();
    assert_eq!(restored.to_bits(), residual.to_bits());
}

#[test]
fn a_tween_that_finished_after_the_snapshot_comes_back_with_it() {
    let mut app = app();
    let node = animated(&app, "n_platform", "library = \"\"\n");
    let to = toml::Value::Array(vec![0.0.into(), 4.0.into(), 0.0.into()]);
    let tween =
        balaur_anim::tween::start_to(&app.engine, node, "position", &to, 0.2, None).unwrap();
    tick(&mut app, 2);
    let frame = snapshot::capture(&app.engine);

    tick(&mut app, 30);
    assert!(
        !balaur_anim::tween::is_running(&app.engine, tween),
        "thirty steps outlast a tween of a fifth of a second"
    );

    snapshot::restore(&app.engine, &frame);
    assert!(balaur_anim::tween::is_running(&app.engine, tween));
    tick(&mut app, 30);
    assert!(
        (height(&app, node) - 4.0).abs() < 1e-5,
        "the restored tween ran again to its end"
    );
}

#[test]
fn a_player_on_a_node_the_snapshot_never_saw_is_dropped_by_the_restore() {
    let mut app = app();
    let first = animated(&app, "n_first", &rise("loop"));
    tick(&mut app, 5);
    let frame = snapshot::capture(&app.engine);

    let second = animated(&app, "n_second", &rise("loop"));
    tick(&mut app, 5);
    assert!(balaur_anim::is_playing(&app.engine, second));

    snapshot::restore(&app.engine, &frame);
    let state = app.engine.resource::<AnimationState>();
    let state = state.borrow();
    assert!(state.players.contains_key(&first));
    assert!(
        !state.players.contains_key(&second),
        "a player spawned after the snapshot is not part of the world it restores"
    );
}
