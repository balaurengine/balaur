//! The library's save addon, as a game uses it: slots in the user data
//! folder.

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

/// A project carrying the addon, its user data kept in the test's folder,
/// with `old` written as a slot before the script runs.
fn app_with(script: &str, old: &[(&str, &str)]) -> (tempfile::TempDir, balaur::App) {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n".to_string(),
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

    pub fn on_saved(slot: any) {{
        tell("saved {{slot.text()}}");
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
    let (dir, app) = app_with(&script, &[]);
    let said = said(&app);
    assert!(said.contains("ran;"), "{said}");
    assert!(said.contains("wrote true;"), "{said}");
    assert!(said.contains("level 4 coins 120;"), "{said}");
    assert!(said.contains("slots 1 slot1;"), "{said}");
    assert!(said.contains("absent true;"), "{said}");
    assert!(said.contains("saved slot1;"), "{said}");
    let saves = dir.path().join("data/saves");
    let text = std::fs::read_to_string(saves.join("slot1.toml")).unwrap();
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(doc["data"]["coins"].as_integer(), Some(120), "{text}");
    assert!(!saves.join("slot1.toml.part").exists());
}

#[test]
fn a_file_that_does_not_parse_and_a_slot_named_like_a_path_are_refused() {
    let script = probe(
        r#"
        tell("broken {save::read("broken") == nil}");
        tell("path {save::write("../escape", { "n": 1 })}");
"#,
    );
    let (dir, app) = app_with(&script, &[("broken", "data = [1, 2")]);
    let said = said(&app);
    assert!(said.contains("ran;"), "{said}");
    assert!(said.contains("broken true;"), "{said}");
    assert!(said.contains("path false;"), "{said}");
    assert!(!dir.path().join("data/escape.toml").exists());
}
