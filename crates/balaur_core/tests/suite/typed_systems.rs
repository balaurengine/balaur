//! Typed systems (PLAN-ecs §2): a function whose parameters say what it reads
//! and writes.
#![allow(
    clippy::needless_pass_by_value,
    reason = "a typed system takes its parameters by value, as the engine hands them"
)]

use balaur_core::schedule::System;
use balaur_core::system_param::{
    Added, Changed, Commands, Local, MessageReader, MessageWriter, Query, Res, ResMut, Step,
};
use balaur_core::timer::Timer;
use balaur_core::{App, AppConfig, Name, Stage, fixed_dt};

#[derive(Default)]
struct Score(u32);
struct Bonus(u32);
#[derive(Clone)]
struct Hit(u32);
#[derive(Default)]
struct Heard(Vec<u32>);
#[derive(Default)]
struct Seen(Vec<String>);

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    app.engine.insert_resource(Score::default());
    app.engine.insert_resource(Bonus(5));
    app.engine.insert_resource(Heard::default());
    app.engine.insert_resource(Seen::default());
    app.add_messages::<Hit>();
    app
}

fn add_bonus(mut score: ResMut<'_, Score>, bonus: Res<'_, Bonus>) {
    score.0 += bonus.0;
}

#[test]
fn resources_are_read_and_written() {
    let mut app = app();
    app.add_typed(Stage::Update, add_bonus);
    app.tick(0.0);
    app.tick(0.0);
    assert_eq!(app.engine.resource::<Score>().borrow().0, 10);
}

fn count_runs(mut runs: Local<'_, u32>, mut score: ResMut<'_, Score>) {
    *runs += 1;
    score.0 = *runs;
}

#[test]
fn a_local_lives_between_runs() {
    let mut app = app();
    app.add_typed(Stage::Update, count_runs);
    for _ in 0..3 {
        app.tick(0.0);
    }
    assert_eq!(app.engine.resource::<Score>().borrow().0, 3);
}

fn names(mut nodes: Query<'_, &Name>, mut seen: ResMut<'_, Seen>) {
    seen.0 = nodes.borrow().iter().map(|name| name.0.clone()).collect();
    seen.0.sort();
}

#[test]
fn a_query_iterates_its_matches() {
    let mut app = app();
    let root = app.engine.root();
    app.engine.spawn_node("Mast", root);
    app.engine.spawn_node("Sail", root);
    app.add_typed(Stage::Update, names);
    app.tick(0.0);
    let seen = app.engine.resource::<Seen>();
    assert!(seen.borrow().0.contains(&"Mast".to_owned()));
    assert!(seen.borrow().0.contains(&"Sail".to_owned()));
}

fn shoot(hits: MessageWriter<'_, Hit>, mut fired: Local<'_, u32>) {
    *fired += 1;
    hits.send(Hit(*fired));
}

fn listen(mut hits: MessageReader<'_, Hit>, mut heard: ResMut<'_, Heard>) {
    hits.read(|hit| heard.0.push(hit.0));
}

#[test]
fn messages_go_from_writer_to_reader() {
    let mut app = app();
    app.add_typed(Stage::Update, shoot);
    app.add_typed(Stage::PostUpdate, listen);
    app.tick(0.0);
    app.tick(0.0);
    assert_eq!(app.engine.resource::<Heard>().borrow().0, vec![1, 2]);
}

fn spawn_one(commands: Commands<'_>, mut done: Local<'_, bool>) {
    if !*done {
        commands.defer_spawn_node("Spawned", commands.root());
        *done = true;
    }
}

#[test]
fn commands_defer_their_writes() {
    let mut app = app();
    app.add_typed(Stage::Update, spawn_one);
    app.add_typed(Stage::PostUpdate, names);
    app.tick(0.0);
    assert!(
        app.engine
            .resource::<Seen>()
            .borrow()
            .0
            .contains(&"Spawned".to_owned())
    );
}

fn fixed_step(step: Step, mut score: ResMut<'_, Score>) {
    score.0 = (step.0 * 1000.0).round() as u32;
}

#[test]
fn step_is_what_the_stage_hands() {
    let mut app = app();
    app.add_typed(Stage::FixedUpdate, fixed_step);
    app.tick(fixed_dt());
    assert_eq!(
        app.engine.resource::<Score>().borrow().0,
        (fixed_dt() * 1000.0).round() as u32
    );
}

fn timers(added: Added<Timer>, changed: Changed<Timer>, mut heard: ResMut<'_, Heard>) {
    heard
        .0
        .push(u32::try_from(added.len() * 10 + changed.len()).unwrap());
}

#[test]
fn added_and_changed_name_what_moved_since_the_last_run() {
    let mut app = app();
    let root = app.engine.root();
    let node = app.engine.spawn_node("Clock", root);
    app.add_typed(Stage::Update, timers);
    app.tick(0.0);
    balaur_core::components::add(&app.engine, node, "timer", None).unwrap();
    app.tick(0.0);
    let wait = toml::Value::Table(toml::toml! { wait_time = 3.0 });
    balaur_core::components::patch(&app.engine, node, "timer", &wait).unwrap();
    app.tick(0.0);
    app.tick(0.0);
    assert_eq!(app.engine.resource::<Heard>().borrow().0, vec![0, 11, 1, 0]);
}

fn both(_score: ResMut<'_, Score>, _again: Res<'_, Score>) {}

#[test]
#[should_panic(expected = "system clashing: balaur_core::system_param::Res")]
fn a_clash_names_the_system_and_the_parameter() {
    let mut app = app();
    app.add_scheduled(Stage::Update, System::typed("clashing", both));
    app.tick(0.0);
}
