//! Godot's control signals as the widget handler keys that answer them.

use crate::godot::walk::import_project;

/// A page picked on a tab container is the widget's `change`, so the handler
/// a scene connected to `tab_changed` is its `on_change`.
#[test]
fn a_tab_container_s_tab_changed_is_its_on_change() {
    let godot = tempfile::tempdir().expect("a temporary Godot project");
    let put = |path: &str, text: &str| {
        let file = godot.path().join(path);
        std::fs::create_dir_all(file.parent().expect("a parent folder"))
            .expect("the folder is written");
        std::fs::write(file, text).expect("the file is written");
    };
    put(
        "project.godot",
        "config_version=5\n\n[application]\n\nconfig/name=\"Deck\"\nrun/main_scene=\"res://scenes/main.tscn\"\n",
    );
    put(
        "scripts/main.gd",
        "extends Control\n\nfunc _on_tab_changed(tab: int) -> void:\n\tpass\n",
    );
    put(
        "scenes/main.tscn",
        &[
            "[gd_scene load_steps=2 format=3]",
            "",
            "[ext_resource type=\"Script\" path=\"res://scripts/main.gd\" id=\"1_m\"]",
            "",
            "[node name=\"Main\" type=\"Control\"]",
            "script = ExtResource(\"1_m\")",
            "",
            "[node name=\"Tabs\" type=\"TabContainer\" parent=\".\"]",
            "",
            "[node name=\"Crew\" type=\"Control\" parent=\"Tabs\"]",
            "",
            "[connection signal=\"tab_changed\" from=\"Tabs\" to=\".\" method=\"_on_tab_changed\"]",
            "",
        ]
        .join("\n"),
    );
    let out = tempfile::tempdir().expect("an output folder");
    import_project(&godot.path().join("project.godot"), out.path()).expect("the import runs");
    let main = std::fs::read_to_string(out.path().join("scenes/main.toml")).expect("the scene");
    let scene: toml::Value = toml::from_str(&main).expect("the scene is TOML");
    let tabs = scene["nodes"]
        .as_array()
        .expect("a node list")
        .iter()
        .find(|n| n["name"].as_str() == Some("Tabs"))
        .expect("the tab container");
    assert_eq!(
        tabs.get("widget")
            .and_then(|w| w.get("on_change"))
            .and_then(toml::Value::as_str),
        Some("_on_tab_changed"),
        "{main}"
    );
}
