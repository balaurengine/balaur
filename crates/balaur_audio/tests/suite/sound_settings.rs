//! The `sound` keys that shape a play: where it starts and stops, what
//! plays with it and after it, its fades, its pause, and the effects it
//! passes through. Every assertion reads the intent the component keeps and
//! the clock it counts on the fixed step, never a sink.

use balaur_audio::AudioState;
use balaur_core::hecs::Entity;
use balaur_core::{App, components};

use crate::sound_component::{app_in, handle_of, sound_node, write_wav_of};

const STEP: f32 = 1.0 / 60.0;

/// 8 kHz mono: a tenth of a second is 800 samples.
fn tenths(n: u32) -> u32 {
    n * 800
}

fn state(app: &App) -> std::rc::Rc<std::cell::RefCell<AudioState>> {
    app.engine.resource::<AudioState>()
}

fn playback_time(app: &App, entity: Entity) -> f64 {
    components::get(&app.engine, entity, "sound")
        .and_then(|table| table.get("playback_time").and_then(toml::Value::as_float))
        .expect("a sound reports its playback time")
}

fn patch(app: &App, entity: Entity, params: &str) {
    let params: toml::Value = toml::from_str(params).unwrap();
    components::patch(&app.engine, entity, "sound", &params).unwrap();
}

/// Ticks until the node's sound has ended, up to a limit.
fn steps_to_end(app: &mut App, entity: Entity, limit: u32) -> u32 {
    let mut steps = 0;
    while handle_of(app, entity).is_some() && steps < limit {
        app.tick(STEP);
        steps += 1;
    }
    steps
}

#[test]
fn every_sound_key_reads_back_what_was_set() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(1));
    let app = app_in(dir.path());
    let entity = sound_node(
        &app,
        r#"
file = "tone.wav"
paused = true
start_time = 0.25
end_time = 0.75
delay = 0.5
loop_offset = 0.125
layers = ["a.wav", "b.wav"]
queue = ["c.wav"]
fade_in_time = 0.5
fade_out_time = 1.5
crossfade_time = 2.0
pan = -0.5
attenuation = "inverse_square"
low_pass_hz = 800.0
low_pass_q = 0.75
high_pass_hz = 60.0
high_pass_q = 1.25
distortion_gain = 3.0
distortion_threshold = 0.5
reverb_time = 0.25
reverb_level = 0.5
auto_gain = true
auto_gain_target = 0.75
auto_gain_attack_time = 2.0
auto_gain_release_time = 0.5
auto_gain_max = 5.0
auto_gain_floor = 0.25
"#,
    );
    let table = components::get(&app.engine, entity, "sound").unwrap();
    let number = |key: &str| table.get(key).and_then(toml::Value::as_float).unwrap();
    for (key, want) in [
        ("start_time", 0.25),
        ("end_time", 0.75),
        ("delay", 0.5),
        ("loop_offset", 0.125),
        ("fade_in_time", 0.5),
        ("fade_out_time", 1.5),
        ("crossfade_time", 2.0),
        ("pan", -0.5),
        ("low_pass_hz", 800.0),
        ("low_pass_q", 0.75),
        ("high_pass_hz", 60.0),
        ("high_pass_q", 1.25),
        ("distortion_gain", 3.0),
        ("distortion_threshold", 0.5),
        ("reverb_time", 0.25),
        ("reverb_level", 0.5),
        ("auto_gain_target", 0.75),
        ("auto_gain_attack_time", 2.0),
        ("auto_gain_release_time", 0.5),
        ("auto_gain_max", 5.0),
        ("auto_gain_floor", 0.25),
    ] {
        assert!(
            (number(key) - want).abs() < 1e-6,
            "{key} read back {}",
            number(key)
        );
    }
    assert_eq!(
        table.get("paused").and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        table.get("auto_gain").and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        table.get("attenuation").and_then(toml::Value::as_str),
        Some("inverse_square")
    );
    let files = |key: &str| -> Vec<String> {
        table
            .get(key)
            .and_then(toml::Value::as_array)
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(files("layers"), ["a.wav", "b.wav"]);
    assert_eq!(files("queue"), ["c.wav"]);
}

#[test]
fn a_paused_sound_holds_its_count_until_it_is_let_go() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(1));
    let mut app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"tone.wav\"\nautoplay = true\npaused = true\n",
    );
    let handle = handle_of(&app, entity).expect("autoplay started it, held");
    for _ in 0..30 {
        app.tick(STEP);
    }
    assert_eq!(
        handle_of(&app, entity),
        Some(handle),
        "a held sound played out"
    );
    assert!(playback_time(&app, entity).abs() < 1e-9);
    assert!(state(&app).borrow().is_paused(handle));

    patch(&app, entity, "paused = false");
    assert_eq!(
        handle_of(&app, entity),
        Some(handle),
        "letting go restarted it"
    );
    let steps = steps_to_end(&mut app, entity, 60);
    assert!((6..=7).contains(&steps), "it ended after {steps} steps");
}

