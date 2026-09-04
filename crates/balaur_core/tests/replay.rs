//! The replay file and the source registry it is built from.

use balaur_core::replay::{
    self, Frame, Header, PlayState, Recorder, ReplayPlayer, Session, Trailer,
};
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
        ..Header::default()
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

    let mut recorder = Recorder::create(&path, header(), true, 0).unwrap();
    for tick in 1..=3u64 {
        recorder
            .write(&Frame {
                tick,
                dt: (1.0f32 / 60.0).to_bits(),
                sources: serde_json::Map::from_iter([(
                    String::from("dial"),
                    serde_json::json!(tick),
                )]),
                digest: Some(tick * 1000),
                ..Frame::default()
            })
            .unwrap();
    }
    recorder
        .finish(&Trailer {
            reason: String::from("stop"),
            tick: 3,
            digest: Some(3000),
        })
        .unwrap();

    let session = Session::read(&path).unwrap();
    assert_eq!(session.header.seed, 7);
    assert_eq!(session.frames.len(), 3);
    assert_eq!(session.frames[2].tick, 3);
    assert_eq!(session.frames[2].digest, Some(3000));
    assert!((session.frames[0].step() - 1.0 / 60.0).abs() < 1e-9);
    assert_eq!(session.trailer.map(|t| t.reason).as_deref(), Some("stop"));
}

#[test]
fn a_recording_from_a_future_format_is_refused_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");
    let mut future = header();
    future.format = replay::FORMAT + 1;
    let mut recorder = Recorder::create(&path, future, false, 0).unwrap();
    recorder.write(&Frame::default()).unwrap();
    drop(recorder);

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

/// Simulation state derived from the dial, so what the world becomes depends
/// on what it was fed and nothing else.
struct Total(pub i64);

/// The dial as the outside world moves it, and the sum a simulation makes of
/// it. The dial is a replay source, so it is fed rather than simulated: a
/// value that is both would be overwritten every tick by its own recording.
fn ratchet(app: &mut App) {
    app.engine.insert_resource(Total(0));
    app.add_system(balaur_core::Stage::First, |eng: &Engine, _| {
        if balaur_core::replay::is_playing(eng) {
            return;
        }
        let dial = eng.resource::<Dial>();
        let n = dial.borrow().0;
        dial.borrow_mut().0 = n + 1;
    });
    // FixedUpdate, because that is the stage a freeze holds: a paused replay
    // stops the simulation, not every system in the frame.
    app.add_system(balaur_core::Stage::FixedUpdate, |eng: &Engine, _| {
        let n = eng.resource::<Dial>().borrow().0;
        eng.resource::<Total>().borrow_mut().0 += n;
    });
}

/// The whole point of the session recorder: a recording made in a live
/// process replays in that same process, with the counters it started from.
#[test]
fn a_session_records_and_replays_in_one_process() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut app = app_with_dial();
    ratchet(&mut app);
    // Ticks and tokens the session did not start at, which is the case the
    // editor is: it has been running long before Play is pressed.
    for _ in 0..5 {
        app.advance(1.0 / 60.0);
    }
    let started_at = app.engine.tick();
    let token_at = app.engine.next_token();

    // A recording holds what the world was fed, not the world it started
    // from, so what a replay reproduces is the change over its own window.
    let total_before = app.engine.resource::<Total>().borrow().0;
    replay::start_recording(&app.engine, &path, ".", "hash", true).unwrap();
    for _ in 0..4 {
        app.advance(1.0 / 60.0);
    }
    let recorded_total = app.engine.resource::<Total>().borrow().0 - total_before;
    let live_token = app.engine.next_token();
    replay::stop_recording(&app.engine, "stop").unwrap();

    let session = Session::read(&path).unwrap();
    assert_eq!(session.frames.len(), 4);
    assert_eq!(session.header.scripts, "hash");
    assert_eq!(session.header.origin.tokens, token_at + 1);

    // A fresh app, as the editor's rebuilt mirror is: same code, same start.
    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::begin(&app.engine, session);
    assert_eq!(app.engine.tick(), started_at);
    assert_eq!(
        app.engine.next_token(),
        token_at + 1,
        "a replay hands out the ids the recording did"
    );
    app.engine.set_tokens(token_at + 1);

    replay::play(&app.engine);
    while replay::is_running(&app.engine) {
        app.advance(1.0 / 60.0);
    }
    assert_eq!(app.engine.tick(), started_at + 4);
    assert_eq!(app.engine.resource::<Total>().borrow().0, recorded_total);
    assert_eq!(app.engine.next_token(), live_token);
    assert!(
        app.engine
            .resource::<ReplayPlayer>()
            .borrow()
            .diverged
            .is_none(),
        "every recorded digest should have been reproduced"
    );
}

