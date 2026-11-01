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
    host.reload("scripts/game.bs").unwrap();
    assert_eq!(field(&app, "game", "reloads"), 1, "the control: a reload");
}
