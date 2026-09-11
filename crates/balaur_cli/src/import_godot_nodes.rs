//! One Godot node as the node keys and components a balaur node carries.
//!
//! Godot's node is a class; here it is a plain node plus components, so each
//! class is a row that says which components it becomes and how its
//! properties land on them. A class with no row keeps its transform and is
//! reported, so nothing is dropped silently.
//!
//! Godot's 2D is pixels with y down; balaur's is world units with y up, at the
//! 100 pixels per unit every 2D component defaults to. A widget stays in
//! design pixels, y down, because that is what widgets measure in.

use std::collections::BTreeMap;
use std::path::Path;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::import_godot::{Section, Value};

/// Texture pixels per world unit: the default of every 2D component here.
pub(crate) const PIXELS_PER_UNIT: f64 = 100.0;

/// The resources a scene file declares, by the ids its nodes name them with.
pub(crate) struct Resources<'a> {
    /// `[ext_resource]`: id to `(type, project-relative path)`.
    pub external: BTreeMap<String, (String, String)>,
    /// `[sub_resource]`: id to its section, owned so a library loaded from
    /// another file can be looked up the same way.
    pub internal: BTreeMap<String, Section>,
    /// The Godot project's root, for reading an image's size off disk.
    pub root: &'a Path,
}

impl Resources<'_> {
    /// The path an `ExtResource("id")` names.
    pub(crate) fn path(&self, value: &Value) -> Option<&str> {
        let id = value.call("ExtResource")?.first()?.as_str()?;
        self.external.get(id).map(|(_, path)| path.as_str())
    }

    /// The type an `ExtResource("id")` declares.
    pub(crate) fn kind(&self, value: &Value) -> Option<&str> {
        let id = value.call("ExtResource")?.first()?.as_str()?;
        self.external.get(id).map(|(kind, _)| kind.as_str())
    }

    /// The section a `SubResource("id")` names.
    pub(crate) fn sub(&self, value: &Value) -> Option<&Section> {
        let id = value.call("SubResource")?.first()?.as_str()?;
        self.internal.get(id)
    }

    /// An image's pixel size, read from a PNG's header. Other formats answer
    /// `None`, and a caller that needs a size reports it.
    pub(crate) fn image_size(&self, path: &str) -> Option<(u32, u32)> {
        let bytes = std::fs::read(self.root.join(path)).ok()?;
        let header = bytes.get(..24)?;
        if &header[..8] != b"\x89PNG\r\n\x1a\n" {
            return None;
        }
        let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
        Some((width, height))
    }
}

/// What one node became.
#[derive(Default)]
pub(crate) struct Mapped {
    /// `visible`, `tint`, `z_index`: keys every node has.
    pub keys: toml::Table,
    /// Component name to its table.
    pub components: toml::Table,
    /// Inline `[[assets]]`, each with the component whose `mesh` names it;
    /// the scene walker gives it an id and writes the `#id` there.
    pub assets: Vec<(&'static str, toml::Table)>,
    pub notes: Vec<String>,
}

impl Mapped {
    fn set(&mut self, component: &str, key: &str, value: Toml) {
        let table = self
            .components
            .entry(component)
            .or_insert_with(|| Toml::Table(toml::Table::new()));
        if let Toml::Table(table) = table {
            table.insert(key.to_string(), value);
        }
    }

    fn touch(&mut self, component: &str) {
        self.components
            .entry(component)
            .or_insert_with(|| Toml::Table(toml::Table::new()));
    }

    fn note(&mut self, text: impl Into<String>) {
        self.notes.push(text.into());
    }
}

/// Where a Godot class sits: a `Control` is placed by the widget layer in
/// design pixels, a `Node2D` by its transform in world units, and a plain
/// `Node` by nothing at all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    Control,
    Node2d,
    Plain,
}

pub(crate) fn family(class: &str) -> Family {
    if WIDGET_KINDS.iter().any(|(godot, _)| *godot == class) || class == "Control" {
        return Family::Control;
    }
    if PLAIN.contains(&class) {
        return Family::Plain;
    }
    Family::Node2d
}

/// Classes that are neither placed nor drawn.
const PLAIN: &[&str] = &[
    "Node",
    "AnimationPlayer",
    "AnimationTree",
    "Timer",
    "AudioStreamPlayer",
    "CanvasLayer",
    "HTTPRequest",
    "ResourcePreloader",
    "WorldEnvironment",
    "CanvasModulate",
    "MultiplayerSpawner",
    "MultiplayerSynchronizer",
];

