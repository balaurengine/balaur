//! Replaying input: a replayed tick reads what its recorded tick read, the
//! one-frame fields included.

use balaur_core::replay::{self, Session};
use balaur_core::{App, AppConfig};
use balaur_input::{InputPlugin, InputSnapshot};

/// What one tick read: `KeyZ` just pressed, just released, the scroll and the
/// typed text.
type Read = (bool, bool, (f32, f32), String);

fn app(dir: &std::path::Path) -> App {
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"replay test\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.toml"), "").unwrap();
    let mut app = App::new(AppConfig::bare(dir.to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut InputPlugin).unwrap();
    app.load_project().unwrap();
    app
}

fn read(app: &App) -> Read {
    let input = app.engine.resource::<InputSnapshot>();
    let input = input.borrow();
    (
        input.key_just_pressed("KeyZ"),
        input.key_just_released("KeyZ"),
        input.scroll_delta(),
        input.typed().to_string(),
    )
}

/// A window's pump: the frame begins, then what the OS said arrives.
fn pump(app: &App, feed: fn(&mut InputSnapshot)) {
    let input = app.engine.resource::<InputSnapshot>();
    let mut input = input.borrow_mut();
    input.begin_frame();
    feed(&mut input);
}

#[test]
fn a_replayed_tick_reads_the_edges_scroll_and_text_its_recorded_tick_read() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.blr");
    let mut recording = app(dir.path());
    recording.tick(1.0 / 60.0);
    replay::start_recording(&recording.engine, &session, "t", "", false).unwrap();
    let ticks: [fn(&mut InputSnapshot); 3] = [
        |input| {
            input.key_event("KeyZ", true);
            input.add_scroll(0.0, 3.0);
            input.char_event('z');
        },
        |_| {},
        |input| input.key_event("KeyZ", false),
    ];
    let mut recorded = Vec::new();
    for feed in ticks {
        pump(&recording, feed);
        recording.tick(1.0 / 60.0);
        recorded.push(read(&recording));
    }
    replay::stop_recording(&recording.engine, "test");
    assert_eq!(recorded[0], (true, false, (0.0, 3.0), String::from("z")));
    assert!(recorded[2].1, "the recording saw the release");

    // Headless, and under a window whose own events the replay must drop.
    for windowed in [false, true] {
        let mut replaying = app(dir.path());
        replaying.tick(1.0 / 60.0);
        replay::begin(&replaying.engine, Session::read(&session).unwrap());
        replay::play(&replaying.engine);
        let mut replayed = Vec::new();
        while replay::is_running(&replaying.engine) {
            if windowed {
                pump(&replaying, |input| {
                    input.key_event("KeyQ", true);
                    input.add_scroll(5.0, 0.0);
                });
            }
            replaying.advance(1.0 / 60.0);
            replayed.push(read(&replaying));
        }
        assert_eq!(replayed, recorded, "windowed: {windowed}");
    }
}
