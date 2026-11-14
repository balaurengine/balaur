//! What audio tells a loop about to sleep: a sound that will announce
//! `sound_finished` is pending until it does, and a loop the device plays is not.

use balaur_core::{App, fixed_dt, rest};

use crate::sound_component::{app_in, handle_of, sound_node, write_wav_of};

/// The name the plugin's probe answers to.
const AUDIO: &str = "audio";

fn pends(app: &App) -> bool {
    rest::pending_names(&app.engine).contains(&AUDIO)
}

#[test]
fn a_one_shot_sound_is_pending_until_it_plays_out() {
    let dir = tempfile::tempdir().unwrap();
    // A tenth of a second: six fixed steps.
    write_wav_of(dir.path(), "tone.wav", 800);
    let mut app = app_in(dir.path());
    let entity = sound_node(&app, "file = \"tone.wav\"\nautoplay = true\n");

    app.tick(fixed_dt());
    assert!(pends(&app), "its `sound_finished` is still to come");

    let mut steps = 1;
    while handle_of(&app, entity).is_some() && steps < 60 {
        app.tick(fixed_dt());
        steps += 1;
    }
    assert!(steps < 60, "the sound never played out");
    assert!(!pends(&app), "it ended on this step");
}

#[test]
fn a_looping_sound_does_not_keep_the_loop_awake() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "music.wav", 800);
    let mut app = app_in(dir.path());
    let entity = sound_node(&app, "file = \"music.wav\"\nautoplay = true\nloop = true\n");

    for _ in 0..30 {
        app.tick(fixed_dt());
        assert!(!pends(&app), "a loop never announces an end");
    }
    assert!(handle_of(&app, entity).is_some(), "and it is still playing");
}

#[test]
fn a_stopped_sound_is_pending_while_it_fades_out() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "music.wav", 4000);
    let mut app = app_in(dir.path());
    let params = "file = \"music.wav\"\nautoplay = true\nloop = true\nfade_out_time = 0.1\n";
    let entity = sound_node(&app, params);
    app.tick(fixed_dt());
    assert!(!pends(&app));

    balaur_audio::stop_on(&app.engine, entity);
    app.tick(fixed_dt());
    assert!(pends(&app), "the fade is counted on the fixed step");

    for _ in 0..10 {
        app.tick(fixed_dt());
    }
    assert!(!pends(&app), "silent, and let go");
}

#[test]
fn a_paused_game_holds_a_sound_at_rest() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", 8000);
    let mut app = app_in(dir.path());
    sound_node(&app, "file = \"tone.wav\"\nautoplay = true\n");
    app.tick(fixed_dt());
    assert!(pends(&app));

    app.engine.set_paused(true);
    // The hold lands in this frame's sweep, after its fixed step.
    app.tick(fixed_dt());
    app.tick(fixed_dt());
    assert!(!pends(&app), "a held voice is not counting down");

    app.engine.set_paused(false);
    app.tick(fixed_dt());
    app.tick(fixed_dt());
    assert!(pends(&app));
}
