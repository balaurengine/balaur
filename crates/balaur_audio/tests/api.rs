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
fn undecodable_bytes_hand_out_a_silent_handle_instead_of_erroring() {
    let app = app();
    let state = app.engine.resource::<AudioState>();
    let mut state = state.borrow_mut();
    let first = state.play(b"not audio".to_vec(), 1.0, 1.0, false);
    let second = state.play(Vec::new(), 1.0, 1.0, true);
    assert!(first > 0, "handles count up from one");
    assert_ne!(first, second, "every play hands out a fresh handle");
    assert!(!state.is_playing(first));
}

#[test]
fn an_unknown_handle_answers_not_playing_and_its_setters_no_op() {
    let app = app();
    let state = app.engine.resource::<AudioState>();
    let mut state = state.borrow_mut();
    assert!(!state.is_playing(0));
    assert!(!state.is_playing(9_999));
    state.set_volume(9_999, 0.5);
    state.set_pitch(9_999, 2.0);
    state.stop(9_999);
}

#[test]
fn stopping_when_nothing_plays_is_harmless() {
    let app = app();
    app.engine.resource::<AudioState>().borrow_mut().stop_all();
}
