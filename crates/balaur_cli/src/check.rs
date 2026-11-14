//! `balaur check`: every finding in a project, with its file and its line.
//!
//! The editor's Problems list, at a prompt and in CI. The findings come from
//! `balaur::check_project_using`; what is printed and what the exit code says
//! about it are decided here.

use crate::vocabulary::words;
use anyhow::Result;

pub(crate) fn project(path: &std::path::Path, strict: bool) -> Result<()> {
    // A project that means to stay clean says so in its own manifest; the
    // flag is for the run that wants it anyway.
    let strict = strict || project_is_strict(path);
    let found = findings(path)?;
    let mut errors = 0;
    let mut warnings = 0;
    for one in &found {
        if one.severity == words::log_levels::ERROR {
            errors += 1;
        } else {
            warnings += 1;
        }
        let at = if one.line > 0 {
            format!("{}:{}:{}", one.file, one.line, one.column)
        } else {
            one.file.clone()
        };
        println!("{at}: {}: {}", one.severity, one.message);
    }
    if errors == 0 && (!strict || warnings == 0) {
        if warnings == 0 {
            println!("no problems");
        }
        return Ok(());
    }
    std::process::exit(1);
}

/// Every finding in the project at `path`, compiled with this binary's own
/// modules: a script of the editor's may call them, any other is refused.
#[cfg(not(target_family = "wasm"))]
fn findings(path: &std::path::Path) -> Result<Vec<balaur::Finding>> {
    balaur::check_project_using(
        path,
        &mut crate::own_modules(path, crate::editor_project(None)),
    )
}

#[cfg(target_family = "wasm")]
fn findings(path: &std::path::Path) -> Result<Vec<balaur::Finding>> {
    balaur::check_project(path)
}

/// Whether `project.toml` asked for `[check] strict = true`.
///
/// A manifest that will not parse reads as no: booting the project is about
/// to say so with the error a person can act on.
fn project_is_strict(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path.join("project.toml"))
        .ok()
        .and_then(|text| balaur_core::project::ProjectManifest::parse(&text).ok())
        .is_some_and(|manifest| manifest.check.strict)
}

/// Who may call what only the editor has: a file of the editor project, or
/// one under a game's or an addon's `editor/` folder, and no other file, in
/// `balaur check` and in the editor's own app alike.
#[cfg(test)]
#[cfg(all(feature = "editor", not(target_family = "wasm")))]
#[cfg(any(feature = "bs", feature = "bs-jit"))]
mod tests {
    use std::path::{Path, PathBuf};

    use balaur::AppConfig;
    use balaur_script::Value::Int;

    use super::words::log_levels::ERROR;

    const ASKS: &str = "pub class Asks {
    node: Node,
    scoped: bool,
    ran: int,
}

impl Asks {
    fn init() {
        self.scoped = debugger::scope().is_valid();
        self.ran = 1;
    }
}
";

    const PLUGIN: &str = "pub fn scoped() -> bool {
    debugger::scope().is_valid()
}
";

    const VERBS: &str = "pub class Verbs {
    node: Node,
    read: int,
}

impl Verbs {
    fn init() {
        self.read = dependency::list().len();
    }
}

fn recent() -> int {
    project::recent().len()
}

fn targets() -> int {
    export::targets().len()
}

