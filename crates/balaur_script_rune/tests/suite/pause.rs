//! What a paused game looks like from a script: who stops ticking, who keeps
//! going, and who hears about it.

use balaur_core::process::{self, ProcessMode};
use balaur_core::{App, AppConfig};
use balaur_script_rune::RuneHost;

fn app_in(dir: &std::path::Path) -> App {
    App::new(AppConfig {
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.to_path_buf())
    })
    .unwrap()
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "name = \"t\"\n").unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

fn attach(app: &App, parent: hecs::Entity, name: &str, rel: &str) -> hecs::Entity {
    let entity = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, parent);
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(entity), rel)
        .unwrap();
    entity
}

fn field(app: &App, node: hecs::Entity, name: &str) -> Option<f64> {
    balaur_script_rune::rune_of(&app.engine).number_field(node, name)
}

/// Counts its own ticks and files the last `on_paused_changed` it was told about.
const COUNTER: &str = "pub fn init(this) {\n    this.n = 0;\n    this.told = -1.0;\n}\n\
pub fn update(this, dt) {\n    this.n = this.n + 1;\n}\n\
pub fn on_paused_changed(this, paused) {\n    this.told = if paused { 1.0 } else { 0.0 };\n}\n";

#[test]
fn a_pause_stops_update_and_an_always_node_keeps_ticking() {
    let dir = project(&[("s.rn", COUNTER)]);
    let mut app = app_in(dir.path());
    let game = attach(&app, app.engine.root(), "Game", "s.rn");
    let menu = attach(&app, app.engine.root(), "Menu", "s.rn");
    process::set(&mut app.engine.world_mut(), menu, ProcessMode::Always);

    app.tick(0.016);
    assert_eq!(field(&app, game, "n"), Some(1.0));

    app.engine.set_paused(true);
    app.tick(0.016);
    app.tick(0.016);
    assert_eq!(field(&app, game, "n"), Some(1.0), "the game is held");
    assert_eq!(field(&app, menu, "n"), Some(3.0), "the menu is not");

    app.engine.set_paused(false);
    app.tick(0.016);
    assert_eq!(field(&app, game, "n"), Some(2.0), "and it runs again");
}

#[test]
fn on_paused_reaches_the_scripts_the_pause_just_stopped() {
    let dir = project(&[("s.rn", COUNTER)]);
    let mut app = app_in(dir.path());
    let game = attach(&app, app.engine.root(), "Game", "s.rn");

    app.engine.set_paused(true);
    app.tick(0.016);
    assert_eq!(
        field(&app, game, "told"),
        Some(1.0),
        "a script the pause holds is the one that most wants to know"
    );

    app.engine.set_paused(false);
    app.tick(0.016);
    assert_eq!(field(&app, game, "told"), Some(0.0));
}

#[test]
fn a_disabled_node_never_ticks() {
    let dir = project(&[("s.rn", COUNTER)]);
    let mut app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Off", "s.rn");
    process::set(&mut app.engine.world_mut(), node, ProcessMode::Disabled);

    app.tick(0.016);
    app.tick(0.016);
    assert_eq!(field(&app, node, "n"), Some(0.0));
}

#[test]
fn a_script_pauses_and_reads_the_pause_back() {
    let source = "pub fn init(this) { this.seen = 0.0; }\n\
pub fn update(this, dt) {\n    engine::set_paused(true);\n    this.seen = if engine::paused() { 1.0 } else { 0.0 };\n}\n";
    let dir = project(&[("s.rn", source)]);
    let mut app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Game", "s.rn");
    process::set(&mut app.engine.world_mut(), node, ProcessMode::Always);

    app.tick(0.016);
    assert_eq!(field(&app, node, "seen"), Some(1.0));
    assert!(app.engine.paused());
}

#[test]
fn a_node_reports_and_sets_its_own_process_mode() {
    let source = "pub fn init(this) { this.mode = \"\"; }\n\
pub fn update(this, dt) {\n    this.node.set_process(\"always\");\n    this.mode = this.node.process();\n}\n";
    let dir = project(&[("s.rn", source)]);
    let mut app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Menu", "s.rn");

    app.tick(0.016);
    let host: RuneHost = balaur_script_rune::rune_of(&app.engine);
    assert_eq!(host.text_field(node, "mode").as_deref(), Some("always"));
    assert_eq!(process::own(&app.engine.world(), node), ProcessMode::Always);
}

#[test]
fn a_paused_script_still_hears_the_window_lose_focus() {
    let dir = project(&[(
        "a.rn",
        "pub fn init(this) { this.focus = 0.0; }\n\
         pub fn on_focused_changed(this, focused) { if !focused { this.focus += 1.0; } }\n",
    )]);
    let mut app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Listener", "a.rn");
    app.tick(1.0 / 60.0);
    app.engine.set_paused(true);
    app.tick(1.0 / 60.0);
    balaur_core::facts::update_device(&app.engine, |facts| facts.focused = false);
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    assert_eq!(field(&app, node, "focus"), Some(1.0));
}
