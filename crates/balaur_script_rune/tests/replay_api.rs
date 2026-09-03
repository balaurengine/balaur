//! The `replay` script module, driven from script the way the editor's
//! Session dock drives it.
//!
//! The editor is written in Rune, so these bindings are the whole surface it
//! has: a module that registers but answers wrongly is a dock that draws
//! nothing, and nothing in Rust would have caught it.

use balaur_core::{App, AppConfig};
use balaur_script::Value;

fn app_in(dir: &std::path::Path) -> App {
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap()
}

/// The recorder folds the engine's log into a session's events, and the log
/// buffer only exists once a subscriber is installed. Idempotent, so every
/// test may ask.
fn capture_logs() {
    balaur_core::logbuf::capture(tracing::level_filters::LevelFilter::INFO);
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "name = \"t\"\n").unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

/// The editor's arrangement, which every test needs: the game is a subtree
/// and the script driving playback sits outside it. A held replay stops the
/// scope, and a driver inside it would stop answering exactly when it is
/// needed.
fn scope_the_game(app: &App) {
    let root = app.engine.root();
    let game = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Game", root);
    app.engine.set_debug_scope(Some(game));
}

fn attach(app: &App, name: &str, rel: &str) -> hecs::Entity {
    let root = app.engine.root();
    let entity = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root);
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(entity), rel)
        .unwrap();
    entity
}

fn call(app: &App, node: hecs::Entity, method: &str) -> Option<Value> {
    app.engine
        .script_host()
        .unwrap()
        .call_on(balaur_core::node_id_of(node), method, &[])
}

