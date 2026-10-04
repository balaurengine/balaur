//! A `Polygon2D` bound to a `Skeleton2D` arrives skinned: its bone weights on
//! the mesh asset and the rig on the component.

use crate::godot::walk::import_project;

fn import(scene: &str) -> toml::Value {
    let godot = tempfile::tempdir().expect("a temporary Godot project");
    let put = |path: &str, text: &str| {
        std::fs::write(godot.path().join(path), text).expect("the file is written");
    };
    put(
        "project.godot",
        "config_version=5\n\n[application]\n\nconfig/name=\"Rig\"\nrun/main_scene=\"res://main.tscn\"\n",
    );
    put("main.tscn", scene);
    let out = tempfile::tempdir().expect("an output folder");
    import_project(&godot.path().join("project.godot"), out.path()).expect("the import runs");
    let text = std::fs::read_to_string(out.path().join("main.toml")).expect("the scene is written");
    toml::from_str(&text).expect("the scene parses")
}

fn scene(bones: &str) -> String {
    [
        "[gd_scene format=3]",
        "",
        "[node name=\"Root\" type=\"Node2D\"]",
        "",
        "[node name=\"Body\" type=\"Polygon2D\" parent=\".\"]",
        "skeleton = NodePath(\"Skeleton2D\")",
        "polygon = PackedVector2Array(0, 0, 100, 0, 0, 100)",
        &format!("bones = [{bones}, PackedFloat32Array(1, 0.5, 0)]"),
        "",
        "[node name=\"Skeleton2D\" type=\"Skeleton2D\" parent=\"Body\"]",
        "",
        "[node name=\"Arm\" type=\"Bone2D\" parent=\"Body/Skeleton2D\"]",
        "rest = Transform2D(1, 0, 0, 1, 0, 0)",
        "",
    ]
    .join("\n")
}

fn skin_of(table: &toml::Value) -> (&toml::Value, &toml::Value) {
    let body = table["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"].as_str() == Some("Body"))
        .expect("the polygon node");
    let mesh_id = body["polygon"]["mesh"]
        .as_str()
        .unwrap()
        .trim_start_matches('#');
    let mesh = table["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"].as_str() == Some(mesh_id))
        .expect("the polygon's mesh asset");
    (&body["polygon"], &mesh["skin"])
}

#[test]
fn a_godot_4_polygon_keeps_its_bones_and_its_rig() {
    // Godot 4 saves a bone path as a plain string.
    let table = import(&scene("\"Arm\""));
    let (polygon, skin) = skin_of(&table);
    assert_eq!(polygon["skeleton"].as_str(), Some("Skeleton2D"));
    let bone = &skin["bones"][0];
    assert_eq!(bone["path"].as_str(), Some("Arm"));
    let weights: Vec<f64> = bone["weights"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_float().unwrap())
        .collect();
    assert_eq!(weights, [1.0, 0.5, 0.0]);
}

#[test]
fn an_older_node_path_bone_still_imports() {
    let table = import(&scene("NodePath(\"Arm\")"));
    let (_, skin) = skin_of(&table);
    assert_eq!(skin["bones"][0]["path"].as_str(), Some("Arm"));
}
