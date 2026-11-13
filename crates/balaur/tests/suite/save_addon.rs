//! The library's save addon, as a game uses it: slots in the user data
//! folder, stamped with `[save] version` and brought forward when older.

use balaur::{AppConfig, standard_app};
use balaur_core::variables::Variables;
use balaur_script::Value;

const ADDON: &str = include_str!("../../../../editor/library/addons/save/save.bs");

const SCENE: &str = r#"
[variables]
said = { type = "text", value = "" }

[[nodes]]
id = "n_probe"
name = "Probe"
script = { source = "scripts/probe.bs" }
"#;

/// A project carrying the addon at `version`, its user data kept in the
/// test's folder, with `old` written as a slot before the script runs.
fn app_with(script: &str, version: i64, old: &[(&str, &str)]) -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "project.toml",
            format!(
                "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n\n[save]\nversion = {version}\n"
            ),
        ),
        ("main.toml", SCENE.to_string()),
        ("scripts/probe.bs", script.to_string()),
        ("addons/save/save.bs", ADDON.to_string()),
    ] {
        let at = dir.path().join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let data = dir.path().join("data");
    std::fs::create_dir_all(data.join("saves")).unwrap();
    for (slot, text) in old {
        std::fs::write(data.join(format!("saves/{slot}.toml")), text).unwrap();
    }
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    balaur_core::file_api::add_root(&app.engine, &data);
    balaur_core::engine_api::set_data_home(&app.engine, Some(data));
    app.load_project().unwrap();
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    (dir, app)
}

fn said(app: &balaur::App) -> String {
    let variables = app.engine.resource::<Variables>();
    match variables.borrow().get("said").cloned() {
        Some(Value::Str(text)) => text,
        other => panic!("said is {other:?}"),
    }
}

/// The probe's script: `body` runs once, in the first `update`, and `tell`
/// appends to `said`.
fn probe(body: &str) -> String {
    format!(
        r#"use addons::save::save;

fn tell(what: str) {{
    scene::set_variable("said", scene::variable("said").as_str() + what + ";");
}}

fn step(version: int, data: any) -> any {{
    data["steps"] = data["steps"].as_str() + "{{version}}";
    data
}}

pub class Probe {{
    node: Node,
    ran: bool,
}}

impl Probe {{
    pub fn init() {{
        events::listen(self.node, save::SAVED);
    }}

    pub fn update(dt: float) {{
        if self.ran {{
            return;
        }}
        self.ran = true;
{body}
        tell("ran");
    }}

    pub fn on_saved(count: any) {{
        tell("saved {{count.as_int()}}");
    }}
}}
"#
    )
}

#[test]
fn a_written_slot_reads_back_is_listed_and_leaves_no_part_file_beside_it() {
    let script = probe(
        r#"
        tell("wrote {save::write("slot1", { "level": 4, "coins": 120 })}");
        let back = save::read("slot1");
        tell("level {back["level"].as_int()} coins {back["coins"].as_int()}");
        tell("slots {save::slots().len()} {save::slots()[0]}");
        tell("absent {save::read("never") == nil}");
"#,
    );
    let (dir, app) = app_with(&script, 1, &[]);
    let said = said(&app);
    assert!(said.contains("ran;"), "{said}");
    assert!(said.contains("wrote true;"), "{said}");
    assert!(said.contains("level 4 coins 120;"), "{said}");
    assert!(said.contains("slots 1 slot1;"), "{said}");
    assert!(said.contains("absent true;"), "{said}");
    assert!(said.contains("saved 1;"), "{said}");
    let saves = dir.path().join("data/saves");
    let text = std::fs::read_to_string(saves.join("slot1.toml")).unwrap();
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(doc["version"].as_integer(), Some(1), "{text}");
    assert_eq!(doc["data"]["coins"].as_integer(), Some(120), "{text}");
    assert!(!saves.join("slot1.toml.part").exists());
}

#[test]
fn an_older_save_is_brought_forward_one_version_at_a_time() {
    let script = probe(
        r#"
        save::migrate_with(step);
        tell("steps {save::read("old")["steps"].as_str()}");
"#,
    );
    let old = "version = 1\n[data]\nsteps = \"\"\n";
    let (_dir, app) = app_with(&script, 3, &[("old", old)]);
    let said = said(&app);
    assert!(said.contains("ran;"), "{said}");
    assert!(said.contains("steps 12;"), "{said}");
}

#[test]
fn a_save_from_a_newer_build_and_a_slot_named_like_a_path_are_refused() {
    let script = probe(
        r#"
        tell("newer {save::read("future") == nil}");
        tell("path {save::write("../escape", { "n": 1 })}");
"#,
    );
    let future = "version = 9\n[data]\nn = 1\n";
    let (dir, app) = app_with(&script, 2, &[("future", future)]);
    let said = said(&app);
    assert!(said.contains("ran;"), "{said}");
    assert!(said.contains("newer true;"), "{said}");
    assert!(said.contains("path false;"), "{said}");
    assert!(!dir.path().join("data/escape.toml").exists());
}