#[test]
fn start_time_is_where_the_count_begins() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(5));
    let mut app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"tone.wav\"\nautoplay = true\nstart_time = 0.4\n",
    );
    assert!((playback_time(&app, entity) - 0.4).abs() < 1e-6);
    let steps = steps_to_end(&mut app, entity, 60);
    assert!(
        (6..=7).contains(&steps),
        "a tenth was left; it took {steps} steps"
    );
}

#[test]
fn end_time_stops_the_sound_early() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(5));
    let mut app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"tone.wav\"\nautoplay = true\nend_time = 0.1\n",
    );
    let steps = steps_to_end(&mut app, entity, 60);
    assert!((6..=7).contains(&steps), "it ended after {steps} steps");
}

#[test]
fn a_delay_holds_the_sound_back_before_it_counts() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(1));
    let mut app = app_in(dir.path());
    let entity = sound_node(&app, "file = \"tone.wav\"\nautoplay = true\ndelay = 0.1\n");
    for _ in 0..3 {
        app.tick(STEP);
    }
    assert!(
        playback_time(&app, entity).abs() < 1e-9,
        "the file moved during its delay"
    );
    let steps = 3 + steps_to_end(&mut app, entity, 60);
    assert!((12..=13).contains(&steps), "it ended after {steps} steps");
}

#[test]
fn a_seek_moves_the_playback_time_and_the_end() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(5));
    let mut app = app_in(dir.path());
    let entity = sound_node(&app, "file = \"tone.wav\"\nautoplay = true\n");
    let handle = handle_of(&app, entity).unwrap();
    app.tick(STEP);
    balaur_audio::seek_on(&app.engine, entity, 0.4);
    assert_eq!(
        handle_of(&app, entity),
        Some(handle),
        "a seek keeps the handle"
    );
    assert!((playback_time(&app, entity) - 0.4).abs() < 1e-6);
    let steps = steps_to_end(&mut app, entity, 60);
    assert!(
        (6..=7).contains(&steps),
        "a tenth was left; it took {steps} steps"
    );
}

#[test]
fn the_queue_plays_after_the_file_and_skip_moves_through_it() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.wav", "b.wav", "c.wav"] {
        write_wav_of(dir.path(), name, tenths(1));
    }
    let mut app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"a.wav\"\nautoplay = true\nqueue = [\"b.wav\", \"c.wav\"]\n",
    );
    let handle = handle_of(&app, entity).unwrap();
    let index = |app: &App| state(app).borrow().playing_index(handle);
    assert_eq!(index(&app), Some(0));
    for _ in 0..8 {
        app.tick(STEP);
    }
    assert_eq!(
        index(&app),
        Some(1),
        "the first queued file follows the file"
    );

    balaur_audio::skip_on(&app.engine, entity);
    assert_eq!(index(&app), Some(2));
    assert!(
        playback_time(&app, entity).abs() < 1e-9,
        "a skip starts the next file from its top"
    );
    balaur_audio::skip_on(&app.engine, entity);
    assert_eq!(
        handle_of(&app, entity),
        None,
        "skipping past the last file ends it"
    );
    app.tick(STEP);
    assert_eq!(
        balaur_core::events::delivered_from(&app.engine, entity, balaur_audio::FINISHED_EVENT),
        vec![balaur_script::Value::Int(i64::try_from(handle).unwrap())]
    );
}

#[test]
fn a_queue_is_counted_file_by_file_to_its_end() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "a.wav", tenths(1));
    write_wav_of(dir.path(), "b.wav", tenths(1));
    let mut app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"a.wav\"\nautoplay = true\nqueue = [\"b.wav\"]\n",
    );
    let steps = steps_to_end(&mut app, entity, 60);
    assert!((12..=13).contains(&steps), "two tenths took {steps} steps");
}

#[test]
fn layers_play_as_long_as_the_longest_file() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "short.wav", tenths(1));
    write_wav_of(dir.path(), "long.wav", tenths(2));
    let mut app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"short.wav\"\nautoplay = true\nlayers = [\"long.wav\"]\n",
    );
    let steps = steps_to_end(&mut app, entity, 60);
    assert!(
        (12..=13).contains(&steps),
        "the layer's two tenths took {steps} steps"
    );
}

#[test]
fn fade_out_time_brings_the_gain_down_before_the_end() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(5));
    let mut app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"tone.wav\"\nautoplay = true\nfade_out_time = 0.25\n",
    );
    let handle = handle_of(&app, entity).unwrap();
    let gain = |app: &App| state(app).borrow().effective_volume(handle).unwrap();
    assert!((gain(&app) - 1.0).abs() < 1e-6, "full before the fade");
    // 0.375 s in: halfway through the last quarter second.
    for _ in 0..22 {
        app.tick(STEP);
    }
    let halfway = gain(&app);
    assert!((halfway - 0.5).abs() < 0.05, "{halfway}");
    app.tick(STEP);
    assert!(gain(&app) < halfway, "the fade goes on falling");
}