fn installs() {
    dependency::install(\"\");
}
";

    const REFUSED: &str =
        "is the editor's: a game's runtime does not have it. Call it from a file under `editor/`";

    /// A project at `dir`, named `name`, whose main scene runs `script` on
    /// a node, with `files` beside it.
    fn project(dir: &Path, name: &str, script: &str, files: &[(&str, &str)]) {
        let main = format!(
            "[[nodes]]\nid = \"one\"\nname = \"One\"\nscript = {{ source = \"{script}\" }}\n"
        );
        let manifest = format!("[application]\nname = \"{name}\"\nmain_scene = \"main.toml\"\n");
        let own = [("project.toml", manifest.as_str()), ("main.toml", &main)];
        for (rel, text) in own.iter().chain(files) {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    /// Counts from 1.
    fn line_of(source: &str, needle: &str) -> usize {
        source
            .lines()
            .position(|line| line.contains(needle))
            .unwrap()
            + 1
    }

    #[test]
    fn check_refuses_an_editor_call_in_a_game_script_and_passes_it_under_editor() {
        let dir = tempfile::tempdir().unwrap();
        project(
            dir.path(),
            "fixture-game",
            "scripts/asks.bs",
            &[
                ("scripts/asks.bs", ASKS),
                ("editor/plugin.bs", PLUGIN),
                ("addons/a/editor/p.bs", PLUGIN),
                ("editor/typo.bs", "pub fn typo() -> int {\n    true\n}\n"),
            ],
        );
        let found = super::findings(dir.path()).unwrap();
        let errors: Vec<_> = found.iter().filter(|f| f.severity == ERROR).collect();
        let refused: Vec<_> = errors
            .iter()
            .filter(|f| f.file == "scripts/asks.bs")
            .collect();
        assert_eq!(refused.len(), 1, "{found:?}");
        assert_eq!(refused[0].line, line_of(ASKS, "debugger::scope"));
        assert_eq!(refused[0].message, format!("`debugger::scope` {REFUSED}"));
        // The control: check compiles what sits under `editor/`, and its one
        // error is the file's own mistake.
        assert!(
            errors.iter().any(|f| f.file == "editor/typo.bs"),
            "{found:?}"
        );
        for plugin in ["editor/plugin.bs", "addons/a/editor/p.bs"] {
            assert!(
                !found.iter().any(|f| f.file == plugin),
                "{plugin}: {found:?}"
            );
        }
    }

    #[test]
    fn check_refuses_the_cli_verbs_in_a_game_script_and_keeps_core_s_dependency_reads() {
        let dir = tempfile::tempdir().unwrap();
        project(
            dir.path(),
            "fixture-verbs",
            "scripts/verbs.bs",
            &[("scripts/verbs.bs", VERBS)],
        );
        let found = super::findings(dir.path()).unwrap();
        let mut lines: Vec<(usize, String)> = found
            .iter()
            .filter(|f| f.severity == ERROR)
            .map(|f| (f.line, f.message.clone()))
            .collect();
        lines.sort();
        let expected: Vec<(usize, String)> = [
            ("project::recent", "`project::recent`"),
            ("export::targets", "`export::targets`"),
            ("dependency::install", "`dependency::install`"),
        ]
        .into_iter()
        .map(|(needle, name)| (line_of(VERBS, needle), format!("{name} {REFUSED}")))
        .collect();
        assert_eq!(lines, expected, "`dependency::list` is core's and compiles");
    }

    // The editor's app as `balaur edit <game>` boots it: what Play and the live Problems list
    // compile is what `balaur check` does.
    #[test]
    fn the_editor_s_app_refuses_an_editor_call_in_the_game_it_plays() {
        let editor = tempfile::tempdir().unwrap();
        let game = tempfile::tempdir().unwrap();
        let (editor_root, game_root) = (real(editor.path()), real(game.path()));
        project(
            &editor_root,
            "fixture-editor",
            "scripts/asks.bs",
            &[("scripts/asks.bs", ASKS)],
        );
        project(
            &game_root,
            "fixture-played",
            "scripts/asks.bs",
            &[("scripts/asks.bs", ASKS), ("editor/plugin.bs", PLUGIN)],
        );
        let mut app =
            balaur::standard_app(AppConfig::dev(editor_root.to_string_lossy().as_ref())).unwrap();
        let mut modules = crate::own_modules(&game_root, Some(editor_root.clone()));
        balaur_plugin::load_all(&mut app, &mut modules).unwrap();
        balaur::file_api::add_root(&app.engine, &game_root);
        app.load_project().unwrap();
        let node = {
            let world = app.engine.world();
            balaur::scene::find_node(&world, app.engine.root(), "One").unwrap()
        };
        let scripts = app.engine.script_host().unwrap();
        // The control: the editor's own script calls it and runs.
        assert_eq!(
            scripts.script_field(balaur::node_id_of(node), "ran"),
            Some(Int(1))
        );
        let played = game_root
            .join("scripts/asks.bs")
            .to_string_lossy()
            .into_owned();
        let refused = scripts
            .attach(balaur::node_id_of(node), &played)
            .unwrap_err();
        assert!(
            format!("{refused:#}").contains(&format!("`debugger::scope` {REFUSED}")),
            "{refused:#}"
        );
        let problems = |path: &Path, source: &str| {
            let path = path.to_string_lossy();
            scripts.tools_for(&path).unwrap().check(&path, source)
        };
        let found = problems(&game_root.join("scripts/asks.bs"), ASKS);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].line, line_of(ASKS, "debugger::scope"));
        assert!(problems(&game_root.join("editor/plugin.bs"), PLUGIN).is_empty());
    }

    const USES: &str = "use editor::tool;

pub class Uses {
    node: Node,
    pinged: int,
}

impl Uses {
    fn init() {
        self.pinged = tool::ping();
    }
}
";

    const TOOL: &str = "pub class Tool {
    node: Node,
}

