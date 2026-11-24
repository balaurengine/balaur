//! What `google.sign_in` can be held to off a phone: it answers
//! `unsupported`, saying why, and a test's stand-in answers in its place.
//! Credential Manager itself needs an Android 14 device signed in to Google.

use balaur_core::{App, AppConfig};
use balaur_google::{GooglePlugin, GoogleSnapshot, GoogleState, SERVER_CLIENT_ID};
use balaur_platform::{PlatformPlugin, StandIns};
use balaur_script::Value;

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
    balaur_plugin::load(&mut app, &mut GooglePlugin).unwrap();
    app
}

/// Sign in under `id`, and what answered on the next tick.
fn sign_in(app: &mut App, id: u64) -> Vec<Value> {
    let state = app.engine.resource::<GoogleState>();
    state.borrow().sign_in(&app.engine, id);
    app.tick(1.0 / 60.0);
    app.engine
        .resource::<GoogleSnapshot>()
        .borrow()
        .events
        .clone()
}

fn field(map: &Value, key: &str) -> Option<Value> {
    match map {
        Value::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        _ => None,
    }
}

fn text(value: &str) -> Value {
    Value::Str(value.into())
}

fn error_of(event: &Value) -> String {
    match field(event, "error") {
        Some(Value::Str(message)) => message,
        other => panic!("no error in {other:?}"),
    }
}

#[test]
fn with_no_client_id_sign_in_answers_unsupported_naming_the_setting() {
    let mut app = app();
    let events = sign_in(&mut app, 3);
    assert_eq!(field(&events[0], "kind"), Some(text("unsupported")));
    assert_eq!(field(&events[0], "request"), Some(Value::Int(3)));
    assert!(error_of(&events[0]).contains("server_client_id"));
}

#[test]
fn off_android_sign_in_answers_unsupported_even_with_a_client_id() {
    let mut app = app();
    balaur_core::settings::set(
        &app.engine,
        SERVER_CLIENT_ID,
        toml::Value::String("123-web.apps.googleusercontent.com".into()),
    );
    assert_eq!(
        balaur_google::server_client_id(&app.engine),
        "123-web.apps.googleusercontent.com"
    );
    let events = sign_in(&mut app, 1);
    assert_eq!(field(&events[0], "kind"), Some(text("unsupported")));
    assert!(error_of(&events[0]).contains("not Android"));
}

#[test]
fn a_stand_in_answers_in_place_of_credential_manager() {
    let mut app = app();
    let stand_ins = StandIns::parse(
        r#"{ "google.sign_in": { "kind": "signed_in", "token": "test-google:leo", "email": "leo@google.example", "name": "" } }"#,
    )
    .unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    let events = sign_in(&mut app, 7);
    assert_eq!(field(&events[0], "kind"), Some(text("signed_in")));
    assert_eq!(field(&events[0], "token"), Some(text("test-google:leo")));
    assert_eq!(field(&events[0], "request"), Some(Value::Int(7)));
}

#[test]
fn a_null_stand_in_is_a_sheet_left_open_that_never_answers() {
    let mut app = app();
    let stand_ins = StandIns::parse(r#"{ "google.sign_in": null }"#).unwrap();
    balaur_platform::set_stand_ins(&app.engine, stand_ins).unwrap();
    assert!(sign_in(&mut app, 2).is_empty());
}
