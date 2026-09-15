//! The game's own pause: which nodes it holds, what it announces, and the
//! time scale and tick rate that ride beside it.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::process::{self, ProcessMode};
use balaur_core::scene::spawn_node;
use balaur_core::{App, AppConfig, Stage, fixed_dt, set_tick_hz};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

/// A root child, a grandchild under it, and the app they live in.
fn tree() -> (App, hecs::Entity, hecs::Entity) {
    let app = app();
    let root = app.engine.root();
    let (menu, label) = {
        let mut world = app.engine.world_mut();
        let menu = spawn_node(&mut world, "Menu", root);
        let label = spawn_node(&mut world, "Label", menu);
        (menu, label)
    };
    (app, menu, label)
}

#[test]
fn a_pausable_node_stops_while_paused_and_an_always_node_does_not() {
    let (app, menu, _) = tree();
    assert!(process::ticking(&app.engine, menu));

    app.engine.set_paused(true);
    assert!(!process::ticking(&app.engine, menu));

    process::set(&mut app.engine.world_mut(), menu, ProcessMode::Always);
    assert!(process::ticking(&app.engine, menu));
}

#[test]
fn a_process_mode_reaches_the_whole_subtree() {
    let (app, menu, label) = tree();
    app.engine.set_paused(true);
    process::set(&mut app.engine.world_mut(), menu, ProcessMode::Always);
    assert!(
        process::ticking(&app.engine, label),
        "a child inherits the mode its parent was set to"
    );

    process::set(&mut app.engine.world_mut(), label, ProcessMode::Pausable);
    assert!(
        !process::ticking(&app.engine, label),
        "and may say otherwise for itself"
    );
}

#[test]
fn a_disabled_node_ticks_neither_paused_nor_running() {
    let (app, menu, label) = tree();
    process::set(&mut app.engine.world_mut(), menu, ProcessMode::Disabled);
    assert!(!process::ticking(&app.engine, label));
    app.engine.set_paused(true);
    assert!(!process::ticking(&app.engine, label));
}

#[test]
fn a_when_paused_node_ticks_only_while_paused() {
    let (app, menu, _) = tree();
    process::set(&mut app.engine.world_mut(), menu, ProcessMode::WhenPaused);
    assert!(!process::ticking(&app.engine, menu));
    app.engine.set_paused(true);
    assert!(process::ticking(&app.engine, menu));
}

#[test]
fn setting_a_node_back_to_inherit_takes_the_key_off() {
    let (app, menu, _) = tree();
    process::set(&mut app.engine.world_mut(), menu, ProcessMode::Always);
    assert_eq!(process::own(&app.engine.world(), menu), ProcessMode::Always);
    process::set(&mut app.engine.world_mut(), menu, ProcessMode::Inherit);
    assert_eq!(
        process::own(&app.engine.world(), menu),
        ProcessMode::Inherit
    );
}

#[test]
fn a_pause_inside_an_editor_reaches_the_game_and_not_the_shell() {
    let (app, menu, _) = tree();
    let shell = spawn_node(&mut app.engine.world_mut(), "Shell", app.engine.root());
    app.engine.set_debug_scope(Some(menu));
    app.engine.set_paused(true);

    assert!(!process::ticking(&app.engine, menu), "the game is held");
    assert!(
        process::ticking(&app.engine, shell),
        "the editor around it keeps drawing and keeps ticking"
    );
}

#[test]
fn a_pause_is_announced_once_and_a_repeat_says_nothing() {
    let app = app();
    assert_eq!(app.engine.take_pause_change(), None);

    app.engine.set_paused(true);
    app.engine.set_paused(true);
    assert_eq!(app.engine.take_pause_change(), Some(true));
    assert_eq!(app.engine.take_pause_change(), None, "announced once");

    app.engine.set_paused(false);
    assert_eq!(app.engine.take_pause_change(), Some(false));
}

#[test]
fn the_fixed_step_keeps_running_while_paused_so_an_always_subtree_can_tick() {
    let mut app = app();
    let steps = Rc::new(RefCell::new(0));
    let counter = Rc::clone(&steps);
    app.add_system(Stage::FixedUpdate, move |_, _| *counter.borrow_mut() += 1);

    app.engine.set_paused(true);
    app.tick(fixed_dt() * 2.0);
    assert_eq!(
        *steps.borrow(),
        2,
        "the stage runs and each subsystem skips what the pause holds"
    );
}

