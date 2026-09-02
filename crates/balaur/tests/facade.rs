//! The batteries-included facade: backend selection, pack building, and
//! booting a project. This is the entry point a game actually uses.

use balaur::{standard_app, App, AppConfig};

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

const RUNE: (&str, &str) = ("s.rn", "pub fn init(this) { this.ran = true; }\n");

/// The scene's `Root`, the node the project's script is attached to.
fn root_node(app: &App) -> balaur::hecs::Entity {
    let world = app.engine.world();
    balaur::scene::find_node(&world, app.engine.root(), "Root").expect("the scene has a Root")
}

#[test]
fn a_project_without_a_language_runs_on_rune() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, RUNE);
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().expect("a backend was installed");
    assert!(host
        .as_any()
        .downcast_ref::<balaur::rune::RuneHost>()
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

#[test]
fn an_unknown_language_is_a_named_error() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), Some("brainfuck"), RUNE);
    let Err(err) = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())) else {
        panic!("an unknown language was accepted");
    };
    let text = format!("{err:#}");
    assert!(
        text.contains("brainfuck"),
        "does not name the language: {text}"
    );
    assert!(text.contains("rune"), "does not say what is available");
}

#[test]
fn the_standard_app_has_every_plugin_registered() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, RUNE);
    let app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    let names = balaur_core::components::names(&app.engine);
    for expected in [
        "body3d",
        "collider3d",
        "body2d",
        "shape3d",
        "widget",
        "animation",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "`{expected}` is missing"
        );
    }
}

#[test]
fn build_pack_compiles_with_or_without_a_language_line() {
    for language in [None, Some("rune")] {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), language, RUNE);
        let pack = balaur::build_pack(dir.path()).unwrap();
        let key = format!("scripts/{}", RUNE.0);
        assert!(
            pack.scripts.contains_key(&key),
            "{language:?}: {key} is not in the pack"
        );
        assert!(pack.scenes.contains_key("main.toml"));
    }
}

#[test]
fn a_packed_project_boots_without_its_sources() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, RUNE);
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

#[test]
fn a_script_can_attach_another_script_and_read_it_back() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, RUNE);
    std::fs::write(
        dir.path().join("scripts/other.rn"),
        "pub fn init(this) { this.other_ran = 1; }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        r#"
        pub fn init(this) {
            let kid = this.node.add_child("Kid");
            assert!(kid.script_path() is Tuple, "a fresh node claims a script");
            kid.attach_script("scripts/other.rn");
            this.kid_path = kid.script_path();
        }
        "#,
    )
    .unwrap();

    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();

    let rune = balaur::rune::rune_of(&app.engine);
    let root = root_node(&app);
    let kid = {
        let world = app.engine.world();
        balaur::scene::find_node(&world, app.engine.root(), "Root/Kid")
            .expect("the script added Kid")
    };
    assert_eq!(
        rune.number_field(kid, "other_ran"),
        Some(1.0),
        "the attached script's init never ran"
    );
    assert_eq!(
        rune.text_field(root, "kid_path").as_deref(),
        Some("scripts/other.rn"),
        "script_path did not report the attachment"
    );
    assert_eq!(app.engine.script_host().unwrap().instance_count(), 2);
}

#[test]
fn reload_script_picks_up_a_rewritten_file() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, RUNE);
    // `reload` asks through the script API, the way an editor would.
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        "pub fn init(this) { this.version = 1; }\n\
         pub fn update(this, dt) {}\n\
         pub fn reload(this) { engine::reload_script(\"scripts/s.rn\"); }\n",
    )
    .unwrap();

    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let rune = balaur::rune::rune_of(&app.engine);
    let root = root_node(&app);
    assert_eq!(rune.number_field(root, "version"), Some(1.0));

    std::fs::write(
        dir.path().join("scripts/s.rn"),
        "pub fn init(this) { this.version = 2; }\n\
         pub fn update(this, dt) { this.version = 2; }\n\
         pub fn reload(this) { engine::reload_script(\"scripts/s.rn\"); }\n",
    )
    .unwrap();
    app.engine
        .script_host()
        .unwrap()
        .call_on(balaur::node_id_of(root), "reload", &[]);
    app.tick(1.0 / 60.0);

    assert_eq!(
        rune.number_field(root, "version"),
        Some(2.0),
        "the reload did not take"
    );
}

#[test]
fn mouse_position_is_readable_without_a_window() {
    let dir = tempfile::tempdir().unwrap();
    project(
        dir.path(),
        None,
        (
            "s.rn",
            "pub fn init(this) {\n\
             \x20   let (x, y) = input::mouse_position();\n\
             \x20   assert!(x is f64 && y is f64);\n\
             \x20   let (dx, dy) = input::scroll_delta();\n\
             \x20   assert!(dx == 0.0 && dy == 0.0);\n\
             \x20   this.done = 1;\n\
             }\n",
        ),
    );
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    assert_eq!(
        balaur::rune::rune_of(&app.engine).number_field(root_node(&app), "done"),
        Some(1.0),
        "the script did not run to its end"
    );
}
