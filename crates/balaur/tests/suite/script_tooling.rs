//! What the caret is offered, hovers over, and jumps to in a `.bs` file, as
//! the language server and the editor ask a booted engine.
//!
//! One case per kind of thing a caret can be after, plus the two verbs that
//! write: formatting has to be idempotent and a rename has to be refused when
//! the new name is not an identifier.

use balaur::{App, AppConfig, standard_app};
use balaur_script::LanguageTools;

const PATH: &str = "ball.bs";

const SCRIPT: &str = "\
use helper;

pub class Ball { node: Node, speed: float = 4.0 }
impl Ball {
    fn init() {
        let node = self.node;
        node.body2d.apply_impulse(0.0, 5.0);
    }
    fn update(dt: float) {
        let hit = physics2d::raycast(nil);
        self.speed = helper::assist(self.speed);
    }
}
";

const HELPER: &str = "\
pub fn assist(speed: float) -> float {
    return speed;
}
";

/// A project whose scripts the tooling answers about. `standard_app` rather
/// than a bare `App`: the completions under test are the engine's own modules,
/// and a bare app registers none of them.
fn host() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"ball\"\nname = \"Root\"\nscript = { source = \"ball.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join(PATH), SCRIPT).unwrap();
    std::fs::write(dir.path().join("helper.bs"), HELPER).unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

/// What answers for a `.bs` file: the engine's own script host.
fn tools(app: &App) -> balaur::script_bs::BsHost {
    balaur::script_bs::BsHost::of(&app.engine).expect("a .bs host")
}

/// The completions for `probe` typed as a statement at the start of `init`,
/// the caret at its end.
fn complete_after(app: &App, probe: &str) -> Vec<String> {
    let source = SCRIPT.replace(
        "        let node = self.node;",
        &format!("        {probe}\n        let node = self.node;"),
    );
    let column = 8 + probe.chars().count() + 1;
    tools(app)
        .complete(PATH, &source, 6, column)
        .into_iter()
        .map(|one| one.label)
        .collect()
}

/// The 1-based line and column of the second character of `word`'s first
/// use on a line containing `on`.
fn place(on: &str, word: &str) -> (usize, usize) {
    let line = SCRIPT.lines().position(|l| l.contains(on)).unwrap();
    let column = SCRIPT.lines().nth(line).unwrap().find(word).unwrap() + 2;
    (line + 1, column)
}

#[test]
fn a_module_path_offers_that_modules_functions_and_constants() {
    let (_dir, app) = host();
    let found = complete_after(&app, "input::");
    assert!(found.contains(&"action_down".to_string()), "{found:?}");
    assert!(found.contains(&"KEY_SPACE".to_string()), "{found:?}");
}

#[test]
fn a_module_path_narrows_to_the_prefix_typed() {
    let (_dir, app) = host();
    let found = complete_after(&app, "input::KEY_S");
    assert!(found.contains(&"KEY_SPACE".to_string()), "{found:?}");
    assert!(
        found.iter().all(|f| f.starts_with("KEY_S")),
        "a completion outside the prefix: {found:?}"
    );
}

#[test]
fn a_component_handle_offers_what_acts_on_that_component() {
    let (_dir, app) = host();
    let found = complete_after(&app, "self.node.body2d.");
    assert!(found.contains(&"apply_impulse".to_string()), "{found:?}");
    assert!(
        !found.contains(&"raycast".to_string()),
        "a function acting on no component reached a handle: {found:?}"
    );
}

#[test]
fn self_offers_the_scripts_own_fields_and_methods() {
    let (_dir, app) = host();
    let found = complete_after(&app, "self.");
    assert!(found.contains(&"speed".to_string()), "{found:?}");
    assert!(found.contains(&"update".to_string()), "{found:?}");
}

#[test]
fn a_bare_prefix_offers_modules() {
    let (_dir, app) = host();
    let found = complete_after(&app, "phys");
    assert!(found.contains(&"physics2d".to_string()), "{found:?}");
}

