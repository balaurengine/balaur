//! Required components (PLAN-ecs §2): a component that brings others, each
//! put on at its defaults before its first write where the node lacks it.

use balaur_core::app::{App, AppConfig};
use balaur_core::components::derived::Component;
use balaur_core::components::{self, ComponentDef};

/// Brings a timer, and through `alarm` the timer brings nothing more.
#[derive(Debug, Component)]
#[component(name = "alarm", requires = ["timer"])]
struct Alarm {
    #[property(default = 1)]
    rings: i64,
}

/// Brings an alarm, which brings a timer.
#[derive(Debug, Component)]
#[component(name = "clock_tower", requires = ["alarm"])]
struct ClockTower {
    #[property(default = 4)]
    faces: i64,
}

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    app.register_component(Alarm::NAME, ComponentDef::of::<Alarm>(None));
    app.register_component(ClockTower::NAME, ComponentDef::of::<ClockTower>(None));
    app
}

#[test]
fn adding_brings_what_it_requires_at_its_defaults() {
    let app = app();
    let node = app.engine.spawn_node("N", app.engine.root());
    components::add(&app.engine, node, "clock_tower", None).unwrap();
    assert!(components::has(&app.engine, node, "alarm"));
    assert!(
        components::has(&app.engine, node, "timer"),
        "the alarm's own"
    );
    let timer = components::get(&app.engine, node, "timer").unwrap();
    assert_eq!(timer["wait_time"].as_float(), Some(1.0));
}

#[test]
fn a_patch_that_adds_brings_them_too_and_one_held_is_kept() {
    let app = app();
    let node = app.engine.spawn_node("N", app.engine.root());
    let wait = toml::Value::Table(toml::toml! { wait_time = 5.0 });
    components::add(&app.engine, node, "timer", Some(&wait)).unwrap();
    let rings = toml::Value::Table(toml::toml! { rings = 3 });
    components::patch(&app.engine, node, "alarm", &rings).unwrap();
    let timer = components::get(&app.engine, node, "timer").unwrap();
    assert_eq!(
        timer["wait_time"].as_float(),
        Some(5.0),
        "held, so untouched"
    );
}

#[test]
fn taking_it_off_leaves_what_it_brought() {
    let app = app();
    let node = app.engine.spawn_node("N", app.engine.root());
    components::add(&app.engine, node, "alarm", None).unwrap();
    components::remove(&app.engine, node, "alarm").unwrap();
    assert!(components::has(&app.engine, node, "timer"));
}