/// A replay that no longer reproduces the recording names the tick it parted
/// on rather than failing silently.
#[test]
fn a_diverging_replay_names_the_tick() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::start_recording(&app.engine, &path, ".", "", true).unwrap();
    for _ in 0..3 {
        app.advance(1.0 / 60.0);
    }
    replay::stop_recording(&app.engine, "stop").unwrap();

    // The same session against a world that counts by two: the code changed
    // under the recording, which is what a hot reload does.
    let mut app = app_with_dial();
    app.engine.insert_resource(Total(0));
    app.add_system(balaur_core::Stage::FixedUpdate, |eng: &Engine, _| {
        let n = eng.resource::<Dial>().borrow().0;
        eng.resource::<Total>().borrow_mut().0 += n * 2;
    });
    app.add_digest_source("total", |eng: &Engine, out| {
        let mut h = balaur_core::digest::Hasher::new();
        h.write_u64(eng.resource::<Total>().borrow().0 as u64);
        out.push(balaur_core::digest::Entry {
            label: String::from("value"),
            digest: h.finish(),
        });
    });
    replay::begin(&app.engine, Session::read(&path).unwrap());
    replay::play(&app.engine);
    while replay::is_running(&app.engine) {
        app.advance(1.0 / 60.0);
    }
    let player = app.engine.resource::<ReplayPlayer>();
    let diverged = player.borrow().diverged;
    assert!(diverged.is_some(), "a changed world has to be caught");
}

/// Pausing holds the simulation and stops feeding, so the world stands still
/// while the frame loop keeps running.
#[test]
fn a_paused_replay_holds_the_world_still() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::start_recording(&app.engine, &path, ".", "", false).unwrap();
    for _ in 0..6 {
        app.advance(1.0 / 60.0);
    }
    replay::stop_recording(&app.engine, "stop").unwrap();

    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::begin(&app.engine, Session::read(&path).unwrap());
    replay::play(&app.engine);
    app.advance(1.0 / 60.0);
    app.advance(1.0 / 60.0);
    let held = app.engine.resource::<Total>().borrow().0;

    app.engine.resource::<ReplayPlayer>().borrow_mut().state = PlayState::Paused;
    for _ in 0..5 {
        app.advance(1.0 / 60.0);
    }
    assert_eq!(
        app.engine.resource::<Total>().borrow().0,
        held,
        "a paused replay must not advance the world"
    );
    assert!(app.engine.frozen_root().is_some());
}

/// Seeking runs many recorded frames in one call, and stops on the tick asked
/// for rather than running to the end.
#[test]
fn seeking_runs_to_the_tick_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::start_recording(&app.engine, &path, ".", "", false).unwrap();
    for _ in 0..40 {
        app.advance(1.0 / 60.0);
    }
    replay::stop_recording(&app.engine, "stop").unwrap();

    let session = Session::read(&path).unwrap();
    let target = session.first_tick() + 20;
    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::begin(&app.engine, session);
    app.engine
        .resource::<ReplayPlayer>()
        .borrow_mut()
        .seek(target);
    app.advance(1.0 / 60.0);

    let player = app.engine.resource::<ReplayPlayer>();
    assert_eq!(player.borrow().position(), target);
    assert_eq!(player.borrow().state, PlayState::Paused);
    assert_eq!(app.engine.tick(), target);
}

