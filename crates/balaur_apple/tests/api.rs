//! What the Apple plugin can be held to without a device: that it loads, that
//! it takes over the portable verbs, and that it says so where Game Center
//! has nothing to offer.
//!
//! A real sign-in, unlock or leaderboard needs an Apple ID, a provisioning
//! profile and hardware — docs/PLAN-apple.md §4 draws that line.

use balaur_apple::ApplePlugin;
use balaur_core::{App, AppConfig};
use balaur_platform::{Call, PlatformPlugin, PlatformSnapshot, PlatformState};
use balaur_script::Value;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin::default()).unwrap();
    balaur_plugin::load(&mut app, &mut ApplePlugin::default()).unwrap();
    app
}

fn kinds(app: &App) -> Vec<String> {
    let snapshot = app.engine.resource::<PlatformSnapshot>();
    
    snapshot
        .borrow()
        .events
        .iter()
        .filter_map(|event| match event {
            Value::Map(pairs) => {
                pairs
                    .iter()
                    .find(|(k, _)| k == "kind")
                    .and_then(|(_, v)| match v {
                        Value::Str(kind) => Some(kind.clone()),
                        _ => None,
                    })
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_plugin_takes_over_the_portable_verbs() {
    let app = app();
    let state = app.engine.resource::<PlatformState>();
    assert_eq!(state.borrow().backend_name(), "apple");
}

/// Game Center has no presence, and answering `done` to a call that did
/// nothing would be worse than saying so.
#[test]
fn presence_is_unsupported_rather_than_pretended() {
    let mut app = app();
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().start(
        &app.engine,
        1,
        Call::SetPresence {
            text: "in the caves".into(),
        },
        None,
    );
    drop(state);
    app.tick(1.0 / 60.0);
    assert_eq!(kinds(&app), ["unsupported"]);
}

#[test]
fn availability_says_whether_the_frameworks_are_behind_this_build() {
    assert_eq!(balaur_apple::AVAILABLE, cfg!(target_vendor = "apple"));
}
