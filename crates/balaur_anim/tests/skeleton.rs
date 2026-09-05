//! A rig is nodes, so a clip poses it with no new machinery: a track that
//! names a bone by path turns the bone, and the joint palette a skin would
//! deform by follows.

use balaur_anim::AnimationPlugin;
use balaur_core::hecs::Entity;
use balaur_core::scene::{self, Transform};
use balaur_core::skeleton::{angle_about_z, joint_matrices_2d};
use balaur_core::{components, App, AppConfig};
use glamx::Mat3;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(std::path::PathBuf::from("tests/fixtures")))
    .unwrap();
    balaur_plugin::load(&mut app, &mut AnimationPlugin::default()).unwrap();
    app
}

/// A clip on the rig root that swings `Hip/Thigh` from rest to 1.2 radians
/// over one second, and lifts the hip a little with it.
const SWING: &str = r#"
[library]
length = 1.0
loop = "loop"

[[library.tracks]]
target = "Hip/Thigh"
property = "rotation_euler"
keys = [
  { t = 0.0, value = [0.0, 0.0, 0.0] },
  { t = 1.0, value = [0.0, 0.0, 1.2] },
]

[[library.tracks]]
target = "Hip"
property = "position"
keys = [
  { t = 0.0, value = [0.0, 1.0, 0.0] },
  { t = 1.0, value = [0.0, 1.3, 0.0] },
]
"#;

fn bone(app: &App, name: &str, parent: Entity, rest: [f64; 2]) -> Entity {
    let entity = scene::spawn_node(&mut app.engine.world_mut(), name, parent);
    let params: toml::Value =
        toml::from_str(&format!("rest_position = [{}, {}]", rest[0], rest[1])).unwrap();
    components::add(&app.engine, entity, "bone2d", Some(&params)).unwrap();
    scene::propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    entity
}

/// Rig, hip, thigh, skin — the rig at rest and the clip armed but not started.
fn rigged(app: &App) -> (Entity, Entity, Entity, Entity) {
    let root = app.engine.root();
    let rig = scene::spawn_node(&mut app.engine.world_mut(), "Rig", root);
    let hip = bone(app, "Hip", rig, [0.0, 1.0]);
    let thigh = bone(app, "Thigh", hip, [0.0, -1.0]);
    balaur_core::skeleton::apply_rest(&mut app.engine.world_mut(), rig);
    scene::propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let skin = scene::spawn_node(&mut app.engine.world_mut(), "Skin", rig);
    let params: toml::Value = toml::from_str(SWING).unwrap();
    components::add(&app.engine, rig, "animation", Some(&params)).unwrap();
    (rig, hip, thigh, skin)
}

fn palette(app: &App, skin: Entity, rig: Entity, hip: Entity, thigh: Entity) -> Vec<[u32; 9]> {
    let world = app.engine.world();
    joint_matrices_2d(&world, skin, rig, &[Some(hip), Some(thigh)])
        .into_iter()
        .map(|m| m.to_cols_array().map(f32::to_bits))
        .collect()
}

fn run(frames: u32) -> (Vec<[u32; 9]>, f32) {
    let mut app = app();
    let (rig, hip, thigh, skin) = rigged(&app);
    balaur_anim::play(&app.engine, rig, "").unwrap();
    for _ in 0..frames {
        app.tick(1.0 / 60.0);
    }
    let angle = angle_about_z(
        app.engine
            .world()
            .get::<&Transform>(thigh)
            .unwrap()
            .rotation,
    );
    (palette(&app, skin, rig, hip, thigh), angle)
}

#[test]
fn a_clip_turns_a_bone_by_path_and_the_palette_follows() {
    let (palette, angle) = run(30);
    assert!(
        (angle - 0.6).abs() < 0.02,
        "half way is 0.6 rad, not {angle}"
    );
    let identity = Mat3::IDENTITY.to_cols_array().map(f32::to_bits);
    assert_ne!(
        palette[1], identity,
        "the thigh's joint matrix did not move"
    );
    // The hip rose, so its joint moved too; a skin vertex weighted to it
    // follows the hip up.
    assert_ne!(palette[0], identity, "the hip's joint matrix did not move");
}

#[test]
fn two_runs_of_the_same_clip_produce_the_same_palette_bit_for_bit() {
    let (first, _) = run(45);
    let (second, _) = run(45);
    assert_eq!(first, second);
}

#[test]
fn a_rig_that_has_not_started_playing_is_at_rest() {
    let app = app();
    let (rig, hip, thigh, skin) = rigged(&app);
    let identity = Mat3::IDENTITY.to_cols_array().map(f32::to_bits);
    for joint in palette(&app, skin, rig, hip, thigh) {
        assert_eq!(joint, identity);
    }
}
