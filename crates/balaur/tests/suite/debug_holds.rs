//! What a debugger stop holds: the subtree it was told to, and the file the
//! call is stopped in. An editor around the game keeps running.

use balaur::{App, AppConfig, DEFAULT_FIXED_DT, standard_app};
use balaur_script::{StepMode, Value};

const SHELL: &str = "pub class Shell { node: Node, pub frames: int }
impl Shell {
    fn update(dt: float) {
        self.frames = self.frames + 1;
    }
}
";

const GAME: &str = "pub class Game { node: Node, pub frames: int, pub reloads: int }
impl Game {
    fn update(dt: float) {
        self.frames = self.frames + 1;
    }
    fn on_hot_reload() {
        self.reloads = self.reloads + 1;
    }
}
";

const SCENE: &str = "[[nodes]]
id = \"shell\"
name = \"Shell\"
script = { source = \"scripts/shell.bs\" }

[[nodes]]
id = \"game\"
name = \"Game\"
parent = \"shell\"
script = { source = \"scripts/game.bs\" }
";

fn booted() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), SCENE).unwrap();
    std::fs::write(dir.path().join("scripts/shell.bs"), SHELL).unwrap();
    std::fs::write(dir.path().join("scripts/game.bs"), GAME).unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn node(app: &App, id: &str) -> balaur::hecs::Entity {
    let world = app.engine.world();
    balaur_core::ids::find(&world, app.engine.root(), id).unwrap()
}

fn field(app: &App, id: &str, name: &str) -> i64 {
    let host = app.engine.script_host().unwrap();
    match host.script_field(balaur::node_id_of(node(app, id)), name) {
        Some(Value::Int(n)) => n,
        other => panic!("{id}.{name} is {other:?}"),
    }
}

#[test]
fn a_stop_holds_its_scope_and_the_editor_around_it_runs() {
    let (_dir, mut app) = booted();
    app.engine.set_debug_scope(Some(node(&app, "game")));
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/game.bs", &[4]).unwrap();
    app.tick(DEFAULT_FIXED_DT);
    assert!(host.paused().is_some(), "the control: it stopped");
    assert!(app.engine.is_frozen());
    let shell = field(&app, "shell", "frames");
    for _ in 0..3 {
        app.tick(DEFAULT_FIXED_DT);
    }
    assert_eq!(field(&app, "shell", "frames"), shell + 3, "the editor runs");

    host.set_breakpoints("scripts/game.bs", &[]).unwrap();
    host.resume(StepMode::Continue);
    assert!(!app.engine.is_frozen());
    assert_eq!(
        field(&app, "game", "frames"),
        1,
        "the game held: only the stopped call finished"
    );
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(field(&app, "game", "frames"), 2, "it goes on");
}

#[test]
fn a_rebuild_for_the_debugger_is_no_reload_to_the_script() {
    let (_dir, mut app) = booted();
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/game.bs", &[4]).unwrap();
    assert_eq!(field(&app, "game", "reloads"), 0, "breakpoints set");
    host.set_breakpoints("scripts/game.bs", &[]).unwrap();
    assert_eq!(field(&app, "game", "reloads"), 0, "breakpoints cleared");
    app.tick(DEFAULT_FIXED_DT);
    host.reload(&["scripts/game.bs"]).unwrap();
    assert_eq!(field(&app, "game", "reloads"), 1, "the control: a reload");
}

// A stop's call lives in its instance's memory, so a reload of any file of
// that instance waits until the call goes on, and its script hears it then.
#[test]
fn a_reload_waits_for_the_stop_it_would_end() {
    let (dir, mut app) = booted();
    app.engine.set_debug_scope(Some(node(&app, "game")));
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/game.bs", &[4]).unwrap();
    app.tick(DEFAULT_FIXED_DT);
    assert!(host.paused().is_some(), "the control: it stopped");

    let edited = GAME.replace("self.frames + 1", "self.frames + 100");
    std::fs::write(dir.path().join("scripts/game.bs"), edited).unwrap();
    host.reload(&["scripts/game.bs"]).unwrap();
    assert!(host.paused().is_some(), "the stop holds through the reload");

    host.set_breakpoints("scripts/game.bs", &[]).unwrap();
    host.resume(StepMode::Continue);
    app.tick(DEFAULT_FIXED_DT);
    assert!(host.paused().is_none());
    assert_eq!(field(&app, "game", "reloads"), 1, "it lands once it may");
    assert_eq!(field(&app, "game", "frames"), 101, "the new code runs");
}

