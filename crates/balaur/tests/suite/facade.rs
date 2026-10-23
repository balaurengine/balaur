//! The batteries-included facade: backend selection, pack building, and
//! booting a project. This is the entry point a game actually uses.

use balaur::{App, AppConfig, standard_app};

fn project(dir: &std::path::Path, language: Option<&str>, script: (&str, &str)) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let lang = language.map_or(String::new(), |l| format!("script_language = \"{l}\"\n"));
    std::fs::write(
        dir.join("project.toml"),
        format!("[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n{lang}"),
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        format!(
            "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = {{ source = \"scripts/{}\" }}\n",
            script.0
        ),
    )
    .unwrap();
    std::fs::write(dir.join("scripts").join(script.0), script.1).unwrap();
}

const SCRIPT: (&str, &str) = (
    "s.bs",
    "pub struct Root { node: Node, ran: bool }\nimpl Root { fn init() { self.ran = true; } }\n",
);

/// The scene's `Root`, the node the project's script is attached to.
fn root_node(app: &App) -> balaur::hecs::Entity {
    let world = app.engine.world();
    balaur::scene::find_node(&world, app.engine.root(), "Root").expect("the scene has a Root")
}

/// What the script on `node` holds in `name`.
fn field(app: &App, node: balaur::hecs::Entity, name: &str) -> Option<balaur_script::Value> {
    let host = app.engine.script_host().expect("a script host");
    host.script_field(balaur::node_id_of(node), name)
}

#[test]
fn a_project_without_a_language_runs_its_bs_scripts() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    assert!(balaur::script_bs::BsHost::of(&app.engine).is_some());
    let host = app.engine.script_host().expect("a backend was installed");
    assert_eq!(
        host.instance_count(),
        1,
        "the scene's script did not attach"
    );
    assert_eq!(
        field(&app, root_node(&app), "ran"),
        Some(balaur_script::Value::Bool(true))
    );
}

#[test]
fn an_unknown_language_is_a_named_error() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), Some("brainfuck"), SCRIPT);
    let Err(err) = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())) else {
        panic!("an unknown language was accepted");
    };
    let text = format!("{err:#}");
    assert!(
        text.contains("brainfuck"),
        "does not name the language: {text}"
    );
}

#[test]
fn the_standard_app_records_every_plugin_it_loaded() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    let app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    let names: Vec<String> = app.plugins().into_iter().map(|p| p.name).collect();
    for expected in ["input", "physics", "animation", "render", "ui"] {
        assert!(
            names.contains(&expected.to_string()),
            "`{expected}` is missing from {names:?}"
        );
    }
    #[cfg(feature = "http")]
    assert!(names.contains(&"http".to_string()), "{names:?}");
    #[cfg(feature = "audio")]
    assert!(names.contains(&"audio".to_string()), "{names:?}");
}

/// A project whose `project.toml` carries a `[plugins]` table.
fn project_asking(dir: &std::path::Path, plugins: &str) {
    project(dir, None, SCRIPT);
    let manifest = std::fs::read_to_string(dir.join("project.toml")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        format!("{manifest}\n[plugins]\n{plugins}\n"),
    )
    .unwrap();
}

fn booted(dir: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let app = standard_app(AppConfig::dev(dir.to_string_lossy().as_ref()))?;
    Ok(app.plugins().into_iter().map(|p| p.name).collect())
}

#[cfg(feature = "http")]
#[test]
fn a_module_the_project_turns_off_does_not_load() {
    let dir = tempfile::tempdir().unwrap();
    project_asking(dir.path(), "http = false");

    let names = booted(dir.path()).unwrap();
    assert!(!names.contains(&"http".to_string()), "{names:?}");
    assert!(names.contains(&"render".to_string()), "{names:?}");
}

#[cfg(feature = "http")]
#[test]
fn a_table_in_plugins_leaves_the_module_on() {
    let dir = tempfile::tempdir().unwrap();
    project_asking(dir.path(), "http = { timeout_seconds = 5 }");

    assert!(booted(dir.path()).unwrap().contains(&"http".to_string()));
}

#[test]
fn a_table_for_a_plugin_this_build_has_not_got_is_still_an_error() {
    let dir = tempfile::tempdir().unwrap();
    project_asking(dir.path(), "nowhere = { any = 1 }");

    let err = format!("{:#}", booted(dir.path()).unwrap_err());
    assert!(err.contains("nowhere"), "does not name it: {err}");
}

#[cfg(feature = "http")]
#[test]
fn a_module_the_project_asks_for_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    project_asking(dir.path(), "http = true");

    assert!(booted(dir.path()).unwrap().contains(&"http".to_string()));
}

#[test]
fn a_project_that_names_no_plugins_gets_every_module() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);

    let named = booted(dir.path()).unwrap();
    let dir2 = tempfile::tempdir().unwrap();
    project_asking(dir2.path(), "");
    assert_eq!(named, booted(dir2.path()).unwrap());
}

