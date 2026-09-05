//! The platform plugin driven from Rust, against a store that answers on the
//! spot. No device and no network: what these check is the seam every real
//! backend sits behind.

use std::sync::mpsc::Sender;

use balaur_core::rollback::Clock;
use balaur_core::{App, AppConfig};
use balaur_platform::{
    Call, PlatformBackend, PlatformEvent, PlatformPlugin, PlatformSnapshot, PlatformState, Player,
};
use balaur_script::Value;

/// A store that says yes to everything, immediately.
struct Canned;

impl PlatformBackend for Canned {
    fn name(&self) -> &'static str {
        "canned"
    }

    fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        let event = match call {
            Call::SignIn => PlatformEvent::SignedIn {
                request,
                player: Player {
                    id: "p1".into(),
                    alias: "Ada".into(),
                },
            },
            Call::CloudRead { key } => PlatformEvent::Read {
                request,
                key: key.clone(),
                value: Some("saved".into()),
            },
            other => PlatformEvent::Done {
                request,
                call: other.name().to_string(),
            },
        };
        let _ = report.send(event);
    }
}

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin::default()).unwrap();
    app
}

fn app_with_store() -> App {
    let app = app();
    balaur_platform::set_backend(&app.engine, Box::new(Canned)).unwrap();
    app
}

fn field(map: &Value, key: &str) -> Option<Value> {
    match map {
        Value::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        _ => None,
    }
}

/// The events one tick delivered, as `kind` strings.
fn kinds(app: &App) -> Vec<String> {
    let snapshot = app.engine.resource::<PlatformSnapshot>();
    
    snapshot
        .borrow()
        .events
        .iter()
        .filter_map(|event| match field(event, "kind") {
            Some(Value::Str(kind)) => Some(kind),
            _ => None,
        })
        .collect()
}

fn set_clock(app: &App, tick: u64, settled: u64) {
    *app.engine.resource::<Clock>().borrow_mut() = Clock { tick, settled };
}

fn start(app: &App, id: u64, call: Call) {
    let state = app.engine.resource::<PlatformState>();
    let mut state = state.borrow_mut();
    state.start(&app.engine, id, call, None);
}

#[test]
fn with_no_store_a_call_answers_unsupported_rather_than_failing() {
    let mut app = app();
    start(
        &app,
        1,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
    );
    app.tick(1.0 / 60.0);
    assert_eq!(kinds(&app), ["unsupported"]);
    let state = app.engine.resource::<PlatformState>();
    assert_eq!(state.borrow().backend_name(), "none");
}

#[test]
fn a_sign_in_lands_a_player_the_module_can_read_back() {
    let mut app = app_with_store();
    start(&app, 1, Call::SignIn);
    app.tick(1.0 / 60.0);
    assert_eq!(kinds(&app), ["signed_in"]);
    let state = app.engine.resource::<PlatformState>();
    let state = state.borrow();
    assert_eq!(state.player().map(|p| p.alias.as_str()), Some("Ada"));
    assert_eq!(state.backend_name(), "canned");
}

#[test]
fn an_unlock_reaches_the_store_and_comes_back_done() {
    let mut app = app_with_store();
    start(
        &app,
        7,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
    );
    app.tick(1.0 / 60.0);
    let snapshot = app.engine.resource::<PlatformSnapshot>();
    let event = snapshot.borrow().events.first().cloned().expect("an event");
    assert_eq!(field(&event, "kind"), Some(Value::Str("done".into())));
    assert_eq!(field(&event, "call"), Some(Value::Str("unlock".into())));
    assert_eq!(field(&event, "request"), Some(Value::Int(7)));
}

/// The rule the whole deferral exists for: an achievement awarded on a tick a
/// late input could still take back must not reach the store until that tick
/// is final. A rollback cannot un-award one.
#[test]
fn an_unlock_from_a_tick_that_can_still_roll_back_waits_for_it_to_settle() {
    let mut app = app_with_store();
    set_clock(&app, 5, 3);
    start(
        &app,
        1,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
    );
    set_clock(&app, 6, 3);
    app.tick(1.0 / 60.0);
    assert!(kinds(&app).is_empty(), "tick 5 can still be re-simulated");

    set_clock(&app, 9, 8);
    app.tick(1.0 / 60.0);
    assert_eq!(kinds(&app), ["done"], "nothing can take tick 5 back now");
}

/// A read costs nothing to repeat, so it never waits.
#[test]
fn a_read_from_a_tick_that_can_still_roll_back_goes_out_at_once() {
    let mut app = app_with_store();
    set_clock(&app, 5, 3);
    start(&app, 1, Call::CloudRead { key: "save".into() });
    app.tick(1.0 / 60.0);
    assert_eq!(kinds(&app), ["read"]);
}

/// A tick being re-run queues its writes again, so the run being replaced
/// must not leave one behind as well.
#[test]
fn a_re_simulated_tick_does_not_award_an_achievement_twice() {
    let mut app = app_with_store();
    set_clock(&app, 5, 3);
    start(
        &app,
        1,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
    );
    // Tick 5 runs again: its pump drops what the run being replaced left,
    // and its script unlocks again under a new id.
    set_clock(&app, 5, 3);
    app.tick(1.0 / 60.0);
    start(
        &app,
        2,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
    );
    set_clock(&app, 9, 8);
    app.tick(1.0 / 60.0);
    assert_eq!(
        kinds(&app),
        ["done"],
        "only the surviving run's unlock reaches the store"
    );
}
