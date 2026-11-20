//! `audio::play_file`: a sound on a node of its own, through the bus and at
//! the place it names, and gone once it plays out, a pause or not.

use balaur_audio::{AudioPlugin, AudioState};
use balaur_core::glamx::Vec3;
use balaur_core::hecs::Entity;
use balaur_core::{App, AppConfig, node_id_of, scene};

const PLAYS: &str = r#"
pub class Plays { node: Node }
impl Plays {
    pub fn init() {
        audio::play_file("hit.wav", audio::PlayFileOptions { bus: "sfx", volume_linear: 0.9 });
        audio::play_file("hit.wav", audio::PlayFileOptions { position: vec3(3.0, 0.0, 0.0), max_distance: 10.0 });
        let player = audio::play_file("hit.wav");
        player.sound.volume_linear = 0.25;
    }
}
"#;

fn bs_backend() -> balaur_core::ScriptHostFactory {
    Box::new(|setup| {
        let host = balaur_script_bs::BsHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack.clone(),
            setup.watch,
        );
        host.register(setup.engine);
        Ok(std::rc::Rc::new(host))
    })
}

/// The sound's length in samples, at the 8 kHz `write_wav_of` writes.
const SAMPLES: u32 = 800;
const RATE_HZ: f32 = 8_000.0;
const STEP: f32 = 1.0 / 60.0;

/// An app whose one node runs `source`, beside a short sound.
fn played(dir: &std::path::Path, source: &str) -> (App, anyhow::Result<()>) {
    crate::sound_component::write_wav_of(dir, "hit.wav", SAMPLES);
    std::fs::write(dir.join("plays.bs"), source).unwrap();
    let mut app = App::new(AppConfig {
        script_backend: Some(bs_backend()),
        ..AppConfig::bare(dir.to_path_buf())
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut AudioPlugin).unwrap();
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "Plays", root);
    let attached = app
        .engine
        .script_host()
        .unwrap()
        .attach(node_id_of(entity), "plays.bs");
    (app, attached)
}

/// Each node a sound plays on, with the handle it holds.
fn voices(app: &App) -> Vec<(Entity, u64)> {
    let state = app.engine.resource::<AudioState>();
    let state = state.borrow();
    state
        .nodes
        .iter()
        .filter_map(|(entity, voice)| Some((*entity, voice.handle?)))
        .collect()
}

#[test]
fn a_played_file_sounds_on_a_node_of_its_own_through_its_bus_and_place() {
    let dir = tempfile::tempdir().unwrap();
    let (app, attached) = played(dir.path(), PLAYS);
    attached.unwrap();
    let voices = voices(&app);
    assert_eq!(voices.len(), 3, "one node a played file");
    let state = app.engine.resource::<AudioState>();
    let state = state.borrow();
    let routed: Vec<Option<(String, f32)>> = voices
        .iter()
        .map(|(_, handle)| state.routing_of(*handle))
        .collect();
    assert!(
        routed.contains(&Some((String::from("sfx"), 0.9))),
        "the bus and level it named: {routed:?}"
    );
    assert!(
        routed.iter().any(|r| r
            .as_ref()
            .is_some_and(|(_, v)| (*v - 0.25).abs() < f32::EPSILON)),
        "the node's `sound` changes what it plays: {routed:?}"
    );
    let placed: Vec<Option<Vec3>> = voices
        .iter()
        .map(|(_, handle)| state.emitter_position(*handle))
        .collect();
    assert_eq!(
        placed.iter().flatten().collect::<Vec<_>>(),
        [&Vec3::new(3.0, 0.0, 0.0)],
        "only the one given a position is placed"
    );
    let root = app.engine.root();
    let world = app.engine.world();
    for (entity, _) in &voices {
        assert_eq!(
            world.get::<&scene::Parent>(*entity).map(|p| p.0).ok(),
            Some(root),
            "a played file hangs under the scene's root"
        );
    }
}

#[test]
fn a_played_files_node_frees_itself_once_it_plays_out_through_a_pause() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, attached) = played(dir.path(), PLAYS);
    attached.unwrap();
    let nodes: Vec<Entity> = voices(&app).into_iter().map(|(e, _)| e).collect();
    assert_eq!(nodes.len(), 3);
    app.engine.set_paused(true);
    let steps = (SAMPLES as f32 / RATE_HZ / STEP).ceil() as usize;
    for _ in 0..steps * 2 {
        app.tick(STEP);
    }
    let world = app.engine.world();
    for entity in nodes {
        assert!(
            !world.contains(entity),
            "a played file's node outlives its sound"
        );
    }
}

#[test]
fn a_play_option_the_sound_component_does_not_spell_does_not_compile() {
    let dir = tempfile::tempdir().unwrap();
    let source = PLAYS.replace("volume_linear: 0.9", "volume: 0.9");
    let (_app, attached) = played(dir.path(), &source);
    let why = format!("{:#}", attached.unwrap_err());
    assert!(why.contains("volume"), "{why}");
}
