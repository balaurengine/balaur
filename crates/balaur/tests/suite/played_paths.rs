//! A game another project plays, as the editor plays the one it edits: a
//! relative path its scripts spell means its own project, and the host's
//! scripts keep their own.

use balaur::{App, AppConfig, DEFAULT_FIXED_DT, standard_app};
use balaur_script::Value;

const READER: &str = r#"pub class Reader { node: Node, read: str, listed: str, there: bool, shaped: bool }
impl Reader {
    pub fn init() {
        self.read = fs::read("data/note.txt").or("nil");
        for entry in fs::list("data") {
            self.listed = self.listed + entry.name + ";";
        }
        self.there = fs::exists("data/only.txt");
        fs::write("data/written.txt", "from {self.node.name()}");
        self.node.set_component("collider3d", { "shape": { "kind": physics3d::SHAPE_SPHERE, "radius": 0.5 } });
        self.shaped = true;
    }
}
"#;

fn project(dir: &std::path::Path, name: &str, note: &str, only: bool) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(
        dir.join("project.toml"),
        format!("[application]\nname = \"{name}\"\nmain_scene = \"main.toml\"\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        format!("[[nodes]]\nid = \"r\"\nname = \"{name}\"\nscript = {{ source = \"scripts/reader.bs\" }}\n"),
    )
    .unwrap();
    std::fs::write(dir.join("scripts").join("reader.bs"), READER).unwrap();
    std::fs::write(dir.join("data").join("note.txt"), note).unwrap();
    if only {
        std::fs::write(dir.join("data").join("only.txt"), "").unwrap();
    }
}

fn field(app: &App, path: &str, name: &str) -> Option<Value> {
    let world = app.engine.world();
    let at = balaur_core::scene::find_node(&world, app.engine.root(), path)?;
    drop(world);
    app.engine
        .script_host()?
        .script_field(balaur::node_id_of(at), name)
}

#[test]
fn a_played_games_relative_paths_mean_its_own_project() {
    let host_dir = tempfile::tempdir().unwrap();
    let game_dir = tempfile::tempdir().unwrap();
    project(host_dir.path(), "Host", "the host's", false);
    project(game_dir.path(), "Game", "the game's", true);
    let mut config = AppConfig::dev(host_dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    balaur::file_api::add_root(&app.engine, game_dir.path());
    app.load_project().unwrap();
    let scene = std::fs::read_to_string(game_dir.path().join("main.toml")).unwrap();
    let root = app.engine.root();
    balaur::project::instantiate_scene_from_root(&app.engine, &scene, root, true, game_dir.path())
        .unwrap();
    app.tick(DEFAULT_FIXED_DT);

    let read = |path: &str, name: &str| field(&app, path, name);
    assert_eq!(read("Host", "read"), Some(Value::Str("the host's".into())));
    assert_eq!(read("Host", "there"), Some(Value::Bool(false)));
    assert_eq!(read("Game", "read"), Some(Value::Str("the game's".into())));
    assert_eq!(
        read("Game", "listed"),
        Some(Value::Str("note.txt;only.txt;".into()))
    );
    assert_eq!(read("Game", "there"), Some(Value::Bool(true)));
    assert_eq!(
        read("Game", "shaped"),
        Some(Value::Bool(true)),
        "an inline asset's `#` reference is the game's spelling already"
    );
    let written = |dir: &std::path::Path| {
        std::fs::read_to_string(dir.join("data").join("written.txt")).unwrap_or_default()
    };
    assert_eq!(written(game_dir.path()), "from Game");
    assert_eq!(written(host_dir.path()), "from Host");
}