// A game's script read from outside the project is a unit of the instance
// beside it: stopping it parks nothing of the editor's, and a debugger's
// probes rebuild only that instance.
#[test]
fn a_file_from_outside_the_project_stops_beside_it() {
    let (_dir, mut app) = booted();
    let game = tempfile::tempdir().unwrap();
    let root = game.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    let outside = root.join("scripts/visitor.bs");
    std::fs::write(
        &outside,
        GAME.replace("Game", "Visitor")
            .replace("frames + 1", "frames + 2"),
    )
    .unwrap();
    balaur::file_api::add_root(&app.engine, &root);
    let path = outside.to_string_lossy().into_owned();
    let host = app.engine.script_host().unwrap();
    let visitor = node(&app, "game");
    app.engine.set_debug_scope(Some(visitor));
    host.attach_with_props(balaur::node_id_of(visitor), &path, &[])
        .unwrap();
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(field(&app, "game", "frames"), 2, "it runs");

    let shell = field(&app, "shell", "frames");
    assert_eq!(host.set_breakpoints(&path, &[4]).unwrap(), [4]);
    assert_eq!(field(&app, "game", "frames"), 2, "carried across");
    app.tick(DEFAULT_FIXED_DT);
    let stop = host.paused().expect("it stopped in its own instance");
    // A stop names its file by the host's key, forward slashes on Windows too.
    let key = path.replace('\\', "/");
    assert_eq!((stop.path.as_str(), stop.line), (key.as_str(), 4));
    for _ in 0..2 {
        app.tick(DEFAULT_FIXED_DT);
    }
    assert_eq!(
        field(&app, "shell", "frames"),
        shell + 3,
        "the shell runs on"
    );

    host.set_breakpoints(&path, &[]).unwrap();
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(field(&app, "game", "frames"), 4);
}

// A stop is said with where it is, and the script going on is said once,
// with nil, under a name of its own.
#[test]
fn a_stop_and_the_resume_after_it_are_two_events() {
    let (_dir, mut app) = booted();
    app.engine.set_debug_scope(Some(node(&app, "game")));
    let ear = node(&app, "shell");
    for name in [
        balaur_core::events::DEBUGGER_STOPPED,
        balaur_core::events::DEBUGGER_RESUMED,
    ] {
        balaur_core::events::subscribe(&app.engine, ear, name, None);
    }
    let heard = |app: &App, name: &str| balaur_core::events::delivered(&app.engine, name);
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/game.bs", &[4]).unwrap();
    app.tick(DEFAULT_FIXED_DT);
    assert!(host.paused().is_some(), "the control: it stopped");
    app.tick(DEFAULT_FIXED_DT);
    let stopped = heard(&app, balaur_core::events::DEBUGGER_STOPPED);
    assert!(
        matches!(stopped.as_slice(), [Value::Map(_)]),
        "where it stopped: {stopped:?}"
    );
    assert!(heard(&app, balaur_core::events::DEBUGGER_RESUMED).is_empty());

    host.set_breakpoints("scripts/game.bs", &[]).unwrap();
    host.resume(StepMode::Continue);
    balaur_core::events::resumed(&app.engine);
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(
        heard(&app, balaur_core::events::DEBUGGER_RESUMED),
        [Value::Nil]
    );
    assert!(heard(&app, balaur_core::events::DEBUGGER_STOPPED).is_empty());
    app.tick(DEFAULT_FIXED_DT);
    assert!(
        heard(&app, balaur_core::events::DEBUGGER_RESUMED).is_empty(),
        "said once"
    );
}
