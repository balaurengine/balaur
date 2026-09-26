//! Exports no script prop holds, carried through the node's `meta`.

use crate::godot::walk::import_project;

/// A `Rect2` and an `Array[Dictionary]` a scene set are filed in `meta`,
/// tagged where TOML would lose their type, and `init` reads them back.
#[test]
fn a_rect_and_a_list_of_dictionaries_reach_the_script() {
    let godot = tempfile::tempdir().expect("a temporary Godot project");
    let put = |path: &str, text: &str| {
        let file = godot.path().join(path);
        std::fs::create_dir_all(file.parent().expect("a parent folder"))
            .expect("the folder is written");
        std::fs::write(file, text).expect("the file is written");
    };
    put(
        "project.godot",
        "config_version=5\n\n[application]\n\nconfig/name=\"Atlas\"\nrun/main_scene=\"res://map.tscn\"\n",
    );
    put(
        "cache.gd",
        "extends Node\n@export var country_bounds := Rect2()\n@export var cities: Array[Dictionary] = []\n",
    );
    put(
        "map.tscn",
        &[
            "[gd_scene load_steps=2 format=3]",
            "",
            "[ext_resource type=\"Script\" path=\"res://cache.gd\" id=\"1_c\"]",
            "",
            "[node name=\"Map\" type=\"Node\"]",
            "script = ExtResource(\"1_c\")",
            "country_bounds = Rect2(16, 13, 166, 115)",
            "cities = Array[Dictionary]([{",
            "\"id\": \"bucuresti\",",
            "\"pos\": Vector2(120, 88)",
            "}])",
            "",
        ]
        .join("\n"),
    );
    let out = tempfile::tempdir().expect("an output folder");
    import_project(&godot.path().join("project.godot"), out.path()).expect("the import runs");
    let text = std::fs::read_to_string(out.path().join("map.toml")).expect("the scene");
    let scene: toml::Value = toml::from_str(&text).expect("the scene is TOML");
    let report = std::fs::read_to_string(out.path().join("import-report.md")).unwrap_or_default();
    let filed = scene["nodes"][0]
        .get("meta")
        .and_then(|m| m.get("__exports"))
        .unwrap_or_else(|| panic!("{text}\n{report}"));
    assert_eq!(
        filed["country_bounds"]["__godot"].as_str(),
        Some("Rect2"),
        "{text}"
    );
    assert_eq!(filed["cities"][0]["id"].as_str(), Some("bucuresti"));
    assert_eq!(
        filed["cities"][0]["pos"]["__godot"].as_str(),
        Some("Vector2")
    );
    let script = std::fs::read_to_string(out.path().join("cache.rn")).expect("the script");
    assert!(
        script.contains("this.country_bounds = (script::require(\"gd.rn\").export_value)(this.node, \"country_bounds\", this.country_bounds);"),
        "{script}"
    );
}
