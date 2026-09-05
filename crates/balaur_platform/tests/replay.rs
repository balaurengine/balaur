//! Replaying a store's answers: the file answers, not the store.
//!
//! The point of this is the *absence* of a backend during playback. A
//! recorded session that quietly asked the store again would pass a digest
//! check and still be wrong — it would be awarding achievements twice.

use std::sync::mpsc::Sender;

use balaur_core::replay::{self, ReplayFeed, ReplayMode};
use balaur_core::{App, AppConfig};
use balaur_platform::{
    Call, PlatformBackend, PlatformEvent, PlatformPlugin, PlatformSnapshot, PlatformState,
};
use balaur_script::Value;

struct Canned;

impl PlatformBackend for Canned {
    fn name(&self) -> &'static str {
        "canned"
    }

    fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        let _ = report.send(PlatformEvent::Done {
            request,
            call: call.name().to_string(),
        });
    }
}

fn app() -> App {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin::default()).unwrap();
    app
}

fn unlock(app: &App) {
    let state = app.engine.resource::<PlatformState>();
    state.borrow_mut().start(
        &app.engine,
        1,
        Call::Unlock {
            achievement: "first_blood".into(),
        },
        None,
    );
}

fn first_kind(app: &App) -> Option<String> {
    let snapshot = app.engine.resource::<PlatformSnapshot>();
    let kind = snapshot
        .borrow()
        .events
        .first()
        .and_then(|event| match event {
            Value::Map(pairs) => pairs
                .iter()
                .find(|(k, _)| k == "kind")
                .map(|(_, v)| v.clone()),
            _ => None,
        });
    match kind {
        Some(Value::Str(kind)) => Some(kind),
        _ => None,
    }
}

#[test]
fn a_recorded_unlock_replays_with_no_store_behind_it() {
    let mut recorded = app();
    balaur_platform::set_backend(&recorded.engine, Box::new(Canned)).unwrap();
    unlock(&recorded);
    let mut frames = Vec::new();
    for _ in 0..10 {
        recorded.tick(1.0 / 60.0);
        frames.push(replay::capture(&recorded.engine));
        if first_kind(&recorded).is_some() {
            break;
        }
    }
    assert_eq!(first_kind(&recorded).as_deref(), Some("done"));

    // No backend at all this time: a call that reached the seam would answer
    // `unsupported`, and suppression means it never reaches it.
    let mut played = app();
    *played.engine.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    unlock(&played);
    let feed = played.engine.resource::<ReplayFeed>();
    let mut seen = None;
    for frame in &frames {
        feed.borrow_mut().0 = Some(frame.clone());
        played.tick(1.0 / 60.0);
        if let Some(kind) = first_kind(&played) {
            seen = Some(kind);
        }
    }
    assert_eq!(
        seen.as_deref(),
        Some("done"),
        "the recorded answer has to arrive from the file"
    );
}
