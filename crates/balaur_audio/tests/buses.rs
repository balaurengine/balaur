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
        format!("name = \"a\"\nmain_scene = \"main.toml\"\n{buses}"),
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
    assert!((gain(&app, "ui") - 0.25).abs() < 1e-6, "{}", gain(&app, "ui"));
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
    assert!((gain(&app, "ui") - 0.5).abs() < 1e-6, "{}", gain(&app, "ui"));
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
    let (_dir, app) = app(
        "
[audio.buses]
a = { volume = 0.5, parent = \"b\" }
b = { volume = 0.5, parent = \"a\" }
",
    );
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
