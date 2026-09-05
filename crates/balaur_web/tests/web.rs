//! The `web` plugin off the web, and fed from a recording.

use balaur::{standard_app, App, AppConfig};
use balaur_script::Value;
use balaur_web::{WebSnapshot, WebState};
use serde_json::json;

fn app() -> App {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"g\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "[[nodes]]\nname = \"Node\"\n").unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    // The directory may go: the project is loaded and nothing here saves.
    std::mem::forget(dir);
    app
}

#[test]
fn off_the_web_the_page_answers_nil_and_the_tab_counts_as_visible() {
    let app = app();
    let state = app.engine.resource::<WebState>();
    let state = state.borrow();
    assert!(state.visible());
    assert_eq!(state.facts().user_agent, None);
    assert_eq!(state.facts().location, None);
    assert_eq!(state.facts().hardware_concurrency, None);
}

#[test]
fn posting_off_the_web_reports_that_nothing_was_sent() {
    let app = app();
    assert!(!balaur_web::post_message(&app.engine, &Value::Map(Vec::new())).unwrap());
}

#[test]
fn a_recorded_tick_delivers_the_pages_reports_as_the_browser_did() {
    let mut app = app();
    let mut sources = serde_json::Map::new();
    sources.insert(
        "web".into(),
        json!({
            "io": [
                { "Message": { "payload": { "kind": "ready", "n": 2 } } },
                { "Visibility": { "visible": false } }
            ],
            "facts": { "user_agent": "Recorded/1.0", "location": "https://example.test/", "hardware_concurrency": 8 },
            "visible": true
        }),
    );
    balaur_core::replay::restore(&app.engine, &sources);
    app.tick(1.0 / 60.0);

    let snapshot = app.engine.resource::<WebSnapshot>();
    let messages = &snapshot.borrow().messages;
    assert_eq!(messages.len(), 1, "one message arrived this tick");
    let Value::Map(pairs) = &messages[0] else {
        panic!("a message is a map, got {:?}", messages[0]);
    };
    assert!(pairs.iter().any(|(k, v)| k == "n" && *v == Value::Int(2)));

    let state = app.engine.resource::<WebState>();
    let state = state.borrow();
    assert!(
        !state.visible(),
        "the recorded visibility change was applied"
    );
    assert_eq!(state.facts().user_agent.as_deref(), Some("Recorded/1.0"));
    assert_eq!(state.facts().hardware_concurrency, Some(8));
}