/// The events a tick produced ride along with it, and are cleared afterwards
/// so the next tick starts empty.
#[test]
fn events_are_recorded_against_the_tick_that_made_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut app = app_with_dial();
    app.add_system(balaur_core::Stage::Update, |eng: &Engine, _| {
        if eng.tick().is_multiple_of(2) {
            replay::event(eng, "net.request", "GET /score", None);
        }
    });
    replay::start_recording(&app.engine, &path, ".", "", false).unwrap();
    for _ in 0..4 {
        app.advance(1.0 / 60.0);
    }
    replay::stop_recording(&app.engine, "stop").unwrap();

    let session = Session::read(&path).unwrap();
    let requests: Vec<_> = session
        .frames
        .iter()
        .filter(|f| f.events.iter().any(|e| e.kind == "net.request"))
        .map(|f| f.tick)
        .collect();
    assert_eq!(requests.len(), 2, "one event per even tick, and no repeats");
    assert!(requests.iter().all(|t| t % 2 == 0));
}

/// A recording that was never stopped still says so: no trailer means the
/// session did not get to finish.
#[test]
fn a_session_carries_why_it_ended() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");
    let mut app = app_with_dial();
    replay::start_recording(&app.engine, &path, ".", "", false).unwrap();
    app.advance(1.0 / 60.0);
    replay::stop_recording(&app.engine, "reload").unwrap();

    let session = Session::read(&path).unwrap();
    assert_eq!(
        session.trailer.as_ref().map(|t| t.reason.as_str()),
        Some("reload")
    );
}

/// A stand-in for a subsystem that sends and receives: the request takes an
/// await token, and the reply that comes back is keyed by it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Reply {
    token: u64,
    value: i64,
}

#[derive(Default)]
struct Wire {
    io: balaur_core::replay::ExternalIo<Reply>,
    asked: Vec<u64>,
    got: Vec<(u64, i64)>,
}

fn wired(app: &mut App) {
    app.engine.insert_resource(Wire::default());
    app.add_replay_source(
        "wire",
        |eng: &Engine| eng.resource::<Wire>().borrow().io.capture(),
        |eng: &Engine, value| eng.resource::<Wire>().borrow().io.restore(value),
    );
    app.add_system(balaur_core::Stage::First, |eng: &Engine, _| {
        let wire = eng.resource::<Wire>();
        let arrivals = wire.borrow_mut().io.drain();
        for reply in arrivals {
            wire.borrow_mut().got.push((reply.token, reply.value));
        }
    });
}

/// Ask for something. The worker answers at once here; a real one answers on
/// a later tick, which changes nothing about the ids.
fn ask(eng: &Engine) {
    let token = eng.next_token();
    let wire = eng.resource::<Wire>();
    wire.borrow_mut().asked.push(token);
    let value = i64::try_from(token).unwrap_or(0) * 10;
    wire.borrow().io.start(eng, |tx| {
        let _ = tx.send(Reply { token, value });
    });
}

