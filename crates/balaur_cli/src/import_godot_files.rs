//! `balaur import` over a Godot project, or one scene of it, as files.
//!
//! The converters in the `import_godot_*` modules turn text into text; this
//! finds the files, writes what they return, copies the art and audio the
//! scenes name, and gathers everything that did not carry into one
//! `import-report.md`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::import::Imported;

/// File kinds copied across as they are: the engine reads each directly.
const COPIED: &[&str] = &[
    "png", "webp", "jpg", "jpeg", "bmp", "tga", "ogg", "wav", "mp3", "flac", "ttf", "otf",
    "json", "csv", "txt",
];

/// `project.godot`: the settings, every scene, and the files they name.
pub(crate) fn import_project(file: &Path, project: &Path) -> Result<Imported> {
    let root = file.parent().unwrap_or(Path::new("."));
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let document = crate::import_godot::parse(&text).with_context(|| format!("reading {}", file.display()))?;
    let uids = crate::import_godot_project::uid_index(root)?;
    let converted = crate::import_godot_project::convert(&document, &uids)?;

    let mut out = Imported::default();
    let mut report = Report::default();
    write(project, "project.toml", &converted.project_toml, &mut out)?;
    report.section("project.godot", converted.notes);

    let files = walk(root)?;
    let strings = crate::import_godot_strings::convert(root, &files);
    for (path, text) in strings.files()? {
        write(project, &path, &text, &mut out)?;
    }
    report.section("translations", strings.notes.clone());

    let mut scenes = 0;
    let mut failed = 0;
    for relative in files {
        let extension = Path::new(&relative)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "tscn" {
            match scene(root, &relative, project, &strings.keys, &mut out) {
                Ok(notes) => {
                    scenes += 1;
                    report.section(&relative, notes);
                }
                Err(why) => {
                    failed += 1;
                    report.section(&relative, vec![format!("not converted: {why:#}")]);
                }
            }
        } else if COPIED.contains(&extension.as_str()) && !is_translation(root, &relative) {
            let target = project.join(&relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(root.join(&relative), &target)
                .with_context(|| format!("copying {relative}"))?;
            out.files.push(relative);
        }
    }
    let lines = report.write(project, &mut out)?;
    out.note = format!(
        "{scenes} scene{} converted{}; {lines} note{} in import-report.md",
        if scenes == 1 { "" } else { "s" },
        if failed == 0 { String::new() } else { format!(", {failed} would not") },
        if lines == 1 { "" } else { "s" },
    );
    Ok(out)
}

/// One `.tscn`, and the clip files it writes beside itself.
pub(crate) fn import_scene(file: &Path, project: &Path) -> Result<Imported> {
    let file = file
        .canonicalize()
        .with_context(|| format!("reading {}", file.display()))?;
    let root = godot_root(&file)?;
    let relative = file
        .strip_prefix(&root)
        .context("the scene is not inside its project")?
        .to_string_lossy()
        .replace('\\', "/");
    let mut out = Imported::default();
    let strings = crate::import_godot_strings::convert(&root, &walk(&root)?);
    let notes = scene(&root, &relative, project, &strings.keys, &mut out)?;
    let mut report = Report::default();
    report.section(&relative, notes);
    let lines = report.write(project, &mut out)?;
    out.scene = Some(crate::import_godot_scene::scene_path(&relative));
    out.note = if lines == 0 {
        "everything in the scene carried across".to_string()
    } else {
        format!("{lines} note{} in import-report.md", if lines == 1 { "" } else { "s" })
    };
    Ok(out)
}

/// Convert the scene at `relative` under `root`, writing it into `project`.
fn scene(
    root: &Path,
    relative: &str,
    project: &Path,
    keys: &std::collections::BTreeSet<String>,
    out: &mut Imported,
) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(root.join(relative))?;
    let document = crate::import_godot::parse(&text)?;
    let converted = crate::import_godot_scene::convert(&document, relative, root, keys)?;
    let path = crate::import_godot_scene::scene_path(relative);
    write(project, &path, &converted.scene_toml, out)?;
    for (file, text) in &converted.files {
        write(project, file, text, out)?;
    }
    Ok(converted.notes)
}