pub fn ping() -> int {
    1
}
";

    const PLUGIN_USES: &str = "use tool;

pub fn pinged() -> int {
    tool::ping()
}
";

    /// A scene running a game's script, and under it the editor's `editor/tool.bs`.
    const TWO_NODES: &str = "[[nodes]]
id = \"uses\"
name = \"Uses\"
script = { source = \"scripts/uses.bs\" }

[[nodes]]
id = \"tool\"
name = \"Tool\"
parent = \"uses\"
script = { source = \"editor/tool.bs\" }
";

    const USED: &str = "`use editor::tool;` reaches `editor/tool.bs`, which is the editor's: a game's runtime does not run the editor's files, so only a file under `editor/` may use it";

    /// The refusal of an attach, after the path: the editor's app names the
    /// game's files by absolute path.
    const ATTACHED: &str = "editor/tool.bs` is the editor's: a game's runtime does not run the editor's files, so a scene may not attach it";

    /// A scene attaching the editor's `editor/tool.bs` and nothing else.
    const ONE_TOOL: &str = "[[nodes]]
id = \"tool\"
name = \"Tool\"
script = { source = \"editor/tool.bs\" }
";

    /// A game whose script uses `editor/tool.bs`, whose scene attaches it
    /// when `attaches`, and whose own plugin uses it too.
    fn reaching(dir: &Path, attaches: bool) {
        let mut files = vec![
            ("scripts/uses.bs", USES),
            ("editor/tool.bs", TOOL),
            ("editor/plugin.bs", PLUGIN_USES),
        ];
        if attaches {
            files.push(("main.toml", TWO_NODES));
        }
        project(dir, "fixture-reach", "scripts/uses.bs", &files);
    }

    #[test]
    fn check_refuses_a_game_s_use_and_a_scene_s_attach_of_an_editor_file() {
        let dir = tempfile::tempdir().unwrap();
        reaching(dir.path(), true);
        let found = super::findings(dir.path()).unwrap();
        let errors: Vec<(&str, usize, &str)> = found
            .iter()
            .filter(|f| f.severity == ERROR)
            .map(|f| (f.file.as_str(), f.line, f.message.as_str()))
            .collect();
        let used = errors.iter().find(|(file, ..)| *file == "scripts/uses.bs");
        assert_eq!(used, Some(&("scripts/uses.bs", 1, USED)), "{found:?}");
        let attached = errors.iter().find(|(file, ..)| *file == "main.toml");
        let line = line_of(TWO_NODES, "editor/tool.bs");
        assert!(
            attached.is_some_and(|(_, at, message)| *at == line && message.contains(ATTACHED)),
            "{found:?}"
        );
        assert_eq!(
            errors.len(),
            2,
            "the editor's own plugin uses it: {found:?}"
        );
    }

    #[test]
    fn an_export_refuses_a_game_s_use_and_a_scene_s_attach_of_an_editor_file() {
        let export = |dir: &Path| {
            let modules = &mut crate::own_modules(dir, crate::editor_project(None));
            format!(
                "{:#}",
                balaur::build_pack_using(dir, false, modules).unwrap_err()
            )
        };
        let attaching = tempfile::tempdir().unwrap();
        reaching(attaching.path(), true);
        let refused = export(attaching.path());
        assert!(refused.contains(ATTACHED), "{refused}");
        let using = tempfile::tempdir().unwrap();
        reaching(using.path(), false);
        let refused = export(using.path());
        assert!(refused.contains(USED), "{refused}");
    }

    // Play builds the game's scene with its scripts held, then attaches
    // them, as the editor's `model.bs` does.
    #[test]
    fn the_editor_s_play_refuses_a_game_s_use_and_a_scene_s_attach_of_an_editor_file() {
        let editor = tempfile::tempdir().unwrap();
        let game = tempfile::tempdir().unwrap();
        let (editor_root, game_root) = (real(editor.path()), real(game.path()));
        project(
            &editor_root,
            "fixture-editor-plays",
            "scripts/asks.bs",
            &[("scripts/asks.bs", ASKS)],
        );
        reaching(&game_root, true);
        let mut app =
            balaur::standard_app(AppConfig::dev(editor_root.to_string_lossy().as_ref())).unwrap();
        let mut modules = crate::own_modules(&game_root, Some(editor_root.clone()));
        balaur_plugin::load_all(&mut app, &mut modules).unwrap();
        balaur::file_api::add_root(&app.engine, &game_root);
        app.load_project().unwrap();
        let scripts = app.engine.script_host().unwrap();
        let uses = game_root
            .join("scripts/uses.bs")
            .to_string_lossy()
            .into_owned();
        let problems = scripts.tools_for(&uses).unwrap().check(&uses, USES);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].line, 1);
        let plugin = game_root
            .join("editor/plugin.bs")
            .to_string_lossy()
            .into_owned();
        let control = scripts
            .tools_for(&plugin)
            .unwrap()
            .check(&plugin, PLUGIN_USES);
        assert!(control.is_empty(), "{control:?}");
        let node = {
            let world = app.engine.world();
            balaur::scene::find_node(&world, app.engine.root(), "One").unwrap()
        };
        let refused = scripts.attach(balaur::node_id_of(node), &uses).unwrap_err();
        let refused = format!("{refused:#}");
        let reaches = format!(
            "`use editor::tool;` reaches `{}`",
            game_root.join("editor/tool.bs").display()
        );
        assert!(refused.contains(&reaches), "{refused}");
        let played = {
            let mut world = app.engine.world_mut();
            balaur::scene::spawn_node(&mut world, "Played", app.engine.root())
        };
        balaur::project::instantiate_scene_from_root(
            &app.engine,
            ONE_TOOL,
            played,
            false,
            &game_root,
        )
        .unwrap();
        let refused = balaur::project::attach_scripts(&app.engine, played).unwrap_err();
        assert!(format!("{refused:#}").contains(ATTACHED), "{refused:#}");
    }

    #[test]
    fn an_export_leaves_the_editor_s_plugins_out_of_the_pack() {
        let dir = tempfile::tempdir().unwrap();
        let lib = "pub fn ping() -> int {\n    1\n}\n";
        project(
            dir.path(),
            "fixture-shipped",
            "scripts/game.bs",
            &[
                ("scripts/game.bs", "pub class Game {\n    node: Node,\n}\n"),
                ("addons/a/lib.bs", lib),
                ("editor/plugin.bs", PLUGIN),
                ("addons/a/editor/p.bs", PLUGIN),
            ],
        );
        let modules = &mut crate::own_modules(dir.path(), crate::editor_project(None));
        let pack = balaur::build_pack_using(dir.path(), false, modules).unwrap();
        let shipped: Vec<&str> = pack.scripts.keys().map(String::as_str).collect();
        assert_eq!(shipped, ["addons/a/lib.bs", "scripts/game.bs"]);
    }

    #[test]
    fn check_compiles_a_script_no_scene_attaches_and_skips_what_ignore_names() {
        let dir = tempfile::tempdir().unwrap();
        let typo = "pub fn helps() -> int {\n    1 +\n}\n";
        let manifest = "[application]\nname = \"fixture-stray\"\nmain_scene = \"main.toml\"\nignore = [\"tools/**\"]\n";
        project(
            dir.path(),
            "fixture-stray",
            "scripts/game.bs",
            &[
                ("project.toml", manifest),
                ("scripts/game.bs", "pub class Game {\n    node: Node,\n}\n"),
                ("scripts/helper.bs", typo),
                ("tools/broken.bs", typo),
            ],
        );
        let found = super::findings(dir.path()).unwrap();
        let errors: Vec<&str> = found
            .iter()
            .filter(|f| f.severity == ERROR)
            .map(|f| f.file.as_str())
            .collect();
        assert_eq!(errors, ["scripts/helper.bs"], "{found:?}");
    }

    /// `dir` as the editor names a folder: every link resolved.
    fn real(dir: &Path) -> PathBuf {
        dir.canonicalize().unwrap()
    }
}