#[test]
fn stopping_a_sound_fades_it_out_over_fade_out_time() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(5));
    let mut app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"tone.wav\"\nautoplay = true\nloop = true\nfade_out_time = 0.1\n",
    );
    let handle = handle_of(&app, entity).unwrap();
    balaur_audio::stop_on(&app.engine, entity);
    let st = state(&app);
    assert!(
        !st.borrow().is_playing(handle),
        "a stopped sound is not the game's any more"
    );
    let start = st.borrow().releasing_volume(handle).expect("it fades");
    assert!((start - 1.0).abs() < 1e-6, "{start}");
    app.tick(STEP);
    let later = st.borrow().releasing_volume(handle).unwrap();
    assert!(later < start, "{later}");
    for _ in 0..10 {
        app.tick(STEP);
    }
    assert_eq!(
        st.borrow().releasing_volume(handle),
        None,
        "silent after the fade"
    );
}

#[test]
fn a_new_file_crossfades_with_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "a.wav", tenths(5));
    write_wav_of(dir.path(), "b.wav", tenths(5));
    let app = app_in(dir.path());
    let entity = sound_node(
        &app,
        "file = \"a.wav\"\nautoplay = true\ncrossfade_time = 0.5\n",
    );
    let old = handle_of(&app, entity).unwrap();
    patch(&app, entity, "file = \"b.wav\"");
    let new = handle_of(&app, entity).expect("the new file started");
    assert_ne!(new, old);
    let st = state(&app);
    assert!(
        st.borrow().releasing_volume(old).is_some(),
        "the old file fades out"
    );
    assert!(st.borrow().is_playing(new));
}

#[test]
fn effects_land_on_a_sound_already_playing() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(5));
    let app = app_in(dir.path());
    let entity = sound_node(&app, "file = \"tone.wav\"\nautoplay = true\nloop = true\n");
    let handle = handle_of(&app, entity).unwrap();
    patch(
        &app,
        entity,
        "low_pass_hz = 500.0\nhigh_pass_hz = 80.0\ndistortion_threshold = 0.5\npan = 0.5\nreverb_level = 0.25\nauto_gain_target = 0.5",
    );
    assert_eq!(
        handle_of(&app, entity),
        Some(handle),
        "a retune does not restart"
    );
    let effects = state(&app).borrow().effects_of(handle).unwrap();
    assert!((effects.low_pass_hz - 500.0).abs() < 1e-6);
    assert!((effects.high_pass_hz - 80.0).abs() < 1e-6);
    assert!((effects.distortion_threshold - 0.5).abs() < 1e-6);
    assert!((effects.pan - 0.5).abs() < 1e-6);
    assert!((effects.reverb_level - 0.25).abs() < 1e-6);
    assert!((effects.auto_gain.target - 0.5).abs() < 1e-6);
}

/// The component's `loop_offset` wins over the file's own, and below zero
/// leaves the file's own in place.
#[test]
fn loop_offset_on_the_component_overrides_the_files_own() {
    let dir = tempfile::tempdir().unwrap();
    write_wav_of(dir.path(), "tone.wav", tenths(2));
    std::fs::write(
        dir.path().join("tone.wav.import.toml"),
        "loop_offset = 0.05\n",
    )
    .unwrap();
    let mut app = app_in(dir.path());
    let own = sound_node(&app, "file = \"tone.wav\"\nautoplay = true\nloop = true\n");
    let set = sound_node(
        &app,
        "file = \"tone.wav\"\nautoplay = true\nloop = true\nloop_offset = 0.1\n",
    );
    // A quarter second: past the end once, by a twentieth.
    for _ in 0..15 {
        app.tick(STEP);
    }
    let own_at = playback_time(&app, own);
    let set_at = playback_time(&app, set);
    assert!(
        (own_at - 0.10).abs() < 1e-3,
        "the file's own offset: {own_at}"
    );
    assert!(
        (set_at - 0.15).abs() < 1e-3,
        "the component's offset: {set_at}"
    );
}

#[test]
fn a_flat_pan_balances_rather_than_spreads() {
    for (pan, want) in [(0.0, [1.0, 1.0]), (1.0, [0.0, 1.0]), (-0.5, [1.0, 0.5])] {
        let [left, right] = balaur_audio::spatial::balance_gains(pan);
        assert!(
            (left - want[0]).abs() < 1e-6 && (right - want[1]).abs() < 1e-6,
            "pan {pan} gave {left}, {right}"
        );
    }
}

#[test]
fn listing_devices_names_each_one() {
    for name in balaur_audio::devices() {
        assert!(!name.is_empty(), "a listed device has a name");
    }
}