/// Each `Control` subclass, and the widget kind it becomes.
const WIDGET_KINDS: &[(&str, &str)] = &[
    ("Label", "label"),
    ("RichTextLabel", "label"),
    ("Button", "button"),
    ("LinkButton", "button"),
    ("MenuButton", "button"),
    ("TextureButton", "image"),
    ("CheckBox", "check"),
    ("CheckButton", "check"),
    ("LineEdit", "field"),
    ("OptionButton", "dropdown"),
    ("HSlider", "slider"),
    ("VSlider", "slider"),
    ("ProgressBar", "progress"),
    ("TextureProgressBar", "progress"),
    ("TextureRect", "image"),
    ("NinePatchRect", "image"),
    ("ColorRect", "panel"),
    ("Panel", "panel"),
    ("PanelContainer", "panel"),
    ("MarginContainer", "panel"),
    ("CenterContainer", "panel"),
    ("AspectRatioContainer", "panel"),
    ("SubViewportContainer", "panel"),
    ("HBoxContainer", "row"),
    ("VBoxContainer", "column"),
    ("BoxContainer", "row"),
    ("GridContainer", "grid"),
    ("FlowContainer", "flow"),
    ("HFlowContainer", "flow"),
    ("VFlowContainer", "flow"),
    ("ScrollContainer", "scroll"),
    ("TabContainer", "tab"),
    ("TabBar", "tab"),
    ("FoldableContainer", "fold"),
    ("HSeparator", "separator"),
    ("VSeparator", "separator"),
    ("AcceptDialog", "dialog"),
    ("ConfirmationDialog", "dialog"),
    ("Window", "dialog"),
    ("SpinBox", "field"),
    ("TextEdit", "field"),
];

/// Map one node. `parent` is the class of the node above it, which decides
/// which of a Control's two size flags is the one its container reads.
pub(crate) fn map(class: &str, section: &Section, parent: &str, res: &Resources<'_>) -> Mapped {
    let mut out = Mapped::default();
    node_keys(section, &mut out);
    match family(class) {
        Family::Node2d => transform(section, &mut out),
        Family::Control => widget(class, section, parent, res, &mut out),
        Family::Plain => {}
    }
    match class {
        "Node" | "Node2D" | "Control" | "Marker2D" | "CanvasLayer" | "Skeleton2D" => {}
        "Sprite2D" => sprite(section, res, &mut out),
        "AnimatedSprite2D" => {
            out.touch("sprite");
            out.note("AnimatedSprite2D: its SpriteFrames need converting to a `sprite_sheet`");
        }
        "Polygon2D" => polygon(section, res, &mut out),
        "Line2D" => line(section, &mut out),
        "Bone2D" => bone(section, &mut out),
        "Camera2D" => camera(section, &mut out),
        // An area's shape is on its CollisionShape2D child, which is where
        // `sensor` is written; the area itself is a grouping node.
        "Area2D" => {}
        "StaticBody2D" => body(&mut out, "static"),
        "RigidBody2D" => body(&mut out, "dynamic"),
        "AnimatableBody2D" => body(&mut out, "kinematic"),
        "CharacterBody2D" => {
            body(&mut out, "kinematic");
            out.touch("character2d");
        }
        "CollisionShape2D" => collision_shape(section, res, &mut out),
        "CollisionPolygon2D" => collision_polygon(section, &mut out),
        "CPUParticles2D" | "GPUParticles2D" => particles(section, res, &mut out),
        "AudioStreamPlayer" | "AudioStreamPlayer2D" => sound(class, section, res, &mut out),
        "PointLight2D" | "DirectionalLight2D" => light(class, section, &mut out),
        "RemoteTransform2D" => remote(section, &mut out),
        "AnimationPlayer" => {
            // The clips themselves are the animation phase's; the node is here.
            out.touch("animation");
        }
        "TileMapLayer" | "TileMap" => {
            out.note("TileMapLayer: its tile data is not converted yet");
        }
        "Timer" => out.note("Timer: use `task.seconds` in the script that started it"),
        other if family(other) == Family::Control => {}
        other => out.note(format!("{other}: no balaur equivalent; kept as a plain node")),
    }
    if parent == "Area2D" && out.components.contains_key("collider2d") {
        out.set("collider2d", "sensor", Toml::Boolean(true));
    }
    if let Some(material) = section.field("material") {
        if res.sub(material).is_some() || res.path(material).is_some() {
            out.note("a ShaderMaterial: port its .gdshader to WESL by hand");
        }
    }
    out
}

/// `visible`, `modulate` and `z_index`, which every node has here too.
fn node_keys(section: &Section, out: &mut Mapped) {
    if let Some(visible) = section.field("visible") {
        if let Value::Bool(on) = visible {
            out.keys.insert("visible".into(), Toml::Boolean(*on));
        }
    }
    if let Some(color) = section.field("modulate").and_then(colour) {
        out.keys.insert("tint".into(), color);
    }
    if let Some(z) = section.field("z_index").and_then(Value::as_i64) {
        out.keys.insert("z_index".into(), Toml::Integer(z));
    }
    if section.field("z_as_relative") == Some(&Value::Bool(false)) {
        out.keys.insert("z_relative".into(), Toml::Boolean(false));
    }
}