#[test]
fn asking_for_a_plugin_this_build_has_not_got_is_a_named_error() {
    let dir = tempfile::tempdir().unwrap();
    project_asking(dir.path(), "nowhere = true");

    let err = format!("{:#}", booted(dir.path()).unwrap_err());
    assert!(err.contains("nowhere"), "does not name it: {err}");
    assert!(err.contains("--features"), "no way out of it: {err}");
}

#[test]
fn turning_off_a_plugin_this_build_has_not_got_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    project_asking(dir.path(), "nowhere = false");

    booted(dir.path()).expect("the project already has what it asked for");
}

#[test]
fn turning_off_a_plugin_every_build_has_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    project_asking(dir.path(), "render = false");

    let err = format!("{:#}", booted(dir.path()).unwrap_err());
    assert!(err.contains("render"), "does not name it: {err}");
}

#[test]
fn every_plugin_loads_in_name_order() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    let app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    let names: Vec<String> = app.plugins().into_iter().map(|p| p.name).collect();

    // `apple` is pulled up behind `platform` by its requirement; the rest sort.
    let ordered: Vec<&String> = names.iter().filter(|n| *n != "apple").collect();
    let mut sorted = ordered.clone();
    sorted.sort();
    assert_eq!(ordered, sorted, "{names:?}");
}

#[test]
fn the_engines_own_plugins_are_ordered_with_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    let app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    let names: Vec<String> = app.plugins().into_iter().map(|p| p.name).collect();

    for expected in ["animation", "input", "physics", "platform", "render", "ui"] {
        assert!(
            names.contains(&expected.to_string()),
            "`{expected}` is missing from {names:?}"
        );
    }
}

#[test]
fn nothing_loads_before_what_it_requires() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    let app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    let loaded = app.plugins();

    let at = |name: &str| loaded.iter().position(|p| p.name == name);
    assert!(at("platform").is_some(), "{loaded:?}");
    for plugin in &loaded {
        for required in &plugin.requires {
            assert!(
                at(required) < at(&plugin.name),
                "`{}` loaded before `{required}`",
                plugin.name
            );
        }
    }
}

#[test]
fn the_standard_app_has_every_plugin_registered() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
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
fn build_pack_carries_the_scripts_and_scenes() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    let pack = balaur::build_pack(dir.path()).unwrap();
    let key = format!("scripts/{}", SCRIPT.0);
    assert!(pack.scripts.contains_key(&key), "{key} is not in the pack");
    assert!(pack.scenes.contains_key("main.toml"));
}

#[test]
fn a_pack_carries_every_shader_its_game_links() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/shaders");
    let pack = balaur::build_pack(&root).unwrap();
    let bytes = pack
        .assets
        .get(balaur::render::prelinked::PRELINKED)
        .expect("an export links the game's shaders into the pack");
    let table = balaur::render::prelinked::read(bytes).unwrap();
    // The glow material, linked with and without morphing, with its Params.
    let glow = table
        .values()
        .filter(|linked| linked.fields.iter().any(|f| f.name == "rings"))
        .count();
    assert_eq!(glow, 2);
    assert!(table.values().all(|linked| linked.wgsl.contains("fn ")));
}

#[test]
fn a_packed_project_boots_without_its_sources() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
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
fn a_project_whose_script_calls_the_engine_can_be_exported() {
    let dir = tempfile::tempdir().unwrap();
    project(
        dir.path(),
        None,
        (
            "s.bs",
            "pub struct Root { node: Node, ran: bool }\n\
             impl Root {\n\
             \x20   fn init() {\n\
             \x20       if input::key_just_pressed(input::KEY_SPACE) {\n\
             \x20           self.ran = true;\n\
             \x20       }\n\
             \x20   }\n\
             }\n",
        ),
    );
    let pack = balaur::build_pack(dir.path()).unwrap();
    assert!(pack.scripts.contains_key("scripts/s.bs"));
}

#[test]
fn a_script_can_attach_another_script() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    std::fs::write(
        dir.path().join("scripts/other.bs"),
        "pub struct Other { node: Node, other_ran: int }\n\
         impl Other { fn init() { self.other_ran = 1; } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.bs"),
        r#"
        pub struct Root { node: Node }

        impl Root {
            fn init() {
                self.node.add_child("Kid").attach_script("scripts/other.bs");
            }
        }
        "#,
    )
    .unwrap();

    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();

    let kid = {
        let world = app.engine.world();
        balaur::scene::find_node(&world, app.engine.root(), "Root/Kid")
            .expect("the script added Kid")
    };
    assert_eq!(
        field(&app, kid, "other_ran"),
        Some(balaur_script::Value::Int(1)),
        "the attached script's init never ran"
    );
    assert_eq!(app.engine.script_host().unwrap().instance_count(), 2);
}

