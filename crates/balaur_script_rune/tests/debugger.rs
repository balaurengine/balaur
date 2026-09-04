//! Breakpoints, stepping and frames on the Rune host: what a paused script
//! looks like, and what the rest of the engine does meanwhile.

use balaur_core::{App, AppConfig};
use balaur_script::{PauseReason, StepMode, Value};
use balaur_script_rune::RuneHost;

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

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "name = \"t\"\n").unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
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

fn rune(app: &App) -> RuneHost {
    balaur_script_rune::rune_of(&app.engine)
}

fn field(app: &App, node: hecs::Entity, name: &str) -> Option<f64> {
    rune(app).number_field(node, name)
}

fn number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Int(i)) => Some(*i as f64),
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    }
}

/// Line 6 reads the counter, line 7 writes it, line 8 reports the run.
const COUNTER: &str = "pub fn init(this) {\n    this.n = 0;\n    this.ran = 0;\n}\n\
pub fn update(this, dt) {\n    let before = this.n;\n    this.n = before + 1;\n    this.ran = this.ran + 1;\n}\n";

/// Line 7 calls into `helper`, whose body is lines 2 and 3.
const CALLER: &str = "fn helper(x) {\n    let y = x + 1;\n    y\n}\n\
pub fn init(this) { this.ran = 0; }\n\
pub fn update(this, dt) {\n    let v = helper(1);\n    this.ran = v;\n}\n";

#[test]
fn a_breakpoint_pauses_update_on_its_line_with_the_arguments_as_locals() {
    let dir = project(&[("s.rn", COUNTER)]);
    let app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    assert_eq!(host.set_breakpoints("s.rn", &[7]).unwrap(), vec![7]);

    host.update(0.5);
    let pause = host.paused().expect("update should stop on line 7");
    assert_eq!(pause.path, "s.rn");
    assert_eq!(pause.line, 7);
    assert_eq!(pause.reason, PauseReason::Breakpoint);
    assert_eq!(pause.node, balaur_core::node_id_of(node));
    let frame = &pause.frames[0];
    assert_eq!(frame.function, "update");
    assert_eq!(frame.line, 7);
    let local = |name: &str| frame.locals.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    assert_eq!(number(local("dt")), Some(0.5));
    assert!(
        matches!(local("this"), Some(Value::Map(_))),
        "this is the instance"
    );
    assert_eq!(
        field(&app, node, "ran"),
        Some(0.0),
        "line 8 has not run yet"
    );
    assert_eq!(app.engine.frozen_root(), Some(app.engine.root()));

    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(app.engine.frozen_root(), None);
    assert_eq!(field(&app, node, "ran"), Some(1.0));
}

#[test]
fn the_rest_of_the_interrupted_tick_runs_once_on_resume() {
    let dir = project(&[
        ("s.rn", COUNTER),
        (
            "other.rn",
            "pub fn init(this) { this.other = 0; }\npub fn update(this, dt) { this.other = this.other + 1; }\n",
        ),
    ]);
    let app = app_in(dir.path());
    let a = attach(&app, app.engine.root(), "A", "s.rn");
    let b = attach(&app, app.engine.root(), "B", "s.rn");
    let other = attach(&app, app.engine.root(), "Other", "other.rn");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("s.rn", &[7]).unwrap();

    // Each instance stops at the shared breakpoint in turn; the unbroken
    // script runs exactly once per tick whichever side of the pause it is on.
    host.update(0.1);
    let first = host.paused().expect("one of A and B stops").node;
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
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(field(&app, a, "ran"), Some(1.0));
    assert_eq!(field(&app, b, "ran"), Some(1.0));
    assert_eq!(field(&app, other, "other"), Some(1.0), "once, not twice");
}

#[test]
fn stepping_over_walks_the_function_line_by_line() {
    let dir = project(&[("s.rn", COUNTER)]);
    let app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("s.rn", &[6]).unwrap();

    host.update(0.1);
    assert_eq!(host.paused().map(|p| p.line), Some(6));
    host.resume(StepMode::Over);
    let pause = host.paused().expect("a step stops on the next line");
    assert_eq!((pause.line, pause.reason), (7, PauseReason::Step));
    host.resume(StepMode::Over);
    assert_eq!(host.paused().map(|p| p.line), Some(8));
    assert_eq!(field(&app, node, "ran"), Some(0.0));
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(field(&app, node, "ran"), Some(1.0));
}