fn transform(section: &Section, out: &mut Mapped) {
    if let Some([x, y]) = section.field("position").and_then(pair) {
        out.set("transform", "position", floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT, 0.0]));
    }
    if let Some(angle) = section.field("rotation").and_then(Value::as_f64) {
        // y flips, so a turn one way in Godot is the other way here.
        out.set("transform", "rotation_euler", floats(&[0.0, 0.0, -angle]));
    }
    if let Some([x, y]) = section.field("scale").and_then(pair) {
        out.set("transform", "scale", floats(&[x, y, 1.0]));
    }
    if section.field("skew").and_then(Value::as_f64).is_some_and(|s| s != 0.0) {
        out.note("skew: a transform here has no shear, so it was dropped");
    }
}

fn sprite(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("sprite");
    let texture = section.field("texture").and_then(|t| res.path(t));
    if let Some(path) = texture {
        out.set("sprite", "texture", Toml::String(image_path(path, out)));
    }
    if let Some(color) = section.field("self_modulate").and_then(colour) {
        out.set("sprite", "color", color);
    } else {
        out.set("sprite", "color", floats(&[1.0, 1.0, 1.0, 1.0]));
    }
    for (godot, here) in [("flip_h", "flip_x"), ("flip_v", "flip_y")] {
        if let Some(Value::Bool(on)) = section.field(godot) {
            out.set("sprite", here, Toml::Boolean(*on));
        }
    }
    let region = section.field("region_enabled") == Some(&Value::Bool(true));
    if let Some(rect) = section.field("region_rect").and_then(Value::numbers).filter(|_| region) {
        if let [x, y, w, h] = rect[..] {
            out.set("sprite", "region_origin", floats(&[x, y]));
            out.set("sprite", "region_size", floats(&[w, h]));
        }
    }
    let frames = |key| section.field(key).and_then(Value::as_i64);
    let (columns, rows) = (frames("hframes"), frames("vframes"));
    if columns.unwrap_or(1) > 1 || rows.unwrap_or(1) > 1 {
        out.set("sprite", "columns", Toml::Float(columns.unwrap_or(1) as f64));
        out.set("sprite", "rows", Toml::Float(rows.unwrap_or(1) as f64));
        if let Some(frame) = frames("frame") {
            out.set("sprite", "frame", Toml::Float(frame as f64));
        }
    }
    // Godot centres a sprite on its origin by default and shifts it by
    // `offset`; here a sprite is centred and cannot be shifted.
    if section.field("centered") == Some(&Value::Bool(false)) {
        out.note("Sprite2D centered = false: a sprite here is always centred on its node");
    }
    if section.field("offset").and_then(pair).is_some_and(|[x, y]| x != 0.0 || y != 0.0) {
        out.note("Sprite2D offset: move the sprite into a child node to shift it");
    }
}

/// A Polygon2D: its points, loops, UVs and bone weights as an inline `mesh`,
/// which is the shape that asset was written to take.
fn polygon(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("polygon");
    let offset = section.field("offset").and_then(pair).unwrap_or([0.0, 0.0]);
    let points = section.field("polygon").map(points_of).unwrap_or_default();
    if points.len() < 3 {
        out.note("Polygon2D with fewer than three points draws nothing; skipped its mesh");
        return;
    }
    let texture = section.field("texture").and_then(|t| res.path(t));
    if let Some(path) = texture {
        out.set("polygon", "texture", Toml::String(image_path(path, out)));
    }
    let color = section.field("color").and_then(colour);
    let tint = section.field("self_modulate").and_then(colour);
    out.set("polygon", "color", tint.or(color).unwrap_or_else(|| floats(&[1.0, 1.0, 1.0, 1.0])));

    let mut mesh = toml::Table::new();
    mesh.insert("type".into(), Toml::String("mesh".into()));
    let positions = points
        .iter()
        .map(|[x, y]| floats(&[(x + offset[0]) / PIXELS_PER_UNIT, -(y + offset[1]) / PIXELS_PER_UNIT]))
        .collect();
    mesh.insert("positions".into(), Toml::Array(positions));
    if let Some(internal) = section.field("internal_vertex_count").and_then(Value::as_i64) {
        if internal > 0 {
            mesh.insert("internal".into(), Toml::Integer(internal));
        }
    }
    if let Some(loops) = section.field("polygons").and_then(Value::as_array) {
        let loops: Vec<Toml> = loops
            .iter()
            .filter_map(Value::numbers)
            .map(|ring| Toml::Array(ring.into_iter().map(|i| Toml::Integer(i as i64)).collect()))
            .collect();
        if !loops.is_empty() {
            mesh.insert("polygons".into(), Toml::Array(loops));
        }
    }
    // Godot's UVs are texture pixels, and with none the points are the UVs;
    // here they are 0 to 1, so the texture's size turns one into the other.
    let uvs = section.field("uv").map(points_of).filter(|u| u.len() == points.len());
    let size = texture.and_then(|p| res.image_size(p));
    match (size, texture) {
        (Some((w, h)), _) => {
            let texture_offset = section.field("texture_offset").and_then(pair).unwrap_or([0.0, 0.0]);
            let source = uvs.unwrap_or_else(|| points.clone());
            let uvs = source
                .iter()
                .map(|[u, v]| floats(&[(u + texture_offset[0]) / f64::from(w), (v + texture_offset[1]) / f64::from(h)]))
                .collect();
            mesh.insert("uvs".into(), Toml::Array(uvs));
        }
        (None, Some(path)) => out.note(format!(
            "Polygon2D over {path}: its size could not be read, so the UVs are balaur's default"
        )),
        (None, None) => {}
    }
    if let Some(skin) = skin(section, points.len()) {
        mesh.insert("skin".into(), skin);
        if let Some(rig) = section.field("skeleton").and_then(Value::as_str) {
            out.set("polygon", "skeleton", Toml::String(rig.to_string()));
        }
    }
    out.assets.push(("polygon", mesh));
}

