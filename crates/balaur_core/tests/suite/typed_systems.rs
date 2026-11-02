//! Typed systems (PLAN-ecs §2): a function whose parameters say what it reads
//! and writes.
#![allow(
    clippy::needless_pass_by_value,
    reason = "a typed system takes its parameters by value, as the engine hands them"
)]

use balaur_core::schedule::System;
use balaur_core::system_param::{
    Added, Changed, Commands, Local, MessageReader, MessageWriter, Query, Res, ResMut, Step,
    Tracked,
};
use balaur_core::timer::Timer;
use balaur_core::{App, AppConfig, Name, Stage, fixed_dt};
use std::hint::black_box;

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

/// A weight on a loose entity, for the batched calls.
struct Weight(f32);

#[derive(Default)]
struct Total(f32);

fn double_weights(mut weights: Query<'_, &mut Weight>) {
    weights.par_for_each(64, |weight| weight.0 *= 2.0);
}

fn total_weights(mut weights: Query<'_, &Weight>, mut total: ResMut<'_, Total>) {
    total.0 = weights.par_fold(64, || 0.0, |sum, weight| sum + weight.0, |a, b| a + b);
}

#[test]
fn batches_work_every_match_and_fold_the_same_every_run() {
    let run = || {
        let mut app = app();
        app.engine.insert_resource(Total::default());
        for i in 0..5_000 {
            app.engine.spawn((Weight(0.1 + (i % 7) as f32 * 0.37),));
        }
        app.add_typed(Stage::Update, double_weights);
        app.add_typed(Stage::PostUpdate, total_weights);
        app.tick(0.0);
        app.engine.resource::<Total>().borrow().0
    };
    let first = run();
    let serial: f32 = (0..5_000)
        .map(|i| (0.1 + (i % 7) as f32 * 0.37) * 2.0)
        .sum();
    // Summed in batches, not one by one: the same to f32's precision.
    assert!(
        (first - serial).abs() < serial * 1e-4,
        "{first} against {serial}"
    );
    for _ in 0..5 {
        assert_eq!(run().to_bits(), first.to_bits(), "the same bits every run");
    }
}

/// Which pass of the tracked tests runs: what each system writes.
#[derive(Default)]
struct Pass(u32);

fn write_timers(mut timers: Query<'_, Tracked<Timer>>, pass: Res<'_, Pass>) {
    timers.for_each(|mut timer| match pass.0 {
        // Read through the item only: nothing is written.
        0 => {
            black_box(timer.wait);
        }
        1 => timer.wait += 1.0,
        _ => {}
    });
}

fn write_untracked(mut timers: Query<'_, &mut Timer>, pass: Res<'_, Pass>) {
    if pass.0 == 2 {
        timers.for_each(|timer| timer.wait += 1.0);
    }
}

fn write_in_batches(mut timers: Query<'_, Tracked<Timer>>, pass: Res<'_, Pass>) {
    if pass.0 == 3 {
        timers.par_for_each(2, |mut timer| timer.wait += 1.0);
    }
}

fn heard(changed: Changed<Timer>, mut heard: ResMut<'_, Heard>) {
    heard.0.push(u32::try_from(changed.len()).unwrap());
}

#[test]
fn a_tracked_query_s_writes_are_changes_and_a_plain_one_s_are_not() {
    let mut app = app();
    app.engine.insert_resource(Pass(9));
    let root = app.engine.root();
    for i in 0..5 {
        let node = app.engine.spawn_node(&format!("t{i}"), root);
        balaur_core::components::add(&app.engine, node, "timer", None).unwrap();
    }
    app.add_typed(Stage::Update, write_timers);
    app.add_typed(Stage::Update, write_untracked);
    app.add_typed(Stage::Update, write_in_batches);
    app.add_typed(Stage::PostUpdate, heard);
    app.tick(0.0);
    for pass in 0..4 {
        app.engine.resource::<Pass>().borrow_mut().0 = pass;
        app.tick(0.0);
    }
    assert_eq!(
        app.engine.resource::<Heard>().borrow().0,
        vec![5, 0, 5, 0, 5],
        "the adds, a read, a tracked write, a plain one, a batched one"
    );
}
