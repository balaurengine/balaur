//! A script's `pub` field of type `Node`, set by path from a scene: resolved
//! from the scripted node as `node.get_node` resolves one, and loud when the
//! path names nothing.

use balaur::{App, AppConfig, standard_app};
use balaur_script::Value;

const AIMER: &str = r"
pub class Aimer { node: Node, pub target: Node, pub marks: [Node] }
impl Aimer {
    pub fn aimed() -> bool { self.target.is_valid() }
}
";

/// The scene, with `props` as the turret's `[nodes.script.props]` lines.
fn scene(props: &str) -> String {
    format!(
        "[[nodes]]\nid = \"world\"\nname = \"World\"\n\n\
         [[nodes]]\nid = \"enemy\"\nname = \"Enemy\"\nparent = \"world\"\n\n\
         [[nodes]]\nid = \"flag\"\nname = \"Flag\"\nparent = \"world\"\n\n\
         [[nodes]]\nid = \"turret\"\nname = \"Turret\"\nparent = \"world\"\n\n\
         [nodes.script]\nsource = \"scripts/aimer.bs\"\n\n\
         [nodes.script.props]\n{props}\n"
    )
}

fn booted(dir: &std::path::Path, props: &str) -> App {
    let scene = scene(props);
    for (path, text) in [
        (
            "project.toml",
            "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
        ),
        ("main.toml", scene.as_str()),
        ("scripts/aimer.bs", AIMER),
    ] {
        let at = dir.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    let mut config = AppConfig::dev(dir.to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    app
}

fn node(app: &App, path: &str) -> Value {
    let world = app.engine.world();
    let found = balaur_core::scene::find_node(&world, app.engine.root(), path)
        .unwrap_or_else(|| panic!("the scene has no {path}"));
    Value::Node(balaur::node_id_of(found).0)
}

fn turret(app: &App) -> balaur_script::NodeId {
    match node(app, "World/Turret") {
        Value::Node(id) => balaur_script::NodeId(id),
        _ => unreachable!(),
    }
}

fn aimed(app: &App) -> Value {
    app.engine
        .script_host()
        .unwrap()
        .call_on(turret(app), "aimed", &[])
        .expect("the aimer answers `aimed`")
}

/// What this thread warned of the aimer's properties.
fn warnings() -> Vec<String> {
    balaur_core::logbuf::heard()
        .into_iter()
        .filter(|e| {
            e.level == "warn" && e.message.contains("aimer.bs") && e.message.contains("property")
        })
        .map(|e| e.message)
        .collect()
}

#[test]
fn a_node_field_set_by_path_holds_the_node_the_path_names_from_its_own() {
    balaur_core::logbuf::capture_for_test();
    let dir = tempfile::tempdir().unwrap();
    let app = booted(
        dir.path(),
        "target = \"../Enemy\"\nmarks = [\"../Flag\", \"../Enemy\"]",
    );
    let host = app.engine.script_host().unwrap();
    assert_eq!(aimed(&app), Value::Bool(true), "the script ran and aims");
    assert_eq!(
        host.script_field(turret(&app), "target"),
        Some(node(&app, "World/Enemy"))
    );
    assert_eq!(
        host.script_field(turret(&app), "marks"),
        Some(Value::List(vec![
            node(&app, "World/Flag"),
            node(&app, "World/Enemy")
        ]))
    );
    assert!(warnings().is_empty(), "{:#?}", warnings());
}

#[test]
fn a_node_field_whose_path_names_nothing_holds_no_node_and_warns() {
    balaur_core::logbuf::capture_for_test();
    let dir = tempfile::tempdir().unwrap();
    let app = booted(dir.path(), "target = \"../Enmy\"");
    assert_eq!(aimed(&app), Value::Bool(false));
    let said = warnings();
    assert!(
        said.iter()
            .any(|line| line.contains("target") && line.contains("../Enmy")),
        "the warning names the field and the path: {said:#?}"
    );
}
