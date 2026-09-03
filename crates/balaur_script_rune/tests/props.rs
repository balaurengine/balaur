//! Exported script properties: what a script declares, what a scene sets on
//! one node, and what `init` finds already written on `this`.

use balaur_core::{App, AppConfig};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.toml"), "[project]\nname = \"t\"\n").unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn app_in(dir: &std::path::Path) -> App {
    App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap()
}

/// A script whose `init` copies every export onto a field the test can read
/// back through the host, so the assertions are about what `init` saw.
const ENEMY: &str = "pub fn exports() {\n\
     #{ speed: 2.0, jumps: 2, name: \"grunt\" }\n\
 }\n\
 pub fn init(this) {\n\
     this.seen_speed = this.speed;\n\
     this.seen_jumps = this.jumps;\n\
     this.seen_name = this.name;\n\
 }\n";

/// Read one field off a node's live instance. The concrete host owns the
/// readers, and the `Rc` has to outlive the borrow, so each is its own call.
fn number(app: &App, node: hecs::Entity, field: &str) -> Option<f64> {
    let host = app.engine.script_host().unwrap();
    rune(&host).number_field(node, field)
}

fn text(app: &App, node: hecs::Entity, field: &str) -> Option<String> {
    let host = app.engine.script_host().unwrap();
    rune(&host).text_field(node, field)
}

fn rune(
    host: &std::rc::Rc<dyn balaur_script::ScriptHost<balaur_core::Engine>>,
) -> &balaur_script_rune::RuneHost {
    host.as_any()
        .downcast_ref::<balaur_script_rune::RuneHost>()
        .expect("the app is running Rune")
}

fn node_named(app: &App, name: &str) -> hecs::Entity {
    balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), name)
        .unwrap_or_else(|| panic!("no node named {name}"))
}

fn build(scene: &str, script: &str) -> (tempfile::TempDir, App) {
    let dir = project(&[("scripts/enemy.rn", script)]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(&app.engine, scene, root, true).unwrap();
    (dir, app)
}

#[test]
fn a_scene_property_is_on_this_before_init_runs() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Enemy\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speed = 3.5, name = \"brute\" } }\n",
        ENEMY,
    );
    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen_speed"), Some(3.5));
    assert_eq!(text(&app, enemy, "seen_name"), Some(String::from("brute")));
    // Untouched by the scene, so the export's own default is what init read.
    assert_eq!(number(&app, enemy, "seen_jumps"), Some(2.0));
}

#[test]
fn the_string_form_of_the_script_key_still_attaches() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Enemy\"\n\
         script = \"scripts/enemy.rn\"\n",
        ENEMY,
    );
    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen_speed"), Some(2.0));
}

#[test]
fn two_nodes_on_one_script_get_their_own_values() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Fast\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speed = 9.0 } }\n\
         \n\
         [[nodes]]\n\
         name = \"Slow\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speed = 0.5 } }\n",
        ENEMY,
    );
    let (fast, slow) = (node_named(&app, "Fast"), node_named(&app, "Slow"));
    assert_eq!(number(&app, fast, "seen_speed"), Some(9.0));
    assert_eq!(number(&app, slow, "seen_speed"), Some(0.5));
}

/// A property set on a node the script does not export is a typo, and the
/// warning is the only thing that says so — but dropping the value would
/// lose an edit, so it is still written.
#[test]
fn a_property_the_script_does_not_export_is_still_written() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Enemy\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speeed = 3.5 } }\n",
        "pub fn exports() { #{ speed: 2.0 } }\n\
         pub fn init(this) { this.seen = this.speeed; }\n",
    );
    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen"), Some(3.5));
}

#[test]
fn a_script_without_exports_takes_properties_anyway() {
    let (_dir, app) = build(
        "[[nodes]]\n\
         name = \"Enemy\"\n\
         script = { source = \"scripts/enemy.rn\", props = { speed = 7.0 } }\n",
        "pub fn init(this) { this.seen = this.speed; }\n",
    );
    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen"), Some(7.0));
}

