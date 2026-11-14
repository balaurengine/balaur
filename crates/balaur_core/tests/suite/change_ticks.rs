//! Change ticks (PLAN-ecs §2): every write of a registered component is
//! stamped with the system run that made it, and a reader asks what was
//! added, changed or removed after the tick it last ran at.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::components::ticks::{self, ChangeCursor};
use balaur_core::{App, AppConfig, Engine, Stage, snapshot};

fn timer() -> usize {
    let app = App::new(AppConfig::bare(".")).unwrap();
    balaur_core::components::index_of(&app.engine, "timer").unwrap()
}

fn wait(seconds: f64) -> toml::Value {
    toml::Value::Table(toml::toml! { wait_time = seconds })
}

/// An app with one node, `Lamp`, and what a reader in `Last` saw changed on
/// it each frame.
fn app_with_reader() -> (App, hecs::Entity, Rc<RefCell<Vec<Vec<hecs::Entity>>>>) {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let lamp = app.engine.spawn_node("Lamp", app.engine.root());
    let index = timer();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let kept = seen.clone();
    let mut cursor = ChangeCursor::default();
    app.add_system(Stage::Last, move |eng: &Engine, _| {
        let since = cursor.read(eng);
        kept.borrow_mut()
            .push(ticks::changed_since(eng, index, since));
    });
    (app, lamp, seen)
}

#[test]
fn a_reader_hears_a_write_once() {
    let (mut app, lamp, seen) = app_with_reader();
    app.tick(1.0 / 60.0);
    balaur_core::components::patch(&app.engine, lamp, "timer", &wait(2.0)).unwrap();
    app.tick(1.0 / 60.0);
    app.tick(1.0 / 60.0);
    let seen = seen.borrow();
    assert_eq!(
        seen[0],
        Vec::<hecs::Entity>::new(),
        "nothing was written yet"
    );
    assert_eq!(seen[1], vec![lamp], "the write is heard by the next run");
    assert_eq!(seen[2], Vec::<hecs::Entity>::new(), "and not again");
}

#[test]
fn a_write_is_stamped_with_the_system_that_made_it() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let lamp = app.engine.spawn_node("Lamp", app.engine.root());
    let index = timer();
    let at = Rc::new(RefCell::new(0));
    let kept = at.clone();
    let mut seconds = 1.0;
    app.add_system(Stage::Update, move |eng: &Engine, _| {
        // A new value each run: a write of what it holds changes nothing.
        seconds += 1.0;
        balaur_core::components::patch(eng, lamp, "timer", &wait(seconds)).unwrap();
        *kept.borrow_mut() = eng.change_tick();
    });
    app.tick(1.0 / 60.0);
    let stamp = ticks::stamp(&app.engine, lamp, index).unwrap();
    assert_eq!(stamp.changed, *at.borrow());
    assert_eq!(stamp.added, *at.borrow(), "the write put the timer on");
    app.tick(1.0 / 60.0);
    let again = ticks::stamp(&app.engine, lamp, index).unwrap();
    assert!(again.changed > stamp.changed, "a later write stamps later");
    assert_eq!(again.added, stamp.added, "and the timer was put on once");
}

#[test]
fn added_names_only_what_went_on() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let lamp = app.engine.spawn_node("Lamp", app.engine.root());
    let index = timer();
    balaur_core::components::add(&app.engine, lamp, "timer", None).unwrap();
    let after_add = app.engine.change_tick();
    app.engine.advance_change_tick();
    balaur_core::components::patch(&app.engine, lamp, "timer", &wait(2.0)).unwrap();
    assert_eq!(ticks::added_since(&app.engine, index, after_add), vec![]);
    assert_eq!(
        ticks::changed_since(&app.engine, index, after_add),
        vec![lamp]
    );
    assert_eq!(ticks::added_since(&app.engine, index, 0), vec![lamp]);
}

#[test]
fn what_came_off_is_heard_for_two_frames() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let lamp = app.engine.spawn_node("Lamp", app.engine.root());
    let post = app.engine.spawn_node("Post", app.engine.root());
    let index = timer();
    for node in [lamp, post] {
        balaur_core::components::add(&app.engine, node, "timer", None).unwrap();
    }
    app.tick(1.0 / 60.0);
    let before = app.engine.change_tick() - 1;
    balaur_core::components::remove(&app.engine, lamp, "timer").unwrap();
    balaur_core::scene::free_node(&app.engine, post);
    assert_eq!(
        ticks::removed_since(&app.engine, index, before),
        vec![lamp, post],
        "a removal and a freed node, oldest first"
    );
    assert!(ticks::stamp(&app.engine, lamp, index).is_none());
    app.tick(1.0 / 60.0);
    assert_eq!(ticks::removed_since(&app.engine, index, before).len(), 2);
    app.tick(1.0 / 60.0);
    assert_eq!(
        ticks::removed_since(&app.engine, index, before),
        vec![],
        "two frames on, forgotten"
    );
}

#[test]
fn a_restore_is_heard_as_every_holder_changed() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let lamp = app.engine.spawn_node("Lamp", app.engine.root());
    let post = app.engine.spawn_node("Post", app.engine.root());
    let index = timer();
    for node in [lamp, post] {
        balaur_core::components::add(&app.engine, node, "timer", None).unwrap();
    }
    app.tick(1.0 / 60.0);
    let taken = snapshot::capture(&app.engine);
    let before = app.engine.change_tick() - 1;
    balaur_core::components::patch(&app.engine, lamp, "timer", &wait(5.0)).unwrap();
    snapshot::restore(&app.engine, &taken);
    // What a reader that ran at the restore covered.
    let restored = app.engine.change_tick() - 1;
    assert!(restored > before, "the change tick never goes back");
    let mut every = vec![lamp, post];
    every.sort_unstable_by_key(|node| node.to_bits());
    assert_eq!(ticks::changed_since(&app.engine, index, before), every);
    assert_eq!(ticks::changed_since(&app.engine, index, restored), vec![]);
    balaur_core::components::patch(&app.engine, post, "timer", &wait(4.0)).unwrap();
    assert_eq!(
        ticks::changed_since(&app.engine, index, restored),
        vec![post],
        "a write after the restore is stamped as before"
    );
}
