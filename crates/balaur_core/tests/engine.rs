//! The Engine: resources, the command queue, the clock, and quit.
//!
//! Everything a Rust system or a binding closure is handed every frame.

use balaur_core::engine::Command;
use balaur_core::{App, AppConfig, Engine};
use balaur_script::BindingsExt as _;

fn app() -> App {
    App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap()
}

struct Counter(u32);

#[test]
fn a_resource_round_trips_and_is_shared_not_copied() {
    let engine = Engine::new();
    engine.insert_resource(Counter(1));
    engine.resource::<Counter>().borrow_mut().0 = 42;
    assert_eq!(
        engine.resource::<Counter>().borrow().0,
        42,
        "resources are shared"
    );
}

#[test]
fn a_missing_resource_is_none_rather_than_a_panic() {
    let engine = Engine::new();
    assert!(engine.try_resource::<Counter>().is_none());
    engine.insert_resource(Counter(1));
    assert!(engine.try_resource::<Counter>().is_some());
    engine.remove_resource::<Counter>();
    assert!(engine.try_resource::<Counter>().is_none());
}

/// Inserting twice replaces rather than stacking, so a plugin rebuilt during
/// a reload does not leave the old one behind.
#[test]
fn inserting_a_resource_twice_replaces_it() {
    let engine = Engine::new();
    engine.insert_resource(Counter(1));
    engine.insert_resource(Counter(2));
    assert_eq!(engine.resource::<Counter>().borrow().0, 2);
}

/// Commands are deferred so a script can free a node while the world is being
/// iterated. Taking them must drain, or the same free runs every frame.
#[test]
fn commands_queue_and_drain_once() {
    let engine = Engine::new();
    let root = engine.root();
    engine.push_command(Command::Free(root));
    engine.push_command(Command::Free(root));
    assert_eq!(engine.take_commands().len(), 2);
    assert!(
        engine.take_commands().is_empty(),
        "commands were taken twice"
    );
}

#[test]
fn the_clock_advances_and_reports_the_last_step() {
    let engine = Engine::new();
    assert!(engine.time().abs() < f64::EPSILON);
    engine.advance_time(0.5);
    engine.advance_time(0.25);
    assert!((engine.time() - 0.75).abs() < 1e-9);
    assert!(
        (engine.delta() - 0.25).abs() < f32::EPSILON,
        "delta is the last step"
    );
}

#[test]
fn quit_is_off_until_asked_for() {
    let engine = Engine::new();
    assert!(!engine.quit_requested());
    engine.request_quit();
    assert!(engine.quit_requested());
}

#[test]
fn an_engine_has_a_root_node_from_the_start() {
    let engine = Engine::new();
    assert!(engine.world().contains(engine.root()));
}

/// Freeing a node takes its subtree with it, or children outlive their parent
/// as orphans.
#[test]
fn freeing_a_node_frees_its_children() {
    let mut app = app();
    let root = app.engine.root();
    let parent = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "P", root);
    let child = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "C", parent);

    app.engine.push_command(Command::Free(parent));
    app.tick(0.0);

    assert!(!app.engine.world().contains(parent));
    assert!(
        !app.engine.world().contains(child),
        "the child outlived its parent"
    );
}

#[test]
fn an_app_without_a_backend_has_no_script_host() {
    let app = app();
    assert!(app.engine.script_host().is_none());
}

/// A script module on a backendless app must still hand back something to
/// register into, so plugin build order does not depend on scripting.
#[test]
fn script_module_works_without_a_backend() {
    let mut app = app();
    let mut m = app.script_module("anything").unwrap();
    m.function("f", |_: &Engine, ()| Ok(()));
}

#[test]
fn ticking_advances_the_clock_by_the_step_given() {
    let mut app = app();
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    assert!((app.engine.time() - 2.0 / 60.0).abs() < 1e-6);
}
