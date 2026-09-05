//! Positional audio without a sound card: what a frame decides about a
//! placed sound — its distance gain, its pan and its doppler — is kept on the
//! emitter, so every assertion here reads the same numbers a machine with
//! speakers would hear.

use std::path::Path;

use balaur_audio::spatial::{self, Emitter, ListenerPose};
use balaur_audio::{AudioPlugin, AudioState, Cue};
use balaur_core::glamx::Vec3;
use balaur_core::hecs::Entity;
use balaur_core::{components, scene, App, AppConfig, Transform};

/// A valid 16-bit mono PCM wav of a few silent samples, so a machine that
/// does have a device decodes something real.
fn write_wav(dir: &Path, name: &str) {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&52u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8_000u32.to_le_bytes());
    bytes.extend_from_slice(&16_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 16]);
    std::fs::write(dir.join(name), bytes).unwrap();
}

fn app_in(dir: &Path) -> App {
    let mut app = App::new(AppConfig::bare(dir.to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut AudioPlugin::default()).unwrap();
    app
}

fn ears_at(position: Vec3) -> ListenerPose {
    let mut pose = ListenerPose::default();
    pose.place(position);
    pose
}

fn node_at(app: &App, name: &str, position: Vec3) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), name, root);
    move_to(app, entity, position);
    entity
}

fn move_to(app: &App, entity: Entity, position: Vec3) {
    let world = app.engine.world();
    let mut transform = world.get::<&mut Transform>(entity).unwrap();
    transform.position = position;
}

const POSITIONAL: &str = r#"
file = "chime.wav"
autoplay = true
loop = true
positional = true
min_distance = 1.0
max_distance = 100.0
"#;

fn placed_sound(app: &App, params: &str, position: Vec3) -> (Entity, u64) {
    let entity = node_at(app, "Emitter", position);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "sound", Some(&params)).unwrap();
    let state = app.engine.resource::<AudioState>();
    let handle = state
        .borrow()
        .nodes
        .get(&entity)
        .and_then(|sound| sound.handle)
        .expect("autoplay started a playback");
    (entity, handle)
}

fn gain_of(app: &App, handle: u64) -> f32 {
    app.engine
        .resource::<AudioState>()
        .borrow()
        .placement_of(handle)
        .expect("a positional handle keeps its placement")
        .gain
}

fn pan_of(app: &App, handle: u64) -> f32 {
    app.engine
        .resource::<AudioState>()
        .borrow()
        .placement_of(handle)
        .expect("a positional handle keeps its placement")
        .pan
}

#[test]
fn a_sound_within_min_distance_is_at_full_volume_and_centred() {
    let listener = ears_at(Vec3::ZERO);
    let placement = spatial::place(
        &listener,
        &Emitter::new(Vec3::new(0.5, 0.0, 0.0), 1.0, 50.0, 0.0),
    );
    assert!((placement.gain - 1.0).abs() < 1e-6, "{placement:?}");
    assert!(placement.pan.abs() < 0.51, "half a unit is not a hard pan");
    assert!((placement.pitch - 1.0).abs() < 1e-6, "nothing is moving");
}

#[test]
fn volume_halves_with_every_doubling_past_min_distance() {
    let listener = ears_at(Vec3::ZERO);
    let at = |distance: f32| {
        spatial::place(
            &listener,
            &Emitter::new(Vec3::new(0.0, 0.0, distance), 1.0, 1000.0, 0.0),
        )
        .gain
    };
    assert!((at(2.0) - 0.5).abs() < 1e-6, "{}", at(2.0));
    assert!((at(4.0) - 0.25).abs() < 1e-6, "{}", at(4.0));
    assert!(at(8.0) < at(4.0));
}

#[test]
fn a_sound_past_max_distance_is_silent() {
    let listener = ears_at(Vec3::ZERO);
    let emitter = Emitter::new(Vec3::new(0.0, 0.0, 60.0), 1.0, 50.0, 0.0);
    assert!(spatial::place(&listener, &emitter).gain < 1e-6);
}

/// The listener's right is where the pan comes from, so a sound off to that
/// side has to reach the right ear louder.
#[test]
fn a_sound_to_the_listeners_right_leans_right() {
    let listener = ears_at(Vec3::ZERO);
    let right = spatial::place(
        &listener,
        &Emitter::new(Vec3::new(10.0, 0.0, 0.0), 1.0, 50.0, 0.0),
    );
    let left = spatial::place(
        &listener,
        &Emitter::new(Vec3::new(-10.0, 0.0, 0.0), 1.0, 50.0, 0.0),
    );
    assert!((right.pan - 1.0).abs() < 1e-6, "{right:?}");
    assert!((left.pan + 1.0).abs() < 1e-6, "{left:?}");

    let [l, r] = spatial::stereo_gains(right.pan);
    assert!(r > l, "a sound on the right is louder on the right");
    let ahead = spatial::place(
        &listener,
        &Emitter::new(Vec3::new(0.0, 0.0, 10.0), 1.0, 50.0, 0.0),
    );
    assert!(ahead.pan.abs() < 1e-6, "straight ahead is centred");
}