/// `bones = [NodePath, PackedFloat32Array, …]` as `skin.bones`. The paths
/// are relative to the rig in both engines, so they carry as they are.
fn skin(section: &Section, vertices: usize) -> Option<Toml> {
    let flat = section.field("bones")?.as_array()?;
    let mut bones = Vec::new();
    for pair in flat.chunks(2) {
        let [path, weights] = pair else { continue };
        let path = path.call("NodePath").and_then(|a| a.first()).and_then(Value::as_str)?;
        let weights = weights.numbers()?;
        if weights.len() != vertices {
            continue;
        }
        let mut bone = toml::Table::new();
        bone.insert("path".into(), Toml::String(path.to_string()));
        bone.insert("weights".into(), Toml::Array(weights.into_iter().map(Toml::Float).collect()));
        bones.push(Toml::Table(bone));
    }
    if bones.is_empty() {
        return None;
    }
    let mut skin = toml::Table::new();
    skin.insert("bones".into(), Toml::Array(bones));
    Some(Toml::Table(skin))
}

fn line(section: &Section, out: &mut Mapped) {
    out.set("shape2d", "kind", Toml::String("polyline".into()));
    let points: Vec<Toml> = section
        .field("points")
        .map(points_of)
        .unwrap_or_default()
        .iter()
        .map(|[x, y]| floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT]))
        .collect();
    out.set("shape2d", "points", Toml::Array(points));
    let width = section.field("width").and_then(Value::as_f64).unwrap_or(10.0);
    out.set("shape2d", "width", Toml::Float(width / PIXELS_PER_UNIT));
    if let Some(color) = section.field("default_color").and_then(colour) {
        out.set("shape2d", "color", color);
    }
    if section.field("closed") == Some(&Value::Bool(true)) {
        out.set("shape2d", "closed", Toml::Boolean(true));
    }
    if section.field("gradient").is_some() {
        out.note("Line2D gradient: set `shape2d.gradient` to its end colour by hand");
    }
}

fn bone(section: &Section, out: &mut Mapped) {
    out.touch("bone2d");
    // The rest pose is Godot's `rest` transform; with none the bone rests
    // where the scene put it.
    let rest = section.field("rest").and_then(Value::numbers);
    let (x, y, angle) = match rest.as_deref() {
        Some([a, b, _, _, ox, oy]) => (*ox, *oy, b.atan2(*a)),
        _ => {
            let [x, y] = section.field("position").and_then(pair).unwrap_or([0.0, 0.0]);
            (x, y, section.field("rotation").and_then(Value::as_f64).unwrap_or(0.0))
        }
    };
    out.set("bone2d", "rest_position", floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT]));
    out.set("bone2d", "rest_rotation", Toml::Float(-angle));
    if let Some(length) = section.field("length").and_then(Value::as_f64) {
        out.set("bone2d", "length", Toml::Float(length / PIXELS_PER_UNIT));
    }
    if let Some(angle) = section.field("bone_angle").and_then(Value::as_f64) {
        out.set("bone2d", "angle", Toml::Float(-angle.to_radians()));
    }
}

fn camera(section: &Section, out: &mut Mapped) {
    out.set("camera", "kind", Toml::String("2d".into()));
    if let Some([zoom, _]) = section.field("zoom").and_then(pair) {
        out.set("camera", "zoom", Toml::Float(zoom));
    }
    if section.field("enabled") == Some(&Value::Bool(false)) {
        out.set("camera", "current", Toml::Boolean(false));
    }
}

fn body(out: &mut Mapped, kind: &str) {
    out.set("body2d", "kind", Toml::String(kind.into()));
}

