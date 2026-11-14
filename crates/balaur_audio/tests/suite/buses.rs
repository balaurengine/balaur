//! Audio buses: the gain and the effects a sound plays through, and what
//! moving a slider does to what is already sounding.
//!
//! No output device on a CI runner, so what is asserted is the *routing* —
//! which bus a handle is on, at what volume, through what — rather than
//! anything heard.

use balaur_audio::bus::{self, Buses, Effects};
use balaur_audio::{AudioPlugin, AudioState};
use balaur_core::{App, AppConfig};

fn app(buses: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"a\"\nmain_scene = \"main.toml\"\n{buses}"),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut app = App::new(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut AudioPlugin).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

const NESTED: &str = "
[audio.buses]
sfx = { volume_linear = 0.5 }
ui = { volume_linear = 0.5, parent = \"sfx\" }
music = { volume_linear = 0.25 }
";

fn gain(app: &App, bus: &str) -> f32 {
    bus::ensure_loaded(&app.engine);
    app.engine.resource::<Buses>().borrow().gain(bus)
}

// Master exists whether or not the project declares it, because there has to
// be a name for "everything".
#[test]
fn master_exists_without_being_declared() {
    let (_dir, app) = app("");
    assert!((gain(&app, "master") - 1.0).abs() < 1e-6);
    assert_eq!(
        app.engine.resource::<Buses>().borrow().names(),
        vec!["master"]
    );
}

#[test]
fn a_gain_is_the_product_of_the_chain() {
    let (_dir, app) = app(NESTED);
    assert!((gain(&app, "sfx") - 0.5).abs() < 1e-6);
    // ui feeds sfx feeds master: 0.5 * 0.5.
    assert!(
        (gain(&app, "ui") - 0.25).abs() < 1e-6,
        "{}",
        gain(&app, "ui")
    );
    assert!((gain(&app, "music") - 0.25).abs() < 1e-6);
}

// A typo should leave a sound audible and findable, not silently delete it.
#[test]
fn a_bus_nobody_declared_is_unity_rather_than_silence() {
    let (_dir, app) = app(NESTED);
    assert!((gain(&app, "sffx") - 1.0).abs() < 1e-6);
}

#[test]
fn an_empty_bus_name_is_master() {
    let (_dir, app) = app("\n[audio.buses]\nmaster = { volume_linear = 0.5 }\n");
    assert!((gain(&app, "") - 0.5).abs() < 1e-6);
}

// Setting a volume moves everything below it, which is what a slider on
// "sound effects" has to do to the menu clicks underneath.
#[test]
fn setting_a_parents_volume_moves_its_children() {
    let (_dir, app) = app(NESTED);
    bus::ensure_loaded(&app.engine);
    app.engine
        .resource::<Buses>()
        .borrow_mut()
        .set_volume("sfx", 1.0);
    assert!(
        (gain(&app, "ui") - 0.5).abs() < 1e-6,
        "{}",
        gain(&app, "ui")
    );
}

// A bus nobody declared is made rather than refused, so a game may build its
// mix in script alone.
#[test]
fn setting_a_volume_makes_a_bus_that_was_not_declared() {
    let (_dir, app) = app("");
    bus::ensure_loaded(&app.engine);
    app.engine
        .resource::<Buses>()
        .borrow_mut()
        .set_volume("voice", 0.3);
    assert!((gain(&app, "voice") - 0.3).abs() < 1e-6);
    assert!(
        app.engine
            .resource::<Buses>()
            .borrow()
            .names()
            .contains(&"voice".to_string())
    );
}

// The rest of the mix is fine, so a cycle is cut and reported rather than
// refused — and `gain` has to terminate whatever the file says.
#[test]
fn a_cycle_is_cut_rather_than_looping_forever() {
    let (_dir, app) = app("
[audio.buses]
a = { volume_linear = 0.5, parent = \"b\" }
b = { volume_linear = 0.5, parent = \"a\" }
");
    // The assertion is that this returns at all; the value is whatever the
    // chain came to before the cut.
    let gain = gain(&app, "a");
    assert!(gain > 0.0 && gain <= 1.0, "{gain}");
}

const EFFECTS: &str = "
[audio.buses]
sfx = { volume_linear = 0.5 }
cave = { volume_linear = 0.6, low_pass_hz = 800.0, low_pass_q = 0.75, high_pass_hz = 60.0, high_pass_q = 1.25, distortion_gain = 3.0, distortion_threshold = 0.5, reverb_time = 0.25, reverb_level = 0.5, auto_gain = true, auto_gain_target = 0.75, auto_gain_attack_time = 2.0, auto_gain_release_time = 0.5, auto_gain_max = 5.0, auto_gain_floor = 0.25 }
loud = { limit = true }
tuned = { low_pass_q = 2.0 }
";

fn effects(app: &App, bus: &str) -> Option<Effects> {
    bus::ensure_loaded(&app.engine);
    app.engine.resource::<Buses>().borrow().effects(bus)
}

#[test]
fn a_bus_reads_back_every_effect_it_declares() {
    let (_dir, app) = app(EFFECTS);
    let cave = effects(&app, "cave").expect("the cave has effects");
    for (key, have, want) in [
        ("low_pass_hz", cave.low_pass_hz, 800.0),
        ("low_pass_q", cave.low_pass_q, 0.75),
        ("high_pass_hz", cave.high_pass_hz, 60.0),
        ("high_pass_q", cave.high_pass_q, 1.25),
        ("distortion_gain", cave.distortion_gain, 3.0),
        ("distortion_threshold", cave.distortion_threshold, 0.5),
        ("reverb_time", cave.reverb_time, 0.25),
        ("reverb_level", cave.reverb_level, 0.5),
        ("auto_gain_target", cave.auto_gain.target, 0.75),
        ("auto_gain_attack_time", cave.auto_gain.attack_time, 2.0),
        ("auto_gain_release_time", cave.auto_gain.release_time, 0.5),
        ("auto_gain_max", cave.auto_gain.max, 5.0),
        ("auto_gain_floor", cave.auto_gain.floor, 0.25),
    ] {
        assert!((have - want).abs() < 1e-6, "{key} read back {have}");
    }
    assert!(cave.auto_gain.on);
    assert!(
        (gain(&app, "cave") - 0.6).abs() < 1e-6,
        "effects leave the bus's gain alone"
    );
}

// Nothing switched on is no effects at all, so the bus costs the mix no
// chain: a Q with no cutoff filters nothing.
#[test]
fn a_bus_with_no_effect_switched_on_has_none() {
    let (_dir, app) = app(EFFECTS);
    for bus in ["sfx", "loud", "tuned", "master", "nobody"] {
        assert_eq!(effects(&app, bus), None, "{bus}");
    }
}

// A number past what the adapter takes is held to it where it is read, so
// the read-back is what the device is given.
#[test]
fn a_buses_effects_are_held_to_what_the_adapters_take() {
    let (_dir, app) = app("
[audio.buses]
harsh = { distortion_threshold = 4.0, reverb_time = 600.0, low_pass_hz = -5.0, auto_gain = true, auto_gain_attack_time = 60.0 }
");
    let harsh = effects(&app, "harsh").expect("switched on");
    assert!((harsh.distortion_threshold - 1.0).abs() < 1e-6);
    assert!((harsh.reverb_time - bus::MAX_REVERB_TIME).abs() < 1e-6);
    assert!(harsh.low_pass_hz.abs() < 1e-6);
    assert!((harsh.auto_gain.attack_time - bus::MAX_AUTO_GAIN_TIME).abs() < 1e-6);
}

// An effect, like a limiter, has to hear the bus's own mix, so a bus with
// either is mixed alone before it joins its parent; any other joins the
// nearest mixer above it.
#[test]
fn a_bus_with_an_effect_or_a_limiter_is_mixed_alone() {
    let (_dir, app) = app(EFFECTS);
    bus::ensure_loaded(&app.engine);
    let routes = app.engine.resource::<Buses>().borrow().routes();
    let alone = |name: &str| {
        routes
            .iter()
            .find(|route| route.name == name)
            .expect("routed")
            .mixed_alone()
    };
    assert!(alone("cave"), "effects and no limiter");
    assert!(alone("loud"), "a limiter and no effects");
    assert!(!alone("sfx"));
    assert!(!alone("tuned"));
    assert!(!alone("master"));
}

// A handle remembers the bus it plays on and the volume it started at, which
// is what lets a slider moved later recompute one from the other.
#[test]
fn a_played_handle_remembers_its_bus_and_volume() {
    let (_dir, app) = app(NESTED);
    bus::ensure_loaded(&app.engine);
    let state = app.engine.resource::<AudioState>();
    let handle = state
        .borrow_mut()
        .play_on_bus(Vec::new(), 0.8, 1.0, false, "ui", 0.25);
    let routing = state.borrow().routing_of(handle);
    assert_eq!(routing, Some(("ui".to_string(), 0.8)));
}

#[test]
fn stopping_a_handle_forgets_its_routing() {
    let (_dir, app) = app(NESTED);
    let state = app.engine.resource::<AudioState>();
    let handle = state
        .borrow_mut()
        .play_on_bus(Vec::new(), 1.0, 1.0, false, "sfx", 0.5);
    state.borrow_mut().stop(handle);
    assert_eq!(state.borrow().routing_of(handle), None);
}

#[test]
fn stopping_everything_forgets_every_routing() {
    let (_dir, app) = app(NESTED);
    let state = app.engine.resource::<AudioState>();
    let a = state
        .borrow_mut()
        .play_on_bus(Vec::new(), 1.0, 1.0, false, "sfx", 0.5);
    let b = state
        .borrow_mut()
        .play_on_bus(Vec::new(), 1.0, 1.0, false, "music", 0.25);
    state.borrow_mut().stop_all();
    assert_eq!(state.borrow().routing_of(a), None);
    assert_eq!(state.borrow().routing_of(b), None);
}

// Every chain ends at master, a slider moves what its children carry, and a
// handle's own volume still goes through the chain — the three halves of the
// mix, on one sound.
#[test]
fn a_sound_on_a_child_bus_follows_master_the_slider_and_its_own_volume() {
    let (_dir, app) = app(NESTED);
    bus::ensure_loaded(&app.engine);
    let buses = app.engine.resource::<Buses>();
    let state = app.engine.resource::<AudioState>();
    buses.borrow_mut().set_volume("master", 0.5);

    let gain = buses.borrow().gain("ui");
    let handle = state
        .borrow_mut()
        .play_on_bus(Vec::new(), 1.0, 1.0, false, "ui", gain);
    let applied = || state.borrow().effective_volume(handle).expect("routed");
    // master 0.5 * sfx 0.5 * ui 0.5.
    assert!((applied() - 0.125).abs() < 1e-6, "{}", applied());

    buses.borrow_mut().set_volume("sfx", 1.0);
    state.borrow_mut().reroute(&buses.borrow(), "sfx");
    assert!((applied() - 0.25).abs() < 1e-6, "{}", applied());

    state.borrow_mut().set_volume(handle, 0.5, &buses.borrow());
    assert!((applied() - 0.125).abs() < 1e-6, "{}", applied());
}

// A slider on a bus nothing plays on directly still moves the sounds under
// it, which is what "pull the music slider" means.
#[test]
fn moving_a_parent_bus_reroutes_the_sounds_on_its_children() {
    let (_dir, app) = app(NESTED);
    bus::ensure_loaded(&app.engine);
    let buses = app.engine.resource::<Buses>();
    let state = app.engine.resource::<AudioState>();
    let gain = buses.borrow().gain("ui");
    let child = state
        .borrow_mut()
        .play_on_bus(Vec::new(), 1.0, 1.0, false, "ui", gain);
    let other = state
        .borrow_mut()
        .play_on_bus(Vec::new(), 1.0, 1.0, false, "music", 0.25);

    buses.borrow_mut().set_volume("master", 0.0);
    state.borrow_mut().reroute(&buses.borrow(), "master");
    assert_eq!(state.borrow().effective_volume(child), Some(0.0));
    assert_eq!(state.borrow().effective_volume(other), Some(0.0));
}

// A typo should leave a sound audible, so no slider reaches it either.
#[test]
fn a_sound_on_a_bus_nobody_declared_is_left_at_unity() {
    let (_dir, app) = app(NESTED);
    bus::ensure_loaded(&app.engine);
    let buses = app.engine.resource::<Buses>();
    let state = app.engine.resource::<AudioState>();
    let handle = state
        .borrow_mut()
        .play_on_bus(Vec::new(), 1.0, 1.0, false, "sffx", 1.0);
    buses.borrow_mut().set_volume("master", 0.0);
    state.borrow_mut().reroute(&buses.borrow(), "master");
    assert_eq!(state.borrow().effective_volume(handle), Some(1.0));
}

fn play(app: &App, dir: &std::path::Path, options: &str) -> u64 {
    crate::sound_component::write_wav_of(dir, "hit.wav", 800);
    let options: FileOptions = toml::from_str(options).unwrap();
    play_file(&app.engine, "hit.wav", &options).unwrap()
}

use balaur_audio::play_file::{FileOptions, play_file};
use balaur_core::glamx::Vec3;

// A sound no node carries still goes through the bus it names.
#[test]
fn a_played_file_goes_through_its_bus_at_its_volume() {
    let (dir, app) = app(NESTED);
    let handle = play(&app, dir.path(), "bus = \"sfx\"\nvolume_linear = 0.9\n");
    assert_eq!(
        app.engine
            .resource::<AudioState>()
            .borrow()
            .routing_of(handle),
        Some((String::from("sfx"), 0.9))
    );
}

#[test]
fn a_played_file_is_unplaced_until_given_a_position() {
    let (dir, app) = app(NESTED);
    let flat = play(&app, dir.path(), "");
    let placed = play(
        &app,
        dir.path(),
        "position = [3.0, 0.0, 0.0]\nmax_distance = 10.0\n",
    );
    let state = app.engine.resource::<AudioState>();
    assert!(state.borrow().emitter(flat).is_none());
    assert_eq!(
        state.borrow().emitter_position(placed),
        Some(Vec3::new(3.0, 0.0, 0.0))
    );
}

#[test]
fn a_play_option_the_sound_component_does_not_spell_is_refused() {
    assert!(
        toml::from_str::<FileOptions>("volume = 0.5\n").is_err(),
        "the key is volume_linear"
    );
}
