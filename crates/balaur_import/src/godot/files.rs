//! `balaur import` over a Godot project, or one scene of it, as files.
//!
//! The converters in the `godot::*` modules turn text into text; this
//! finds the files, writes what they return, copies the art and audio the
//! scenes name, and gathers everything that did not carry into one
//! `import-report.md`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::Imported;
use crate::godot::nodes::Project;

/// File kinds copied across as they are: the engine reads each directly.
const COPIED: &[&str] = &[
    "png", "webp", "jpg", "jpeg", "bmp", "tga", "ogg", "wav", "mp3", "flac", "ttf", "otf", "json",
    "csv", "txt",
];

/// `project.godot`: the settings, every scene, and the files they name.
pub(crate) fn import_project(file: &Path, project: &Path) -> Result<Imported> {
    let root = file.parent().unwrap_or(Path::new("."));
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let document =
        crate::godot::parse(&text).with_context(|| format!("reading {}", file.display()))?;
    let uids = crate::godot::project::uid_index(root)?;
    let converted = crate::godot::project::convert(&document, &uids)?;

    let mut out = Imported::default();
    let mut report = Report::default();
    write(project, "project.toml", &converted.project_toml, &mut out)?;
    report.section("project.godot", converted.notes);
    // A project's own faces come first in every font chain, from `fonts/`.
    if let Some(font) = crate::godot::project::custom_font(&document, &uids, root)
        && let Some(name) = Path::new(&font).file_name()
    {
        let target = Path::new("fonts").join(name);
        std::fs::create_dir_all(project.join("fonts"))?;
        std::fs::copy(root.join(&font), project.join(&target))
            .with_context(|| format!("copying the project font {font}"))?;
        out.files.push(target.to_string_lossy().replace('\\', "/"));
    }

    let files = walk(root)?;
    let lookups = lookups(root, &files, uids, project, &mut out, &mut report)?;

    let mut scenes = 0;
    let mut scripts = 0;
    let mut failed = 0;
    for relative in files {
        let extension = Path::new(&relative)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "tscn" {
            match scene(root, &relative, project, &lookups, &mut out) {
                Ok(notes) => {
                    scenes += 1;
                    report.section(&relative, notes);
                }
                Err(why) => {
                    failed += 1;
                    report.section(&relative, vec![format!("not converted: {why:#}")]);
                }
            }
        } else if extension == "gd" {
            let source = std::fs::read_to_string(root.join(&relative))
                .with_context(|| format!("reading {relative}"))?;
            let converted =
                crate::godot::script::convert(&source, &relative, &lookups.classes);
            let target = format!("{}.rn", relative.trim_end_matches(".gd"));
            write(project, &target, &converted.rune, &mut out)?;
            scripts += 1;
            report.section(&relative, converted.notes);
        } else if extension == "tres" {
            if let Some(notes) = theme(root, &relative, project, &lookups, &mut out)? {
                report.section(&relative, notes);
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
        "{scenes} scene{} and {scripts} script skeleton{} converted{}; {lines} note{} in import-report.md",
        if scenes == 1 { "" } else { "s" },
        if scripts == 1 { "" } else { "s" },
        if failed == 0 {
            String::new()
        } else {
            format!(", {failed} would not")
        },
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
    let mut report = Report::default();
    let uids = crate::godot::project::uid_index(&root)?;
    let lookups = lookups(&root, &walk(&root)?, uids, project, &mut out, &mut report)?;
    let notes = scene(&root, &relative, project, &lookups, &mut out)?;
    report.section(&relative, notes);
    let lines = report.write(project, &mut out)?;
    out.scene = Some(crate::godot::scene::scene_path(&relative));
    out.note = if lines == 0 {
        "everything in the scene carried across".to_string()
    } else {
        format!(
            "{lines} note{} in import-report.md",
            if lines == 1 { "" } else { "s" }
        )
    };
    Ok(out)
}

/// Convert the scene at `relative` under `root`, writing it into `project`.
fn scene(
    root: &Path,
    relative: &str,
    project: &Path,
    lookups: &Project,
    out: &mut Imported,
) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(root.join(relative))?;
    let document = crate::godot::parse(&text)?;
    let converted = crate::godot::scene::convert(&document, relative, root, lookups)?;
    let path = crate::godot::scene::scene_path(relative);
    write(project, &path, &converted.scene_toml, out)?;
    for (file, text) in &converted.files {
        write(project, file, text, out)?;
    }
    Ok(converted.notes)
}

/// A `.tres` that is a `Theme`, as the `widget_theme` beside it; `None` for
/// any other resource, which a scene reads where it names it.
fn theme(
    root: &Path,
    relative: &str,
    project: &Path,
    lookups: &Project,
    out: &mut Imported,
) -> Result<Option<Vec<String>>> {
    let text = std::fs::read_to_string(root.join(relative))?;
    if !text.starts_with("[gd_resource type=\"Theme\"") {
        return Ok(None);
    }
    let document = crate::godot::parse(&text)?;
    let res = crate::godot::nodes::resources_of(&document, root, lookups);
    let Some(converted) = crate::godot::theme::convert(&document, &res) else {
        return Ok(None);
    };
    write(
        project,
        &crate::godot::theme::theme_path(relative),
        &converted.toml,
        out,
    )?;
    Ok(Some(converted.notes))
}

/// The project-wide lookups every scene reads: translation keys, which are
/// written as `strings/` on the way, and each SVG's raster, written beside it.
fn lookups(
    root: &Path,
    files: &[String],
    uids: std::collections::BTreeMap<String, String>,
    project: &Path,
    out: &mut Imported,
    report: &mut Report,
) -> Result<Project> {
    let strings = crate::godot::strings::convert(root, files);
    for (path, text) in strings.files()? {
        write(project, &path, &text, out)?;
    }
    report.section("translations", strings.notes);
    let mut rasters = std::collections::BTreeMap::new();
    for svg in files.iter().filter(|f| has_extension(f, "svg")) {
        let Some((bytes, extension)) = crate::godot::textures::raster(root, svg) else {
            continue;
        };
        let target = format!("{}.{extension}", svg.trim_end_matches(".svg"));
        let path = project.join(&target);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("writing {target}"))?;
        out.files.push(target.clone());
        rasters.insert(svg.clone(), target);
    }
    let shaders = shaders(root, files, project, out, report)?;
    Ok(Project {
        keys: strings.keys,
        uids,
        rasters,
        classes: crate::godot::exports::class_index(root, files),
        shaders,
    })
}

/// Every `.gdshader` translated to WESL beside it, and the ones that compile
/// by their Godot path. One that will not translate is reported; one that
/// translates but will not compile is still written, for fixing by hand.
fn shaders(
    root: &Path,
    files: &[String],
    project: &Path,
    out: &mut Imported,
    report: &mut Report,
) -> Result<std::collections::BTreeMap<String, std::rc::Rc<crate::godot::material::Shader>>> {
    let mut shaders = std::collections::BTreeMap::new();
    for godot in files.iter().filter(|f| has_extension(f, "gdshader")) {
        let source = std::fs::read_to_string(root.join(godot))
            .with_context(|| format!("reading {godot}"))?;
        let translated = match crate::godot::shader::translate(&source) {
            Ok(translated) => translated,
            Err(why) => {
                report.section(godot, vec![format!("not translated: {why:#}")]);
                continue;
            }
        };
        let path = crate::godot::material::shader_path(godot);
        write(project, &path, &translated.wesl, out)?;
        let mut notes = translated.notes.clone();
        match crate::godot::shader::check(&translated) {
            Ok(()) => {
                shaders.insert(
                    godot.clone(),
                    std::rc::Rc::new(crate::godot::material::Shader { path, translated }),
                );
            }
            Err(why) => notes.push(format!(
                "{path} does not compile, so no material draws with it: {why:#}"
            )),
        }
        report.section(godot, notes);
    }
    Ok(shaders)
}

/// Whether a project path ends in an extension, whatever its case.
pub(crate) fn has_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

/// Whether a CSV is a translation table, which becomes `strings/` rather
/// than a copy.
fn is_translation(root: &Path, relative: &str) -> bool {
    has_extension(relative, "csv")
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
/// the editor's cache, every dot-directory is someone's tooling, and a
/// folder with a `.gdignore` is one Godot skips, so all three are.
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
                // Godot's own rule: a folder holding `.gdignore` is not the game's.
                if !path.join(".gdignore").exists() {
                    dirs.push(path);
                }
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
            let _ = write!(text, "\n## {file}\n\n");
            for note in notes {
                let _ = writeln!(text, "- {note}");
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

[gui]

theme/custom="res://themes/game.tres"
"#;

    /// Every mapping this converter makes, in one scene small enough to read:
    /// a scripted root with an export, a sprite, an area with a shape, an
    /// instance edited inside, a VBox of widgets, a player and two signals.
    const MAIN: &str = r#"[gd_scene load_steps=8 format=3 uid="uid://cmain"]

[ext_resource type="Texture2D" path="res://art/hull.png" id="1_hull"]
[ext_resource type="PackedScene" path="res://scenes/crate.tscn" id="2_crate"]
[ext_resource type="Script" path="res://scripts/root.gd" id="3_root"]
[ext_resource type="PackedScene" path="res://scenes/extras.tscn" id="4_extras"]

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
weight = 3.0

[node name="Lid" parent="Box"]
visible = false

[node name="Grip" parent="Box/Handle"]
visible = false

[node name="Sticker" type="Node2D" parent="Box/Lid"]
position = Vector2(0, 5)

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
toggle_mode = true
button_group = SubResource("ButtonGroup_tabs")

[node name="Player" type="AnimationPlayer" parent="."]
libraries/ = SubResource("AnimationLibrary_1")
autoplay = "fade"

[node name="Extras" parent="." instance=ExtResource("4_extras")]

[connection signal="pressed" from="Hud/Go" to="." method="on_go"]
[connection signal="body_entered" from="Dock" to="." method="on_dock"]
"#;

    const CRATE: &str = r#"[gd_scene format=3 uid="uid://ccrate"]

[ext_resource type="PackedScene" path="res://scenes/knob.tscn" id="1_knob"]
[ext_resource type="Script" path="res://scripts/crate.gd" id="2_crate"]

[node name="Crate" type="Node2D"]
script = ExtResource("2_crate")

[node name="Lid" type="Sprite2D" parent="."]
position = Vector2(0, -10)

[node name="Handle" parent="." instance=ExtResource("1_knob")]
"#;

    /// A prefab inside the crate prefab, so an edit two instances deep has to
    /// name each prefab's root on the way down.
    const KNOB: &str = r#"[gd_scene format=3 uid="uid://cknob"]

[node name="Knob" type="Node2D"]

[node name="Grip" type="Node2D" parent="."]
"#;

    /// What the first scene leaves out: a shear, a shifted sprite drawn with
    /// a shader, a state machine, a timer, a wide anchor, a theme and a
    /// dialog's answer.
    const EXTRAS: &str = r#"[gd_scene format=3 uid="uid://cextras"]

[ext_resource type="Shader" path="res://shaders/glow.gdshader" id="1_glow"]
[ext_resource type="Texture2D" path="res://art/hull.png" id="2_hull"]
[ext_resource type="Theme" path="res://themes/game.tres" id="3_theme"]

[sub_resource type="ShaderMaterial" id="Glow"]
shader = ExtResource("1_glow")
shader_parameter/glow_intensity = 3.0

[sub_resource type="Animation" id="Animation_idle"]
length = 1.0
loop_mode = 1

[sub_resource type="Animation" id="Animation_walk"]
length = 1.0
loop_mode = 1

[sub_resource type="AnimationLibrary" id="Lib"]
_data = {
&"idle": SubResource("Animation_idle"),
&"walk": SubResource("Animation_walk")
}

[sub_resource type="AnimationNodeAnimation" id="Idle"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="Walk"]
animation = &"walk"

[sub_resource type="AnimationNodeStateMachineTransition" id="Enter"]
advance_mode = 2

[sub_resource type="AnimationNodeStateMachineTransition" id="Go"]
xfade_time = 0.2
advance_mode = 2
advance_condition = &"moving"

[sub_resource type="AnimationNodeStateMachine" id="Machine"]
states/idle/node = SubResource("Idle")
states/walk/node = SubResource("Walk")
transitions = ["Start", "idle", SubResource("Enter"), "idle", "walk", SubResource("Go")]

[node name="Extras" type="Node2D"]

[node name="Leaning" type="Sprite2D" parent="."]
skew = 0.25
offset = Vector2(10, -4)
centered = false
texture = ExtResource("2_hull")
material = SubResource("Glow")

[node name="Tree" type="AnimationTree" parent="."]
libraries/ = SubResource("Lib")
tree_root = SubResource("Machine")

[node name="Clock" type="Timer" parent="."]
wait_time = 0.5
autostart = true

[node name="Bar" type="PanelContainer" parent="."]
anchors_preset = 10
anchor_right = 1.0
offset_left = 8.0
offset_right = -8.0
offset_bottom = 40.0
theme = ExtResource("3_theme")

[node name="Ask" type="ConfirmationDialog" parent="."]
title = "Leave?"
dialog_text = "Leave the harbour?"

[connection signal="timeout" from="Clock" to="." method="on_tick"]
[connection signal="confirmed" from="Ask" to="." method="on_leave"]
"#;

    const GLOW: &str = "shader_type canvas_item;
uniform vec4 glow_color : source_color = vec4(1.0, 0.5, 0.5, 1.0);
uniform float glow_intensity = 2.0;
void fragment() {
    vec4 tex = texture(TEXTURE, UV);
    COLOR = tex + vec4(glow_color.rgb * glow_intensity, tex.a);
}
";

    const THEME: &str = r#"[gd_resource type="Theme" load_steps=2 format=3]

[sub_resource type="StyleBoxFlat" id="Plain"]
bg_color = Color(1, 0.98, 0.93, 1)
border_width_left = 2
border_color = Color(0.4, 0.3, 0.2, 1)
corner_radius_top_left = 16

[sub_resource type="StyleBoxFlat" id="Green"]
bg_color = Color(0.2, 0.6, 0.2, 1)

[resource]
default_font_size = 40
Button/colors/font_color = Color(0.4, 0.3, 0.2, 1)
Button/styles/normal = SubResource("Plain")
ButtonGreen/base_type = &"Button"
ButtonGreen/styles/normal = SubResource("Green")
PanelContainer/styles/panel = SubResource("Plain")
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
        put("scenes/knob.tscn", KNOB);
        put("scenes/extras.tscn", EXTRAS);
        put("shaders/glow.gdshader", GLOW);
        put("themes/game.tres", THEME);
        put("store/.gdignore", "");
        std::fs::copy(HULL, dir.path().join("store/shot.png")).unwrap();
        put("scripts/root.gd", SCRIPT);
        put("scripts/crate.gd", "extends Node2D\n\n@export var weight := 1.0\n");
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
        assert_eq!(
            floats(&ship["transform"]["rotation_euler"]),
            vec![0.0, 0.0, -0.5]
        );
        assert_eq!(
            floats(&ship["tint"]),
            vec![1.0, 0.5, 0.5, 1.0],
            "modulate is the inherited tint"
        );
        assert_eq!(ship["sprite"]["texture"].as_str(), Some("art/hull.png"));
        assert_eq!(ship["sprite"]["flip_x"].as_bool(), Some(true));
        assert_eq!(ship["tags"][0].as_str(), Some("boats"));

        let shape = node(&scene, "Shape");
        assert_eq!(shape["collider2d"]["kind"].as_str(), Some("rect"));
        assert_eq!(floats(&shape["collider2d"]["half_extents"]), vec![0.2, 0.1]);
        assert_eq!(
            shape["collider2d"]["sensor"].as_bool(),
            Some(true),
            "an area's shape senses"
        );
        assert_eq!(shape["collider2d"]["events"][0].as_str(), Some("collision"));
        let row = &shape["bindings"][0];
        assert_eq!(row["event"].as_str(), Some("collision_start"));
        assert_eq!(row["action"].as_str(), Some("call"));
        assert_eq!(
            row["target"].as_str(),
            Some("../.."),
            "from the shape up to the root"
        );
        assert_eq!(row["value"].as_str(), Some("on_dock"));

        let hud = node(&scene, "Hud");
        assert_eq!(hud["widget"]["kind"].as_str(), Some("column"));
        assert_eq!(hud["widget"]["x"].as_float(), Some(16.0));
        assert_eq!(hud["widget"]["width"].as_float(), Some(200.0));
        let go = node(&scene, "Go");
        assert_eq!(go["widget"]["kind"].as_str(), Some("button"));
        assert_eq!(go["widget"]["on_click"].as_str(), Some("on_go"));
        assert_eq!(go["widget"]["toggle"].as_bool(), Some(true));
        assert_eq!(go["widget"]["group"].as_str(), Some("ButtonGroup_tabs"));
        assert_eq!(
            go["widget"]["grow"].as_float(),
            Some(1.0),
            "EXPAND along a VBox"
        );
        assert_eq!(
            node(&scene, "Title")["widget"]["text_align"].as_str(),
            Some("center")
        );

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

        assert!(
            out.path().join("art/hull.png").is_file(),
            "the art is copied"
        );
        assert!(
            !out.path().join("store/shot.png").exists(),
            "a folder Godot ignores is not the game's"
        );
    }

    #[test]
    fn shear_shaders_machines_timers_anchors_themes_and_dialogs_carry() {
        let godot = godot();
        let out = tempfile::tempdir().unwrap();
        import_project(&godot.path().join("project.godot"), out.path()).unwrap();
        let scene = read(out.path(), "scenes/extras.toml");

        let leaning = node(&scene, "Leaning");
        assert_eq!(
            leaning["transform"]["skew"].as_float(),
            Some(-0.25),
            "y flips, so the lean does too"
        );
        assert_eq!(floats(&leaning["sprite"]["offset"]), vec![10.0, -4.0]);
        assert_eq!(leaning["sprite"]["centered"].as_bool(), Some(false));
        let reference = leaning["sprite"]["material"].as_str().unwrap();
        let material = scene["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| Some(a["id"].as_str().unwrap()) == reference.strip_prefix('#'))
            .expect("the material is an inline asset");
        assert_eq!(material["shader"].as_str(), Some("shaders/glow.wesl"));
        assert_eq!(material["params"]["glow_intensity"].as_float(), Some(3.0));
        let glow = floats(&material["params"]["glow_color"]);
        assert!(
            (glow[1] - 0.214).abs() < 0.01,
            "a source_color default is in linear light: {glow:?}"
        );
        assert!(out.path().join("shaders/glow.wesl").is_file());

        let tree = node(&scene, "Tree");
        let machine = read(
            out.path(),
            tree["state_machine"]["machine"].as_str().unwrap(),
        );
        assert_eq!(machine["start"].as_str(), Some("idle"));
        let go = &machine["transitions"][0];
        assert_eq!(go["condition"].as_str(), Some("moving"));
        assert_eq!(go["advance"].as_str(), Some("auto"));
        assert_eq!(go["fade"].as_float(), Some(0.2));
        assert!(
            tree["animation"]["library"].as_str().is_some(),
            "the tree plays its own clips"
        );

        let clock = node(&scene, "Clock");
        assert_eq!(clock["timer"]["wait_time"].as_float(), Some(0.5));
        assert_eq!(
            clock["bindings"][0]["event"].as_str(),
            Some("emitted:timeout")
        );
        assert_eq!(clock["bindings"][0]["value"].as_str(), Some("on_tick"));

        let bar = node(&scene, "Bar");
        assert_eq!(bar["widget"]["anchor"].as_str(), Some("fill_top"));
        assert_eq!(floats(&bar["widget"]["inset"]), vec![8.0, 0.0, 8.0, 0.0]);
        assert_eq!(bar["widget"]["height"].as_float(), Some(40.0));
        assert_eq!(bar["widget"]["theme"].as_str(), Some("themes/game.toml"));

        let ask = node(&scene, "Ask");
        assert_eq!(
            ask["visible"].as_bool(),
            Some(false),
            "a dialog waits to be shown"
        );
        assert_eq!(
            node(&scene, "Ok")["widget"]["on_click"].as_str(),
            Some("on_leave")
        );
        assert_eq!(
            node(&scene, "Cancel")["widget"]["text"].as_str(),
            Some("Cancel")
        );

        let project = read(out.path(), "project.toml");
        assert_eq!(project["ui"]["theme"].as_str(), Some("themes/game.toml"));
        let theme = read(out.path(), "themes/game.toml");
        assert_eq!(theme["button"]["radius"].as_float(), Some(16.0));
        assert_eq!(theme["button"]["size"].as_float(), Some(40.0));
        assert!(theme["roles"]["ButtonGreen"]["fill"].as_str().is_some());
    }

    /// A Godot instance node is its prefab's root, and so is the node here:
    /// what its line sets, and every edit inside it, are overrides by the
    /// Godot path from it.
    #[test]
    fn an_instance_is_its_prefabs_root() {
        let godot = godot();
        let out = tempfile::tempdir().unwrap();
        import_project(&godot.path().join("project.godot"), out.path()).unwrap();
        let scene = read(out.path(), "scenes/main.toml");
        let boxed = node(&scene, "Box");
        assert_eq!(boxed["instance"].as_str(), Some("scenes/crate.toml"));
        let overrides = &boxed["overrides"];
        assert_eq!(boxed["instance_root"].as_bool(), Some(true));
        assert_eq!(
            overrides["."]["script"]["props"]["weight"].as_float(),
            Some(3.0),
            "an export set on the instance line retunes the prefab root's script"
        );
        assert_eq!(
            floats(&overrides["."]["transform"]["position"]),
            vec![-0.5, 0.0, 0.0],
            "the instance line moves the node, which is the prefab's root"
        );
        assert_eq!(
            overrides["Lid"]["visible"].as_bool(),
            Some(false),
            "an edit inside the instance names the Godot path from it"
        );
        assert_eq!(
            overrides["Handle/Grip"]["visible"].as_bool(),
            Some(false),
            "and two instances deep, still the Godot path"
        );

        assert_eq!(
            node(&scene, "Sticker")["parent"].as_str(),
            Some("World/Box/Lid"),
            "a node added inside an instance names its parent by path"
        );
    }

    /// The converted project booted by the engine: every component, override,
    /// binding and clip above has to parse for the scene to load at all.
    #[test]
    fn the_converted_project_loads_in_the_engine() {
        let godot = godot();
        let out = tempfile::tempdir().unwrap();
        import_project(&godot.path().join("project.godot"), out.path()).unwrap();
        assert!(
            out.path().join("scripts/root.rn").is_file(),
            "the script's skeleton is written beside the scene that names it"
        );

        let mut config = balaur::AppConfig::dev(out.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);

        let world = app.engine.world();
        let root = app.engine.root();
        let lid = balaur_core::scene::find_node(&world, root, "World/Box/Lid")
            .expect("the instance built its prefab under the instance node");
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(lid)
                .unwrap()
                .visible,
            "the override inside the instance reached the prefab's node"
        );
        let grip = balaur_core::scene::find_node(&world, root, "World/Box/Handle/Grip")
            .expect("the nested prefab was built inside the outer one");
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(grip)
                .unwrap()
                .visible,
            "the override two instances deep reached its node"
        );
        assert!(
            balaur_core::scene::find_node(&world, root, "World/Box/Lid/Sticker").is_some(),
            "the node added inside the instance sits under the prefab's node"
        );
        let ship = balaur_core::scene::find_node(&world, root, "World/Ship").unwrap();
        let tint = world
            .get::<&balaur_core::scene::Appearance>(ship)
            .unwrap()
            .tint;
        assert!(tint.w < 1.0, "autoplay started the fade: alpha {}", tint.w);
        let tree = balaur_core::scene::find_node(&world, root, "World/Extras/Tree")
            .expect("the extras prefab was built");
        drop(world);
        for _ in 0..3 {
            app.tick(1.0 / 60.0);
        }
        assert_eq!(
            balaur::animation::machine::state(&app.engine, tree).as_deref(),
            Some("idle"),
            "the converted machine entered its start"
        );
    }
}
