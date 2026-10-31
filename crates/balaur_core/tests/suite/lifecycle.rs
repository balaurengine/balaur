//! A component's lifecycle calls (PLAN-ecs §2): what runs as it goes onto a
//! node, is written, is written over, comes off and goes with its node.

use std::cell::RefCell;

use anyhow::Result;
use balaur_core::Engine;
use balaur_core::app::{App, AppConfig};
use balaur_core::components::derived::{Component, Written};
use balaur_core::components::{self, ComponentDef};
use hecs::Entity;

thread_local! {
    /// Each call as it ran, with the level the dial held then.
    static HEARD: RefCell<Vec<(&'static str, f32)>> = const { RefCell::new(Vec::new()) };
}

/// A level to 1, refused past it, that says each point of its life.
#[derive(Debug, Component)]
#[component(
    name = "logged_dial",
    on_add = add,
    on_insert = insert,
    on_replace = replace,
    on_remove = remove,
    on_despawn = despawn
)]
struct LoggedDial {
    #[property(default = 0.5)]
    level: f32,
}

fn level(eng: &Engine, entity: Entity) -> f32 {
    eng.world()
        .get::<&LoggedDial>(entity)
        .map_or(f32::NAN, |dial| dial.level)
}

fn note(point: &'static str, eng: &Engine, entity: Entity) {
    let level = level(eng, entity);
    HEARD.with(|heard| heard.borrow_mut().push((point, level)));
}

fn add(eng: &Engine, entity: Entity) {
    note("add", eng, entity);
}
fn insert(eng: &Engine, entity: Entity) {
    note("insert", eng, entity);
}
fn replace(eng: &Engine, entity: Entity) {
    note("replace", eng, entity);
}
fn remove(eng: &Engine, entity: Entity) {
    note("remove", eng, entity);
}
fn despawn(eng: &Engine, entity: Entity) {
    note("despawn", eng, entity);
}

fn refuse_past_one(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let level = level(eng, entity);
    anyhow::ensure!(level <= 1.0, "a dial goes to 1, not {level}");
    Ok(())
}

fn heard() -> Vec<(&'static str, f32)> {
    HEARD.with(|heard| std::mem::take(&mut *heard.borrow_mut()))
}

fn app_with_dial() -> (App, Entity) {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    app.register_component(
        LoggedDial::NAME,
        ComponentDef::of::<LoggedDial>(Some(refuse_past_one)),
    );
    let e = app.engine.spawn_node("Dial", app.engine.root());
    heard();
    (app, e)
}

fn level_table(level: f64) -> toml::Value {
    toml::Value::Table(toml::toml! { level = level })
}

#[test]
fn going_on_runs_add_then_insert() {
    let (app, e) = app_with_dial();
    components::add(&app.engine, e, "logged_dial", None).unwrap();
    assert_eq!(heard(), vec![("add", 0.5), ("insert", 0.5)]);
}

#[test]
fn a_write_runs_replace_on_what_was_held_then_insert_on_what_is() {
    let (app, e) = app_with_dial();
    components::add(&app.engine, e, "logged_dial", None).unwrap();
    heard();
    components::patch(&app.engine, e, "logged_dial", &level_table(0.75)).unwrap();
    assert_eq!(heard(), vec![("replace", 0.5), ("insert", 0.75)]);
    components::set_property(
        &app.engine,
        e,
        "logged_dial",
        "level",
        &toml::Value::Float(0.25),
    )
    .unwrap();
    assert_eq!(
        heard(),
        vec![("replace", 0.75), ("insert", 0.25)],
        "one property written alone runs the same pair"
    );
}

#[test]
fn a_refused_write_inserts_what_was_put_back() {
    let (app, e) = app_with_dial();
    components::add(&app.engine, e, "logged_dial", None).unwrap();
    heard();
    components::patch(&app.engine, e, "logged_dial", &level_table(3.0)).unwrap_err();
    assert_eq!(heard(), vec![("replace", 0.5), ("insert", 0.5)]);
}

#[test]
fn a_refused_first_write_runs_nothing() {
    let (app, e) = app_with_dial();
    components::add(&app.engine, e, "logged_dial", Some(&level_table(3.0))).unwrap_err();
    assert_eq!(heard(), vec![]);
}

#[test]
fn coming_off_runs_replace_then_remove_with_the_values_there() {
    let (app, e) = app_with_dial();
    components::add(&app.engine, e, "logged_dial", None).unwrap();
    heard();
    components::remove(&app.engine, e, "logged_dial").unwrap();
    assert_eq!(heard(), vec![("replace", 0.5), ("remove", 0.5)]);
}

#[test]
fn a_freed_node_runs_despawn_first() {
    let (app, e) = app_with_dial();
    components::add(&app.engine, e, "logged_dial", None).unwrap();
    heard();
    balaur_core::scene::free_node(&app.engine, e);
    assert_eq!(
        heard(),
        vec![("despawn", 0.5), ("replace", 0.5), ("remove", 0.5)]
    );
}

#[test]
fn an_unchanged_write_runs_nothing() {
    let (app, e) = app_with_dial();
    components::add(&app.engine, e, "logged_dial", None).unwrap();
    heard();
    components::patch(&app.engine, e, "logged_dial", &level_table(0.5)).unwrap();
    assert_eq!(heard(), vec![]);
}