/// The reason the header carries a token counter: a reply is keyed by the id
/// its request took, so a replay that hands out different ids delivers a
/// recorded reply to nothing at all.
#[test]
fn a_recorded_reply_reaches_the_request_that_asked_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut live = app();
    wired(&mut live);
    // Tokens taken before the session, as a long-lived editor has taken many.
    for _ in 0..7 {
        live.engine.next_token();
    }

    replay::start_recording(&live.engine, &path, ".", "", false).unwrap();
    ask(&live.engine);
    for _ in 0..3 {
        live.advance(1.0 / 60.0);
    }
    replay::stop_recording(&live.engine, "stop").unwrap();
    let (asked, got) = {
        let wire = live.engine.resource::<Wire>();
        let wire = wire.borrow();
        (wire.asked.clone(), wire.got.clone())
    };
    assert_eq!(got.len(), 1, "the reply arrived while recording");

    let mut again = app();
    wired(&mut again);
    replay::begin(&again.engine, Session::read(&path).unwrap());
    // Before the first frame, exactly where a script's `init` would ask.
    ask(&again.engine);
    replay::play(&again.engine);
    while replay::is_running(&again.engine) {
        again.advance(1.0 / 60.0);
    }

    let wire = again.engine.resource::<Wire>();
    let wire = wire.borrow();
    assert_eq!(wire.asked, asked, "a replay asks under the recorded ids");
    assert_eq!(
        wire.got, got,
        "and the recorded reply lands on the request that asked"
    );
}

/// The clock has to be the recording's, not the replaying process's. Every
/// frame a paused replay draws used to count a tick, so a script branching on
/// `engine::tick()` read a number no recorded frame ever ran at.
#[test]
fn frames_held_between_begin_and_play_do_not_move_the_replayed_tick() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::start_recording(&app.engine, &path, ".", "", true).unwrap();
    for _ in 0..4 {
        app.advance(1.0 / 60.0);
    }
    replay::stop_recording(&app.engine, "stop").unwrap();
    let recorded = Session::read(&path).unwrap();
    let first = recorded.first_tick();
    let last = recorded.last_tick();

    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::begin(&app.engine, recorded);
    // Five frames of a paused replay: the editor draws while the timeline
    // sits before the first recorded tick.
    for _ in 0..5 {
        app.advance(1.0 / 60.0);
    }
    assert_eq!(
        app.engine.tick(),
        first.saturating_sub(1),
        "a held frame is not a tick"
    );

    replay::play(&app.engine);
    app.advance(1.0 / 60.0);
    assert_eq!(app.engine.tick(), first, "the first frame runs at its tick");
    while replay::is_running(&app.engine) {
        app.advance(1.0 / 60.0);
    }
    assert_eq!(app.engine.tick(), last);
    assert!(
        app.engine
            .resource::<ReplayPlayer>()
            .borrow()
            .diverged
            .is_none(),
        "a replayed tick that ran at the wrong number is a divergence"
    );
}

/// A frame recorded while the debugger held the root took no fixed step; the
/// replay has to hold it the same way or it steps a tick nothing recorded.
#[test]
fn a_frame_recorded_while_the_debugger_froze_the_root_replays_frozen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.blr");

    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::start_recording(&app.engine, &path, ".", "", true).unwrap();
    app.advance(1.0 / 60.0);
    app.engine.set_frozen(true);
    for _ in 0..3 {
        app.advance(1.0 / 60.0);
    }
    app.engine.set_frozen(false);
    app.advance(1.0 / 60.0);
    let recorded_total = app.engine.resource::<Total>().borrow().0;
    replay::stop_recording(&app.engine, "stop").unwrap();

    let session = Session::read(&path).unwrap();
    assert!(
        session.frames.iter().filter(|f| f.frozen).count() == 3,
        "the pause has to be in the file, or the replay cannot reproduce it"
    );

    let mut app = app_with_dial();
    ratchet(&mut app);
    replay::begin(&app.engine, session);
    replay::play(&app.engine);
    while replay::is_running(&app.engine) {
        app.advance(1.0 / 60.0);
    }
    assert_eq!(app.engine.resource::<Total>().borrow().0, recorded_total);
    assert!(
        app.engine
            .resource::<ReplayPlayer>()
            .borrow()
            .diverged
            .is_none(),
        "a --verify session with one breakpoint hit must not part from itself"
    );
    assert!(!app.engine.is_frozen(), "and the freeze did not leak out");
}
