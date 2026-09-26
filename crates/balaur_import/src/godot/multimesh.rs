//! `MultiMeshInstance2D` and `MultiMeshInstance3D` with their `MultiMesh`:
//! the `multimesh2d` or `multimesh3d` component over an inline `multimesh`
//! asset, its `buffer` read into instances.
//!
//! The buffer is Godot's own layout, which `balaur_render` reads for a
//! script's `set_buffer` too. What this adds is the 2D frame: Godot's pixels
//! with y down become world units with y up.

use balaur_plugin::toml;
use balaur_render::{Instance, instances_from_buffer};
use glamx::{EulerRot, Mat4, Quat, Vec3, Vec4};
use toml::Value as Toml;

use crate::godot::nodes::{Asset, Mapped, PIXELS_PER_UNIT, Resources, floats, image_path};
use crate::godot::{Section, Value};

/// A `MultiMeshInstance2D`.
pub(crate) fn instance_2d(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("multimesh2d");
    if let Some(texture) = section.field("texture").and_then(|t| res.path(t)) {
        let texture = image_path(texture, res, out);
        out.set("multimesh2d", "texture", Toml::String(texture));
    }
    if let Some(asset) = asset(section, res, true, out) {
        out.assets.push(Asset {
            component: "multimesh2d",
            key: "source",
            table: asset,
        });
    }
}

/// A `MultiMeshInstance3D`. The importer reads 2D scenes, so the node's own
/// `transform` is read here rather than by the 2D transform every node gets.
pub(crate) fn instance_3d(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("multimesh3d");
    if let Some(columns) = section
        .field("transform")
        .and_then(|t| t.call("Transform3D"))
        && let Some(numbers) = columns
            .iter()
            .map(Value::as_f64)
            .collect::<Option<Vec<_>>>()
        && numbers.len() == 12
    {
        let (scale, rotation, position) = matrix_3d(&numbers).to_scale_rotation_translation();
        let (yaw, pitch, roll) = rotation.to_euler(EulerRot::ZYX);
        out.set("transform", "position", vec3(position));
        out.set(
            "transform",
            "rotation_euler",
            floats(&[f64::from(roll), f64::from(pitch), f64::from(yaw)]),
        );
        out.set("transform", "scale", vec3(scale));
    }
    if let Some(Value::Int(setting)) = section.field("cast_shadow") {
        // Godot's SHADOW_CASTING_SETTING_OFF is 0; on, double-sided and
        // shadows-only all cast.
        out.set("multimesh3d", "cast_shadow", Toml::Boolean(*setting != 0));
    }
    if section.field("material_override").is_some() {
        out.note("MultiMeshInstance3D: its material_override is not carried; give multimesh3d a material");
    }
    if let Some(asset) = asset(section, res, false, out) {
        out.assets.push(Asset {
            component: "multimesh3d",
            key: "source",
            table: asset,
        });
    }
}

/// Godot's `Transform3D(xx, xy, xz, yx, yy, yz, zx, zy, zz, ox, oy, oz)`:
/// three basis columns, then the origin.
fn matrix_3d(n: &[f64]) -> Mat4 {
    let f = |i: usize| n[i] as f32;
    Mat4::from_cols(
        Vec4::new(f(0), f(1), f(2), 0.0),
        Vec4::new(f(3), f(4), f(5), 0.0),
        Vec4::new(f(6), f(7), f(8), 0.0),
        Vec4::new(f(9), f(10), f(11), 1.0),
    )
}

fn vec3(v: Vec3) -> Toml {
    floats(&[f64::from(v.x), f64::from(v.y), f64::from(v.z)])
}

