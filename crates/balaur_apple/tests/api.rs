//! What the Apple plugin can be held to without a device: that it loads, that
//! it takes over the portable verbs, and that it says so where Game Center
//! has nothing to offer.
//!
//! A real sign-in, unlock or leaderboard needs an Apple ID, a provisioning
//! profile and hardware, which is docs/PLAN-apple.md step 1.

use balaur_apple::ApplePlugin;
use balaur_core::{App, AppConfig};
use balaur_platform::{Call, PlatformPlugin, PlatformSnapshot, PlatformState};
use balaur_script::Value;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
    balaur_plugin::load(&mut app, &mut ApplePlugin).unwrap();
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
fn the_plugin_takes_over_the_portable_verbs_on_apple_alone() {
    let app = app();
    let state = app.engine.resource::<PlatformState>();
    let store = if cfg!(target_vendor = "apple") {
        "apple"
    } else {
        "none"
    };
    assert_eq!(state.borrow().backend_name(), store);
}

// Game Center has no presence, and answering `done` to a call that did
// nothing would be worse than saying so.
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
    );
    drop(state);
    app.tick(1.0 / 60.0);
    assert_eq!(kinds(&app), ["unsupported"]);
}

#[test]
fn availability_says_whether_the_frameworks_are_behind_this_build() {
    assert_eq!(balaur_apple::AVAILABLE, cfg!(target_vendor = "apple"));
}

fn apple_field(app: &App, key: &str) -> Option<Value> {
    let snapshot = app.engine.resource::<balaur_apple::AppleSnapshot>();
    let snapshot = snapshot.borrow();
    match snapshot.events.first() {
        Some(Value::Map(pairs)) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        _ => None,
    }
}

// A test signs in with Apple from its stand-in file: no account, no window.
#[test]
fn a_stand_in_answers_sign_in_with_apple_for_a_test() {
    let mut app = app();
    let stand_ins = balaur_platform::StandIns::parse(
        r#"{ "apple.sign_in": { "kind": "signed_in", "user": "leo", "code": "test-apple:leo" } }"#,
    )
    .unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    let state = app.engine.resource::<balaur_apple::AppleState>();
    state
        .borrow_mut()
        .start(&app.engine, 2, &balaur_apple::AppleCall::SignIn);
    app.tick(1.0 / 60.0);
    assert_eq!(
        apple_field(&app, "kind"),
        Some(Value::Str("signed_in".into()))
    );
    assert_eq!(
        apple_field(&app, "code"),
        Some(Value::Str("test-apple:leo".into()))
    );
    assert_eq!(apple_field(&app, "request"), Some(Value::Int(2)));
}

// With no window to show it over, a sign-in page says so rather than never
// answering; off Apple there is no sheet at all.
#[test]
fn a_sign_in_page_with_nothing_to_show_it_over_answers_at_once() {
    let mut app = app();
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().start(
        &app.engine,
        1,
        Call::WebSignIn {
            url: "https://provider.example/authorize".into(),
            scheme: None,
        },
    );
    app.tick(1.0 / 60.0);
    let expected = if balaur_apple::AVAILABLE {
        "error"
    } else {
        "unsupported"
    };
    assert_eq!(kinds(&app), [expected]);
}

// Purchases are `platform.*` on Apple too: StoreKit sits behind the same
// seam, and a binary with no bundle id is one the App Store does not know.
#[test]
fn a_purchase_goes_through_the_portable_verbs_and_says_so_without_a_bundle() {
    let mut app = app();
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().start(
        &app.engine,
        1,
        Call::Purchase {
            product: "gold".into(),
        },
    );
    state.borrow_mut().start(
        &app.engine,
        2,
        Call::Products {
            ids: vec!["gold".into()],
        },
    );
    drop(state);
    app.tick(1.0 / 60.0);
    assert_eq!(kinds(&app), ["unsupported", "unsupported"]);
}
