//! Godot's autoloads: a node of the main scene, or a module when the script
//! is code alone.

use crate::godot::walk::import_project;

fn node<'a>(scene: &'a toml::Value, name: &str) -> &'a toml::Value {
    scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no node {name} in {scene:#?}"))
}

#[test]
fn a_code_only_autoload_is_a_module_and_a_node_autoload_is_read_by_its_id() {
    let godot = tempfile::tempdir().unwrap();
    let put = |path: &str, text: &str| {
        let file = godot.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    };
    put(
        "project.godot",
        &[
            "config_version=5",
            "[application]",
            "config/name=\"Deck\"",
            "run/main_scene=\"res://scenes/main.tscn\"",
            "[autoload]",
            "Rules=\"*res://scripts/rules.gd\"",
            "Board=\"*res://scripts/board.gd\"",
            "",
        ]
        .join("\n"),
    );
    put(
        "scenes/main.tscn",
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
    );
    put(
        "scripts/rules.gd",
        "extends Node\n\nconst LIMIT = 3\n\nstatic func allowed(n):\n\treturn n < LIMIT\n",
    );
    put(
        "scripts/board.gd",
        "extends Node\n\nsignal moved\nvar seen := 0\n",
    );
    put(
        "scripts/game.gd",
        "extends Node\n\nfunc _ready():\n\tif Rules.allowed(2):\n\t\tBoard.moved.emit()\n",
    );
    let out = tempfile::tempdir().unwrap();
    import_project(&godot.path().join("project.godot"), out.path()).unwrap();
    let main = std::fs::read_to_string(out.path().join("scenes/main.toml")).unwrap();
    assert!(
        !main.contains("name = \"Rules\""),
        "no node for code alone: {main}"
    );
    assert!(
        main.contains("name = \"Board\""),
        "a node for the rest: {main}"
    );
    let game = std::fs::read_to_string(out.path().join("scripts/game.rn")).unwrap();
    assert!(
        game.contains("script::require(\"scripts/rules.rn\")"),
        "the module: {game}"
    );
    assert!(
        game.contains("scene::node_by_id(\"autoload_Board\")"),
        "the node: {game}"
    );
}

#[test]
fn an_autoload_is_the_first_node_under_the_main_scenes_root() {
    let godot = tempfile::tempdir().unwrap();
    let put = |path: &str, text: &str| {
        let file = godot.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    };
    put(
        "project.godot",
        "config_version=5\n\n[application]\n\nconfig/name=\"Deck\"\nrun/main_scene=\"res://scenes/main.tscn\"\n\n[autoload]\n\nBoard=\"*res://scripts/board.gd\"\n",
    );
    put(
        "scenes/main.tscn",
        &[
            "[gd_scene format=3]",
            "",
            "[node name=\"Main\" type=\"Node2D\"]",
            "",
            "[node name=\"Mast\" type=\"Node2D\" parent=\".\"]",
            "",
            "[node name=\"Sail\" type=\"Node2D\" parent=\".\"]",
            "",
            "[connection signal=\"hoisted\" from=\"Mast\" to=\"Sail\" method=\"_on_hoisted\"]",
            "",
        ]
        .join("\n"),
    );
    put("scripts/board.gd", "extends Node\n\nvar seen := 0\n");
    let out = tempfile::tempdir().unwrap();
    import_project(&godot.path().join("project.godot"), out.path()).unwrap();
    let main = std::fs::read_to_string(out.path().join("scenes/main.toml")).unwrap();
    let board = main.find("name = \"Board\"").expect("the autoload's node");
    let mast = main.find("name = \"Mast\"").expect("the scene's own child");
    assert!(board < mast, "the autoload comes first: {main}");
    assert!(
        main.contains("source = \"scripts/board.rn\""),
        "the autoload's script: {main}"
    );
    // The row the connection wrote stays on Mast: the autoload went in
    // at index 1 after every row had found its node by index.
    let scene: toml::Value = toml::from_str(&main).unwrap();
    let mast = node(&scene, "Mast");
    assert_eq!(
        mast["bindings"]["rows"][0]["value"].as_str(),
        Some("_on_hoisted"),
        "the connection on its emitter: {main}"
    );
    assert!(node(&scene, "Board").get("bindings").is_none(), "{main}");
}