#[test]
fn stereo_gains_keep_their_power_at_any_pan() {
    for pan in [-1.5, -1.0, -0.3, 0.0, 0.4, 1.0, 2.0] {
        let [l, r] = spatial::stereo_gains(pan);
        let power = l * l + r * r;
        assert!((power - 1.0).abs() < 1e-5, "pan {pan} has power {power}");
    }
}

#[test]
fn closing_raises_the_pitch_and_opening_lowers_it() {
    let listener = ears_at(Vec3::ZERO);
    let mut approaching = Emitter::new(Vec3::new(0.0, 0.0, 50.0), 1.0, 500.0, 1.0);
    approaching.velocity = Vec3::new(0.0, 0.0, -30.0);
    let mut receding = approaching.clone();
    receding.velocity = -approaching.velocity;

    assert!(spatial::place(&listener, &approaching).pitch > 1.0);
    assert!(spatial::place(&listener, &receding).pitch < 1.0);
}

#[test]
fn doppler_stays_off_until_a_sound_asks_for_it() {
    let listener = ears_at(Vec3::ZERO);
    let mut emitter = Emitter::new(Vec3::new(0.0, 0.0, 50.0), 1.0, 500.0, 0.0);
    emitter.velocity = Vec3::new(0.0, 0.0, -300.0);
    assert!((spatial::place(&listener, &emitter).pitch - 1.0).abs() < 1e-6);
}

/// A pitch a listener could not survive is a bug in the scene, not something
/// to reproduce faithfully.
#[test]
fn a_teleporting_emitter_cannot_screech() {
    let listener = ears_at(Vec3::ZERO);
    let mut emitter = Emitter::new(Vec3::new(0.0, 0.0, 50.0), 1.0, 500.0, 1.0);
    emitter.velocity = Vec3::new(0.0, 0.0, -100_000.0);
    let pitch = spatial::place(&listener, &emitter).pitch;
    assert!((0.5..=2.0).contains(&pitch), "{pitch}");
}

/// A game that never placed its ears should still be audible.
#[test]
fn with_no_listener_a_positional_sound_plays_flat() {
    let placement = spatial::place(
        &ListenerPose::default(),
        &Emitter::new(Vec3::new(0.0, 0.0, 900.0), 1.0, 50.0, 1.0),
    );
    assert!((placement.gain - 1.0).abs() < 1e-6);
    assert!(placement.pan.abs() < 1e-6);
    assert!((placement.pitch - 1.0).abs() < 1e-6);
}

#[test]
fn a_positional_sound_follows_the_node_that_carries_it() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let mut app = app_in(dir.path());
    let listener = node_at(&app, "Ears", Vec3::ZERO);
    components::add(&app.engine, listener, "listener", None).unwrap();
    let (emitter, handle) = placed_sound(&app, POSITIONAL, Vec3::new(2.0, 0.0, 0.0));

    app.tick(1.0 / 60.0);
    let near = gain_of(&app, handle);
    assert!(near < 1.0 && near > 0.0, "{near}");
    assert!(pan_of(&app, handle) > 0.0, "it is off to the right");

    move_to(&app, emitter, Vec3::new(-40.0, 0.0, 0.0));
    app.tick(1.0 / 60.0);
    assert!(gain_of(&app, handle) < near, "further away is quieter");
    assert!(pan_of(&app, handle) < 0.0, "and now on the left");
}

#[test]
fn the_ears_follow_the_listener_node() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let mut app = app_in(dir.path());
    let ears = node_at(&app, "Ears", Vec3::new(30.0, 0.0, 0.0));
    components::add(&app.engine, ears, "listener", None).unwrap();
    let (_, handle) = placed_sound(&app, POSITIONAL, Vec3::new(30.0, 0.0, 0.0));

    app.tick(1.0 / 60.0);
    assert!(
        (gain_of(&app, handle) - 1.0).abs() < 1e-6,
        "the listener is standing on it"
    );

    move_to(&app, ears, Vec3::new(-30.0, 0.0, 0.0));
    app.tick(1.0 / 60.0);
    assert!(gain_of(&app, handle) < 0.1, "walking away turns it down");
}

