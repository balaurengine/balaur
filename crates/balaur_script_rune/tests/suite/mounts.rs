//! Addons mounted as native modules: `addons/<name>/<file>.rn` called as
//! `<name>::<file>::function` from any script, in a dev run, after a save,
//! from an exported pack, and from a root added after the host started.

use std::path::Path;
use std::time::{Duration, Instant};

use balaur_core::{App, AppConfig, Pack};
use balaur_script::ScriptCompiler;

const MATH: &str = "\
pub const LIMIT = 3;
pub mod colors {
    pub const RED = \"red\";
}
/// Twice `x`.
pub fn double(x) { x * 2 }
pub async fn later(x) { x + 1 }
";

const USER: &str = "\
pub async fn init(this) {
    this.out = kit::math::double(kit::math::LIMIT);
    this.color = kit::math::colors::RED;
    this.later = kit::math::later(1).await;
}
pub fn again(this) { this.out = kit::math::double(kit::math::LIMIT); }
";

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    write(dir.path(), files);
    dir
}

fn write(root: &Path, files: &[(&str, &str)]) {
    for (name, body) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
}

fn app_in(dir: &Path, watch: bool, pack: Option<Pack>) -> App {
    App::new(AppConfig {
        pack,
        watch,
        script_backend: Some(balaur_script_rune::factory()),
        ..AppConfig::bare(dir.to_path_buf())
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

#[test]
fn a_script_calls_an_addon_and_reads_its_constants_by_path() {
    let dir = project(&[("addons/kit/math.rn", MATH), ("user.rn", USER)]);
    let app = app_in(dir.path(), false, None);
    let node = spawn(&app, "User");
    let host = rune(&app);
    host.attach(node, "user.rn").unwrap();
    assert_eq!(host.number_field(node, "out"), Some(6.0));
    assert_eq!(host.text_field(node, "color").as_deref(), Some("red"));
    assert_eq!(
        host.number_field(node, "later"),
        Some(2.0),
        "an async addon function is awaited like any other"
    );
}

#[test]
fn an_addon_file_calls_its_sibling_by_path() {
    let dir = project(&[
        ("addons/kit/a.rn", "pub fn value() { kit::b::base() + 1 }\n"),
        ("addons/kit/b.rn", "pub fn base() { 41 }\n"),
        (
            "user.rn",
            "pub fn init(this) { this.out = kit::a::value(); }\n",
        ),
    ]);
    let app = app_in(dir.path(), false, None);
    let node = spawn(&app, "User");
    let host = rune(&app);
    host.attach(node, "user.rn").unwrap();
    assert_eq!(host.number_field(node, "out"), Some(42.0));
}

#[test]
fn a_file_deeper_in_an_addon_is_not_mounted() {
    let dir = project(&[
        ("addons/kit/editor/dock.rn", "pub fn value() { 1 }\n"),
        (
            "user.rn",
            "pub fn init(this) { this.out = kit::editor::dock::value(); }\n",
        ),
    ]);
    let app = app_in(dir.path(), false, None);
    let node = spawn(&app, "User");
    let host = rune(&app);
    assert!(
        host.attach(node, "user.rn").is_err(),
        "only `addons/<name>/<file>.rn` is a mount; an editor plugin is not"
    );
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the wait is on an OS file watcher, not on simulation"
)]
fn saving_an_addon_reaches_every_caller() {
    let dir = project(&[("addons/kit/math.rn", MATH), ("user.rn", USER)]);
    let app = app_in(dir.path(), true, None);
    let node = spawn(&app, "User");
    let host = rune(&app);
    host.attach(node, "user.rn").unwrap();
    assert_eq!(host.number_field(node, "out"), Some(6.0));

    write(
        dir.path(),
        &[("addons/kit/math.rn", &MATH.replace("x * 2", "x * 3"))],
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut out = None;
    while Instant::now() < deadline {
        host.pump_reloads();
        host.call_on(node, "again", &[]);
        out = host.number_field(node, "out");
        if out == Some(9.0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(out, Some(9.0), "the caller still ran the addon's old code");
}

#[test]
fn an_exported_pack_keeps_its_mounts() {
    let dir = project(&[("addons/kit/math.rn", MATH), ("user.rn", USER)]);
    let app = app_in(dir.path(), false, None);
    let pack = Pack::build(dir.path(), &rune(&app) as &dyn ScriptCompiler).unwrap();

    let packed = app_in(dir.path(), false, Some(pack));
    let node = spawn(&packed, "User");
    let host = rune(&packed);
    host.attach(node, "user.rn").unwrap();
    assert_eq!(host.number_field(node, "out"), Some(6.0));
    assert_eq!(host.text_field(node, "color").as_deref(), Some("red"));
}

#[test]
fn a_root_added_after_start_mounts_its_addons() {
    let own = project(&[("solo.rn", "pub fn init(this) { this.out = 1; }\n")]);
    let game = project(&[("addons/kit/math.rn", MATH), ("user.rn", USER)]);
    let app = app_in(own.path(), false, None);
    let host = rune(&app);
    let first = spawn(&app, "Solo");
    host.attach(first, "solo.rn").unwrap();

    balaur_core::file_api::add_root(&app.engine, game.path());
    let node = spawn(&app, "User");
    let key = game.path().join("user.rn");
    host.attach(node, &key.to_string_lossy()).unwrap();
    assert_eq!(
        host.number_field(node, "out"),
        Some(6.0),
        "a game the editor opens mounts its own addons"
    );
    assert_eq!(host.number_field(first, "out"), Some(1.0));
}

#[test]
fn an_addon_shares_its_name_with_an_engine_module() {
    let dir = project(&[
        ("addons/math/extra.rn", "pub fn triple(x) { x * 3 }\n"),
        (
            "user.rn",
            "pub fn init(this) { this.out = math::extra::triple(2); this.pi = math::PI; }\n",
        ),
    ]);
    let app = app_in(dir.path(), false, None);
    let node = spawn(&app, "User");
    let host = rune(&app);
    host.attach(node, "user.rn").unwrap();
    assert_eq!(host.number_field(node, "out"), Some(6.0));
    assert_eq!(host.number_field(node, "pi"), Some(std::f64::consts::PI));
}

#[test]
fn a_mount_clashing_with_an_engine_path_is_left_out_and_the_rest_still_runs() {
    let dir = project(&[
        ("addons/math/sin.rn", "pub fn value() { 1 }\n"),
        ("addons/kit/math.rn", MATH),
        ("user.rn", USER),
        (
            "trig.rn",
            "pub fn init(this) { this.out = math::sin(0.0); }\n",
        ),
    ]);
    let app = app_in(dir.path(), false, None);
    let host = rune(&app);
    let node = spawn(&app, "User");
    host.attach(node, "user.rn").unwrap();
    assert_eq!(host.number_field(node, "out"), Some(6.0));
    let trig = spawn(&app, "Trig");
    host.attach(trig, "trig.rn").unwrap();
    assert_eq!(
        host.number_field(trig, "out"),
        Some(0.0),
        "the engine's own `math::sin` still answers"
    );
}

/// The labels offered at the end of `probe`, typed on a line of its own.
fn complete_after(host: &balaur_script_rune::RuneHost, probe: &str) -> Vec<String> {
    let source = format!("pub fn init(this) {{}}\n// {probe}\n");
    let column = probe.chars().count() + 4;
    host.complete("user.rn", &source, 2, column)
        .unwrap()
        .into_iter()
        .map(|one| one.label)
        .collect()
}

#[test]
fn completion_walks_an_addon_path_down_to_its_constants() {
    let dir = project(&[("addons/kit/math.rn", MATH)]);
    let app = app_in(dir.path(), false, None);
    let host = rune(&app);
    assert_eq!(complete_after(&host, "kit::"), ["math"]);
    assert_eq!(
        complete_after(&host, "kit::math::"),
        ["LIMIT", "colors", "double", "later"]
    );
    assert_eq!(complete_after(&host, "kit::math::d"), ["double"]);
    assert_eq!(complete_after(&host, "kit::math::colors::"), ["RED"]);
}

#[test]
fn hovering_a_mounted_function_shows_its_parameters_and_doc() {
    let dir = project(&[("addons/kit/math.rn", MATH)]);
    let app = app_in(dir.path(), false, None);
    let source = "pub fn init(this) { kit::math::double(1); }\n";
    let column = source.find("double").unwrap() + 2;
    let found = rune(&app)
        .hover("user.rn", source, 1, column)
        .unwrap()
        .expect("a mounted function hovers");
    assert_eq!(found.title, "kit::math::double");
    assert_eq!(found.detail, "(x)");
    assert_eq!(found.doc, "Twice `x`.");
}