#[test]
fn exports_reports_the_declared_defaults_at_their_own_types() {
    let dir = project(&[("scripts/enemy.rn", ENEMY)]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let declared = host.exports("scripts/enemy.rn").unwrap();
    assert_eq!(
        declared,
        vec![
            (String::from("jumps"), balaur_script::Value::Int(2)),
            (
                String::from("name"),
                balaur_script::Value::Str("grunt".into())
            ),
            (String::from("speed"), balaur_script::Value::Num(2.0)),
        ],
        "sorted by name, with an int default staying an int"
    );
}

#[test]
fn a_script_without_exports_declares_nothing() {
    let dir = project(&[("scripts/bare.rn", "pub fn init(this) { }\n")]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    assert!(host.exports("scripts/bare.rn").unwrap().is_empty());
}

#[test]
fn exports_returning_something_other_than_an_object_is_an_error() {
    let dir = project(&[("scripts/bad.rn", "pub fn exports() { 3 }\n")]);
    let app = app_in(dir.path());
    let host = app.engine.script_host().unwrap();
    let err = host.exports("scripts/bad.rn").unwrap_err().to_string();
    assert!(err.contains("object of defaults"), "{err}");
}

/// A shipped game reads its properties out of the pack, not off disk. Scenes
/// travel as their own source, so this is really asking whether the packed
/// boot path takes the same `script` key the dev one does.
#[test]
fn a_packed_game_boots_with_the_properties_its_scene_set() {
    let dir = project(&[
        ("project.toml", "name = \"t\"\nmain_scene = \"main.toml\"\n"),
        ("scripts/enemy.rn", ENEMY),
        (
            "main.toml",
            "[[nodes]]\n\
             name = \"Enemy\"\n\
             script = { source = \"scripts/enemy.rn\", props = { speed = 4.25 } }\n",
        ),
    ]);
    // The host is the compiler: a script is compiled against the modules the
    // game will actually have.
    let built = app_in(dir.path());
    let compiler = built.engine.script_host().unwrap();
    let pack = balaur_core::Pack::build(dir.path(), rune(&compiler)).unwrap();
    let pack = balaur_core::Pack::decode(&pack.encode()).unwrap();
    drop(built);

    let mut app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: Some(pack),
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap();
    app.load_project().unwrap();

    let enemy = node_named(&app, "Enemy");
    assert_eq!(number(&app, enemy, "seen_speed"), Some(4.25));
    assert_eq!(number(&app, enemy, "seen_jumps"), Some(2.0));
}

/// A prefab's scripts attach once the whole outer tree exists, with the
/// properties the prefab set — and an override on the instance reaches the
/// node inside it.
#[test]
fn scripts_inside_a_prefab_attach_with_their_properties() {
    let dir = project(&[
        ("scripts/enemy.rn", ENEMY),
        (
            "scenes/enemy.toml",
            "[[nodes]]\n\
             id = \"n_body\"\n\
             name = \"Body\"\n\
             script = { source = \"scripts/enemy.rn\", props = { speed = 1.5 } }\n",
        ),
    ]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        "[[nodes]]\n\
         id = \"n_left\"\n\
         name = \"Left\"\n\
         instance = \"scenes/enemy.toml\"\n\
         \n\
         [[nodes]]\n\
         id = \"n_right\"\n\
         name = \"Right\"\n\
         instance = \"scenes/enemy.toml\"\n",
        root,
        true,
    )
    .unwrap();

    for name in ["Left/Body", "Right/Body"] {
        let node = node_named(&app, name);
        assert_eq!(number(&app, node, "seen_speed"), Some(1.5), "{name}");
        assert_eq!(number(&app, node, "seen_jumps"), Some(2.0), "{name}");
    }
}

/// Prefabs travel in the pack as their own scene files, so a shipped game
/// resolves `instance` the way a dev run does.
#[test]
fn a_packed_game_builds_its_prefabs() {
    let dir = project(&[
        ("project.toml", "name = \"t\"\nmain_scene = \"main.toml\"\n"),
        ("scripts/enemy.rn", ENEMY),
        (
            "scenes/enemy.toml",
            "[[nodes]]\n\
             id = \"n_body\"\n\
             name = \"Body\"\n\
             script = { source = \"scripts/enemy.rn\", props = { speed = 6.5 } }\n",
        ),
        (
            "main.toml",
            "[[nodes]]\n\
             id = \"n_enemy\"\n\
             name = \"Enemy\"\n\
             instance = \"scenes/enemy.toml\"\n",
        ),
    ]);
    let built = app_in(dir.path());
    let compiler = built.engine.script_host().unwrap();
    let pack = balaur_core::Pack::build(dir.path(), rune(&compiler)).unwrap();
    let pack = balaur_core::Pack::decode(&pack.encode()).unwrap();
    drop(built);

    let mut app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: Some(pack),
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_rune::factory()),
    })
    .unwrap();
    app.load_project().unwrap();

    let body = node_named(&app, "Enemy/Body");
    assert_eq!(number(&app, body, "seen_speed"), Some(6.5));
}

/// Retuning a prefab from the instance that named it: two enemies from one
/// file, differing only in a property the scene overrode.
#[test]
fn an_override_retunes_a_prefabs_script() {
    let dir = project(&[
        ("scripts/enemy.rn", ENEMY),
        (
            "scenes/enemy.toml",
            "[[nodes]]\n\
             id = \"n_body\"\n\
             name = \"Body\"\n\
             script = { source = \"scripts/enemy.rn\", props = { speed = 1.5 } }\n",
        ),
    ]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        "[[nodes]]\n\
         id = \"n_slow\"\n\
         name = \"Slow\"\n\
         instance = \"scenes/enemy.toml\"\n\
         \n\
         [[nodes]]\n\
         id = \"n_fast\"\n\
         name = \"Fast\"\n\
         instance = \"scenes/enemy.toml\"\n\
         \n\
         [nodes.overrides.\"Body\".script.props]\n\
         speed = 12.0\n",
        root,
        true,
    )
    .unwrap();

    assert_eq!(
        number(&app, node_named(&app, "Slow/Body"), "seen_speed"),
        Some(1.5)
    );
    assert_eq!(
        number(&app, node_named(&app, "Fast/Body"), "seen_speed"),
        Some(12.0)
    );
    // Untouched by the override, so both still take the export's default.
    assert_eq!(
        number(&app, node_named(&app, "Fast/Body"), "seen_jumps"),
        Some(2.0)
    );
}