#[test]
fn the_last_current_listener_is_the_one_heard_from() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let mut app = app_in(dir.path());
    let first = node_at(&app, "Ears", Vec3::new(50.0, 0.0, 0.0));
    components::add(&app.engine, first, "listener", None).unwrap();
    let second = node_at(&app, "OtherEars", Vec3::ZERO);
    components::add(&app.engine, second, "listener", None).unwrap();
    let (_, handle) = placed_sound(&app, POSITIONAL, Vec3::ZERO);

    app.tick(1.0 / 60.0);
    assert!((gain_of(&app, handle) - 1.0).abs() < 1e-6);

    let off: toml::Value = toml::from_str("current = false").unwrap();
    components::add(&app.engine, second, "listener", Some(&off)).unwrap();
    app.tick(1.0 / 60.0);
    assert!(
        gain_of(&app, handle) < 1.0,
        "the remaining listener is 50 units away"
    );
}

#[test]
fn a_sound_the_scene_did_not_place_keeps_no_emitter() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let app = app_in(dir.path());
    let (_, handle) = placed_sound(&app, "file = \"chime.wav\"\nautoplay = true\n", Vec3::ZERO);
    assert!(
        app.engine
            .resource::<AudioState>()
            .borrow()
            .placement_of(handle)
            .is_none(),
        "a flat sound has nowhere to be"
    );
}

#[test]
fn stopping_a_positional_handle_forgets_where_it_was() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let app = app_in(dir.path());
    let (_, handle) = placed_sound(&app, POSITIONAL, Vec3::new(3.0, 0.0, 0.0));
    let state = app.engine.resource::<AudioState>();
    assert!(state.borrow().emitter_position(handle).is_some());

    state.borrow_mut().stop(handle);
    assert!(state.borrow().emitter_position(handle).is_none());
    assert!(state.borrow().placement_of(handle).is_none());
}

#[test]
fn freeing_a_listener_node_leaves_the_ears_where_they_were() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let mut app = app_in(dir.path());
    let ears = node_at(&app, "Ears", Vec3::new(10.0, 0.0, 0.0));
    components::add(&app.engine, ears, "listener", None).unwrap();
    app.tick(1.0 / 60.0);

    scene::free_subtree(&mut app.engine.world_mut(), ears);
    app.tick(1.0 / 60.0);

    let state = app.engine.resource::<AudioState>();
    let state = state.borrow();
    assert!(
        state.listener().placed,
        "the ears are still where they were"
    );
    assert!((state.listener().position.x - 10.0).abs() < 1e-6);
}

/// A one-shot at a point in the world: no node, no component, just a place.
#[test]
fn a_cue_can_be_played_at_a_point_with_no_node_behind_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = app_in(dir.path());
    let state = app.engine.resource::<AudioState>();
    state.borrow_mut().set_listener(Vec3::ZERO);
    let handle = state.borrow_mut().play_cue(
        Vec::new(),
        Cue {
            emitter: Some(Emitter::new(Vec3::new(80.0, 0.0, 0.0), 1.0, 50.0, 0.0)),
            ..Cue::default()
        },
    );
    assert_eq!(
        state.borrow().placement_of(handle).map(|p| p.gain),
        Some(0.0),
        "placed as it starts, not a frame later"
    );

    state
        .borrow_mut()
        .set_emitter_position(handle, Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(
        state.borrow().emitter_position(handle),
        Some(Vec3::new(1.0, 0.0, 0.0))
    );
}

/// Moving the ears by hand is for a game whose view is not a node; a scene
/// with a `listener` in it takes the position back on the next frame.
#[test]
fn a_listener_node_overrules_one_set_by_hand() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let mut app = app_in(dir.path());
    let ears = node_at(&app, "Ears", Vec3::new(7.0, 0.0, 0.0));
    components::add(&app.engine, ears, "listener", None).unwrap();
    let state = app.engine.resource::<AudioState>();
    state.borrow_mut().set_listener(Vec3::new(-100.0, 0.0, 0.0));
    assert!((state.borrow().listener().position.x + 100.0).abs() < 1e-6);

    app.tick(1.0 / 60.0);
    assert!((state.borrow().listener().position.x - 7.0).abs() < 1e-6);
}

/// A scene's first `update` runs before that frame's scene sync, so a
/// listener has to place the ears as it is applied — or the one-shot a script
/// fires there is heard from the origin.
#[test]
fn a_listener_places_the_ears_as_soon_as_it_is_applied() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let app = app_in(dir.path());
    let ears = node_at(&app, "Ears", Vec3::new(100.0, 0.0, 0.0));
    components::add(&app.engine, ears, "listener", None).unwrap();
    let (_, handle) = placed_sound(&app, POSITIONAL, Vec3::new(150.0, 0.0, 0.0));

    let gain = gain_of(&app, handle);
    assert!(gain < 0.5, "fifty units away, with no tick yet: {gain}");
    assert!(pan_of(&app, handle) > 0.0, "and off to the right");
}
