//! The fixed stage: how many steps a frame takes, and at what dt.
//!
//! The point of the stage is that a system inside it never sees the frame's
//! measured time, so these assert on the dt handed to it as much as on the
//! count.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::{App, AppConfig, Stage, FIXED_DT, MAX_SUBSTEPS};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

/// Every dt the fixed stage was called with, in order.
fn recording_app() -> (App, Rc<RefCell<Vec<f32>>>) {
    let mut app = app();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    app.add_system(Stage::FixedUpdate, move |_, dt| sink.borrow_mut().push(dt));
    (app, seen)
}

#[test]
fn a_frame_of_exactly_one_step_runs_the_stage_once() {
    let (mut app, seen) = recording_app();
    app.tick(FIXED_DT);
    assert_eq!(*seen.borrow(), vec![FIXED_DT]);
}

#[test]
fn a_long_frame_runs_several_steps_all_at_the_fixed_dt() {
    let (mut app, seen) = recording_app();
    app.tick(FIXED_DT * 3.0);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 3, "three steps' worth of time is three steps");
    assert!(
        seen.iter().all(|&dt| dt == FIXED_DT),
        "a fixed step that varies is not a fixed step: {seen:?}"
    );
}

#[test]
fn a_short_frame_runs_no_steps_and_carries_the_remainder() {
    let (mut app, seen) = recording_app();
    app.tick(FIXED_DT * 0.5);
    assert!(seen.borrow().is_empty(), "half a step is not a step");
    app.tick(FIXED_DT * 0.5);
    assert_eq!(
        seen.borrow().len(),
        1,
        "the two halves have to add up to one step"
    );
}

#[test]
fn time_past_the_substep_cap_is_dropped_rather_than_caught_up_on() {
    let (mut app, seen) = recording_app();
    app.tick(FIXED_DT * 100.0);
    assert_eq!(seen.borrow().len(), MAX_SUBSTEPS as usize);
}

/// The variable stage still sees the real frame time — it is the half of the
/// split that presentation code wants.
#[test]
fn the_update_stage_still_sees_the_measured_frame_time() {
    let mut app = app();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    app.add_system(Stage::Update, move |_, dt| sink.borrow_mut().push(dt));
    app.tick(0.25);
    assert_eq!(*seen.borrow(), vec![0.25]);
}

#[test]
fn the_fixed_stage_runs_after_update_and_before_post_update() {
    let mut app = app();
    let order = Rc::new(RefCell::new(Vec::new()));
    for (stage, label) in [
        (Stage::PostUpdate, "post"),
        (Stage::FixedUpdate, "fixed"),
        (Stage::Update, "update"),
    ] {
        let sink = Rc::clone(&order);
        app.add_system(stage, move |_, _| sink.borrow_mut().push(label));
    }
    app.tick(FIXED_DT);
    assert_eq!(*order.borrow(), vec!["update", "fixed", "post"]);
}
