//! A script's `pub` field typed as another script's class, set by path from a
//! scene: it holds the script running on that node, not a copy of one.

use balaur::{App, AppConfig, standard_app};
use balaur_script::Value;

const SETTINGS: &str = r"
pub class Settings {
    node: Node,
    pub volume: float = 0.5,
    pub step: float = 0.1,
}
impl Settings {
    pub fn louder() -> float {
        self.volume += self.step;
        self.volume
    }
}
";

const LEVEL: &str = r"
use settings;

pub class Level {
    node: Node,
    pub settings: settings::Settings?,
    pub seen: float,
}
impl Level {
    fn init() {
        if self.settings != nil { self.seen = self.settings.unwrap().volume; }
    }
    pub fn louder() -> float {
        if self.settings == nil { return -1.0; }
        self.settings.unwrap().louder()
    }
    pub fn holds() -> bool { self.settings != nil }
}
";

const OTHER: &str = "pub class Other { node: Node }\n";

/// The level listed before the node it names, so the field is pointed only
/// once every script of the scene exists.
fn project(dir: &std::path::Path, target: &str) {
    let scene = format!(
        "[[nodes]]\nid = \"game\"\nname = \"Game\"\n\n\
         [[nodes]]\nid = \"level\"\nname = \"Level\"\nparent = \"game\"\n\n\
         [nodes.script]\nsource = \"scripts/level.bs\"\n\n\
         [nodes.script.props]\nsettings = \"{target}\"\n\n\
         [[nodes]]\nid = \"settings\"\nname = \"Settings\"\nparent = \"game\"\n\
         script = {{ source = \"scripts/settings.bs\" }}\n\n\
         [[nodes]]\nid = \"other\"\nname = \"Other\"\nparent = \"game\"\n\
         script = {{ source = \"scripts/other.bs\" }}\n"
    );
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
        ),
        ("main.toml", scene.as_str()),
        ("scripts/settings.bs", SETTINGS),
        ("scripts/level.bs", LEVEL),
        ("scripts/other.bs", OTHER),
    ] {
        let at = dir.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
}

fn booted(target: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), target);
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn id(app: &App, path: &str) -> balaur_script::NodeId {
    let world = app.engine.world();
    let found = balaur_core::scene::find_node(&world, app.engine.root(), path)
        .unwrap_or_else(|| panic!("the scene has no {path}"));
    balaur::node_id_of(found)
}

fn call(app: &App, method: &str) -> Value {
    app.engine
        .script_host()
        .unwrap()
        .call_on(id(app, "Game/Level"), method, &[])
        .unwrap_or_else(|| panic!("the level answers `{method}`"))
}

fn field(app: &App, path: &str, name: &str) -> Option<Value> {
    app.engine
        .script_host()
        .unwrap()
        .script_field(id(app, path), name)
}

fn warnings() -> Vec<String> {
    balaur_core::logbuf::heard()
        .into_iter()
        // A build with native code says which units it lacks; that is no field's.
        .filter(|e| {
            e.level == "warn"
                && e.message.contains("level.bs")
                && !e.message.contains("native build")
        })
        .map(|e| e.message)
        .collect()
}

#[test]
fn a_class_field_set_by_path_holds_that_nodes_script_itself() {
    balaur_core::logbuf::capture_for_test();
    let (_dir, app) = booted("../Settings");
    assert_eq!(
        field(&app, "Game/Level", "seen"),
        Some(Value::Num(0.5)),
        "init read the settings' own default, through the field"
    );
    assert_eq!(call(&app, "louder"), Value::Num(0.6));
    assert_eq!(
        field(&app, "Game/Settings", "volume"),
        Some(Value::Num(0.6)),
        "the call moved the settings node's own object"
    );
    assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn a_class_field_naming_a_node_of_another_script_is_nil_and_warns() {
    balaur_core::logbuf::capture_for_test();
    let (_dir, app) = booted("../Other");
    assert_eq!(call(&app, "holds"), Value::Bool(false));
    let said = warnings();
    assert!(
        said.iter()
            .any(|w| w.contains("'settings'") && w.contains("runs no")),
        "the warning names the field: {said:#?}"
    );
}

#[test]
fn freeing_the_node_a_class_field_holds_leaves_it_nil() {
    let (_dir, app) = booted("../Settings");
    assert_eq!(call(&app, "holds"), Value::Bool(true), "the control");
    let settings = balaur_core::entity_of(id(&app, "Game/Settings")).unwrap();
    balaur_core::scene::free_node(&app.engine, settings);
    assert_eq!(call(&app, "holds"), Value::Bool(false));
    assert_eq!(call(&app, "louder"), Value::Num(-1.0));
}

#[test]
fn a_restore_leaves_a_class_field_on_the_object_running() {
    let (_dir, app) = booted("../Settings");
    let taken = balaur_core::snapshot::capture(&app.engine);
    assert_eq!(call(&app, "louder"), Value::Num(0.6));
    balaur_core::snapshot::restore(&app.engine, &taken);
    assert_eq!(
        field(&app, "Game/Settings", "volume"),
        Some(Value::Num(0.5))
    );
    assert_eq!(call(&app, "louder"), Value::Num(0.6), "through the field");
    assert_eq!(
        field(&app, "Game/Settings", "volume"),
        Some(Value::Num(0.6)),
        "still the settings node's own object"
    );
}

#[test]
fn a_reload_of_the_held_script_points_the_field_at_its_new_object() {
    let (dir, app) = booted("../Settings");
    let edited = SETTINGS.replace(
        "self.volume += self.step;",
        "self.volume += self.step * 10.0;",
    );
    std::fs::write(dir.path().join("scripts/settings.bs"), edited).unwrap();
    let host = app.engine.script_host().unwrap();
    host.reload(&["scripts/settings.bs"]).unwrap();
    assert_eq!(call(&app, "louder"), Value::Num(1.5), "the new code ran");
    assert_eq!(
        field(&app, "Game/Settings", "volume"),
        Some(Value::Num(1.5)),
        "on the object the node runs now"
    );
}
