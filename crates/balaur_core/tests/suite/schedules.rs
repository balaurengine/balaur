//! Schedules (PLAN-ecs §2): names, sets, order and conditions on a stage's
//! systems, sorted once; no order keeps the order of adding.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use balaur_core::schedule::{Set, System};
use balaur_core::{App, AppConfig, Engine, Stage};

type Log = Rc<RefCell<Vec<&'static str>>>;

fn note(log: &Log, name: &'static str) -> impl FnMut(&Engine, f32) + 'static {
    let log = log.clone();
    move |_, _| log.borrow_mut().push(name)
}

fn ran(app: &mut App, log: &Log) -> Vec<&'static str> {
    log.borrow_mut().clear();
    app.tick(0.0);
    log.borrow().clone()
}

#[test]
fn no_order_runs_in_the_order_added() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let log = Log::default();
    for name in ["a", "b", "c"] {
        app.add_scheduled(Stage::Update, System::new(name, note(&log, name)));
    }
    assert_eq!(ran(&mut app, &log), vec!["a", "b", "c"]);
}

#[test]
fn before_and_after_reorder_only_what_they_name() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let log = Log::default();
    app.add_scheduled(
        Stage::Update,
        System::new("draw", note(&log, "draw")).after("move"),
    );
    app.add_scheduled(Stage::Update, System::new("input", note(&log, "input")));
    app.add_scheduled(Stage::Update, System::new("move", note(&log, "move")));
    app.add_scheduled(
        Stage::Update,
        System::new("sound", note(&log, "sound")).before("input"),
    );
    let order = ran(&mut app, &log);
    let at = |name| order.iter().position(|ran| *ran == name).unwrap();
    assert!(
        at("move") < at("draw") && at("sound") < at("input"),
        "{order:?}"
    );
    assert_eq!(order.len(), 4);
}

#[test]
fn a_set_orders_and_gates_its_members() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let log = Log::default();
    let on = Rc::new(Cell::new(true));
    let gate = on.clone();
    app.add_scheduled(Stage::Update, System::new("hud", note(&log, "hud")));
    app.add_scheduled(
        Stage::Update,
        System::new("walk", note(&log, "walk")).in_set("sim"),
    );
    app.add_scheduled(
        Stage::Update,
        System::new("swim", note(&log, "swim")).in_set("sim"),
    );
    let asked = Rc::new(Cell::new(0));
    let counted = asked.clone();
    app.configure_set(
        Stage::Update,
        "sim",
        Set::new().before("hud").run_if(move |_| {
            counted.set(counted.get() + 1);
            gate.get()
        }),
    );
    assert_eq!(ran(&mut app, &log), vec!["walk", "swim", "hud"]);
    assert_eq!(
        asked.get(),
        1,
        "a set's condition is asked once a stage run"
    );
    on.set(false);
    assert_eq!(ran(&mut app, &log), vec!["hud"]);
}

#[test]
fn a_system_condition_skips_its_run() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let log = Log::default();
    let frames = Rc::new(Cell::new(0));
    let count = frames.clone();
    app.add_scheduled(
        Stage::Update,
        System::new("odd", note(&log, "odd")).run_if(move |_| {
            count.set(count.get() + 1);
            count.get() % 2 == 1
        }),
    );
    assert_eq!(ran(&mut app, &log), vec!["odd"]);
    assert_eq!(ran(&mut app, &log), Vec::<&str>::new());
    assert_eq!(ran(&mut app, &log), vec!["odd"]);
}

#[test]
fn a_cycle_is_named() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let log = Log::default();
    app.add_scheduled(Stage::Update, System::new("a", note(&log, "a")).before("b"));
    app.add_scheduled(Stage::Update, System::new("b", note(&log, "b")).before("c"));
    app.add_scheduled(Stage::Update, System::new("c", note(&log, "c")).before("a"));
    app.add_scheduled(Stage::Update, System::new("d", note(&log, "d")).after("c"));
    let why = app.check_schedules().unwrap_err().to_string();
    assert!(why.contains("Update"), "{why}");
    assert!(
        why.contains("a -> b -> c -> a")
            || why.contains("b -> c -> a -> b")
            || why.contains("c -> a -> b -> c"),
        "{why}"
    );
}

#[test]
fn an_unknown_name_orders_nothing() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let log = Log::default();
    app.add_scheduled(
        Stage::Update,
        System::new("a", note(&log, "a")).after("missing"),
    );
    app.add_scheduled(Stage::Update, System::new("b", note(&log, "b")));
    app.check_schedules().unwrap();
    assert_eq!(ran(&mut app, &log), vec!["a", "b"]);
}

#[test]
fn a_plain_system_is_named_by_its_function() {
    fn count_system(_: &Engine, _: f32) {}
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    let log = Log::default();
    app.add_system(Stage::Update, count_system);
    let name = std::any::type_name_of_val(&count_system);
    app.add_scheduled(
        Stage::Update,
        System::new("first", note(&log, "first")).before(name),
    );
    app.check_schedules().unwrap();
    assert!(name.ends_with("count_system"), "{name}");
}
