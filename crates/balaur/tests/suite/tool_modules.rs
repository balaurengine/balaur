//! `debugger` and `script`: the modules the editor's binary adds with
//! [`balaur::ToolsPlugin`], which a game's runtime leaves out.

#![cfg(any(feature = "bs", feature = "bs-jit"))]

use std::path::Path;

use balaur::{App, AppConfig, standard_app};

use super::bs::{field, one_script};

/// A dev run of the project at `dir` as the editor's binary boots the editor:
/// every script of it may call the tools.
pub(super) fn booted(dir: &Path) -> App {
    let mut app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref())).unwrap();
    balaur_plugin::load(&mut app, &mut tools(dir)).unwrap();
    app.load_project().unwrap();
    app
}

/// What `balaur check` finds in the project at `dir`, checked as the editor.
pub(super) fn checked(dir: &Path) -> Vec<balaur::Finding> {
    balaur::check_project_using(dir, &mut [Box::new(tools(dir))]).unwrap()
}

/// The tools, with `dir` the project the binary runs as the editor.
fn tools(dir: &Path) -> balaur::ToolsPlugin {
    balaur::ToolsPlugin::new(Some(dir.to_path_buf()))
}

const ASKS_TOOLS_BS: &str = r#"pub class Editor {
    node: Node,
    completed: int,
    first: str,
    errors: int,
    hover: str,
    symbols: int,
    hook: str,
    renamed: str,
    formatted: str,
}

impl Editor {
    fn init() {
        let src = "pub struct Racer {{ speed: float }}\nimpl Racer {{\n    fn update(dt: float) {{\n        self.\n    }}\n}}\n";
        let rows = script::complete("scripts/racer.bs", src, 4, 14);
        self.completed = rows.len();
        self.first = rows[0].label;
        self.errors = script::check("scripts/racer.bs", "fn wrong() -> int {{ true }}").len();
        let hover = script::hover("scripts/racer.bs", "pub struct Racer {{ speed: float }}\nfn speed_of(racer: Racer) -> float {{ racer.speed }}\n", 2, 45);
        self.hover = hover.unwrap().title;
        let syms = script::symbols("scripts/racer.bs", "pub struct Racer {{ pub hp: int = 3 }}\nimpl Racer {{\n    fn update(dt: float) {{\n    }}\n}}\n");
        self.symbols = syms.len();
        self.hook = syms[2].name + ":" + syms[2].kind;
        self.renamed = script::rename("scripts/racer.bs", "pub struct Racer {{ hp: int }}\n", "hp", "health")[0].source;
        self.formatted = script::format("scripts/racer.bs", "fn tidy() {{\nlet count = 1;\n}}\n");
    }
}
"#;

#[test]
fn the_editor_s_script_module_answers_for_a_bs_file() {
    use balaur_script::Value::{Int, Str};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"editor\"\nname = \"Editor\"\nscript = { source = \"scripts/editor.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/editor.bs"), ASKS_TOOLS_BS).unwrap();
    let app = booted(dir.path());
    // The field and the method.
    assert_eq!(field(&app, "Editor", "completed"), Some(Int(2)));
    assert_eq!(field(&app, "Editor", "first"), Some(Str("speed".into())));
    assert_eq!(field(&app, "Editor", "errors"), Some(Int(1)));
    assert_eq!(
        field(&app, "Editor", "hover"),
        Some(Str("speed: float".into()))
    );
    assert_eq!(field(&app, "Editor", "symbols"), Some(Int(3)));
    assert_eq!(
        field(&app, "Editor", "hook"),
        Some(Str("update:function".into()))
    );
    assert_eq!(
        field(&app, "Editor", "renamed"),
        Some(Str("pub struct Racer { health: int }\n".into()))
    );
    assert_eq!(
        field(&app, "Editor", "formatted"),
        Some(Str("fn tidy() {\n    let count = 1;\n}\n".into()))
    );
}

const EXPORTER_BS: &str =
    "pub class Tuned { pub speed: float = 2.5, pub label: str = \"go\", seen: str }
impl Tuned {
    fn init() {
        let rows = script::exports(\"scripts/tuned.bs\");
        self.seen = \"{rows.len()} {rows[0][\"name\"]}={rows[0][\"default\"]} {rows[1][\"name\"]}:{rows[1][\"type\"]}\";
    }
}
";

#[test]
fn a_script_reads_what_another_declares_tunable() {
    use balaur_script::Value::Str;
    let dir = one_script("Tuned", EXPORTER_BS);
    let app = booted(dir.path());
    assert_eq!(
        field(&app, "Tuned", "seen"),
        Some(Str("2 label=go speed:float".into()))
    );
}

const PAUSER_BS: &str = "pub class Pauser { node: Node, tidy: str, scoped: bool }
impl Pauser {
    fn init() {
        self.tidy = script::format(\"scripts/pauser.bs\", \"fn tidy() {{\\nlet count = 1;\\n}}\\n\");
        self.scoped = debugger::scope().is_valid();
    }
}
";

/// The functions of `module` a script on `app` compiles against.
fn functions_of(app: &App, module: &str) -> Vec<String> {
    let prefix = format!("{module}::");
    balaur::script_bs::BsHost::of(&app.engine)
        .unwrap()
        .api()
        .functions
        .into_iter()
        .map(|function| function.key)
        .filter(|key| key.starts_with(&prefix))
        .collect()
}

// `debugger` and `script` are the editor binary's: a game's runtime offers
// neither, so a game that names one fails its check rather than its export.
#[test]
fn a_game_runtime_has_no_debugger_and_no_script_tools() {
    use balaur_script::Value::{Bool, Str};
    let dir = one_script("Pauser", PAUSER_BS);
    let game = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    for module in ["debugger", "script"] {
        let offered = functions_of(&game, module);
        assert!(offered.is_empty(), "a game is offered {offered:?}");
    }
    assert!(
        !functions_of(&game, "engine").is_empty(),
        "the control: a game is offered `engine`"
    );
    let refused = balaur::check_project(dir.path()).unwrap();
    assert!(
        refused.iter().any(|found| found.message.contains("script")),
        "{refused:?}"
    );
    let tools = booted(dir.path());
    assert!(functions_of(&tools, "debugger").contains(&"debugger::scope".to_string()));
    assert!(functions_of(&tools, "script").contains(&"script::format".to_string()));
    assert_eq!(
        field(&tools, "Pauser", "tidy"),
        Some(Str("fn tidy() {\n    let count = 1;\n}\n".into()))
    );
    assert_eq!(field(&tools, "Pauser", "scoped"), Some(Bool(false)));
}
