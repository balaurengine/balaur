//! Audio without a sound card. CI runners have no output device, so the
//! plugin has to build and tick regardless — a game that will not start
//! headless cannot be tested at all.

use balaur_audio::{AudioPlugin, AudioState};
use balaur_core::{App, AppConfig};

fn app() -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut AudioPlugin::default()).unwrap();
    app
}

#[test]
fn the_plugin_builds_without_an_output_device() {
    let app = app();
    assert!(app.engine.try_resource::<AudioState>().is_some());
}

#[test]
fn ticking_with_audio_does_not_panic() {
    let mut app = app();
    for _ in 0..10 {
        app.tick(1.0 / 60.0);
    }
}

#[test]
fn playing_a_missing_file_does_not_take_the_frame_down() {
    let app = app();
    let state = app.engine.resource::<AudioState>();
    let missing = std::path::Path::new("no/such/sound.ogg");
    let _ = state.borrow_mut().play(missing, 1.0, false);
    let _ = state.borrow_mut().play(std::path::Path::new(""), 1.0, true);
}

#[test]
fn stopping_when_nothing_plays_is_harmless() {
    let app = app();
    app.engine.resource::<AudioState>().borrow_mut().stop_all();
}
