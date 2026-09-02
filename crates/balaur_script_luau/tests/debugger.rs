//! Breakpoints, stepping and frames on the Luau host: what a paused script
//! looks like, and what the rest of the engine does meanwhile.

use balaur_core::{App, AppConfig};
use balaur_script::{PauseReason, StepMode, Value};

fn make_app(dir: &std::path::Path) -> App {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap()
}

fn write_script(app: &App, rel: &str, source: &str) {
    let dir = app.engine.resource::<balaur_core::project::ProjectRoot>();
    std::fs::write(dir.borrow().0.join(rel), source).unwrap();
}

fn attach(app: &App, parent: hecs::Entity, name: &str, rel: &str) -> hecs::Entity {
    let entity = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, parent);
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur_core::node_id_of(entity), rel)
        .unwrap();
    entity
}

fn global_number(app: &App, name: &str) -> Option<f64> {
    balaur_script_luau::lua_of(&app.engine)
        .globals()
        .get(name)
        .ok()
}

fn number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Int(i)) => Some(*i as f64),
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    }
}

/// Line 6 reads the counter, line 7 writes it, line 8 reports the run.
const COUNTER: &str = "local S = {}\n\
function S:init()\n    self.n = 0\nend\n\
function S:update(dt)\n    local before = self.n\n    self.n = before + 1\n    _G.ran = (_G.ran or 0) + 1\nend\n\
return S\n";

/// Line 7 calls into `helper`, whose body is lines 3 and 4. A method, not a
/// local function: the O2 compiler inlines those, and an inlined call is
/// nothing to step into.
const CALLER: &str = "local S = {}\n\
function S:helper(x)\n    local y = x + 1\n    return y\nend\n\
function S:update(dt)\n    local v = self:helper(1)\n    _G.ran = v\nend\n\
return S\n";

#[test]
fn a_breakpoint_pauses_update_on_its_line_with_the_locals_in_scope() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", COUNTER);
    let node = attach(&app, app.engine.root(), "Holder", "scripts/s.luau");
    let host = app.engine.script_host().unwrap();
    assert_eq!(
        host.set_breakpoints("scripts/s.luau", &[7]).unwrap(),
        vec![7]
    );

    host.update(0.5);
    let pause = host.paused().expect("update should stop on line 7");
    assert_eq!(pause.path, "scripts/s.luau");
    assert_eq!(pause.line, 7);
    assert_eq!(pause.reason, PauseReason::Breakpoint);
    assert_eq!(pause.node, balaur_core::node_id_of(node));
    let frame = &pause.frames[0];
    assert_eq!(frame.function, "update");
    assert_eq!(frame.line, 7);
    let local = |name: &str| frame.locals.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    assert_eq!(number(local("dt")), Some(0.5));
    assert_eq!(number(local("before")), Some(0.0));
    assert!(
        matches!(local("self"), Some(Value::Map(_))),
        "self is the instance table"
    );
    assert_eq!(global_number(&app, "ran"), None, "line 8 has not run yet");
    assert_eq!(app.engine.frozen_root(), Some(app.engine.root()));

    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(app.engine.frozen_root(), None);
    assert_eq!(global_number(&app, "ran"), Some(1.0));
}