/// Whether a CSV is a translation table, which becomes `strings/` rather
/// than a copy.
fn is_translation(root: &Path, relative: &str) -> bool {
    relative.ends_with(".csv")
        && std::fs::read_to_string(root.join(format!("{relative}.import")))
            .is_ok_and(|text| text.contains("importer=\"csv_translation\""))
}

/// The directory holding the `project.godot` a file belongs to.
fn godot_root(file: &Path) -> Result<PathBuf> {
    let mut dir = file.parent();
    while let Some(here) = dir {
        if here.join("project.godot").is_file() {
            return Ok(here.to_path_buf());
        }
        dir = here.parent();
    }
    bail!(
        "{} is not inside a Godot project: no project.godot above it",
        file.display()
    )
}

/// Every file under `root`, project-relative with `/`, sorted. `.godot` is
/// the editor's cache and every dot-directory is someone's tooling, so both
/// are skipped.
fn walk(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .flatten()
        {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                dirs.push(path);
            } else if let Ok(relative) = path.strip_prefix(root) {
                files.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    files.sort();
    Ok(files)
}

fn write(project: &Path, relative: &str, text: &str, out: &mut Imported) -> Result<()> {
    let path = project.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    out.files.push(relative.to_string());
    Ok(())
}

/// What did not carry, one heading per file.
#[derive(Default)]
struct Report {
    sections: Vec<(String, Vec<String>)>,
}

impl Report {
    fn section(&mut self, file: &str, notes: Vec<String>) {
        if !notes.is_empty() {
            self.sections.push((file.to_string(), notes));
        }
    }

    /// Write `import-report.md` when there is anything in it, and say how
    /// many notes it holds.
    fn write(self, project: &Path, out: &mut Imported) -> Result<usize> {
        let count = self.sections.iter().map(|(_, notes)| notes.len()).sum();
        if count == 0 {
            return Ok(0);
        }
        let mut text = String::from("# What did not convert\n\n");
        text.push_str(
            "Every line is something `balaur import` read and could not carry. \
             Each names the file it came from, and the node where there is one.\n",
        );
        for (file, notes) in self.sections {
            text.push_str(&format!("\n## {file}\n\n"));
            for note in notes {
                text.push_str(&format!("- {note}\n"));
            }
        }
        write(project, "import-report.md", &text, out)?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::import_project;
    use std::path::Path;

    const HULL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../balaur_render/tests/fixtures/sprite_200x100.png"
    );

    const PROJECT: &str = r#"config_version=5

[application]

config/name="Harbour"
run/main_scene="res://scenes/main.tscn"
"#;

    /// Every mapping this converter makes, in one scene small enough to read:
    /// a scripted root with an export, a sprite, an area with a shape, an
    /// instance edited inside, a VBox of widgets, a player and two signals.
    const MAIN: &str = r#"[gd_scene load_steps=8 format=3 uid="uid://cmain"]

[ext_resource type="Texture2D" path="res://art/hull.png" id="1_hull"]
[ext_resource type="PackedScene" path="res://scenes/crate.tscn" id="2_crate"]
[ext_resource type="Script" path="res://scripts/root.gd" id="3_root"]

[sub_resource type="RectangleShape2D" id="Rect_1"]
size = Vector2(40, 20)

[sub_resource type="Animation" id="Animation_fade"]
resource_name = "fade"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/path = NodePath("Ship:modulate")
tracks/0/interp = 1
tracks/0/keys = {
"times": PackedFloat32Array(0, 1),
"transitions": PackedFloat32Array(1, 1),
"update": 0,
"values": [Color(1, 1, 1, 1), Color(1, 1, 1, 0)]
}
tracks/1/type = "value"
tracks/1/path = NodePath("Ship:position")
tracks/1/interp = 1
tracks/1/keys = {
"times": PackedFloat32Array(0, 1),
"transitions": PackedFloat32Array(1, 1),
"update": 0,
"values": [Vector2(0, 0), Vector2(100, 50)]
}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_1"]
_data = {
&"fade": SubResource("Animation_fade")
}

[node name="World" type="Node2D"]
script = ExtResource("3_root")
speed = 4.5

[node name="Ship" type="Sprite2D" parent="." groups=["boats"]]
position = Vector2(200, 100)
rotation = 0.5
modulate = Color(1, 0.5, 0.5, 1)
texture = ExtResource("1_hull")
flip_h = true

[node name="Dock" type="Area2D" parent="."]

[node name="Shape" type="CollisionShape2D" parent="Dock"]
shape = SubResource("Rect_1")

[node name="Box" parent="." instance=ExtResource("2_crate")]
position = Vector2(-50, 0)

[node name="Lid" parent="Box"]
visible = false

[node name="Hud" type="VBoxContainer" parent="."]
offset_left = 16.0
offset_top = 24.0
offset_right = 216.0
offset_bottom = 124.0

[node name="Title" type="Label" parent="Hud"]
text = "Ahoy"
horizontal_alignment = 1

[node name="Go" type="Button" parent="Hud"]
text = "Sail"
size_flags_vertical = 3

[node name="Player" type="AnimationPlayer" parent="."]
libraries = {
&"": SubResource("AnimationLibrary_1")
}
autoplay = "fade"

[connection signal="pressed" from="Hud/Go" to="." method="on_go"]
[connection signal="body_entered" from="Dock" to="." method="on_dock"]
"#;

    const CRATE: &str = r#"[gd_scene format=3 uid="uid://ccrate"]

[node name="Crate" type="Node2D"]

[node name="Lid" type="Sprite2D" parent="."]
position = Vector2(0, -10)
"#;

    const SCRIPT: &str = "extends Node2D\n\n@export var speed := 2.0\nvar hidden := 1\n";

    fn godot() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        };
        put("project.godot", PROJECT);
        put("scenes/main.tscn", MAIN);
        put("scenes/crate.tscn", CRATE);
        put("scripts/root.gd", SCRIPT);
        std::fs::create_dir_all(dir.path().join("art")).unwrap();
        std::fs::copy(HULL, dir.path().join("art/hull.png")).unwrap();
        dir
    }

    fn read(dir: &Path, path: &str) -> toml::Value {
        let text = std::fs::read_to_string(dir.join(path)).unwrap();
        toml::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}\n{text}"))
    }

    fn node<'a>(scene: &'a toml::Value, name: &str) -> &'a toml::Value {
        scene["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("no node {name} in {scene:#?}"))
    }

    fn floats(value: &toml::Value) -> Vec<f64> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_float().unwrap())
            .collect()
    }

    #[test]
    fn a_godot_project_converts_node_by_node() {
        let godot = godot();
        let out = tempfile::tempdir().unwrap();
        import_project(&godot.path().join("project.godot"), out.path()).unwrap();
        let scene = read(out.path(), "scenes/main.toml");

        let ship = node(&scene, "Ship");
        assert_eq!(
            floats(&ship["transform"]["position"]),
            vec![2.0, -1.0, 0.0],
            "pixels become units, and y flips"
        );
        assert_eq!(floats(&ship["transform"]["rotation_euler"]), vec![0.0, 0.0, -0.5]);
        assert_eq!(floats(&ship["tint"]), vec![1.0, 0.5, 0.5, 1.0], "modulate is the inherited tint");
        assert_eq!(ship["sprite"]["texture"].as_str(), Some("art/hull.png"));
        assert_eq!(ship["sprite"]["flip_x"].as_bool(), Some(true));
        assert_eq!(ship["tags"][0].as_str(), Some("boats"));

        let shape = node(&scene, "Shape");
        assert_eq!(shape["collider2d"]["kind"].as_str(), Some("rect"));
        assert_eq!(floats(&shape["collider2d"]["half_extents"]), vec![0.2, 0.1]);
        assert_eq!(shape["collider2d"]["sensor"].as_bool(), Some(true), "an area's shape senses");
        assert_eq!(shape["collider2d"]["events"][0].as_str(), Some("collision"));
        let row = &shape["bindings"][0];
        assert_eq!(row["event"].as_str(), Some("collision_start"));
        assert_eq!(row["action"].as_str(), Some("call"));
        assert_eq!(row["target"].as_str(), Some("../.."), "from the shape up to the root");
        assert_eq!(row["value"].as_str(), Some("on_dock"));

        let boxed = node(&scene, "Box");
        assert_eq!(boxed["instance"].as_str(), Some("scenes/crate.toml"));
        let overrides = &boxed["overrides"];
        assert_eq!(
            floats(&overrides["Crate"]["transform"]["position"]),
            vec![-0.5, 0.0, 0.0],
            "the instance line moves the prefab's root"
        );
        assert_eq!(
            overrides["Crate/Lid"]["visible"].as_bool(),
            Some(false),
            "an edit inside the instance is an override under the prefab's root"
        );

        let hud = node(&scene, "Hud");
        assert_eq!(hud["widget"]["kind"].as_str(), Some("column"));
        assert_eq!(hud["widget"]["x"].as_float(), Some(16.0));
        assert_eq!(hud["widget"]["width"].as_float(), Some(200.0));
        let go = node(&scene, "Go");
        assert_eq!(go["widget"]["kind"].as_str(), Some("button"));
        assert_eq!(go["widget"]["on_click"].as_str(), Some("on_go"));
        assert_eq!(go["widget"]["grow"].as_float(), Some(1.0), "EXPAND along a VBox");
        assert_eq!(node(&scene, "Title")["widget"]["text_align"].as_str(), Some("center"));

        let world = node(&scene, "World");
        assert_eq!(world["script"]["source"].as_str(), Some("scripts/root.rn"));
        assert_eq!(world["script"]["props"]["speed"].as_float(), Some(4.5));

        let player = node(&scene, "Player");
        let library = player["animation"]["library"].as_str().unwrap();
        assert_eq!(player["animation"]["autoplay"].as_str(), Some("fade"));
        let clips = read(out.path(), library);
        let fade = &clips["clips"]["fade"];
        assert_eq!(fade["loop"].as_str(), Some("loop"));
        let tracks = fade["tracks"].as_array().unwrap();
        assert_eq!(tracks[0]["property"].as_str(), Some("tint"));
        assert_eq!(tracks[0]["target"].as_str(), Some("Ship"));
        assert_eq!(tracks[1]["property"].as_str(), Some("position"));
        assert_eq!(floats(&tracks[1]["keys"][1]["value"]), vec![1.0, -0.5, 0.0]);

        assert!(out.path().join("art/hull.png").is_file(), "the art is copied");
    }

    /// The converted project booted by the engine: every component, override,
    /// binding and clip above has to parse for the scene to load at all.
    #[test]
    fn the_converted_project_loads_in_the_engine() {
        let godot = godot();
        let out = tempfile::tempdir().unwrap();
        import_project(&godot.path().join("project.godot"), out.path()).unwrap();
        // The script phase writes these; a stub stands in so what is tested
        // is the scene, not whether its script exists yet.
        std::fs::create_dir_all(out.path().join("scripts")).unwrap();
        std::fs::write(out.path().join("scripts/root.rn"), "pub fn init(self) {}\n").unwrap();

        let mut config = balaur::AppConfig::dev(out.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);

        let world = app.engine.world();
        let root = app.engine.root();
        let lid = balaur_core::scene::find_node(&world, root, "World/Box/Crate/Lid")
            .expect("the instance built its prefab under the instance node");
        assert!(
            !world.get::<&balaur_core::scene::Appearance>(lid).unwrap().visible,
            "the override inside the instance reached the prefab's node"
        );
        let ship = balaur_core::scene::find_node(&world, root, "World/Ship").unwrap();
        let tint = world.get::<&balaur_core::scene::Appearance>(ship).unwrap().tint;
        assert!(tint.w < 1.0, "autoplay started the fade: alpha {}", tint.w);
    }
}
