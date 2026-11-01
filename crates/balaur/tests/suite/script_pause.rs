//! What a pause and a `process` mode do to a script: the passes stop or run by
//! the mode the node resolves to, and every script still hears of the pause.

use balaur::hecs::Entity;
use balaur::{App, AppConfig, DEFAULT_FIXED_DT, standard_app};
use balaur_script::Value;

const COUNTER: &str = "pub class Counter {
    node: Node,
    pub updates: int,
    pub fixed: int,
    pub told: int,
    pub woke: int,
    pub token: int,
}
impl Counter {
    fn update(dt: float) {
        self.updates = self.updates + 1;
    }
    fn fixed_update(dt: float) {
        self.fixed = self.fixed + 1;
    }
    fn on_paused_changed(paused: bool) {
        self.told = self.told + 1;
    }
    async fn waits_on(token: int) {
        await task::wait(token);
        self.woke = self.woke + 1;
    }
}
";

/// One script on five nodes in a row, so the pass over them batches, and one
/// child under `always`, which takes its parent's mode.
const SCENE: &str = r#"
[[nodes]]
id = "root"
name = "Root"

[[nodes]]
id = "pausable"
name = "Pausable"
parent = "root"
script = { source = "scripts/counter.bs" }

[[nodes]]
id = "always"
name = "Always"
parent = "root"
process = "always"
script = { source = "scripts/counter.bs" }

[[nodes]]
id = "child"
name = "Child"
parent = "always"
script = { source = "scripts/counter.bs" }

[[nodes]]
id = "when_paused"
name = "WhenPaused"
parent = "root"
process = "when_paused"
script = { source = "scripts/counter.bs" }

[[nodes]]
id = "disabled"
name = "Disabled"
parent = "root"
process = "disabled"
script = { source = "scripts/counter.bs" }
"#;

/// Two nodes and no mode anywhere: the pass that asks nothing of a node.
const NO_MODES: &str = r#"
[[nodes]]
id = "root"
name = "Root"

[[nodes]]
id = "a"
name = "A"
parent = "root"
script = { source = "scripts/counter.bs" }

[[nodes]]
id = "b"
name = "B"
parent = "root"
script = { source = "scripts/counter.bs" }
"#;

fn booted(scene_text: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene_text).unwrap();
    std::fs::write(dir.path().join("scripts/counter.bs"), COUNTER).unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn node(app: &App, name: &str) -> Entity {
    let world = app.engine.world();
    balaur_core::ids::find(&world, app.engine.root(), name).expect("the scene names it")
}

fn field(app: &App, name: &str, field: &str) -> i64 {
    let host = app.engine.script_host().unwrap();
    match host.script_field(balaur::node_id_of(node(app, name)), field) {
        Some(Value::Int(n)) => n,
        other => panic!("{name}.{field} is {other:?}"),
    }
}

/// How many `update` and `fixed_update` each named node ran over `ticks` ticks.
fn ran(app: &mut App, ticks: usize, names: &[&str]) -> Vec<(i64, i64)> {
    let count = |app: &App| -> Vec<(i64, i64)> {
        let row = |name: &&str| (field(app, name, "updates"), field(app, name, "fixed"));
        names.iter().map(row).collect()
    };
    let before = count(app);
    for _ in 0..ticks {
        app.tick(DEFAULT_FIXED_DT);
    }
    let after = count(app);
    let moved = |(was, now): (&(i64, i64), &(i64, i64))| (now.0 - was.0, now.1 - was.1);
    before.iter().zip(&after).map(moved).collect()
}

const ALL: [&str; 5] = ["pausable", "always", "child", "when_paused", "disabled"];