#[test]
fn the_rest_of_the_interrupted_tick_runs_once_on_resume() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", COUNTER);
    write_script(
        &app,
        "scripts/other.luau",
        "local S = {}\nfunction S:update(dt)\n    _G.other = (_G.other or 0) + 1\nend\nreturn S\n",
    );
    let a = attach(&app, app.engine.root(), "A", "scripts/s.luau");
    let b = attach(&app, app.engine.root(), "B", "scripts/s.luau");
    attach(&app, app.engine.root(), "Other", "scripts/other.luau");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.luau", &[7]).unwrap();

    // Each instance stops at the shared breakpoint in turn; the unbroken
    // script runs exactly once per tick whichever side of the pause it is on.
    host.update(0.1);
    let first = host.paused().expect("one of A and B stops").node;
    assert!(global_number(&app, "ran").unwrap_or(0.0) <= 1.0);
    host.resume(StepMode::Continue);
    let second = host
        .paused()
        .expect("the other stops when the tick goes on")
        .node;
    assert_ne!(first, second);
    assert!([a, b].iter().all(|e| {
        let id = balaur_core::node_id_of(*e);
        id == first || id == second
    }));
    assert_eq!(global_number(&app, "ran"), Some(1.0));
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(
        global_number(&app, "ran"),
        Some(2.0),
        "both ran exactly once"
    );
    assert_eq!(
        global_number(&app, "other"),
        Some(1.0),
        "and the third once, not twice"
    );

    host.update(0.1);
    host.resume(StepMode::Continue);
    host.resume(StepMode::Continue);
    assert_eq!(global_number(&app, "ran"), Some(4.0));
    assert_eq!(global_number(&app, "other"), Some(2.0));
}

#[test]
fn stepping_over_walks_the_function_line_by_line() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", COUNTER);
    attach(&app, app.engine.root(), "Holder", "scripts/s.luau");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.luau", &[6]).unwrap();

    host.update(0.1);
    assert_eq!(host.paused().map(|p| p.line), Some(6));
    host.resume(StepMode::Over);
    let pause = host.paused().expect("a step stops on the next line");
    assert_eq!((pause.line, pause.reason), (7, PauseReason::Step));
    host.resume(StepMode::Over);
    assert_eq!(host.paused().map(|p| p.line), Some(8));
    assert_eq!(global_number(&app, "ran"), None);
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(global_number(&app, "ran"), Some(1.0));
}

#[test]
fn stepping_into_enters_the_callee_and_out_returns_to_the_caller() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", CALLER);
    attach(&app, app.engine.root(), "Holder", "scripts/s.luau");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.luau", &[7]).unwrap();

    host.update(0.1);
    assert_eq!(host.paused().map(|p| p.line), Some(7));
    host.resume(StepMode::Into);
    let pause = host.paused().expect("into stops inside helper");
    assert_eq!(pause.line, 3);
    assert_eq!(pause.frames.len(), 2);
    assert_eq!(pause.frames[0].function, "helper");
    assert_eq!(
        number(
            pause.frames[0]
                .locals
                .iter()
                .find(|(n, _)| n == "x")
                .map(|(_, v)| v)
        ),
        Some(1.0)
    );
    assert_eq!(
        (pause.frames[1].function.as_str(), pause.frames[1].line),
        ("update", 7)
    );

    host.resume(StepMode::Out);
    let pause = host.paused().expect("out stops back in update");
    assert_eq!(pause.frames.len(), 1);
    assert_eq!(pause.frames[0].function, "update");
    assert!(
        (7..=8).contains(&pause.line),
        "back on the call line or the next: {}",
        pause.line
    );
    host.resume(StepMode::Continue);
    assert_eq!(global_number(&app, "ran"), Some(2.0));
}

#[test]
fn breakpoints_survive_a_hot_reload() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", COUNTER);
    attach(&app, app.engine.root(), "Holder", "scripts/s.luau");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.luau", &[7]).unwrap();
    host.update(0.1);
    host.resume(StepMode::Continue);

    write_script(&app, "scripts/s.luau", &format!("{COUNTER}-- edited\n"));
    host.reload("scripts/s.luau").unwrap();
    assert_eq!(host.breakpoints("scripts/s.luau"), vec![7]);
    host.update(0.1);
    assert_eq!(
        host.paused().map(|p| p.line),
        Some(7),
        "the new chunk is patched too"
    );
}

#[test]
fn reloading_the_paused_script_lifts_the_pause() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", COUNTER);
    attach(&app, app.engine.root(), "Holder", "scripts/s.luau");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.luau", &[7]).unwrap();
    host.update(0.1);
    assert!(host.paused().is_some());

    host.reload("scripts/s.luau").unwrap();
    assert!(host.paused().is_none());
    assert_eq!(app.engine.frozen_root(), None);
}