/// The `multimesh` asset a node's `multimesh` names, or `None` with a note
/// when it names none this can read.
fn asset(
    section: &Section,
    res: &Resources<'_>,
    flat: bool,
    out: &mut Mapped,
) -> Option<toml::Table> {
    let Some(multimesh) = section.field("multimesh").and_then(|m| res.sub(m)) else {
        if section.field("multimesh").is_some() {
            out.note("a MultiMesh saved in its own file; resave it inside the scene to import it");
        }
        return None;
    };
    let mut table = toml::Table::new();
    table.insert("type".into(), Toml::String("multimesh".into()));
    let mesh = multimesh.field("mesh").and_then(|m| res.sub(m));
    let Some(mesh) = mesh.and_then(|mesh| mesh_of(mesh, flat, out)) else {
        out.note("a MultiMesh with no mesh this can read draws nothing; give the asset a mesh");
        return None;
    };
    table.insert("mesh".into(), Toml::Table(mesh));
    let carries = |key: &str| matches!(multimesh.field(key), Some(Value::Bool(true)));
    let buffer: Vec<f32> = multimesh
        .field("buffer")
        .and_then(Value::numbers)
        .unwrap_or_default()
        .into_iter()
        .map(|v| v as f32)
        .collect();
    let format_2d = multimesh.field("transform_format").and_then(Value::as_i64) == Some(0);
    if format_2d != flat {
        out.note(if flat {
            "a MultiMeshInstance2D over a MultiMesh in 3D transforms; read as 2D"
        } else {
            "a MultiMeshInstance3D over a MultiMesh in 2D transforms; read as 3D"
        });
    }
    let instances = match instances_from_buffer(
        &buffer,
        format_2d,
        carries("use_colors"),
        carries("use_custom_data"),
    ) {
        Ok(instances) => instances,
        Err(why) => {
            out.note(format!(
                "MultiMesh buffer: {why}; imported with no instances"
            ));
            Vec::new()
        }
    };
    let rows: Vec<Toml> = instances
        .iter()
        .map(|instance| {
            let instance = if flat {
                from_pixels(instance)
            } else {
                *instance
            };
            Toml::Table(written(&instance))
        })
        .collect();
    table.insert("instances".into(), Toml::Array(rows));
    if let Some(visible) = multimesh
        .field("visible_instance_count")
        .and_then(Value::as_i64)
        .filter(|v| *v >= 0)
    {
        table.insert("visible_instance_count".into(), Toml::Integer(visible));
    }
    Some(table)
}

/// A 2D instance from Godot's pixels, y down, into world units, y up: the
/// same flip every 2D node's transform takes.
fn from_pixels(instance: &Instance) -> Instance {
    let turn = instance.rotation.to_euler(EulerRot::ZYX).0;
    Instance {
        position: Vec3::new(
            instance.position.x / PIXELS_PER_UNIT as f32,
            -instance.position.y / PIXELS_PER_UNIT as f32,
            0.0,
        ),
        rotation: Quat::from_rotation_z(-turn),
        ..*instance
    }
}

/// An instance as the asset's list spells it, leaving out what is the
/// default so a long list reads.
fn written(instance: &Instance) -> toml::Table {
    let mut row = toml::Table::new();
    let euler = {
        let (yaw, pitch, roll) = instance.rotation.to_euler(EulerRot::ZYX);
        [roll, pitch, yaw]
    };
    row.insert("position".into(), vec3(instance.position));
    if euler.iter().any(|a| a.abs() > 1e-6) {
        row.insert("rotation_euler".into(), floats(&euler.map(f64::from)));
    }
    if (instance.scale - Vec3::ONE).abs().max_element() > 1e-6 {
        row.insert("scale".into(), vec3(instance.scale));
    }
    if instance.color.iter().any(|c| (c - 1.0).abs() > 1e-6) {
        row.insert("color".into(), floats(&instance.color.map(f64::from)));
    }
    if instance.custom.iter().any(|c| c.abs() > 1e-6) {
        row.insert("custom".into(), floats(&instance.custom.map(f64::from)));
    }
    row
}