/// A CollisionShape2D's `shape` sub-resource as the collider it describes.
fn collision_shape(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("collider2d");
    if section.field("disabled") == Some(&Value::Bool(true)) {
        out.set("collider2d", "enabled", Toml::Boolean(false));
    }
    if section.field("one_way_collision") == Some(&Value::Bool(true)) {
        out.set("collider2d", "one_way", Toml::Boolean(true));
    }
    let Some(shape) = section.field("shape").and_then(|s| res.sub(s)) else {
        out.note("CollisionShape2D without an inline shape: its collider is the default rect");
        return;
    };
    let number = |key| shape.field(key).and_then(Value::as_f64);
    match shape.attr_str("type").unwrap_or_default() {
        "RectangleShape2D" => {
            let [w, h] = shape.field("size").and_then(pair).unwrap_or([20.0, 20.0]);
            out.set("collider2d", "kind", Toml::String("rect".into()));
            out.set("collider2d", "half_extents", floats(&[w / 2.0 / PIXELS_PER_UNIT, h / 2.0 / PIXELS_PER_UNIT]));
        }
        "CircleShape2D" => {
            out.set("collider2d", "kind", Toml::String("circle".into()));
            out.set("collider2d", "radius", Toml::Float(number("radius").unwrap_or(10.0) / PIXELS_PER_UNIT));
        }
        "CapsuleShape2D" => {
            out.set("collider2d", "kind", Toml::String("capsule".into()));
            out.set("collider2d", "radius", Toml::Float(number("radius").unwrap_or(10.0) / PIXELS_PER_UNIT));
            out.set("collider2d", "height", Toml::Float(number("height").unwrap_or(30.0) / PIXELS_PER_UNIT));
        }
        "SegmentShape2D" => {
            let a = shape.field("a").and_then(pair).unwrap_or([0.0, 0.0]);
            let b = shape.field("b").and_then(pair).unwrap_or([0.0, 10.0]);
            out.set("collider2d", "kind", Toml::String("segment".into()));
            out.set("collider2d", "a", floats(&[a[0] / PIXELS_PER_UNIT, -a[1] / PIXELS_PER_UNIT]));
            out.set("collider2d", "b", floats(&[b[0] / PIXELS_PER_UNIT, -b[1] / PIXELS_PER_UNIT]));
        }
        "WorldBoundaryShape2D" => {
            out.set("collider2d", "kind", Toml::String("halfspace".into()));
        }
        "ConvexPolygonShape2D" | "ConcavePolygonShape2D" => {
            let key = if shape.field("points").is_some() { "points" } else { "segments" };
            let points = shape.field(key).map(points_of).unwrap_or_default();
            polygon_collider(&points, "convex_hull", out);
        }
        other => out.note(format!("{other}: no 2D collider of that shape; left as the default rect")),
    }
}

fn collision_polygon(section: &Section, out: &mut Mapped) {
    let points = section.field("polygon").map(points_of).unwrap_or_default();
    // Godot's build mode 0 is solids, 1 is segments along the outline.
    let kind = match section.field("build_mode").and_then(Value::as_i64) {
        Some(1) => "polyline",
        _ => "convex_hull",
    };
    polygon_collider(&points, kind, out);
}

/// A collider over an inline mesh of `points`, which is how both a convex
/// hull and a polyline take their shape here.
fn polygon_collider(points: &[[f64; 2]], kind: &str, out: &mut Mapped) {
    out.set("collider2d", "kind", Toml::String(kind.into()));
    if points.len() < 2 {
        out.note("a collision polygon with fewer than two points; left as the default rect");
        return;
    }
    let mut mesh = toml::Table::new();
    mesh.insert("type".into(), Toml::String("mesh".into()));
    let positions = points
        .iter()
        .map(|[x, y]| floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT]))
        .collect();
    mesh.insert("positions".into(), Toml::Array(positions));
    out.assets.push(("collider2d", mesh));
}

