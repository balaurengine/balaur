//! Audio buses: the gain a sound plays through, and what moving a slider
//! does to what is already sounding.
//!
//! No output device on a CI runner, so what is asserted is the *routing* —
//! which bus a handle is on and at what volume — rather than anything heard.

use balaur_audio::bus::{self, Buses};
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
    let mut app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut AudioPlugin::default()).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

const NESTED: &str = "
[audio.buses]
sfx = { volume = 0.5 }
ui = { volume = 0.5, parent = \"sfx\" }
music = { volume = 0.25 }
";

fn gain(app: &App, bus: &str) -> f32 {
    bus::ensure_loaded(&app.engine);
    app.engine.resource::<Buses>().borrow().gain(bus)
}

/// Master exists whether or not the project declares it, because there has to
/// be a name for "everything".
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

/// A typo should leave a sound audible and findable, not silently delete it.
#[test]
fn a_bus_nobody_declared_is_unity_rather_than_silence() {
    let (_dir, app) = app(NESTED);
    assert!((gain(&app, "sffx") - 1.0).abs() < 1e-6);
}

#[test]
fn an_empty_bus_name_is_master() {
    let (_dir, app) = app("\n[audio.buses]\nmaster = { volume = 0.5 }\n");
    assert!((gain(&app, "") - 0.5).abs() < 1e-6);
}

/// Setting a volume moves everything below it, which is what a slider on
/// "sound effects" has to do to the menu clicks underneath.
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

/// A bus nobody declared is made rather than refused, so a game may build its
/// mix in script alone.
#[test]
fn setting_a_volume_makes_a_bus_that_was_not_declared() {
    let (_dir, app) = app("");
    bus::ensure_loaded(&app.engine);
    app.engine
        .resource::<Buses>()
        .borrow_mut()
        .set_volume("voice", 0.3);
    assert!((gain(&app, "voice") - 0.3).abs() < 1e-6);
    assert!(app
        .engine
        .resource::<Buses>()
        .borrow()
        .names()
        .contains(&"voice".to_string()));
}

/// The rest of the mix is fine, so a cycle is cut and reported rather than
/// refused — and `gain` has to terminate whatever the file says.
#[test]
fn a_cycle_is_cut_rather_than_looping_forever() {
    let (_dir, app) = app("
[audio.buses]
a = { volume = 0.5, parent = \"b\" }
b = { volume = 0.5, parent = \"a\" }
");
    // The assertion is that this returns at all; the value is whatever the
    // chain came to before the cut.
    let gain = gain(&app, "a");
    assert!(gain > 0.0 && gain <= 1.0, "{gain}");
}

/// A handle remembers the bus it plays on and the volume it started at, which
/// is what lets a slider moved later recompute one from the other.
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

/// Every chain ends at master, a slider moves what its children carry, and a
/// handle's own volume still goes through the chain — the three halves of the
/// mix, on one sound.
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

/// A slider on a bus nothing plays on directly still moves the sounds under
/// it, which is what "pull the music slider" means.
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

/// A typo should leave a sound audible, so no slider reaches it either.
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

fn with_events(events: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("[application]\nname = \"a\"\nmain_scene = \"main.toml\"\n{NESTED}"),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("audio")).unwrap();
    std::fs::write(dir.path().join("audio/events.toml"), events).unwrap();
    let mut app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    balaur_plugin::load(&mut app, &mut AudioPlugin::default()).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

const EVENTS: &str = r#"
[hit]
files = ["sfx/hit1.wav", "sfx/hit2.wav", "sfx/hit3.wav"]
bus = "sfx"
volume = 0.9

[music]
files = ["music/theme.ogg"]
bus = "music"
loop = true
"#;

use balaur_audio::event::{self, Events};

/// A rotation, not a draw: variations exist so the same sample is not heard
/// twice running, and taking them in turn guarantees it.
#[test]
fn variations_are_taken_in_turn_and_wrap() {
    let (_dir, app) = with_events(EVENTS);
    event::ensure_loaded(&app.engine);
    let events = app.engine.resource::<Events>();
    let events = events.borrow();
    let next = || events.next_file("hit").expect("hit is declared");
    assert_eq!(next(), "sfx/hit1.wav");
    assert_eq!(next(), "sfx/hit2.wav");
    assert_eq!(next(), "sfx/hit3.wav");
    assert_eq!(next(), "sfx/hit1.wav", "the rotation wraps");
}

#[test]
fn one_variation_is_a_sound_with_no_variation() {
    let (_dir, app) = with_events(EVENTS);
    event::ensure_loaded(&app.engine);
    let events = app.engine.resource::<Events>();
    let events = events.borrow();
    assert_eq!(
        events.next_file("music").as_deref(),
        Some("music/theme.ogg")
    );
    assert_eq!(
        events.next_file("music").as_deref(),
        Some("music/theme.ogg")
    );
}

#[test]
fn an_event_carries_its_bus_volume_and_loop() {
    let (_dir, app) = with_events(EVENTS);
    event::ensure_loaded(&app.engine);
    let events = app.engine.resource::<Events>();
    let events = events.borrow();
    let hit = events.get("hit").expect("declared");
    assert_eq!(hit.bus, "sfx");
    assert!((hit.volume - 0.9).abs() < 1e-6);
    assert!(!hit.looped);
    assert!(events.get("music").expect("declared").looped);
}

#[test]
fn events_are_listed_in_name_order() {
    let (_dir, app) = with_events(EVENTS);
    event::ensure_loaded(&app.engine);
    assert_eq!(
        app.engine.resource::<Events>().borrow().names(),
        vec!["hit", "music"]
    );
}

#[test]
fn a_project_with_no_events_file_is_empty() {
    let (_dir, app) = app(NESTED);
    event::ensure_loaded(&app.engine);
    assert!(app.engine.resource::<Events>().borrow().names().is_empty());
    assert_eq!(
        app.engine.resource::<Events>().borrow().next_file("hit"),
        None
    );
}

/// Rotations are per event, so one sound playing does not advance another's.
#[test]
fn each_event_keeps_its_own_place() {
    let (_dir, app) = with_events(EVENTS);
    event::ensure_loaded(&app.engine);
    let events = app.engine.resource::<Events>();
    let events = events.borrow();
    assert_eq!(events.next_file("hit").as_deref(), Some("sfx/hit1.wav"));
    let _ = events.next_file("music");
    assert_eq!(events.next_file("hit").as_deref(), Some("sfx/hit2.wav"));
}
