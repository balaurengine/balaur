//! The `sound` component, exercised without a sound card: CI has no output
//! device, so every assertion reads the intent the component tracks — the
//! stored properties and the handle a node keeps — never a sink's own state.

use std::path::Path;

use balaur_audio::cache::SoundCache;
use balaur_audio::{AudioPlugin, AudioState};
use balaur_core::hecs::Entity;
use balaur_core::{components, scene, App, AppConfig};

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

fn sound_node(app: &App, params: &str) -> Entity {
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "Chime", root);
    let params: toml::Value = toml::from_str(params).unwrap();
    components::add(&app.engine, entity, "sound", Some(&params)).unwrap();
    entity
}

fn handle_of(app: &App, entity: Entity) -> Option<u64> {
    let state = app.engine.resource::<AudioState>();
    let state = state.borrow();
    state.nodes.get(&entity).and_then(|sound| sound.handle)
}

const CHIME: &str = r#"
file = "chime.wav"
autoplay = true
loop = true
"#;

const GONG: &str = r#"
file = "gong.wav"
autoplay = true
"#;

#[test]
fn a_sound_component_autoplays_and_stop_on_silences_it() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let mut app = app_in(dir.path());
    let entity = sound_node(&app, CHIME);

    let table =
        components::get(&app.engine, entity, "sound").expect("the sound component reports itself");
    assert_eq!(
        table.get("file").and_then(toml::Value::as_str),
        Some("chime.wav")
    );
    assert_eq!(table.get("loop").and_then(toml::Value::as_bool), Some(true));
    let handle = handle_of(&app, entity).expect("autoplay started a playback");

    app.tick(1.0 / 60.0);
    assert_eq!(
        handle_of(&app, entity),
        Some(handle),
        "a tick's sweep must not drop a started sound's handle"
    );

    balaur_audio::stop_on(&app.engine, entity);
    assert_eq!(handle_of(&app, entity), None, "stop_on silences the node");
    let state = app.engine.resource::<AudioState>();
    assert!(
        !state.borrow().is_playing(handle),
        "a stopped handle answers not playing"
    );
}

#[test]
fn reapplying_the_same_file_does_not_restart_playback() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let mut app = app_in(dir.path());
    let entity = sound_node(&app, CHIME);
    let first = handle_of(&app, entity).expect("autoplay started a playback");

    app.tick(1.0 / 60.0);
    let params: toml::Value = toml::from_str(CHIME).unwrap();
    components::add(&app.engine, entity, "sound", Some(&params)).unwrap();

    assert_eq!(
        handle_of(&app, entity),
        Some(first),
        "re-applying the same file restarted playback"
    );
}

#[test]
fn naming_another_file_swaps_the_playback_to_it() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    write_wav(dir.path(), "gong.wav");
    let app = app_in(dir.path());
    let entity = sound_node(&app, CHIME);
    let first = handle_of(&app, entity).expect("autoplay started a playback");

    let params: toml::Value = toml::from_str(GONG).unwrap();
    components::add(&app.engine, entity, "sound", Some(&params)).unwrap();

    let second = handle_of(&app, entity).expect("the new file started a playback");
    assert_ne!(second, first, "another file plays under a fresh handle");
}

#[test]
fn play_on_restarts_the_nodes_sound_with_a_fresh_handle() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let app = app_in(dir.path());
    let entity = sound_node(&app, CHIME);
    let first = handle_of(&app, entity).expect("autoplay started a playback");

    let second = balaur_audio::play_on(&app.engine, entity).unwrap();
    assert_ne!(
        second, first,
        "an explicit trigger restarts from a fresh handle"
    );
    assert_eq!(handle_of(&app, entity), Some(second));

    let bare = scene::spawn_node(&mut app.engine.world_mut(), "Bare", app.engine.root());
    assert!(
        balaur_audio::play_on(&app.engine, bare).is_err(),
        "a node without a sound component has nothing to play"
    );
}

#[test]
fn freeing_a_node_drops_its_sound_on_the_next_sweep() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let mut app = app_in(dir.path());
    let entity = sound_node(&app, CHIME);
    assert!(
        handle_of(&app, entity).is_some(),
        "control: the sound was started before the node was freed"
    );

    scene::free_subtree(&mut app.engine.world_mut(), entity);
    app.tick(1.0 / 60.0);

    let state = app.engine.resource::<AudioState>();
    assert!(
        state.borrow().nodes.get(&entity).is_none(),
        "the sweep forgets a freed node's sound"
    );
}

#[test]
fn removing_the_component_stops_and_forgets_the_sound() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let app = app_in(dir.path());
    let entity = sound_node(&app, CHIME);
    let handle = handle_of(&app, entity).expect("autoplay started a playback");

    components::remove(&app.engine, entity, "sound").unwrap();

    let state = app.engine.resource::<AudioState>();
    assert!(state.borrow().nodes.get(&entity).is_none());
    assert!(!state.borrow().is_playing(handle));
}

/// A footstep must not cost a read per step, and an edited file must still be
/// heard: the cache holds the bytes and the file's own timestamp retires them.
#[test]
fn a_sounds_bytes_are_cached_until_the_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(dir.path(), "chime.wav");
    let app = app_in(dir.path());
    let entity = sound_node(&app, CHIME);
    let held = |app: &App| app.engine.resource::<SoundCache>().borrow().held();
    let first = held(&app);
    assert!(first > 0, "the first play filled the cache");

    balaur_audio::play_on(&app.engine, entity).unwrap();
    assert_eq!(held(&app), first, "a second play adds nothing");

    rewrite(&dir.path().join("chime.wav"), first + 64);
    balaur_audio::play_on(&app.engine, entity).unwrap();
    assert_eq!(held(&app), first + 64, "an edit retires the cached bytes");
}

/// A file the cache would blow its budget on is played straight from disk.
#[test]
fn a_file_past_the_entry_cap_is_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.wav"), vec![0u8; 3 * 1024 * 1024]).unwrap();
    let app = app_in(dir.path());
    let entity = sound_node(&app, "file = \"big.wav\"\nautoplay = true\n");
    assert!(
        handle_of(&app, entity).is_some(),
        "a handle is still handed out"
    );
    assert_eq!(app.engine.resource::<SoundCache>().borrow().held(), 0);
}

/// Rewrite a file at a new length, far enough after the last write that its
/// modification time has to move — which is what retires a cached entry.
fn rewrite(path: &Path, len: usize) {
    std::thread::sleep(std::time::Duration::from_millis(10));
    std::fs::write(path, vec![0u8; len]).unwrap();
}