#[test]
fn a_timer_is_held_by_a_pause_unless_its_node_runs_always() {
    let (mut app, menu, _) = tree();
    let held = spawn_node(&mut app.engine.world_mut(), "Held", app.engine.root());
    let wait = toml::toml! { wait_time = 1.0 running = true };
    for node in [menu, held] {
        balaur_core::components::patch(
            &app.engine,
            node,
            "timer",
            &toml::Value::Table(wait.clone()),
        )
        .unwrap();
    }
    process::set(&mut app.engine.world_mut(), menu, ProcessMode::Always);

    app.engine.set_paused(true);
    app.tick(fixed_dt() * 6.0);

    let left = |node| {
        balaur_core::components::get(&app.engine, node, "timer")
            .and_then(|v| v.get("time_left").and_then(toml::Value::as_float))
            .unwrap()
    };
    assert!(left(menu) < 1.0, "an always node's timer counts down");
    assert!((left(held) - 1.0).abs() < 1e-9, "a paused one does not");
}

#[test]
fn a_time_scale_of_a_half_takes_half_the_steps_and_double_takes_twice() {
    let count_steps = |scale: f32, frames: u32| {
        let mut app = app();
        let steps = Rc::new(RefCell::new(0));
        let counter = Rc::clone(&steps);
        app.add_system(Stage::FixedUpdate, move |_, _| *counter.borrow_mut() += 1);
        app.engine.set_time_scale(scale);
        for _ in 0..frames {
            app.advance(fixed_dt());
        }
        *steps.borrow()
    };
    assert_eq!(count_steps(1.0, 4), 4);
    assert_eq!(count_steps(0.5, 4), 2, "half speed owes half the time");
    assert_eq!(count_steps(2.0, 4), 8, "and fast forward owes twice");
}

#[test]
fn a_time_scale_of_zero_stops_time_without_stopping_a_node() {
    let (mut app, menu, _) = tree();
    let steps = Rc::new(RefCell::new(0));
    let counter = Rc::clone(&steps);
    app.add_system(Stage::FixedUpdate, move |_, _| *counter.borrow_mut() += 1);
    app.engine.set_time_scale(0.0);
    app.advance(fixed_dt() * 10.0);
    assert_eq!(*steps.borrow(), 0);
    assert!(
        process::ticking(&app.engine, menu),
        "no time passing is not the same thing as a pause"
    );
}

#[test]
fn a_negative_time_scale_is_refused() {
    let app = app();
    app.engine.set_time_scale(-2.0);
    assert!(app.engine.time_scale().abs() < f32::EPSILON);
}

#[test]
fn the_tick_rate_decides_the_step_every_fixed_system_is_handed() {
    let mut app = app();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    app.add_system(Stage::FixedUpdate, move |_, dt| sink.borrow_mut().push(dt));

    set_tick_hz(120);
    assert!((fixed_dt() - 1.0 / 120.0).abs() < f32::EPSILON);
    app.tick(fixed_dt() * 2.0);
    set_tick_hz(60);

    let seen = seen.borrow();
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|&dt| (dt - 1.0 / 120.0).abs() < 1e-6));
}

#[test]
fn a_faster_tick_rate_gets_the_same_wall_clock_of_catch_up() {
    set_tick_hz(120);
    let doubled = balaur_core::max_substeps();
    set_tick_hz(60);
    assert_eq!(doubled, balaur_core::MAX_SUBSTEPS * 2);
}

#[test]
fn a_recording_carries_the_rate_it_was_made_at_and_a_replay_takes_it_back() {
    let app = app();
    set_tick_hz(120);
    let session = balaur_core::replay::Session {
        header: balaur_core::replay::Header {
            tick_hz: balaur_core::tick_hz(),
            ..balaur_core::replay::Header::default()
        },
        frames: Vec::new(),
        trailer: None,
    };
    set_tick_hz(30);

    balaur_core::replay::begin(&app.engine, session);
    assert_eq!(
        balaur_core::tick_hz(),
        120,
        "the rate every frame was taken at"
    );
    set_tick_hz(60);
}

#[test]
fn a_recording_from_before_the_rate_was_a_setting_replays_at_the_default() {
    let app = app();
    let session = balaur_core::replay::Session {
        header: balaur_core::replay::Header::default(),
        frames: Vec::new(),
        trailer: None,
    };
    set_tick_hz(120);
    balaur_core::replay::begin(&app.engine, session);
    assert_eq!(
        balaur_core::tick_hz(),
        120,
        "a zero in the header says nothing, so it does not move the rate"
    );
    set_tick_hz(60);
}

#[test]
fn max_fps_is_what_the_loop_gives_a_frame() {
    let app = app();
    assert_eq!(app.frame_budget(), None, "uncapped by default");
    balaur_core::settings::set(&app.engine, "window/max_fps", toml::Value::Integer(30));
    assert_eq!(
        app.frame_budget(),
        Some(std::time::Duration::from_secs_f32(1.0 / 30.0))
    );
}