fn particles(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("particles");
    let number = |key| section.field(key).and_then(Value::as_f64);
    let lifetime = number("lifetime").unwrap_or(1.0);
    out.set("particles", "lifetime", Toml::Float(lifetime));
    if let Some(amount) = number("amount") {
        out.set("particles", "rate", Toml::Float(amount / lifetime.max(0.05)));
    }
    for (godot, here) in [("emitting", "emitting"), ("one_shot", "one_shot")] {
        if let Some(Value::Bool(on)) = section.field(godot) {
            out.set("particles", here, Toml::Boolean(*on));
        }
    }
    if let Some(explosiveness) = number("explosiveness") {
        out.set("particles", "explosiveness", Toml::Float(explosiveness));
    }
    if let Some(path) = section.field("texture").and_then(|t| res.path(t)) {
        out.set("particles", "texture", Toml::String(image_path(path, out)));
    }
    if let Some(color) = section.field("color").and_then(colour) {
        out.set("particles", "color", color);
    }
    // Godot's direction is a y-down vector; here it is an angle, 90 up.
    if let Some([x, y]) = section.field("direction").and_then(pair) {
        out.set("particles", "angle", Toml::Float((-y).atan2(x).to_degrees()));
    }
    if let Some(spread) = number("spread") {
        out.set("particles", "spread", Toml::Float(spread));
    }
    if let Some([x, y]) = section.field("gravity").and_then(pair) {
        out.set("particles", "gravity", floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT]));
    }
    let low = number("initial_velocity_min");
    let high = number("initial_velocity_max");
    if let Some(speed) = low.zip(high).map(|(a, b)| (a + b) / 2.0).or(high).or(low) {
        out.set("particles", "speed", Toml::Float(speed / PIXELS_PER_UNIT));
    }
    if let Some(scale) = number("scale_amount_max").or_else(|| number("scale_amount_min")) {
        out.set("particles", "size", Toml::Float((scale * 4.0).max(0.5)));
    }
    if section.field("color_ramp").is_some() || section.field("scale_amount_curve").is_some() {
        out.note("CPUParticles2D ramp or curve: only its start and end carry, as `color_end` and `size_end`");
    }
    if let Some(ramp) = section.field("color_ramp").and_then(|r| res.sub(r)) {
        if let Some(colors) = ramp.field("colors").and_then(Value::numbers) {
            if let [.., r, g, b, a] = colors[..] {
                out.set("particles", "color_end", floats(&[r, g, b, a]));
            }
        }
    }
}

fn sound(class: &str, section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("sound");
    if let Some(path) = section.field("stream").and_then(|s| res.path(s)) {
        out.set("sound", "file", Toml::String(path.to_string()));
    }
    if let Some(Value::Bool(on)) = section.field("autoplay") {
        out.set("sound", "autoplay", Toml::Boolean(*on));
    }
    if let Some(db) = section.field("volume_db").and_then(Value::as_f64) {
        out.set("sound", "volume", Toml::Float(10f64.powf(db / 20.0)));
    }
    if let Some(pitch) = section.field("pitch_scale").and_then(Value::as_f64) {
        out.set("sound", "pitch", Toml::Float(pitch));
    }
    if let Some(bus) = section.field("bus").and_then(Value::as_str) {
        out.set("sound", "bus", Toml::String(bus.to_string()));
    }
    out.set("sound", "positional", Toml::Boolean(class == "AudioStreamPlayer2D"));
}

fn light(class: &str, section: &Section, out: &mut Mapped) {
    let kind = if class == "DirectionalLight2D" { "directional" } else { "point" };
    out.set("light2d", "kind", Toml::String(kind.into()));
    if let Some(color) = section.field("color").and_then(colour) {
        out.set("light2d", "color", color);
    }
    if let Some(energy) = section.field("energy").and_then(Value::as_f64) {
        out.set("light2d", "intensity", Toml::Float(energy));
    }
    if let Some(Value::Bool(on)) = section.field("shadow_enabled") {
        out.set("light2d", "shadows", Toml::Boolean(*on));
    }
    if class == "PointLight2D" {
        out.note("PointLight2D: its texture falloff is balaur's radius; set `light2d.radius` by eye");
    }
}

fn remote(section: &Section, out: &mut Mapped) {
    out.set("modifier2d", "kind", Toml::String("follow".into()));
    if let Some(path) = section
        .field("remote_path")
        .and_then(|p| p.call("NodePath"))
        .and_then(|a| a.first())
        .and_then(Value::as_str)
    {
        out.note(format!(
            "RemoteTransform2D pushes onto `{path}`; `follow` pulls, so it belongs on that node"
        ));
    }
}