fn text(v: Option<Value>) -> String {
    match v {
        Some(Value::Str(s)) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

fn int(v: Option<Value>) -> i64 {
    match v {
        Some(Value::Int(i)) => i,
        Some(Value::Num(n)) => n as i64,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// A script that records itself, the way `shell::toggle_play` does, then reads
/// the session back the way the dock does.
const RECORDER: &str = r#"
pub fn init(this) {
    this.file = "";
}

pub fn start(this) {
    this.file = replay::record("session.blr", #{ scripts: "fingerprint" });
    this.file
}

pub fn recording(this) {
    let open = replay::recording();
    if open is String { open } else { "" }
}

pub fn stop(this) {
    replay::stop("stop");
    ""
}

pub fn note(this) {
    log::info("something happened");
    ""
}

pub fn load(this) {
    let head = replay::load(this.file);
    head.started
}

pub fn reason(this) {
    let head = replay::header();
    head.reason
}

pub fn scripts(this) {
    replay::header().scripts
}

pub fn state(this) {
    replay::state()
}

pub fn frames(this) {
    replay::length().frames
}

pub fn play(this) {
    replay::play();
    ""
}

pub fn position(this) {
    replay::position()
}

pub fn events(this) {
    let span = replay::length();
    replay::events(span.first, span.last).len()
}

pub fn key_marks(this) {
    replay::marks("input", "just_pressed").len()
}

pub fn unload(this) {
    replay::unload();
    ""
}

pub fn name(this) {
    replay::session_name()
}
"#;

#[test]
fn a_script_records_a_session_and_reads_it_back() {
    capture_logs();
    let dir = project(&[("r.rn", RECORDER)]);
    let mut app = app_in(dir.path());
    scope_the_game(&app);
    let node = attach(&app, "Recorder", "r.rn");

    let file = text(call(&app, node, "start"));
    assert!(file.ends_with("session.blr"), "record returns its path");
    assert_eq!(
        text(call(&app, node, "recording")),
        file,
        "the open recording is the file that was asked for"
    );

    for _ in 0..3 {
        call(&app, node, "note");
        app.advance(1.0 / 60.0);
    }
    call(&app, node, "stop");
    assert_eq!(
        text(call(&app, node, "recording")),
        "",
        "nothing is recording once the session is closed"
    );

    let started = text(call(&app, node, "load"));
    assert!(
        started.starts_with("20"),
        "the header carries when it started, got {started:?}"
    );
    assert_eq!(text(call(&app, node, "scripts")), "fingerprint");
    assert_eq!(text(call(&app, node, "reason")), "stop");
    assert_eq!(int(call(&app, node, "frames")), 3);
    assert_eq!(text(call(&app, node, "state")), "paused");
    assert!(
        int(call(&app, node, "events")) >= 3,
        "the log lines each tick wrote are in the session"
    );
}

/// Playing feeds one recorded tick per frame, and the position follows.
#[test]
fn playing_walks_the_session_a_tick_at_a_time() {
    capture_logs();
    let dir = project(&[("r.rn", RECORDER)]);
    let mut app = app_in(dir.path());
    scope_the_game(&app);
    let node = attach(&app, "Recorder", "r.rn");

    call(&app, node, "start");
    for _ in 0..4 {
        app.advance(1.0 / 60.0);
    }
    call(&app, node, "stop");
    assert!(
        !text(call(&app, node, "load")).is_empty(),
        "the session has to load before it can play"
    );

    let start = int(call(&app, node, "position"));
    call(&app, node, "play");
    assert_eq!(text(call(&app, node, "state")), "playing");
    app.advance(1.0 / 60.0);
    assert_eq!(
        int(call(&app, node, "position")),
        start + 1,
        "one recorded tick per frame"
    );

    for _ in 0..8 {
        app.advance(1.0 / 60.0);
    }
    assert_eq!(
        text(call(&app, node, "state")),
        "paused",
        "a spent session parks rather than running the game on"
    );
    assert_eq!(int(call(&app, node, "position")), start + 4);
    assert!(
        app.engine.frozen_root().is_some(),
        "a parked session holds the game still"
    );

    call(&app, node, "unload");
    assert_eq!(text(call(&app, node, "state")), "stopped");
    assert!(app.engine.frozen_root().is_none(), "unloading lets it run");
}

/// The lanes the dock draws come from `marks`, which names a source and a
/// field rather than knowing any plugin's shape.
#[test]
fn marks_answer_for_a_source_the_engine_does_not_know() {
    let dir = project(&[("r.rn", RECORDER)]);
    let mut app = app_in(dir.path());
    // A stand-in for the input plugin: core has no such source of its own.
    #[derive(serde::Serialize, serde::Deserialize, Default)]
    struct Keys {
        just_pressed: Vec<String>,
    }
    app.engine.insert_resource(Keys::default());
    app.add_replay_resource::<Keys>("input");
    scope_the_game(&app);
    let node = attach(&app, "Recorder", "r.rn");

    call(&app, node, "start");
    for tick in 0..4 {
        app.engine.resource::<Keys>().borrow_mut().just_pressed = if tick % 2 == 0 {
            vec![String::from("Space")]
        } else {
            Vec::new()
        };
        app.advance(1.0 / 60.0);
    }
    call(&app, node, "stop");
    call(&app, node, "load");

    assert_eq!(
        int(call(&app, node, "key_marks")),
        2,
        "only the ticks that held a key are marks"
    );
}

/// The name a new session takes has to be usable as a file name everywhere,
/// which rules out the colons a readable timestamp has.
#[test]
fn a_session_name_is_a_file_name() {
    let dir = project(&[("r.rn", RECORDER)]);
    let app = app_in(dir.path());
    let node = attach(&app, "Recorder", "r.rn");

    let name = text(call(&app, node, "name"));
    assert!(!name.contains(':'), "Windows refuses a colon, got {name:?}");
    assert!(!name.contains(' '), "a space in a path is a mistake waiting");
    assert!(name.starts_with("20"), "it still reads as a date: {name:?}");
}

/// A script that moves its node every fixed step, so the world it leaves
/// depends on how many steps ran and on nothing else.
const MOVER: &str = r#"
pub fn init(this) {
    this.n = 0;
}

pub fn fixed_update(this, dt) {
    this.n = this.n + 1;
    this.node.set_position(this.n as f64 * 0.25, 0.0, 0.0);
}
"#;

const DRIVER: &str = r#"
pub fn init(this) {
    this.at = 0;
    this.step = 0;
}

// The editor loads and plays from inside a frame, not between two: what the
// rest of that frame does to the world is the thing under test.
pub fn update(this, dt) {
    this.at = this.at + 1;
    if this.step == 1 && this.at == 1 {
        this.step = 2;
        replay::load(this.file);
        replay::play();
    }
}

pub fn arm(this) {
    this.step = 1;
    this.at = 0;
    ""
}

pub fn start(this) {
    this.file = replay::record("session.blr", #{ digest: true });
    this.file
}

pub fn stop(this) {
    replay::stop("stop");
    ""
}

pub fn load(this) {
    replay::load(this.file);
    ""
}

pub fn play(this) {
    replay::play();
    ""
}

pub fn diverged(this) {
    let d = replay::diverged();
    if d is Object { format!("tick {}", d.tick) } else { "" }
}
"#;

/// What the editor does when it replays: the scene is torn down and rebuilt
/// in the same process, and the recording is fed into the new one. A replay
/// that only worked from a fresh process would be no use in an editor.
#[test]
fn a_replay_reproduces_the_recording_after_the_scene_is_rebuilt() {
    capture_logs();
    let dir = project(&[("m.rn", MOVER), ("d.rn", DRIVER)]);
    let mut app = app_in(dir.path());
    let driver = attach(&app, "Driver", "d.rn");

    let build = |app: &App| {
        let root = app.engine.root();
        let game = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Game", root);
        app.engine.set_debug_scope(Some(game));
        let mover = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Mover", game);
        app.engine
            .script_host()
            .unwrap()
            .attach(balaur_core::node_id_of(mover), "m.rn")
            .unwrap();
        game
    };

    let game = build(&app);
    call(&app, driver, "start");
    for _ in 0..5 {
        app.advance(1.0 / 60.0);
    }
    call(&app, driver, "stop");

    // Tear the scene down and build it again, as `build_mirror` does.
    app.engine.push_command(balaur_core::Command::Free(game));
    app.advance(1.0 / 60.0);
    build(&app);

    call(&app, driver, "arm");
    for _ in 0..14 {
        app.advance(1.0 / 60.0);
    }
    assert_eq!(
        text(call(&app, driver, "diverged")),
        "",
        "a rebuilt scene fed the same input has to reach the same world"
    );
}
