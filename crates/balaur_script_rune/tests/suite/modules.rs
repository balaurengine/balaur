//! `mod` submodules: hot reload, and what an export ships.
//!
//! A file pulled in by `mod name;` is folded into its root's unit. It is a
//! key in no map of the host's, so both the watcher and the exporter have to
//! reach it through the root that named it rather than on its own.

use std::path::Path;
use std::time::{Duration, Instant};

use balaur_core::{App, AppConfig, Pack};
use balaur_script::ScriptCompiler;

const ROOT: &str = "\
mod helper;

pub fn init(this) { this.out = helper::value(); }
pub fn again(this) { this.out = helper::value(); }
";

/// A submodule that names an item back up in its root, which is what an
/// export compiling it on its own used to fail on.
const HELPER: &str = "\
pub fn value() { super::BASE + 1 }
";

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

fn app_in(dir: &Path, watch: bool, pack: Option<Pack>) -> App {
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack,
        watch,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap()
}

fn spawn(app: &App, name: &str) -> hecs::Entity {
    let root = app.engine.root();
    balaur_core::scene::spawn_node(&mut app.engine.world_mut(), name, root)
}

fn rune(app: &App) -> balaur_script_rune::RuneHost {
    app.engine
        .script_host()
        .unwrap()
        .as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune")
        .clone()
}

/// Swallow the watcher's own start-up events.
///
/// `project` writes the scripts and `app_in` then watches the directory they
/// are in, so their creation can still be in flight when the test starts. A
/// reload arriving later would end a recording the test had just opened.
fn settle_watcher(host: &balaur_script_rune::RuneHost) {
    for _ in 0..10 {
        host.pump_reloads();
        std::thread::sleep(Duration::from_millis(20));
    }
    host.pump_reloads();
}

/// Pump the watcher until `check` holds, or give up. A file watcher is
/// asynchronous on every platform this runs on, so the wait is a real one.
#[allow(
    clippy::disallowed_methods,
    reason = "the wait is on an OS file watcher, not on simulation"
)]
fn pump_until(host: &balaur_script_rune::RuneHost, check: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        host.pump_reloads();
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn saving_a_mod_submodule_reloads_the_root_that_folded_it_in() {
    let dir = project(&[
        (
            "root.rn",
            "mod helper;\npub const BASE = 10;\npub fn init(this) { this.out = helper::value(); }\npub fn again(this) { this.out = helper::value(); }\n",
        ),
        ("helper.rn", HELPER),
    ]);
    let app = app_in(dir.path(), true, None);
    let node = spawn(&app, "User");
    let host = rune(&app);
    host.attach(node, "root.rn").unwrap();
    assert_eq!(
        host.number_field(node, "out"),
        Some(11.0),
        "the module has to be folded in before a save can change anything"
    );

    std::fs::write(
        dir.path().join("helper.rn"),
        "pub fn value() { super::BASE + 2 }\n",
    )
    .unwrap();

    let reloaded = pump_until(&host, || {
        host.call_on(node, "again", &[]);
        host.number_field(node, "out") == Some(12.0)
    });
    assert!(
        reloaded,
        "saving a submodule left the node running the old code: {:?}",
        host.number_field(node, "out")
    );
}

#[test]
fn saving_a_root_still_reloads_it() {
    let dir = project(&[(
        "solo.rn",
        "pub fn init(this) { this.out = 1; }\npub fn again(this) { this.out = 1; }\n",
    )]);
    let app = app_in(dir.path(), true, None);
    let node = spawn(&app, "Solo");
    let host = rune(&app);
    host.attach(node, "solo.rn").unwrap();
    assert_eq!(host.number_field(node, "out"), Some(1.0));

    std::fs::write(
        dir.path().join("solo.rn"),
        "pub fn init(this) { this.out = 5; }\npub fn again(this) { this.out = 5; }\n",
    )
    .unwrap();

    assert!(pump_until(&host, || {
        host.call_on(node, "again", &[]);
        host.number_field(node, "out") == Some(5.0)
    }));
}

#[test]
fn an_open_recording_ends_where_a_script_reloaded() {
    let dir = project(&[(
        "solo.rn",
        "pub fn init(this) { this.out = 1; }\npub fn update(this, dt) {}\n",
    )]);
    let mut app = app_in(dir.path(), true, None);
    let node = spawn(&app, "Solo");
    let host = rune(&app);
    host.attach(node, "solo.rn").unwrap();
    settle_watcher(&host);
    let session = dir.path().join("session.jsonl");
    balaur_core::replay::start_recording(&app.engine, &session, "t", "", false).unwrap();
    app.tick(1.0 / 60.0);
    assert!(open_recording(&app));

    std::fs::write(
        dir.path().join("solo.rn"),
        "pub fn init(this) { this.out = 2; }\npub fn update(this, dt) {}\n",
    )
    .unwrap();

    assert!(pump_until(&host, || !open_recording(&app)));
    let closed = balaur_core::replay::Session::read(&session).unwrap();
    assert_eq!(
        closed
            .trailer
            .expect("a closed session has a trailer")
            .reason,
        "reload",
        "the frames after a reload came from different code"
    );
}

fn open_recording(app: &App) -> bool {
    app.engine
        .resource::<balaur_core::replay::Recording>()
        .borrow()
        .0
        .is_some()
}

#[test]
fn an_export_compiles_roots_and_lets_them_carry_their_modules() {
    let dir = project(&[
        (
            "root.rn",
            "mod helper;\npub const BASE = 10;\npub fn init(this) { this.out = helper::value(); }\n",
        ),
        ("helper.rn", HELPER),
    ]);
    let app = app_in(dir.path(), false, None);
    let host = rune(&app);
    let pack = Pack::build(dir.path(), &host as &dyn ScriptCompiler)
        .expect("a submodule naming `super::` must not fail the export");

    assert!(
        !pack.scripts["root.rn"].is_empty(),
        "the root ships as a compiled unit"
    );
    assert!(
        pack.scripts["helper.rn"].is_empty(),
        "a submodule ships inside its root, not a second time on its own"
    );

    let packed = app_in(dir.path(), false, Some(pack));
    let node = spawn(&packed, "User");
    let host = rune(&packed);
    host.attach(node, "root.rn").unwrap();
    assert_eq!(
        host.number_field(node, "out"),
        Some(11.0),
        "the packed root still has its module"
    );
}

#[test]
fn a_root_is_not_mistaken_for_one_of_its_own_modules() {
    let dir = project(&[
        ("root.rn", ROOT),
        ("helper.rn", "pub fn value() { 1 }\n"),
        ("solo.rn", "pub fn init(this) {}\n"),
    ]);
    let app = app_in(dir.path(), false, None);
    let host = rune(&app);
    assert!(host.is_submodule("helper.rn"));
    assert!(!host.is_submodule("root.rn"));
    assert!(!host.is_submodule("solo.rn"));
}