#[test]
fn stepping_into_enters_the_callee_and_out_returns_to_the_caller() {
    let dir = project(&[("s.rn", CALLER)]);
    let app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("s.rn", &[7]).unwrap();

    host.update(0.1);
    assert_eq!(host.paused().map(|p| p.line), Some(7));
    host.resume(StepMode::Into);
    let pause = host.paused().expect("into stops inside helper");
    assert!(
        (1..=2).contains(&pause.line),
        "helper's first line: {}",
        pause.line
    );
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
    assert_eq!(field(&app, node, "ran"), Some(2.0));
}

#[test]
fn breakpoints_survive_a_hot_reload() {
    let dir = project(&[("s.rn", COUNTER)]);
    let app = app_in(dir.path());
    attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("s.rn", &[7]).unwrap();
    host.update(0.1);
    host.resume(StepMode::Continue);

    std::fs::write(dir.path().join("s.rn"), format!("{COUNTER}// edited\n")).unwrap();
    host.reload("s.rn").unwrap();
    assert_eq!(host.breakpoints("s.rn"), vec![7]);
    host.update(0.1);
    assert_eq!(
        host.paused().map(|p| p.line),
        Some(7),
        "the new unit is patched too"
    );
}

#[test]
fn reloading_the_paused_script_lifts_the_pause() {
    let dir = project(&[("s.rn", COUNTER)]);
    let app = app_in(dir.path());
    attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("s.rn", &[7]).unwrap();
    host.update(0.1);
    assert!(host.paused().is_some());

    std::fs::write(dir.path().join("s.rn"), format!("{COUNTER}// edited\n")).unwrap();
    host.reload("s.rn").unwrap();
    assert!(host.paused().is_none());
    assert_eq!(app.engine.frozen_root(), None);
}

