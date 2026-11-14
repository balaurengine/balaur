//! What keeps a sleeping loop awake: the registry's OR, the kernel's own
//! probes, and the step it waits for after the last thing pending.

use balaur_core::process::{self, ProcessMode};
use balaur_core::scene::spawn_node;
use balaur_core::task::{self, Progress, Stepped};
use balaur_core::{App, AppConfig, Engine, fixed_dt, rest, timer, timers};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

fn pends(app: &App, name: &str) -> bool {
    rest::pending_names(&app.engine).contains(&name)
}

/// A node under the root carrying a running timer of `seconds`.
fn timer_on(app: &App, name: &str, seconds: f64, one_shot: bool) -> hecs::Entity {
    let node = spawn_node(&mut app.engine.world_mut(), name, app.engine.root());
    let mut asked = toml::Table::new();
    asked.insert("wait_time".into(), seconds.into());
    asked.insert("one_shot".into(), one_shot.into());
    asked.insert("running".into(), true.into());
    let asked = toml::Value::Table(asked);
    balaur_core::components::patch(&app.engine, node, timer::COMPONENT, &asked).unwrap();
    node
}

/// While this is in the engine, the `loud` probe below answers pending.
struct Loud;

fn never(_: &Engine) -> bool {
    false
}

fn while_loud(eng: &Engine) -> bool {
    eng.try_resource::<Loud>().is_some()
}

#[test]
fn the_registry_answers_pending_when_any_one_probe_does() {
    let app = app();
    assert!(
        !rest::pending(&app.engine),
        "a bare app has nothing to wait for"
    );

    rest::add_probe(&app.engine, "quiet", never);
    rest::add_probe(&app.engine, "loud", while_loud);
    assert!(!rest::pending(&app.engine), "neither has anything yet");

    app.engine.insert_resource(Loud);
    assert!(rest::pending(&app.engine));
    assert_eq!(rest::pending_names(&app.engine), ["loud"]);

    app.engine.remove_resource::<Loud>();
    assert!(!rest::pending(&app.engine));
}

#[test]
fn a_one_shot_timer_is_pending_until_its_timeout() {
    let mut app = app();
    timer_on(&app, "Alarm", f64::from(fixed_dt()) * 3.5, true);

    app.tick(fixed_dt());
    assert!(
        pends(&app, timer::COMPONENT),
        "one step in, it still counts"
    );

    app.tick(fixed_dt() * 4.0);
    assert!(!pends(&app, timer::COMPONENT), "it ran out and stopped");
}

#[test]
fn a_repeating_timer_is_pending_across_its_timeouts() {
    let mut app = app();
    timer_on(&app, "Beat", f64::from(fixed_dt()) * 2.0, false);

    for _ in 0..6 {
        app.tick(fixed_dt());
        assert!(pends(&app, timer::COMPONENT));
    }
}

#[test]
fn a_paused_tree_holds_its_timers_at_rest_and_an_always_node_keeps_counting() {
    let mut app = app();
    let alarm = timer_on(&app, "Alarm", 1.0, true);
    app.engine.set_paused(true);

    app.tick(fixed_dt());
    assert!(!pends(&app, timer::COMPONENT), "the pause holds the timer");

    process::set(&mut app.engine.world_mut(), alarm, ProcessMode::Always);
    app.tick(fixed_dt());
    assert!(
        pends(&app, timer::COMPONENT),
        "an always node runs through it"
    );
}

#[test]
fn a_timer_in_a_game_a_tool_hosts_does_not_keep_the_tool_awake() {
    let mut app = app();
    let alarm = timer_on(&app, "Alarm", 1.0, false);

    app.engine.set_debug_scope(Some(alarm));
    app.tick(fixed_dt());
    assert!(
        !pends(&app, timer::COMPONENT),
        "the hosted timer is the tool's"
    );

    app.engine.set_debug_scope(None);
    app.tick(fixed_dt());
    assert!(
        pends(&app, timer::COMPONENT),
        "a game's own timer is its own"
    );
}

