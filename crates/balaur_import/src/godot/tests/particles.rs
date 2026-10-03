//! Godot's emitters arrive as `particles2d` and `particles3d`, each in its own
//! world's units.

use crate::godot::walk::import_project;

#[test]
fn a_2d_and_a_3d_emitter_land_on_their_own_components() {
    let godot = tempfile::tempdir().expect("a temporary Godot project");
    let put = |path: &str, text: &str| {
        std::fs::write(godot.path().join(path), text).expect("the file is written");
    };
    put(
        "project.godot",
        "config_version=5\n\n[application]\n\nconfig/name=\"Sparks\"\nrun/main_scene=\"res://main.tscn\"\n",
    );
    put(
        "main.tscn",
        &[
            "[gd_scene format=3]",
            "",
            "[node name=\"Root\" type=\"Node3D\"]",
            "",
            "[node name=\"Flat\" type=\"CPUParticles2D\" parent=\".\"]",
            "direction = Vector2(0, -1)",
            "gravity = Vector2(0, 980)",
            "",
            "[node name=\"Deep\" type=\"CPUParticles3D\" parent=\".\"]",
            "amount = 40",
            "lifetime = 2.0",
            "direction = Vector3(0, 0, 1)",
            "gravity = Vector3(0, -9.8, 0)",
            "",
        ]
        .join("\n"),
    );
    let out = tempfile::tempdir().expect("an output folder");
    import_project(&godot.path().join("project.godot"), out.path()).expect("the import runs");
    let scene = walkdir(out.path())
        .into_iter()
        .find_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            text.contains("particles3d").then_some(text)
        })
        .expect("a scene holds the 3D emitter");
    let table: toml::Value = toml::from_str(&scene).expect("the scene parses");
    let nodes = table["nodes"].as_array().expect("the scene has nodes");
    let named = |name: &str| {
        nodes
            .iter()
            .find(|n| n["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("no node {name}"))
    };
    let floats = |v: &toml::Value| -> Vec<f64> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_float().unwrap())
            .collect()
    };
    let flat = &named("Flat")["particles2d"];
    assert_eq!(floats(&flat["direction"]), [0.0, 1.0], "2D y turns up");
    let deep = &named("Deep")["particles3d"];
    assert_eq!(floats(&deep["direction"]), [0.0, 0.0, 1.0]);
    assert_eq!(
        floats(&deep["gravity"]),
        [0.0, -9.8, 0.0],
        "3D units carry as they are"
    );
    assert!((deep["rate"].as_float().unwrap() - 20.0).abs() < 1e-9);
}

fn walkdir(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "toml") {
                out.push(path);
            }
        }
    }
    out
}
