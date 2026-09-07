//! What the caret is offered, hovers over, and jumps to.
//!
//! One case per row of the classifier's table, plus the two verbs that write:
//! formatting has to be idempotent and a rename has to be refused when the new
//! name is not an identifier.

use balaur::rune::RuneHost;
use balaur::{App, AppConfig, standard_app};

const SCRIPT: &str = "\
mod helper;

pub fn exports() {
    #{ speed: 4.0, name: \"ball\" }
}

pub fn init(this) {
    let node = this.node;
    node.body2d.apply_impulse(0.0, 5.0);
}

pub fn update(this, dt) {
    let hit = physics2d::raycast(#{});
    helper::assist(hit);
}
";

const HELPER: &str = "\
pub fn assist(hit) { hit }
";

/// A project whose scripts the tooling answers about. `standard_app` rather
/// than a bare `App`: the completions under test are the engine's own modules,
/// and a bare app registers none of them.
fn host() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = \"main.rn\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.rn"), SCRIPT).unwrap();
    std::fs::write(dir.path().join("helper.rn"), HELPER).unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn rune(app: &App) -> RuneHost {
    balaur::rune::rune_of(&app.engine)
}

/// The completions for a probe line appended to the script, with the caret at
/// its end. A probe is a comment, so the file still compiles.
fn complete_after(host: &RuneHost, probe: &str) -> Vec<String> {
    let source = format!("{SCRIPT}// {probe}\n");
    let line = source.lines().count();
    let column = probe.chars().count() + 4;
    host.complete("main.rn", &source, line, column)
        .unwrap()
        .into_iter()
        .map(|one| one.label)
        .collect()
}

#[test]
fn a_module_path_offers_that_modules_functions_and_constants() {
    let (_dir, app) = host();
    let found = complete_after(&rune(&app), "physics2d::");
    assert!(found.contains(&"raycast".to_string()), "{found:?}");
    assert!(found.contains(&"BODY_DYNAMIC".to_string()), "{found:?}");
    assert!(
        !found.contains(&"delta".to_string()),
        "engine's own leaked into physics2d: {found:?}"
    );
}

#[test]
fn a_module_path_narrows_to_the_prefix_typed() {
    let (_dir, app) = host();
    let found = complete_after(&rune(&app), "physics2d::ray");
    assert!(found.contains(&"raycast".to_string()), "{found:?}");
    assert!(
        !found.contains(&"BODY_DYNAMIC".to_string()),
        "the prefix was not applied: {found:?}"
    );
}

#[test]
fn a_component_handle_offers_what_acts_on_that_component() {
    let (_dir, app) = host();
    let found = complete_after(&rune(&app), "node.body2d.");
    assert!(found.contains(&"apply_impulse".to_string()), "{found:?}");
    // The six generic ops every handle answers, whatever it is on.
    assert!(found.contains(&"props".to_string()), "{found:?}");
    assert!(
        !found.contains(&"raycast".to_string()),
        "a function acting on no component reached a handle: {found:?}"
    );
}

#[test]
fn this_offers_the_scripts_own_exports_and_functions() {
    let (_dir, app) = host();
    let found = complete_after(&rune(&app), "this.");
    assert!(found.contains(&"speed".to_string()), "{found:?}");
    assert!(found.contains(&"update".to_string()), "{found:?}");
}

#[test]
fn a_bare_prefix_offers_modules_and_the_files_own_functions() {
    let (_dir, app) = host();
    let found = complete_after(&rune(&app), "phys");
    assert!(found.contains(&"physics2d".to_string()), "{found:?}");
    let found = complete_after(&rune(&app), "upd");
    assert!(found.contains(&"update".to_string()), "{found:?}");
}

#[test]
fn a_receiver_of_unknown_type_offers_runes_own_methods() {
    let (_dir, app) = host();
    let found = complete_after(&rune(&app), "let s = \"\"; s.le");
    assert!(
        found.contains(&"len".to_string()),
        "the context's own functions are not reachable: {found:?}"
    );
}

#[test]
fn hovering_a_function_returns_the_line_the_reference_prints() {
    let (_dir, app) = host();
    let source = SCRIPT;
    // `physics2d::raycast` on the line that calls it.
    let line = source
        .lines()
        .position(|l| l.contains("raycast"))
        .expect("the fixture calls raycast")
        + 1;
    let column = source
        .lines()
        .nth(line - 1)
        .unwrap()
        .find("raycast")
        .unwrap()
        + 2;
    let found = rune(&app)
        .hover("main.rn", source, line, column)
        .unwrap()
        .expect("raycast is documented");
    assert_eq!(found.title, "physics2d::raycast");
    assert!(!found.doc.is_empty(), "the reference's doc line is missing");
}

#[test]
fn a_definition_in_a_submodule_names_that_file_and_line() {
    let (_dir, app) = host();
    let line = SCRIPT
        .lines()
        .position(|l| l.contains("helper::assist"))
        .unwrap()
        + 1;
    let column = SCRIPT
        .lines()
        .nth(line - 1)
        .unwrap()
        .find("assist")
        .unwrap()
        + 2;
    let found = rune(&app)
        .definition("main.rn", SCRIPT, line, column)
        .unwrap()
        .expect("assist is declared in helper.rn");
    assert_eq!(found.file, "helper.rn");
    assert_eq!(found.line, 1);
}