/// A Godot primitive mesh as a `mesh` definition, or `None` with a note.
///
/// In 2D a `QuadMesh` is a rectangle of its `size` in pixels. In 3D each
/// primitive is the mesher's kind of the same name and size.
fn mesh_of(mesh: &Section, flat: bool, out: &mut Mapped) -> Option<toml::Table> {
    let class = mesh.attr_str("type").unwrap_or_default();
    let number =
        |key: &str, fallback: f64| mesh.field(key).and_then(Value::as_f64).unwrap_or(fallback);
    let size = |key: &str, fallback: &[f64]| -> Vec<f64> {
        mesh.field(key)
            .and_then(Value::numbers)
            .unwrap_or_else(|| fallback.to_vec())
    };
    let mut table = toml::Table::new();
    table.insert("type".into(), Toml::String("mesh".into()));
    if flat {
        if class != "QuadMesh" {
            out.note(format!(
                "a {class} in a MultiMeshInstance2D has no 2D equivalent; only a QuadMesh imports"
            ));
            return None;
        }
        let [w, h] = <[f64; 2]>::try_from(size("size", &[1.0, 1.0]).get(..2)?).ok()?;
        let (x, y) = (w / 2.0 / PIXELS_PER_UNIT, h / 2.0 / PIXELS_PER_UNIT);
        table.insert(
            "positions".into(),
            Toml::Array(vec![
                floats(&[-x, -y]),
                floats(&[x, -y]),
                floats(&[x, y]),
                floats(&[-x, y]),
            ]),
        );
        table.insert(
            "uvs".into(),
            Toml::Array(vec![
                floats(&[0.0, 1.0]),
                floats(&[1.0, 1.0]),
                floats(&[1.0, 0.0]),
                floats(&[0.0, 0.0]),
            ]),
        );
        table.insert(
            "indices".into(),
            Toml::Array(vec![
                Toml::Array(vec![Toml::Integer(0), Toml::Integer(1), Toml::Integer(2)]),
                Toml::Array(vec![Toml::Integer(0), Toml::Integer(2), Toml::Integer(3)]),
            ]),
        );
        return Some(table);
    }
    let mut kind = |name: &str| table.insert("kind".into(), Toml::String(name.into()));
    match class {
        "BoxMesh" => {
            kind("box");
            table.insert("size".into(), floats(&size("size", &[1.0, 1.0, 1.0])));
        }
        "SphereMesh" => {
            kind("sphere");
            table.insert("radius".into(), Toml::Float(number("radius", 0.5)));
        }
        "CapsuleMesh" => {
            kind("capsule");
            table.insert("radius".into(), Toml::Float(number("radius", 0.5)));
            table.insert("height".into(), Toml::Float(number("height", 2.0)));
        }
        "CylinderMesh" => {
            let top = number("top_radius", 0.5);
            kind(if top == 0.0 { "cone" } else { "cylinder" });
            table.insert("radius".into(), Toml::Float(number("bottom_radius", 0.5)));
            table.insert("height".into(), Toml::Float(number("height", 2.0)));
        }
        "TorusMesh" => {
            kind("torus");
            let (inner, outer) = (number("inner_radius", 0.5), number("outer_radius", 1.0));
            table.insert("radius".into(), Toml::Float(f64::midpoint(inner, outer)));
            table.insert("tube_radius".into(), Toml::Float((outer - inner) / 2.0));
        }
        "PlaneMesh" => {
            kind("plane");
            table.insert("size".into(), floats(&size("size", &[2.0, 2.0])));
        }
        other => {
            out.note(format!(
                "a {other} in a MultiMeshInstance3D has no equivalent mesh kind; not imported"
            ));
            return None;
        }
    }
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::godot::nodes::{Project, map, resources_of};

    fn mapped(scene: &str, class: &str) -> Mapped {
        let document = crate::godot::parse(scene).expect("the scene parses");
        let project = Project::default();
        let res = resources_of(&document, std::path::Path::new("."), &project);
        let node = document
            .each("node")
            .find(|n| n.attr_str("type") == Some(class))
            .expect("the node");
        map(class, node, "", &res)
    }

    fn instances(table: &toml::Table) -> Vec<toml::Table> {
        table["instances"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_table().unwrap().clone())
            .collect()
    }

    fn number(row: &toml::Table, key: &str, index: usize) -> f64 {
        row[key].as_array().unwrap()[index].as_float().unwrap()
    }

    /// Godot's 2D rows in pixels, y down: an instance 200 px right and 100 px
    /// down lands at (2, -1), and a colour carried with `use_colors` reaches
    /// its instance.
    #[test]
    fn a_2d_multimesh_buffer_becomes_instances_in_world_units() {
        let scene = r#"[gd_scene format=3]

[sub_resource type="QuadMesh" id="QuadMesh_a"]
size = Vector2(32, 32)

[sub_resource type="MultiMesh" id="MultiMesh_a"]
transform_format = 0
use_colors = true
instance_count = 2
mesh = SubResource("QuadMesh_a")
buffer = PackedFloat32Array(1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 200, 0, 1, 0, 100, 1, 0, 0, 0.5)

[node name="Waves" type="MultiMeshInstance2D"]
multimesh = SubResource("MultiMesh_a")
"#;
        let out = mapped(scene, "MultiMeshInstance2D");
        assert!(out.components.contains_key("multimesh2d"));
        let asset = &out.assets[0];
        assert_eq!((asset.component, asset.key), ("multimesh2d", "source"));
        let parsed = balaur_render::MultiMeshAsset::parse(&Toml::Table(asset.table.clone()))
            .expect("the engine reads what the importer wrote");
        assert_eq!(parsed.instances.len(), 2);
        let rows = instances(&asset.table);
        assert!((number(&rows[1], "position", 0) - 2.0).abs() < 1e-6);
        assert!((number(&rows[1], "position", 1) + 1.0).abs() < 1e-6);
        assert!((number(&rows[1], "color", 3) - 0.5).abs() < 1e-6);
        assert!(
            !rows[0].contains_key("color"),
            "white is the default and left out"
        );
    }

    /// A 3D instance keeps Godot's units, and a primitive mesh becomes the
    /// mesher's kind; the node's own transform is read too.
    #[test]
    fn a_3d_multimesh_keeps_its_units_and_names_a_mesh_kind() {
        let scene = r#"[gd_scene format=3]

[sub_resource type="BoxMesh" id="BoxMesh_a"]
size = Vector3(0.5, 2, 0.5)

[sub_resource type="MultiMesh" id="MultiMesh_a"]
transform_format = 1
use_custom_data = true
instance_count = 1
mesh = SubResource("BoxMesh_a")
buffer = PackedFloat32Array(1, 0, 0, 3, 0, 1, 0, 4, 0, 0, 1, 5, 9, 8, 7, 6)

[node name="Posts" type="MultiMeshInstance3D"]
multimesh = SubResource("MultiMesh_a")
cast_shadow = 0
"#
        .to_string()
            + "transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 10, 0, 0)\n";
        let out = mapped(&scene, "MultiMeshInstance3D");
        let table = &out.assets[0].table;
        assert_eq!(table["mesh"]["kind"].as_str(), Some("box"));
        let rows = instances(table);
        assert!((number(&rows[0], "position", 2) - 5.0).abs() < 1e-6);
        assert!((number(&rows[0], "custom", 0) - 9.0).abs() < 1e-6);
        let transform = out.components["transform"].as_table().unwrap();
        assert_eq!(transform["position"][0].as_float(), Some(10.0));
        assert_eq!(
            out.components["multimesh3d"]["cast_shadow"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn a_buffer_that_is_not_whole_instances_imports_none_and_says_so() {
        let scene = r#"[gd_scene format=3]

[sub_resource type="QuadMesh" id="QuadMesh_a"]

[sub_resource type="MultiMesh" id="MultiMesh_a"]
transform_format = 0
mesh = SubResource("QuadMesh_a")
buffer = PackedFloat32Array(1, 0, 0)

[node name="Waves" type="MultiMeshInstance2D"]
multimesh = SubResource("MultiMesh_a")
"#;
        let out = mapped(scene, "MultiMeshInstance2D");
        assert!(instances(&out.assets[0].table).is_empty());
        assert!(
            out.notes.iter().any(|n| n.contains("not whole instances")),
            "{:?}",
            out.notes
        );
    }
}
