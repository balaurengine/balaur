//! `[audio]` and the buses' limiters: what a project asks of the output, read
//! the way the device is opened with it.

use balaur_audio::AudioPlugin;
use balaur_audio::bus::{self, Buses, Limiter};
use balaur_audio::output::OutputSettings;
use balaur_core::{App, AppConfig};

fn app(project: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"a\"\nmain_scene = \"main.toml\"\n{project}"),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut AudioPlugin::default()).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

#[test]
fn an_empty_audio_table_asks_the_device_for_nothing() {
    let (_dir, app) = app("");
    let output = OutputSettings::of(&app.engine);
    assert_eq!(output, OutputSettings::default());
    assert!(output.asks_for_nothing());
}

#[test]
fn the_audio_table_is_what_the_device_is_opened_with() {
    let (_dir, app) = app(
        "[audio]\ndevice = \"Speakers\"\nchannels = 2\nsample_rate_hz = 48000\nbuffer_frames = 512\ndither_bits = 16\n",
    );
    let output = OutputSettings::of(&app.engine);
    assert_eq!(
        output,
        OutputSettings {
            device: "Speakers".to_string(),
            channels: 2,
            sample_rate_hz: 48_000,
            buffer_frames: 512,
            dither_bits: 16,
        }
    );
    assert!(!output.asks_for_nothing());
    assert_eq!(
        balaur_core::settings::get(&app.engine, "audio/sample_rate_hz")
            .and_then(|v| v.as_integer()),
        Some(48_000),
        "the setting reads back"
    );
}

/// Dither alone changes nothing about how the device opens: it is applied to
/// the mix on its way there.
#[test]
fn dither_alone_opens_the_default_device() {
    let (_dir, app) = app("[audio]\ndither_bits = 16\n");
    assert!(OutputSettings::of(&app.engine).asks_for_nothing());
}

#[test]
fn a_bus_declares_its_limiter() {
    let (_dir, app) = app(
        "[audio.buses]\nmusic = { limit = true, limit_threshold_db = -3.0, limit_knee_db = 2.0, limit_attack_time = 0.01, limit_release_time = 0.2 }\nsfx = { volume_linear = 0.5 }\n",
    );
    bus::ensure_loaded(&app.engine);
    let buses = app.engine.resource::<Buses>();
    let buses = buses.borrow();
    assert_eq!(
        buses.limit("music"),
        Some(Limiter {
            threshold_db: -3.0,
            knee_db: 2.0,
            attack_time: 0.01,
            release_time: 0.2,
        })
    );
    assert_eq!(buses.limit("sfx"), None, "a bus with no `limit` has none");
}

#[test]
fn a_limiter_takes_rodios_defaults_for_what_it_leaves_out() {
    let (_dir, app) = app("[audio.buses]\nmaster = { limit = true }\n");
    bus::ensure_loaded(&app.engine);
    let limit = app.engine.resource::<Buses>().borrow().limit("").unwrap();
    assert_eq!(
        limit,
        Limiter {
            threshold_db: -1.0,
            knee_db: 4.0,
            attack_time: 0.005,
            release_time: 0.1,
        }
    );
}

/// Every bus feeds a named parent, `master` where it names none, so the
/// device can hang each limited bus under the right mixer.
#[test]
fn every_route_names_the_bus_it_feeds() {
    let (_dir, app) = app("[audio.buses]\nsfx = { limit = true }\nui = { parent = \"sfx\" }\n");
    bus::ensure_loaded(&app.engine);
    let routes = app.engine.resource::<Buses>().borrow().routes();
    let parent = |name: &str| {
        routes
            .iter()
            .find(|route| route.name == name)
            .map(|route| route.parent.clone())
            .unwrap()
    };
    assert_eq!(parent("master"), "");
    assert_eq!(parent("sfx"), "master");
    assert_eq!(parent("ui"), "sfx");
}

/// A sound on a limited bus, with the whole mix limited and dithered, starts
/// and is routed with no output device as with one.
#[test]
fn a_sound_on_a_limited_bus_plays() {
    let (dir, app) = app(
        "[audio]\ndither_bits = 16\n[audio.buses]\nmaster = { limit = true }\nmusic = { limit = true }\nscore = { parent = \"music\" }\n",
    );
    crate::sound_component::write_wav_of(dir.path(), "tone.wav", 800);
    let entity = crate::sound_component::sound_node(
        &app,
        "file = \"tone.wav\"\nautoplay = true\nbus = \"score\"\n",
    );
    let handle = crate::sound_component::handle_of(&app, entity).expect("autoplay started it");
    let state = app.engine.resource::<balaur_audio::AudioState>();
    assert_eq!(
        state.borrow().routing_of(handle),
        Some(("score".to_string(), 1.0))
    );
}
