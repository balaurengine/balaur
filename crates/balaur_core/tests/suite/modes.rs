//! App modes (PLAN-ecs §2): a value the app is in, systems run on entering
//! and leaving, sub-modes and computed modes, in the digest and snapshot.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::modes::in_mode;
use balaur_core::schedule::System;
use balaur_core::{App, AppConfig, Engine, Stage, digest, snapshot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Screen {
    Menu,
    Game,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Pause {
    Running,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Ticking;

type Log = Rc<RefCell<Vec<String>>>;

fn note(log: &Log, what: &str) -> impl FnMut(&Engine, f32) + 'static {
    let log = log.clone();
    let what = what.to_owned();
    move |_, _| log.borrow_mut().push(what.clone())
}

fn taken(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

fn app(log: &Log) -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    app.add_mode(Screen::Menu)
        .add_sub_mode(Screen::Game, Pause::Running)
        .add_computed_mode::<Ticking>(|eng| {
            (eng.mode::<Pause>() == Some(Pause::Running)).then_some(Ticking)
        });
    app.on_enter(Screen::Menu, note(log, "enter menu"))
        .on_exit(Screen::Menu, note(log, "exit menu"))
        .on_enter(Screen::Game, note(log, "enter game"))
        .on_enter(Pause::Running, note(log, "enter running"))
        .on_exit(Pause::Running, note(log, "exit running"))
        .on_enter(Ticking, note(log, "enter ticking"))
        .on_exit(Ticking, note(log, "exit ticking"));
    app
}

#[test]
fn the_first_value_is_entered_at_the_first_frame() {
    let log = Log::default();
    let mut app = app(&log);
    assert_eq!(app.engine.mode::<Screen>(), None);
    app.tick(0.0);
    assert_eq!(app.engine.mode::<Screen>(), Some(Screen::Menu));
    assert_eq!(app.engine.mode::<Pause>(), None, "the parent is elsewhere");
    assert_eq!(taken(&log), vec!["enter menu"]);
}

#[test]
fn a_set_mode_changes_at_the_next_frame_with_its_sub_and_computed_modes() {
    let log = Log::default();
    let mut app = app(&log);
    app.tick(0.0);
    taken(&log);
    app.engine.set_mode(Screen::Game);
    assert_eq!(app.engine.mode::<Screen>(), Some(Screen::Menu), "not yet");
    app.tick(0.0);
    assert_eq!(app.engine.mode::<Pause>(), Some(Pause::Running));
    assert_eq!(app.engine.mode::<Ticking>(), Some(Ticking));
    assert_eq!(
        taken(&log),
        vec!["exit menu", "enter game", "enter running", "enter ticking"]
    );
    app.engine.set_mode(Pause::Paused);
    app.tick(0.0);
    assert_eq!(taken(&log), vec!["exit running", "exit ticking"]);
    app.engine.set_mode(Screen::Menu);
    app.tick(0.0);
    assert_eq!(app.engine.mode::<Pause>(), None);
    app.engine.set_mode(Screen::Game);
    app.tick(0.0);
    assert_eq!(
        app.engine.mode::<Pause>(),
        Some(Pause::Running),
        "a sub-mode starts at its first value each time"
    );
}

#[test]
fn a_system_runs_in_one_mode() {
    let log = Log::default();
    let mut app = app(&log);
    app.add_scheduled(
        Stage::Update,
        System::new("play", note(&log, "play")).run_if(in_mode(Screen::Game)),
    );
    app.tick(0.0);
    assert!(!taken(&log).contains(&"play".to_owned()));
    app.engine.set_mode(Screen::Game);
    app.tick(0.0);
    assert!(taken(&log).contains(&"play".to_owned()));
}

#[test]
fn a_mode_is_in_the_digest_and_a_restore_enters_nothing() {
    let log = Log::default();
    let mut app = app(&log);
    app.tick(0.0);
    let before = digest::digest(&app.engine);
    let taken_at = snapshot::capture(&app.engine);
    app.engine.set_mode(Screen::Game);
    app.tick(0.0);
    assert_ne!(digest::digest(&app.engine), before);
    taken(&log);
    snapshot::restore(&app.engine, &taken_at);
    assert_eq!(app.engine.mode::<Screen>(), Some(Screen::Menu));
    assert_eq!(digest::digest(&app.engine), before);
    assert!(
        taken(&log).is_empty(),
        "a restore leaves and enters nothing"
    );
}