#[test]
fn a_task_seconds_wait_is_pending_until_it_is_woken() {
    let mut app = app();
    timers::after_seconds(&app.engine, fixed_dt() * 2.5);
    assert!(
        pends(&app, rest::TASK),
        "a wait counts from the moment it is asked"
    );

    app.tick(fixed_dt() * 2.0);
    assert!(pends(&app, rest::TASK));

    app.tick(fixed_dt());
    assert!(!pends(&app, rest::TASK), "woken, it is gone");
}

#[test]
fn a_task_frames_wait_is_pending_until_its_step() {
    let mut app = app();
    timers::after_frames(&app.engine, 2);
    assert!(pends(&app, rest::TASK));

    app.tick(fixed_dt() * 2.0);
    assert!(!pends(&app, rest::TASK));
}

#[test]
fn a_paused_game_holds_every_wait_at_rest() {
    let app = app();
    timers::after_seconds(&app.engine, 1.0);
    app.engine.set_paused(true);
    assert!(
        !pends(&app, rest::TASK),
        "no wait counts while the game is paused"
    );

    app.engine.set_paused(false);
    assert!(pends(&app, rest::TASK));
}

#[test]
fn an_awaited_calls_result_is_pending_through_a_pause_until_its_step() {
    let mut app = app();
    timers::wake_next_step(&app.engine, 7, balaur_script::Value::Nil);
    app.engine.set_paused(true);
    assert!(pends(&app, rest::TASK), "a result is owed, not timed");

    app.tick(fixed_dt());
    assert!(!pends(&app, rest::TASK), "the step delivered it");
}

/// A job that takes `left` slices.
struct Slices {
    left: usize,
}

impl Stepped for Slices {
    fn step(&mut self) -> Progress {
        self.left -= 1;
        if self.left == 0 {
            Progress::Done
        } else {
            Progress::More
        }
    }
}

#[test]
fn a_parked_job_is_pending_until_its_last_slice() {
    let mut app = app();
    task::park(Slices { left: 2 });
    assert!(pends(&app, rest::TASK));

    app.tick(fixed_dt());
    assert!(
        pends(&app, rest::TASK),
        "one slice a frame, and one is left"
    );

    app.tick(fixed_dt());
    assert!(!pends(&app, rest::TASK));
}

#[test]
fn the_loop_stays_up_for_one_step_after_the_last_thing_pending() {
    let mut app = app();
    timer_on(&app, "Alarm", f64::from(fixed_dt()) * 1.5, true);
    app.tick(fixed_dt());
    assert!(rest::busy(&app.engine), "the timer is counting");

    app.tick(fixed_dt());
    assert!(!rest::pending(&app.engine), "it ran out on this step");
    assert!(
        rest::busy(&app.engine),
        "what its timeout started is counted by the next step"
    );
    assert!(
        rest::busy(&app.engine),
        "and no frame has run that step yet"
    );

    app.tick(fixed_dt());
    assert!(!rest::busy(&app.engine), "a step ran and found nothing");
}

#[test]
fn a_frame_owed_for_another_reason_is_followed_by_a_step_before_the_loop_sleeps() {
    let mut app = app();
    assert!(rest::busy(&app.engine));
    app.tick(fixed_dt());
    assert!(!rest::busy(&app.engine), "at rest");

    rest::disturb(&app.engine);
    app.tick(fixed_dt());
    assert!(
        rest::busy(&app.engine),
        "an input's frame may have started work"
    );

    app.tick(fixed_dt() * 0.25);
    assert!(
        rest::busy(&app.engine),
        "a frame too short for a step counts nothing"
    );

    app.tick(fixed_dt());
    assert!(!rest::busy(&app.engine));
}

#[test]
fn a_debugger_stop_and_a_zero_time_scale_are_at_rest_whatever_is_pending() {
    let mut app = app();
    timer_on(&app, "Alarm", 1.0, false);
    app.tick(fixed_dt());
    assert!(rest::busy(&app.engine));

    app.engine.set_frozen(true);
    assert!(
        !rest::busy(&app.engine),
        "no step runs while a debugger holds it"
    );
    app.engine.set_frozen(false);
    assert!(rest::busy(&app.engine));

    app.engine.set_time_scale(0.0);
    assert!(!rest::busy(&app.engine), "nor does time pass at scale zero");
}