#[test]
fn hovering_an_engine_function_shows_what_a_script_passes_it() {
    let (_dir, app) = host();
    let (line, column) = place("raycast", "raycast");
    let found = tools(&app)
        .hover(PATH, SCRIPT, line, column)
        .expect("raycast is engine API");
    assert!(found.contains("raycast"), "{found}");
}

#[test]
fn a_definition_in_the_file_names_its_line() {
    let (_dir, app) = host();
    let (line, column) = place("self.speed =", "speed");
    let found = tools(&app)
        .definition(PATH, SCRIPT, line, column)
        .expect("speed is a field");
    assert_eq!((found.file.as_str(), found.line), (PATH, 3));
}

#[test]
fn symbols_are_the_files_struct_and_methods() {
    let (_dir, app) = host();
    let found = tools(&app).symbols(PATH, SCRIPT);
    let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
    for name in ["Ball", "init", "update"] {
        assert!(names.contains(&name), "no {name}: {names:?}");
    }
}

#[test]
fn references_are_every_use_of_a_name() {
    let (_dir, app) = host();
    let found = tools(&app).references(PATH, SCRIPT, "speed");
    let lines: Vec<usize> = found.iter().map(|l| l.line).collect();
    assert!(lines.contains(&3) && lines.contains(&11), "{lines:?}");
}

#[test]
fn formatting_is_idempotent() {
    let (_dir, app) = host();
    let messy = SCRIPT.replace("    fn init() {", "fn init()   {");
    let once = tools(&app).format(PATH, &messy).unwrap();
    assert_eq!(once, SCRIPT, "the house layout");
    let twice = tools(&app).format(PATH, &once).unwrap();
    assert_eq!(once, twice, "a second format changed the first's output");
}

#[test]
fn renaming_rewrites_every_use_in_the_file() {
    let (_dir, app) = host();
    let written = tools(&app)
        .rename(PATH, SCRIPT, "speed", "pace")
        .expect("a field renames");
    assert!(written.contains("pace: float"), "{written}");
    assert!(!written.contains("speed"), "{written}");
}

#[test]
fn renaming_to_something_that_is_not_an_identifier_is_refused() {
    let (_dir, app) = host();
    for to in ["help out", "2fast", ""] {
        assert_eq!(
            tools(&app).rename(PATH, SCRIPT, "speed", to),
            None,
            "{to:?}"
        );
    }
}

/// Every engine function a script can call hovers, so the popup is as
/// complete as the reference rather than sampled.
#[test]
fn every_engine_function_hovers() {
    let (_dir, app) = host();
    let api = tools(&app).api();
    let mut missing = Vec::new();
    let mut checked = 0;
    for f in &api.functions {
        let Some((module, name)) = f.key.split_once("::") else {
            continue;
        };
        let source = SCRIPT.replace(
            "        let node = self.node;",
            &format!("        {module}::{name};\n        let node = self.node;"),
        );
        let column = 8 + module.len() + 2 + 2;
        match tools(&app).hover(PATH, &source, 6, column) {
            Some(text) if text.contains(name) => checked += 1,
            _ => missing.push(f.key.clone()),
        }
    }
    assert!(checked > 600, "only {checked} functions hovered");
    assert!(
        missing.is_empty(),
        "{} functions do not hover: {:?}",
        missing.len(),
        &missing[..missing.len().min(10)]
    );
}

/// A completion is offered for every module the engine reports, so a module
/// added by a plugin is reachable without touching the provider.
#[test]
fn every_module_completes_from_a_bare_prefix() {
    let (_dir, app) = host();
    let api = tools(&app).api();
    let mut modules: Vec<&str> = api
        .functions
        .iter()
        .filter_map(|f| Some(f.key.split_once("::")?.0))
        .collect();
    modules.dedup();
    for name in modules {
        let found = complete_after(&app, name);
        assert!(
            found.contains(&name.to_string()),
            "{name} is not offered: {found:?}"
        );
    }
}
