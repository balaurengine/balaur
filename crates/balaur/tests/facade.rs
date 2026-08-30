//! The batteries-included facade: backend selection, pack building, and
//! booting a project. This is the entry point a game actually uses.

use balaur::{standard_app, AppConfig};

fn project(dir: &std::path::Path, language: Option<&str>, script: (&str, &str)) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let lang = language.map_or(String::new(), |l| format!("language = \"{l}\"\n"));
    std::fs::write(
        dir.join("project.toml"),
        format!("name = \"t\"\nmain_scene = \"main.toml\"\n{lang}"),
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        format!(
            "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = \"scripts/{}\"\n",
            script.0
        ),
    )
    .unwrap();
    std::fs::write(dir.join("scripts").join(script.0), script.1).unwrap();
}

const LUAU: (&str, &str) = (
    "s.luau",
    "local S = {}\nfunction S:init() _G.ran = true end\nreturn S\n",
);
const RUNE: (&str, &str) = ("s.rn", "pub fn init(this) { this.ran = true; }\n");

#[test]
fn a_project_without_a_language_runs_on_luau() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, LUAU);
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().expect("a backend was installed");
    assert!(host
        .as_any()
        .downcast_ref::<balaur::luau::ScriptHost>()
        .is_some());
    assert_eq!(
        host.instance_count(),
        1,
        "the scene's script did not attach"
    );
}

#[test]
fn language_rune_runs_on_rune() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), Some("rune"), RUNE);
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().unwrap();
    assert!(host
        .as_any()
        .downcast_ref::<balaur::rune::RuneHost>()
        .is_some());
    assert_eq!(host.instance_count(), 1);
}

/// A language this build does not have must say so by name rather than
/// silently falling back to the default and behaving strangely.
#[test]
fn an_unknown_language_is_a_named_error() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), Some("brainfuck"), LUAU);
    let Err(err) = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())) else {
        panic!("an unknown language was accepted");
    };
    let text = format!("{err:#}");
    assert!(
        text.contains("brainfuck"),
        "does not name the language: {text}"
    );
    assert!(
        text.contains("luau") && text.contains("rune"),
        "does not say what is available"
    );
}

#[test]
fn the_standard_app_has_every_plugin_registered() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, LUAU);
    let app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    let names = balaur_core::components::names(&app.engine);
    for expected in ["body", "collider", "body2d", "shape", "color", "widget"] {
        assert!(
            names.contains(&expected.to_string()),
            "`{expected}` is missing"
        );
    }
}

#[test]
fn build_pack_uses_the_compiler_the_project_asks_for() {
    for (language, script) in [(None, LUAU), (Some("rune"), RUNE)] {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), language, script);
        let pack = balaur::build_pack(dir.path()).unwrap();
        let key = format!("scripts/{}", script.0);
        assert!(
            pack.scripts.contains_key(&key),
            "{language:?}: {key} is not in the pack"
        );
        assert!(pack.scenes.contains_key("main.toml"));
    }
}

/// A pack must run with the sources gone; that is what shipping one means.
#[test]
fn a_packed_project_boots_without_its_sources() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, LUAU);
    let bytes = balaur::build_pack(dir.path()).unwrap().encode();
    drop(dir);

    let pack = balaur_core::Pack::decode(&bytes).unwrap();
    let mut app = standard_app(AppConfig::packed(pack)).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(app.engine.script_host().unwrap().instance_count(), 1);
}

#[test]
fn a_project_with_no_manifest_fails_to_load() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    assert!(app.load_project().is_err());
}

/// Rune resolves module paths while compiling, so exporting it through a bare
/// `rune::Context` rejected every script that touched the engine — the whole
/// language could not be exported, while `balaur run` on the same project was
/// fine. The script here deliberately calls a plugin module; one that only did
/// arithmetic would have passed even with the bug.
#[test]
fn a_rune_project_that_calls_the_engine_can_be_exported() {
    let dir = tempfile::tempdir().unwrap();
    project(
        dir.path(),
        Some("rune"),
        (
            "s.rn",
            "pub fn init(this) {\n\
             \x20   if input::just_pressed(input::KEY_SPACE) {\n\
             \x20       this.ran = true;\n\
             \x20   }\n\
             }\n",
        ),
    );
    let pack = balaur::build_pack(dir.path()).unwrap();
    assert!(pack.scripts.contains_key("scripts/s.rn"));
}

/// Export still has to reject a script that cannot compile: the fix above
/// widened the context, it did not switch the check off.
#[test]
fn a_broken_rune_script_fails_the_export() {
    let dir = tempfile::tempdir().unwrap();
    project(
        dir.path(),
        Some("rune"),
        ("s.rn", "pub fn init(this) { nonesuch::gone(); }\n"),
    );
    let err = balaur::build_pack(dir.path()).unwrap_err().to_string();
    assert!(
        err.contains("s.rn"),
        "error does not name the script: {err}"
    );
}

/// Attaching a script at run time, and reading back what a node is running.
/// These need a live backend, so they cannot be tested from core.
#[test]
fn a_script_can_attach_another_script_and_read_it_back() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, LUAU);
    std::fs::write(
        dir.path().join("scripts/other.luau"),
        "local O = {}\nfunction O:init() _G.other_ran = true end\nreturn O\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.luau"),
        r#"
        local S = {}
        function S:init()
            local kid = self.node:add_child("Kid")
            assert(kid:script_path() == nil, "a fresh node claims a script")
            kid:attach_script("scripts/other.luau")
            _G.kid_path = kid:script_path()
        end
        return S
        "#,
    )
    .unwrap();

    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();

    let lua = balaur::luau::lua_of(&app.engine);
    let ran: bool = lua.globals().get("other_ran").unwrap_or(false);
    let path: String = lua.globals().get("kid_path").unwrap_or_default();
    assert!(ran, "the attached script's init never ran");
    assert_eq!(
        path, "scripts/other.luau",
        "script_path did not report the attachment"
    );
    assert_eq!(app.engine.script_host().unwrap().instance_count(), 2);
}

/// `engine.reload_script` is what a tool calls after writing a file outside
/// the watched root; it must pick up the new source.
#[test]
fn reload_script_picks_up_a_rewritten_file() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, LUAU);
    std::fs::write(
        dir.path().join("scripts/s.luau"),
        "local S = {}\nfunction S:init() _G.version = 1 end\nreturn S\n",
    )
    .unwrap();

    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let lua = balaur::luau::lua_of(&app.engine);
    assert_eq!(lua.globals().get::<i64>("version").unwrap_or(0), 1);

    std::fs::write(
        dir.path().join("scripts/s.luau"),
        "local S = {}\nfunction S:init() _G.version = 2 end\nfunction S:hot_reload() _G.version = 2 end\nreturn S\n",
    )
    .unwrap();
    lua.load("engine.reload_script('scripts/s.luau')")
        .exec()
        .unwrap();
    app.tick(1.0 / 60.0);

    assert_eq!(
        lua.globals().get::<i64>("version").unwrap_or(0),
        2,
        "the reload did not take"
    );
}

/// The mouse position is readable headless; it is simply the origin, because
/// nothing has moved it.
#[test]
fn mouse_position_is_readable_without_a_window() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, LUAU);
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let lua = balaur::luau::lua_of(&app.engine);
    lua.load(
        "local x, y = input.mouse_position()\n\
         assert(type(x) == 'number' and type(y) == 'number')\n\
         local dx, dy = input.scroll_delta()\n\
         assert(dx == 0 and dy == 0)",
    )
    .exec()
    .unwrap();
}
