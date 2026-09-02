//! The replay file and the source registry it is built from.

use balaur_core::replay::{self, Frame, Header, Recorder, Session};
use balaur_core::{App, AppConfig, Engine};

fn app() -> App {
    App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap()
}

/// A stand-in for a subsystem that receives something from outside.
struct Dial(pub i64);

fn app_with_dial() -> App {
    let mut app = app();
    app.engine.insert_resource(Dial(0));
    app.add_replay_source(
        "dial",
        |eng: &Engine| serde_json::json!(eng.resource::<Dial>().borrow().0),
        |eng: &Engine, v| {
            if let Some(n) = v.as_i64() {
                eng.resource::<Dial>().borrow_mut().0 = n;
            }
        },
    );
    app
}

fn header() -> Header {
    Header {
        format: replay::FORMAT,
        project: String::from("."),
        seed: 7,
    }
}

#[test]
fn a_registered_source_is_captured_and_fed_back() {
    let app = app_with_dial();
    app.engine.resource::<Dial>().borrow_mut().0 = 42;
    let captured = replay::capture(&app.engine);
    assert_eq!(
        captured.get("dial").and_then(serde_json::Value::as_i64),
        Some(42)
    );

    app.engine.resource::<Dial>().borrow_mut().0 = 0;
    replay::restore(&app.engine, &captured);
    assert_eq!(app.engine.resource::<Dial>().borrow().0, 42);
}

/// An older recording is still playable: what it does not mention is left
/// as it is rather than reset.
#[test]
fn a_source_missing_from_the_recording_is_left_alone() {
    let app = app_with_dial();
    app.engine.resource::<Dial>().borrow_mut().0 = 9;
    replay::restore(&app.engine, &serde_json::Map::new());
    assert_eq!(app.engine.resource::<Dial>().borrow().0, 9);
}

#[test]
fn a_recording_round_trips_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut recorder = Recorder::create(&path, &header()).unwrap();
    for tick in 1..=3u64 {
        recorder
            .write(&Frame {
                tick,
                dt: (1.0f32 / 60.0).to_bits(),
                sources: serde_json::Map::from_iter([(
                    String::from("dial"),
                    serde_json::json!(tick),
                )]),
                digest: tick * 1000,
            })
            .unwrap();
    }
    drop(recorder);

    let session = Session::read(&path).unwrap();
    assert_eq!(session.header.seed, 7);
    assert_eq!(session.frames.len(), 3);
    assert_eq!(session.frames[2].tick, 3);
    assert_eq!(session.frames[2].digest, 3000);
    assert_eq!(session.frames[0].step(), 1.0 / 60.0);
}

#[test]
fn a_recording_from_a_future_format_is_refused_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");
    let mut future = header();
    future.format = replay::FORMAT + 1;
    Recorder::create(&path, &future).unwrap();

    let err = Session::read(&path).unwrap_err().to_string();
    assert!(
        err.contains("format"),
        "the error has to say what is wrong, got {err}"
    );
}

#[test]
fn an_empty_file_is_refused_rather_than_replayed_as_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");
    std::fs::write(&path, "").unwrap();
    assert!(Session::read(&path).is_err());
}