#[test]
fn detaching_the_paused_node_lifts_the_pause() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", COUNTER);
    let node = attach(&app, app.engine.root(), "Holder", "scripts/s.luau");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.luau", &[7]).unwrap();
    host.update(0.1);
    assert!(host.paused().is_some());

    host.detach(balaur_core::node_id_of(node));
    assert!(host.paused().is_none());
    assert_eq!(app.engine.frozen_root(), None);
    host.update(0.1);
    assert!(host.paused().is_none(), "nothing left to break");
}

#[test]
fn a_pause_holds_the_scope_and_nothing_outside_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", COUNTER);
    write_script(
        &app,
        "scripts/b.luau",
        "local S = {}\nfunction S:update(dt)\n    _G.b = (_G.b or 0) + 1\nend\nreturn S\n",
    );
    write_script(
        &app,
        "scripts/c.luau",
        "local S = {}\nfunction S:update(dt)\n    _G.c = (_G.c or 0) + 1\nend\nreturn S\n",
    );
    let root = app.engine.root();
    let game = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Game", root);
    attach(&app, game, "A", "scripts/s.luau");
    attach(&app, root, "B", "scripts/b.luau");
    attach(&app, game, "C", "scripts/c.luau");
    app.engine.set_debug_scope(Some(game));
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.luau", &[7]).unwrap();

    host.update(0.1);
    assert!(host.paused().is_some());
    assert_eq!(app.engine.frozen_root(), Some(game));
    let b = global_number(&app, "b").unwrap_or(0.0);
    let c = global_number(&app, "c");
    host.update(0.1);
    host.update(0.1);
    assert_eq!(
        global_number(&app, "b"),
        Some(b + 2.0),
        "outside the scope, B keeps ticking"
    );
    assert_eq!(global_number(&app, "c"), c, "inside it, C is held");
    assert_eq!(global_number(&app, "ran"), None, "and so is the paused A");

    host.resume(StepMode::Continue);
    assert_eq!(app.engine.frozen_root(), None);
    assert_eq!(global_number(&app, "ran"), Some(1.0));
}

#[test]
fn a_breakpoint_lands_on_the_next_line_with_code() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(
        &app,
        "scripts/s.luau",
        "local S = {}\n\nfunction S:update(dt)\n\n    _G.ran = 1\nend\nreturn S\n",
    );
    let host = app.engine.script_host().unwrap();
    assert_eq!(
        host.set_breakpoints("scripts/s.luau", &[4, 99]).unwrap(),
        vec![4, 99],
        "unloaded, the request is kept as asked"
    );
    attach(&app, app.engine.root(), "Holder", "scripts/s.luau");
    assert_eq!(
        host.breakpoints("scripts/s.luau"),
        vec![5],
        "loaded, the blank line moved down and the line past the end went"
    );
    host.update(0.1);
    assert_eq!(host.paused().map(|p| p.line), Some(5));
    host.resume(StepMode::Continue);
    assert_eq!(global_number(&app, "ran"), Some(1.0));
}

#[test]
fn clearing_the_breakpoints_lets_update_run_through() {
    let dir = tempfile::tempdir().unwrap();
    let app = make_app(dir.path());
    write_script(&app, "scripts/s.luau", COUNTER);
    attach(&app, app.engine.root(), "Holder", "scripts/s.luau");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("scripts/s.luau", &[7]).unwrap();
    host.update(0.1);
    host.resume(StepMode::Continue);

    assert_eq!(
        host.set_breakpoints("scripts/s.luau", &[]).unwrap(),
        Vec::<usize>::new()
    );
    host.update(0.1);
    assert!(host.paused().is_none());
    assert_eq!(global_number(&app, "ran"), Some(2.0));
}