/// A Control as a widget: its kind, its caption, its place, and the
/// properties each kind reads.
fn widget(class: &str, section: &Section, parent: &str, res: &Resources<'_>, out: &mut Mapped) {
    let kind = WIDGET_KINDS
        .iter()
        .find(|(godot, _)| *godot == class)
        .map_or("panel", |(_, kind)| *kind);
    out.set("widget", "kind", Toml::String(kind.into()));
    let text = |key| section.field(key).and_then(Value::as_str).map(str::to_string);
    let number = |key| section.field(key).and_then(Value::as_f64);

    if let Some(caption) = text("text").or_else(|| text("title")) {
        out.set("widget", "text", Toml::String(caption));
    }
    if class == "RichTextLabel" {
        out.set("widget", "markup", Toml::Boolean(true));
    }
    if class == "BoxContainer" && section.field("vertical") == Some(&Value::Bool(true)) {
        out.set("widget", "kind", Toml::String("column".into()));
    }
    if let Some(role) = text("theme_type_variation") {
        out.set("widget", "role", Toml::String(role));
    }
    if let Some(tooltip) = text("tooltip_text") {
        out.set("widget", "tooltip", Toml::String(tooltip));
    }
    if let Some(Value::Bool(on)) = section.field("disabled") {
        out.set("widget", "disabled", Toml::Boolean(*on));
    }
    if section.field("autowrap_mode").and_then(Value::as_i64).is_some_and(|m| m != 0) {
        out.set("widget", "wrap", Toml::Boolean(true));
    }
    if let Some(align) = section.field("horizontal_alignment").and_then(Value::as_i64) {
        let align = match align {
            1 => "center",
            2 => "end",
            _ => "start",
        };
        out.set("widget", "text_align", Toml::String(align.into()));
    }
    if let Some(size) = number("theme_override_font_sizes/font_size") {
        out.set("widget", "font_size", Toml::Float(size));
    }
    if let Some(color) = section.field("theme_override_colors/font_color").and_then(colour) {
        out.set("widget", "text_color", color);
    }
    if let Some(gap) = number("theme_override_constants/separation")
        .or_else(|| number("theme_override_constants/h_separation"))
    {
        out.set("widget", "gap", Toml::Float(gap.max(0.0)));
    }
    let margins: Vec<f64> = ["left", "top", "right", "bottom"]
        .iter()
        .filter_map(|side| number(&format!("theme_override_constants/margin_{side}")))
        .collect();
    if let Some(most) = margins.iter().copied().reduce(f64::max) {
        out.set("widget", "padding", Toml::Float(most.max(0.0)));
        if margins.iter().any(|m| (m - most).abs() > 0.5) {
            out.note("MarginContainer with unequal margins: balaur pads evenly, at the largest");
        }
    }
    if let Some([w, h]) = section.field("custom_minimum_size").and_then(pair) {
        if w > 0.0 {
            out.set("widget", "min_width", Toml::Float(w));
        }
        if h > 0.0 {
            out.set("widget", "min_height", Toml::Float(h));
        }
    }
    // EXPAND is bit 2; the flag a container reads is the one along its axis.
    let flag = match parent {
        "HBoxContainer" | "HFlowContainer" => number("size_flags_horizontal"),
        "VBoxContainer" | "VFlowContainer" => number("size_flags_vertical"),
        _ => None,
    };
    if flag.is_some_and(|f| (f as i64) & 2 != 0) {
        out.set("widget", "grow", Toml::Float(1.0));
    }
    if family(parent) != Family::Control {
        placement(section, out);
    }
    match class {
        "CheckBox" | "CheckButton" => {
            if let Some(Value::Bool(on)) = section.field("button_pressed") {
                out.set("widget", "checked", Toml::Boolean(*on));
            }
            if let Some(group) = section.field("button_group") {
                let name = group
                    .call("SubResource")
                    .or_else(|| group.call("ExtResource"))
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                    .unwrap_or("group");
                out.set("widget", "group", Toml::String(name.to_string()));
            }
        }
        "LineEdit" | "SpinBox" | "TextEdit" => {
            if let Some(hint) = text("placeholder_text") {
                out.set("widget", "placeholder", Toml::String(hint));
            }
            if let Some(Value::Bool(on)) = section.field("secret") {
                out.set("widget", "secret", Toml::Boolean(*on));
            }
            if let Some(length) = number("max_length") {
                out.set("widget", "max_length", Toml::Float(length));
            }
            if class == "SpinBox" {
                out.set("widget", "numeric", Toml::Boolean(true));
            }
        }
        "OptionButton" => {
            let options: Vec<Toml> = (0..)
                .map_while(|i| text(&format!("popup/item_{i}/text")))
                .map(Toml::String)
                .collect();
            let selected = number("selected").map(|i| i as usize);
            if let Some(Toml::String(chosen)) = selected.and_then(|i| options.get(i)) {
                out.set("widget", "text", Toml::String(chosen.clone()));
            }
            out.set("widget", "options", Toml::Array(options));
        }
        "HSlider" | "VSlider" | "ProgressBar" | "TextureProgressBar" => {
            for (godot, here) in [("min_value", "min"), ("max_value", "max"), ("step", "step"), ("value", "value")] {
                if let Some(n) = number(godot) {
                    out.set("widget", here, Toml::Float(n));
                }
            }
            if !section.fields.iter().any(|(k, _)| k == "max_value") {
                out.set("widget", "max", Toml::Float(100.0));
            }
        }
        "TextureRect" | "TextureButton" | "NinePatchRect" => {
            let key = if class == "TextureButton" { "texture_normal" } else { "texture" };
            if let Some(path) = section.field(key).and_then(|t| res.path(t)) {
                out.set("widget", "source", Toml::String(image_path(path, out)));
            }
            if class == "NinePatchRect" {
                let slice: Vec<f64> = ["left", "top", "right", "bottom"]
                    .iter()
                    .map(|side| number(&format!("patch_margin_{side}")).unwrap_or(0.0))
                    .collect();
                out.set("widget", "slice", floats(&slice));
            }
            if class == "TextureButton" {
                out.note("TextureButton: name its `on_click` handler, which is what makes a picture a button here");
            }
        }
        "ColorRect" => {
            if let Some(color) = section.field("color").and_then(colour) {
                out.set("widget", "fill", Toml::String(hex(&color)));
            }
        }
        "GridContainer" => {
            if let Some(columns) = number("columns") {
                out.set("widget", "columns", Toml::Integer(columns as i64));
            }
        }
        "FoldableContainer" => {
            if let Some(Value::Bool(folded)) = section.field("folded") {
                out.set("widget", "open", Toml::Boolean(!*folded));
            }
        }
        "CenterContainer" => {
            out.set("widget", "align", Toml::String("center".into()));
            out.set("widget", "justify", Toml::String("center".into()));
        }
        "Window" => out.note("Window: a second OS window is not planned; kept as a dialog"),
        _ => {}
    }
    if section.field("icon").is_some() && matches!(kind, "button") {
        out.note("Button icon: a texture icon has no slot; `icon` takes a glyph from the theme");
    }
    if section.field("theme").is_some() {
        out.note("a Theme resource: convert it to a `widget_theme` and name it in `theme`");
    }
}