#[test]
#[ignore = "needs engine: the .bs host records no ScriptAttachment, so script_path answers nil"]
fn script_path_names_the_script_a_node_was_given() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    std::fs::write(
        dir.path().join("scripts/other.bs"),
        "pub struct Other { node: Node }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.bs"),
        r#"
        pub struct Root { node: Node, own_path: str, fresh_path: str, kid_path: str }

        impl Root {
            fn init() {
                self.own_path = self.node.script_path();
                let kid = self.node.add_child("Kid");
                self.fresh_path = kid.script_path();
                kid.attach_script("scripts/other.bs");
                self.kid_path = kid.script_path();
            }
        }
        "#,
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    let root = root_node(&app);
    let text = |s: &str| Some(balaur_script::Value::Str(s.into()));
    assert_eq!(field(&app, root, "own_path"), text("scripts/s.bs"));
    assert_eq!(
        field(&app, root, "fresh_path"),
        text(""),
        "a fresh node claims a script"
    );
    assert_eq!(
        field(&app, root, "kid_path"),
        text("scripts/other.bs"),
        "script_path did not report the attachment"
    );
}

#[test]
fn reload_script_picks_up_a_rewritten_file() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), None, SCRIPT);
    // `reload` asks through the script API from a script of its own, the way
    // an editor would.
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = { source = \"scripts/s.bs\" }\n\n\
         [[nodes]]\nid = \"e\"\nname = \"Editor\"\nparent = \"n\"\nscript = { source = \"scripts/e.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/e.bs"),
        "pub struct Editor { node: Node }\n\
         impl Editor { pub fn reload() { engine::reload_script(\"scripts/s.bs\"); } }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.bs"),
        "pub struct Root { node: Node, version: int }\n\
         impl Root {\n\
             fn init() { self.version = 1; }\n\
             fn update(dt: float) {}\n\
         }\n",
    )
    .unwrap();

    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let root = root_node(&app);
    assert_eq!(
        field(&app, root, "version"),
        Some(balaur_script::Value::Int(1))
    );

    std::fs::write(
        dir.path().join("scripts/s.bs"),
        "pub struct Root { node: Node, version: int }\n\
         impl Root {\n\
             fn init() { self.version = 2; }\n\
             fn update(dt: float) { self.version = 2; }\n\
         }\n",
    )
    .unwrap();
    let editor = {
        let world = app.engine.world();
        balaur::scene::find_node(&world, app.engine.root(), "Root/Editor")
            .expect("the scene has an Editor")
    };
    app.engine
        .script_host()
        .unwrap()
        .call_on(balaur::node_id_of(editor), "reload", &[]);
    app.tick(1.0 / 60.0);

    assert_eq!(
        field(&app, root, "version"),
        Some(balaur_script::Value::Int(2)),
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
            "s.bs",
            "pub struct Root { node: Node, done: int }\n\
             impl Root {\n\
             \x20   fn init() {\n\
             \x20       let at = input::mouse_position();\n\
             \x20       let scrolled = input::scroll_delta();\n\
             \x20       if at.x == at.x && scrolled.x == 0.0 && scrolled.y == 0.0 {\n\
             \x20           self.done = 1;\n\
             \x20       }\n\
             \x20   }\n\
             }\n",
        ),
    );
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    assert_eq!(
        field(&app, root_node(&app), "done"),
        Some(balaur_script::Value::Int(1)),
        "the script did not run to its end"
    );
}

/// A sidecar is a document, and a pack keeps its documents apart from its
/// assets: an image's settings used to be read from the assets alone, so a
/// shipped game sampled every texture at the defaults it was imported to
/// avoid. The size a smaller copy was drawn at rides the same key.
#[test]
fn a_packed_project_reads_its_image_sidecars() {
    const PIXEL: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("art")).unwrap();
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    std::fs::write(
        root.join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scenes/main.toml"),
        "[[nodes]]\nid = \"root\"\nname = \"Root\"\n",
    )
    .unwrap();
    std::fs::write(root.join("art/hero.png"), PIXEL).unwrap();
    std::fs::write(
        root.join("art/hero.png.import.toml"),
        "size = [400, 200]\nsrgb = false\n",
    )
    .unwrap();
    let bytes = balaur::build_pack(root).unwrap().encode();
    drop(dir);

    let pack = balaur_core::Pack::decode(&bytes).unwrap();
    let mut app = standard_app(AppConfig::packed(pack)).unwrap();
    app.load_project().unwrap();
    assert_eq!(
        balaur_core::import::drawn_size(&app.engine, "art/hero.png"),
        Some((400, 200)),
        "the size the picture was drawn at, out of the pack"
    );
    let settings = balaur_core::import::resolved(&app.engine, "art/hero.png");
    assert_eq!(
        settings.settings.get("srgb").and_then(toml::Value::as_bool),
        Some(false),
        "and its sampling, which rides the same sidecar"
    );
}