#[test]
fn a_definition_in_the_engine_carries_a_reference_page_not_a_file() {
    let (_dir, app) = host();
    let line = SCRIPT.lines().position(|l| l.contains("raycast")).unwrap() + 1;
    let column = SCRIPT
        .lines()
        .nth(line - 1)
        .unwrap()
        .find("raycast")
        .unwrap()
        + 2;
    let found = rune(&app)
        .definition("main.rn", SCRIPT, line, column)
        .unwrap()
        .expect("raycast resolves");
    assert!(found.file.is_empty(), "engine API claimed a file");
    assert!(found.url.ends_with("physics2d"), "{}", found.url);
}

#[test]
fn symbols_are_the_files_public_functions_and_its_exports() {
    let (_dir, app) = host();
    let found = rune(&app).symbols("main.rn", SCRIPT).unwrap();
    let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"init"), "{names:?}");
    assert!(names.contains(&"update"), "{names:?}");
    assert!(
        names.contains(&"speed"),
        "an export is not a symbol: {names:?}"
    );
}

#[test]
fn references_reach_the_files_a_mod_declaration_names() {
    let (_dir, app) = host();
    let found = rune(&app).references("main.rn", SCRIPT, "assist");
    let files: Vec<&str> = found.iter().map(|l| l.file.as_str()).collect();
    assert!(files.contains(&"main.rn"), "{files:?}");
    assert!(files.contains(&"helper.rn"), "{files:?}");
}

#[test]
fn formatting_is_idempotent() {
    let (_dir, app) = host();
    let host = rune(&app);
    let once = host.format("main.rn", SCRIPT).unwrap();
    let twice = host.format("main.rn", &once).unwrap();
    assert_eq!(once, twice, "a second format changed the first's output");
}

#[test]
fn renaming_rewrites_every_file_the_name_reaches() {
    let (_dir, app) = host();
    let written = rune(&app)
        .rename("main.rn", SCRIPT, "assist", "help_out")
        .unwrap();
    let files: Vec<&str> = written.iter().map(|(f, _)| f.as_str()).collect();
    assert!(files.contains(&"helper.rn"), "{files:?}");
    for (file, text) in &written {
        assert!(text.contains("help_out"), "{file} was not rewritten");
        assert!(!text.contains("assist"), "{file} still names the old one");
    }
}

#[test]
fn renaming_to_something_that_is_not_an_identifier_is_refused() {
    let (_dir, app) = host();
    let host = rune(&app);
    assert!(
        host.rename("main.rn", SCRIPT, "assist", "help out")
            .is_err()
    );
    assert!(host.rename("main.rn", SCRIPT, "assist", "2fast").is_err());
    assert!(host.rename("main.rn", SCRIPT, "assist", "").is_err());
}

/// Section 4 of the plan: every documented function hovers to its doc line.
///
/// `api_lints.py` fails CI on a function with no doc, so this asserts the
/// popup is as complete as the reference rather than sampling it.
#[test]
fn every_documented_function_hovers_to_its_doc_line() {
    let (_dir, app) = host();
    let host = rune(&app);
    let api: serde_json::Value =
        serde_json::from_str(&balaur::rune::api_json(&host).unwrap()).unwrap();
    let mut checked = 0;
    let mut missing = Vec::new();
    for module in api["modules"].as_array().unwrap() {
        let name = module["name"].as_str().unwrap();
        for (function, doc) in module["docs"].as_object().unwrap() {
            let Some(doc) = doc.as_str().filter(|d| !d.is_empty()) else {
                continue;
            };
            // One line naming the call, and the caret inside the name.
            let source = format!("pub fn init(this) {{\n    {name}::{function}();\n}}\n");
            let column = 5 + name.len() + 2 + 1;
            let found = host.hover("main.rn", &source, 2, column).unwrap();
            match found {
                Some(one) if one.doc == doc => checked += 1,
                _ => missing.push(format!("{name}::{function}")),
            }
        }
    }
    assert!(checked > 600, "only {checked} functions hovered");
    assert!(
        missing.is_empty(),
        "{} documented functions do not hover to their doc: {:?}",
        missing.len(),
        &missing[..missing.len().min(10)]
    );
}

/// Section 4 of the plan: formatting is idempotent over the editor's own
/// scripts, which are the largest body of Rune in the tree.
#[test]
fn formatting_the_editors_own_scripts_is_idempotent() {
    let (_dir, app) = host();
    let host = rune(&app);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../editor/scripts")
        .canonicalize()
        .expect("the editor's scripts are in the tree");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rn") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(&path).unwrap();
        let Ok(once) = host.format(&name, &source) else {
            continue;
        };
        let twice = host.format(&name, &once).unwrap();
        assert_eq!(once, twice, "formatting {name} twice differs from once");
        checked += 1;
    }
    assert!(checked > 30, "only {checked} editor scripts were formatted");
}

/// A completion is offered for every module the engine reports, so a module
/// added by a plugin is reachable without touching the provider.
#[test]
fn every_module_completes_from_a_bare_prefix() {
    let (_dir, app) = host();
    let rune = rune(&app);
    let api: serde_json::Value =
        serde_json::from_str(&balaur::rune::api_json(&rune).unwrap()).unwrap();
    for module in api["modules"].as_array().unwrap() {
        let name = module["name"].as_str().unwrap();
        let found = complete_after(&rune, name);
        assert!(
            found.contains(&name.to_string()),
            "`{name}` does not complete: {found:?}"
        );
    }
}