/// A root Control's anchor preset and offsets as a widget's anchor, `x`, `y`
/// and size. Inside a container a Control is placed by it, so only a Control
/// whose parent is not one reaches here.
fn placement(section: &Section, out: &mut Mapped) {
    let number = |key| section.field(key).and_then(Value::as_f64).unwrap_or(0.0);
    let preset = section.field("anchors_preset").and_then(Value::as_i64).unwrap_or(0);
    let anchor = match preset {
        1 => "top_right",
        2 => "bottom_left",
        3 => "bottom_right",
        4 => "center_left",
        5 => "center_top",
        6 => "center_right",
        7 => "center_bottom",
        8 => "center",
        15 => "fill",
        9..=14 => {
            out.note(format!(
                "anchor preset {preset} stretches one axis, which a widget cannot yet; kept at its corner"
            ));
            "top_left"
        }
        _ => "top_left",
    };
    out.set("widget", "anchor", Toml::String(anchor.into()));
    let (left, top, right, bottom) = (
        number("offset_left"),
        number("offset_top"),
        number("offset_right"),
        number("offset_bottom"),
    );
    if anchor == "fill" {
        out.set("widget", "inset", floats(&[left, top, -right, -bottom]));
        return;
    }
    let (width, height) = (right - left, bottom - top);
    if width > 0.0 {
        out.set("widget", "width", Toml::Float(width));
    }
    if height > 0.0 {
        out.set("widget", "height", Toml::Float(height));
    }
    // An anchor on the far edge measures inward, so its offset is the far
    // edge's; a centred axis is measured from the middle of the box.
    let x = match anchor {
        "top_right" | "bottom_right" | "center_right" => -right,
        "center" | "center_top" | "center_bottom" => (left + right) / 2.0,
        _ => left,
    };
    let y = match anchor {
        "bottom_left" | "bottom_right" | "center_bottom" => -bottom,
        "center" | "center_left" | "center_right" => (top + bottom) / 2.0,
        _ => top,
    };
    out.set("widget", "x", Toml::Float(x));
    out.set("widget", "y", Toml::Float(y));
}

/// A texture's project path. An SVG is refused here: balaur rasterises
/// nothing, so the scene names the PNG it expects to find beside it.
fn image_path(path: &str, out: &mut Mapped) -> String {
    match path.strip_suffix(".svg") {
        Some(stem) => {
            out.note(format!("{path}: export it as {stem}.png, which is what the scene now names"));
            format!("{stem}.png")
        }
        None => path.to_string(),
    }
}

fn pair(value: &Value) -> Option<[f64; 2]> {
    match value.numbers()?.as_slice() {
        [x, y] => Some([*x, *y]),
        _ => None,
    }
}

/// A `PackedVector2Array` or a list of `Vector2`s, as points.
pub(crate) fn points_of(value: &Value) -> Vec<[f64; 2]> {
    let flat = value
        .call("PackedVector2Array")
        .and_then(|args| args.iter().map(Value::as_f64).collect::<Option<Vec<_>>>());
    if let Some(flat) = flat {
        return flat.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    }
    value
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(pair)
        .collect()
}

pub(crate) fn colour(value: &Value) -> Option<Toml> {
    let channels = value.call("Color")?.iter().map(Value::as_f64).collect::<Option<Vec<_>>>()?;
    match channels[..] {
        [r, g, b] => Some(floats(&[r, g, b, 1.0])),
        [r, g, b, a] => Some(floats(&[r, g, b, a])),
        _ => None,
    }
}

fn hex(color: &Toml) -> String {
    let channel = |i: usize| {
        color
            .as_array()
            .and_then(|a| a.get(i))
            .and_then(Toml::as_float)
            .map_or(255, |v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    format!("#{:02x}{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2), channel(3))
}

pub(crate) fn floats(values: &[f64]) -> Toml {
    Toml::Array(values.iter().map(|v| Toml::Float(*v)).collect())
}