#[test]
fn a_running_game_runs_pausable_and_always_scripts_only() {
    let (_dir, mut app) = booted(SCENE);
    let moved = ran(&mut app, 3, &ALL);
    assert_eq!(moved[0], (3, 3), "pausable");
    assert_eq!(moved[1], (3, 3), "always");
    assert_eq!(moved[2], (3, 3), "a child takes its parent's mode");
    assert_eq!(moved[3], (0, 0), "when_paused waits for a pause");
    assert_eq!(moved[4], (0, 0), "disabled never runs");
}

#[test]
fn a_paused_game_runs_always_and_when_paused_scripts_only() {
    let (_dir, mut app) = booted(SCENE);
    app.engine.set_paused(true);
    app.tick(DEFAULT_FIXED_DT);
    let moved = ran(&mut app, 3, &ALL);
    assert_eq!(moved[0], (0, 0), "pausable holds");
    assert_eq!(moved[1], (3, 3), "always runs");
    assert_eq!(moved[2], (3, 3), "a child takes its parent's mode");
    assert_eq!(moved[3], (3, 3), "when_paused runs");
    assert_eq!(moved[4], (0, 0), "disabled never runs");

    app.engine.set_paused(false);
    app.tick(DEFAULT_FIXED_DT);
    let moved = ran(&mut app, 3, &ALL);
    assert_eq!(moved[0], (3, 3), "pausable runs again");
    assert_eq!(moved[3], (0, 0), "when_paused holds again");
}

#[test]
fn a_game_with_no_modes_holds_every_script_and_lets_them_go() {
    let (_dir, mut app) = booted(NO_MODES);
    assert_eq!(ran(&mut app, 2, &["a", "b"]), [(2, 2), (2, 2)]);
    app.engine.set_paused(true);
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(ran(&mut app, 2, &["a", "b"]), [(0, 0), (0, 0)]);
    app.engine.set_paused(false);
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(ran(&mut app, 2, &["a", "b"]), [(2, 2), (2, 2)]);
}

#[test]
fn every_script_hears_of_the_pause_the_held_ones_too() {
    let (_dir, mut app) = booted(SCENE);
    app.engine.set_paused(true);
    app.tick(DEFAULT_FIXED_DT);
    for name in ALL {
        assert_eq!(field(&app, name, "told"), 1, "{name}");
    }
    app.engine.set_paused(false);
    app.tick(DEFAULT_FIXED_DT);
    for name in ALL {
        assert_eq!(field(&app, name, "told"), 2, "{name}");
    }
}

#[test]
fn a_task_on_a_held_node_waits_for_the_pause_to_end() {
    let (_dir, mut app) = booted(SCENE);
    let host = app.engine.script_host().unwrap();
    let (held, always) = (node(&app, "pausable"), node(&app, "always"));
    for (at, token) in [(held, 7_001), (always, 7_002)] {
        let token = [Value::Int(token)];
        assert_eq!(
            host.call_on(balaur::node_id_of(at), "waits_on", &token),
            None
        );
    }
    app.engine.set_paused(true);
    app.tick(DEFAULT_FIXED_DT);
    host.wake(7_001, &Value::Nil);
    host.wake(7_002, &Value::Nil);
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(
        field(&app, "always", "woke"),
        1,
        "the control: always goes on"
    );
    assert_eq!(field(&app, "pausable", "woke"), 0, "held with its node");
    app.engine.set_paused(false);
    app.tick(DEFAULT_FIXED_DT);
    assert_eq!(
        field(&app, "pausable", "woke"),
        1,
        "it goes on from the await"
    );
}

#[test]
fn a_call_still_reaches_a_held_script() {
    let (_dir, mut app) = booted(SCENE);
    app.engine.set_paused(true);
    app.tick(DEFAULT_FIXED_DT);
    let host = app.engine.script_host().unwrap();
    let id = balaur::node_id_of(node(&app, "pausable"));
    let before = field(&app, "pausable", "updates");
    host.call_on(id, "update", &[Value::Num(0.0)]);
    assert_eq!(
        field(&app, "pausable", "updates"),
        before + 1,
        "a call is not a tick"
    );
}