#[test]
fn detaching_the_paused_node_lifts_the_pause() {
    let dir = project(&[("s.rn", COUNTER)]);
    let app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("s.rn", &[7]).unwrap();
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
    let dir = project(&[
        ("s.rn", COUNTER),
        (
            "b.rn",
            "pub fn init(this) { this.b = 0; }\npub fn update(this, dt) { this.b = this.b + 1; }\n",
        ),
        (
            "c.rn",
            "pub fn init(this) { this.c = 0; }\npub fn update(this, dt) { this.c = this.c + 1; }\n",
        ),
    ]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    let game = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Game", root);
    let a = attach(&app, game, "A", "s.rn");
    let b = attach(&app, root, "B", "b.rn");
    let c = attach(&app, game, "C", "c.rn");
    app.engine.set_debug_scope(Some(game));
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("s.rn", &[7]).unwrap();

    host.update(0.1);
    assert!(host.paused().is_some());
    assert_eq!(app.engine.frozen_root(), Some(game));
    let b_before = field(&app, b, "b").unwrap();
    let c_before = field(&app, c, "c").unwrap();
    host.update(0.1);
    host.update(0.1);
    assert_eq!(
        field(&app, b, "b"),
        Some(b_before + 2.0),
        "outside the scope, B keeps ticking"
    );
    assert_eq!(field(&app, c, "c"), Some(c_before), "inside it, C is held");
    assert_eq!(field(&app, a, "ran"), Some(0.0), "and so is the paused A");

    host.resume(StepMode::Continue);
    assert_eq!(app.engine.frozen_root(), None);
    assert_eq!(field(&app, a, "ran"), Some(1.0));
}

#[test]
fn a_breakpoint_lands_on_the_next_line_with_code() {
    let dir = project(&[(
        "s.rn",
        "pub fn init(this) { this.ran = 0; }\n\npub fn update(this, dt) {\n\n    this.ran = 1;\n}\n",
    )]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    assert_eq!(
        host.set_breakpoints("s.rn", &[4, 99]).unwrap(),
        vec![4, 99],
        "unloaded, the request is kept as asked"
    );
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    assert_eq!(
        host.breakpoints("s.rn"),
        vec![5],
        "loaded, the blank line moved down and the line past the end went"
    );
    host.update(0.1);
    assert_eq!(host.paused().map(|p| p.line), Some(5));
    host.resume(StepMode::Continue);
    assert_eq!(field(&app, node, "ran"), Some(1.0));
}

#[test]
fn clearing_the_breakpoints_lets_update_run_through() {
    let dir = project(&[("s.rn", COUNTER)]);
    let app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    host.set_breakpoints("s.rn", &[7]).unwrap();
    host.update(0.1);
    host.resume(StepMode::Continue);

    assert_eq!(
        host.set_breakpoints("s.rn", &[]).unwrap(),
        Vec::<usize>::new()
    );
    host.update(0.1);
    assert!(host.paused().is_none());
    assert_eq!(field(&app, node, "ran"), Some(2.0));
}

/// Line 3 divides by a string, which the VM refuses.
const THROWER: &str = "pub fn init(this) { this.ran = 0; }\n\
pub fn update(this, dt) {\n    let bad = 1 + \"two\";\n    this.ran = 1;\n}\n";

#[test]
fn break_on_error_stops_where_the_script_threw() {
    let dir = project(&[("s.rn", THROWER)]);
    let app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    assert!(!host.break_on_error(), "off unless asked for");
    host.set_break_on_error(true);

    host.update(0.5);
    let pause = host.paused().expect("the throw should stop update");
    assert_eq!(pause.reason, PauseReason::Error);
    assert_eq!(pause.line, 3, "the line that threw, not the one after");
    assert!(
        !pause.message.is_empty(),
        "an error pause carries what threw"
    );
    assert_eq!(
        app.engine.frozen_root(),
        Some(app.engine.root()),
        "a pause holds the simulation still, however it was reached"
    );
    // The statement after the throw never ran.
    assert_eq!(field(&app, node, "ran"), Some(0.0));

    // An errored execution has nowhere to go on from: letting go drops it.
    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(app.engine.frozen_root(), None);
}

#[test]
fn a_throw_is_logged_and_passed_over_unless_break_on_error_is_on() {
    let dir = project(&[("s.rn", THROWER)]);
    let app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();

    host.update(0.5);
    assert!(host.paused().is_none(), "the default is not to stop");
    assert_eq!(app.engine.frozen_root(), None);
    assert_eq!(field(&app, node, "ran"), Some(0.0));
}

#[test]
fn an_asked_for_break_stops_at_the_next_line_a_script_runs() {
    let dir = project(&[("s.rn", COUNTER)]);
    let app = app_in(dir.path());
    let node = attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();

    // No breakpoint anywhere: the request itself is what puts the next call
    // through the stepping executor.
    host.request_break();
    assert!(host.paused().is_none(), "nothing stops until a script runs");

    host.update(0.25);
    let pause = host.paused().expect("the next update stops");
    assert_eq!(
        (pause.line, pause.reason),
        (6, PauseReason::Pause),
        "the first line of update, not the entry"
    );
    assert_eq!(pause.frames[0].function, "update");
    assert_eq!(app.engine.frozen_root(), Some(app.engine.root()));

    host.resume(StepMode::Continue);
    assert!(host.paused().is_none());
    assert_eq!(field(&app, node, "ran"), Some(1.0));

    // The request is spent: the next tick runs straight through.
    host.update(0.25);
    assert!(host.paused().is_none());
    assert_eq!(field(&app, node, "ran"), Some(2.0));
}

/// A runtime error names the line it threw on, without the debugger being on.
///
/// The host keeps the sources its unit was compiled from and renders the
/// error against them. Before that it logged the message alone, so a script
/// author saw "expected a number" with nothing saying where.
#[test]
fn a_throw_is_reported_at_the_line_that_threw() {
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();

    let dir = project(&[("s.rn", THROWER)]);
    let app = app_in(dir.path());
    attach(&app, app.engine.root(), "Holder", "s.rn");
    let host = app.engine.script_host().unwrap();
    assert!(!host.break_on_error(), "no debugger involved");

    host.update(0.5);

    let logged = balaur_core::logbuf::recent(64)
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        logged.contains("[s.rn] update"),
        "not attributed:\n{logged}"
    );
    // `let bad = 1 + "two";` is line 3, and the caret goes under it.
    assert!(logged.contains("s.rn:3"), "no line number:\n{logged}");
    assert!(
        logged.contains("1 + \"two\""),
        "the offending line is not shown:\n{logged}"
    );
}
