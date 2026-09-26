//! Godot runs a class's `_init` before the scene's values land on it.

use crate::godot::walk::import_project;

#[test]
fn an_export_the_scene_set_outlives_init_and_one_it_did_not_keeps_init_s_value() {
    let godot = tempfile::tempdir().expect("a temporary Godot project");
    let put = |path: &str, text: &str| {
        std::fs::write(godot.path().join(path), text).expect("the file is written");
    };
    put(
        "project.godot",
        "config_version=5\n\n[application]\n\nconfig/name=\"Order\"\nrun/main_scene=\"res://main.tscn\"\n",
    );
    put(
        "fold.gd",
        &[
            "extends Node2D",
            "@export var caption := \"\"",
            "@export var count := 0",
            "",
            "func _init(text: String = \"\") -> void:",
            "\tcaption = text",
            "\tcount = 5",
            "",
            "func _ready() -> void:",
            "\tif caption == \"Language\" and count == 5:",
            "\t\tz_index = 3",
            "",
        ]
        .join("\n"),
    );
    put(
        "main.tscn",
        &[
            "[gd_scene load_steps=2 format=3]",
            "",
            "[ext_resource type=\"Script\" path=\"res://fold.gd\" id=\"1_f\"]",
            "",
            "[node name=\"Fold\" type=\"Node2D\"]",
            "script = ExtResource(\"1_f\")",
            "caption = \"Language\"",
            "",
        ]
        .join("\n"),
    );
    let out = tempfile::tempdir().expect("an output folder");
    import_project(&godot.path().join("project.godot"), out.path()).expect("the import runs");
    let mut config = balaur::AppConfig::dev(out.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = balaur::standard_app(config).expect("the app builds");
    app.load_project().expect("the project loads");
    app.tick(1.0 / 60.0);
    let world = app.engine.world();
    let fold = balaur_core::scene::find_node(&world, app.engine.root(), "Fold")
        .expect("the scene's root is the fold");
    let script = std::fs::read_to_string(out.path().join("fold.rn")).unwrap_or_default();
    assert_eq!(
        world
            .get::<&balaur_core::scene::Appearance>(fold)
            .expect("a 2D node has an appearance")
            .z_index,
        3,
        "the fold marked itself only if the scene's caption and _init's count both held\n{script}"
    );
}
