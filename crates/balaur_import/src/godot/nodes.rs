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

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::{Section, Value};

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
    /// What the whole project knows, which every scene reads the same way.
    pub project: &'a Project,
}

/// Lookups built once over the whole Godot project.
#[derive(Default)]
pub(crate) struct Project {
    /// Every translation key, so a caption that is one draws as `text_key`,
    /// which is what Godot's auto-translation made of it.
    pub keys: BTreeSet<String>,
    /// `uid://` to project path, for a resource loaded from its own file.
    pub uids: BTreeMap<String, String>,
    /// An SVG's path to the raster Godot imported it as, written beside it.
    pub rasters: BTreeMap<String, String>,
    /// The project's own `class_name`s, to the class each extends.
    pub classes: crate::godot::exports::Classes,
    /// Every `.gdshader` that translated and compiles, by its Godot path.
    pub shaders: BTreeMap<String, std::rc::Rc<crate::godot::material::Shader>>,
}

impl Resources<'_> {
    /// The path an `ExtResource("id")` names.
    pub(crate) fn path(&self, value: &Value) -> Option<&str> {
        let id = value.call("ExtResource")?.first()?.as_str()?;
        self.external.get(id).map(|(_, path)| path.as_str())
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

/// A `.tres` or `.tscn`'s declared resources, resolved the way a scene's are.
pub(crate) fn resources_of<'a>(
    document: &crate::godot::Document,
    root: &'a Path,
    project: &'a Project,
) -> Resources<'a> {
    let uids = &project.uids;
    let mut external = BTreeMap::new();
    for section in document.each("ext_resource") {
        let Some(id) = section.attr_str("id") else {
            continue;
        };
        // Godot follows the uid and only falls back on the path, which goes
        // stale when a file moves; this reads them in the same order.
        let by_uid = section
            .attr_str("uid")
            .and_then(|uid| uids.get(uid))
            .cloned();
        let by_path = section
            .attr_str("path")
            .map(|p| p.strip_prefix("res://").unwrap_or(p).to_string());
        let Some(path) = by_uid.or(by_path) else {
            continue;
        };
        let kind = section.attr_str("type").unwrap_or_default().to_string();
        external.insert(id.to_string(), (kind, path));
    }
    let internal = document
        .each("sub_resource")
        .filter_map(|s| Some((s.attr_str("id")?.to_string(), s.clone())))
        .collect();
    Resources {
        external,
        internal,
        root,
        project,
    }
}

/// A resource an `ExtResource` names, parsed with its own declarations, or
/// `None` when it is a sub-resource or will not load.
pub(crate) fn load<'a>(
    res: &Resources<'a>,
    value: &Value,
) -> Option<(crate::godot::Document, Resources<'a>)> {
    let path = res.path(value)?;
    let text = std::fs::read_to_string(res.root.join(path)).ok()?;
    let document = crate::godot::parse(&text).ok()?;
    let nested = resources_of(&document, res.root, res.project);
    Some((document, nested))
}

/// What one node became.
#[derive(Default)]
pub(crate) struct Mapped {
    /// `visible`, `tint`, `z_index`: keys every node has.
    pub keys: toml::Table,
    /// Component name to its table.
    pub components: toml::Table,
    /// Inline `[[assets]]`; the scene walker gives each an id and writes
    /// the `#id` into the component property that names it.
    pub assets: Vec<Asset>,
    /// Nodes this one becomes the parent of, for a Godot node that is more
    /// than one node here: a tile layer over several atlases, for one.
    pub children: Vec<(String, Mapped)>,
    pub notes: Vec<String>,
    /// Files the node needs written beside the scene, as `(path, text)`: a
    /// shader saved inside the scene, for one.
    pub files: Vec<(String, String)>,
}

/// One inline asset, and the component property that points at it.
pub(crate) struct Asset {
    pub component: &'static str,
    pub key: &'static str,
    pub table: toml::Table,
}

impl Mapped {
    pub(crate) fn set(&mut self, component: &str, key: &str, value: Toml) {
        let table = self
            .components
            .entry(component)
            .or_insert_with(|| Toml::Table(toml::Table::new()));
        if let Toml::Table(table) = table {
            table.insert(key.to_string(), value);
        }
    }

    pub(crate) fn touch(&mut self, component: &str) {
        self.components
            .entry(component)
            .or_insert_with(|| Toml::Table(toml::Table::new()));
    }

    pub(crate) fn note(&mut self, text: impl Into<String>) {
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
    if crate::godot::controls::is_widget(class) || class == "Control" {
        return Family::Control;
    }
    if PLAIN.contains(&class) {
        return Family::Plain;
    }
    Family::Node2d
}

/// Classes whose transform is all they are.
const BARE: &[&str] = &["Node", "Node2D", "Marker2D", "CanvasLayer", "Skeleton2D"];

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

/// Map one node. `parent` is the class of the node above it, which decides
/// which of a Control's two size flags is the one its container reads.
pub(crate) fn map(class: &str, section: &Section, parent: &str, res: &Resources<'_>) -> Mapped {
    let mut out = Mapped::default();
    node_keys(section, &mut out);
    match family(class) {
        Family::Node2d => transform(section, &mut out),
        Family::Control => {
            crate::godot::controls::widget(class, section, parent, res, &mut out);
        }
        Family::Plain => {}
    }
    match class {
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
        // A tree holding libraries of its own plays them itself; the state
        // machine over them is the scene walker's, beside the clips.
        "AnimationTree" => {
            if section
                .fields
                .iter()
                .any(|(key, _)| key.starts_with("libraries"))
            {
                out.touch("animation");
            }
        }
        "TileMapLayer" => crate::godot::tiles::layer(section, res, &mut out),
        "TileMap" => {
            out.note(
                "TileMap: Godot 4.3 split it into TileMapLayers; resave the scene there first",
            );
        }
        "Timer" => timer(section, &mut out),
        // A Control is its widget and these are their transform; neither is
        // anything more.
        other if family(other) == Family::Control || BARE.contains(&other) => {}
        other => out.note(format!(
            "{other}: no balaur equivalent; kept as a plain node"
        )),
    }
    if parent == "Area2D" && out.components.contains_key("collider2d") {
        out.set("collider2d", "sensor", Toml::Boolean(true));
    }
    if let Some(material) = section.field("material")
        && (res.sub(material).is_some() || res.path(material).is_some())
    {
        if family(class) == Family::Control {
            out.note(
                "a material on a Control: widgets draw through the UI layer, which runs no shader",
            );
        } else {
            crate::godot::material::attach(material, res, &mut out);
        }
    }
    out
}

/// `visible`, `modulate` and `z_index`, which every node has here too.
fn node_keys(section: &Section, out: &mut Mapped) {
    if let Some(visible) = section.field("visible")
        && let Value::Bool(on) = visible
    {
        out.keys.insert("visible".into(), Toml::Boolean(*on));
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
    metadata(section, out);
}

/// Godot's `metadata/<key>`, which is the `meta` component here. The editor's
/// own bookkeeping keys (`_edit_`, `_tab_index`) are not the game's.
fn metadata(section: &Section, out: &mut Mapped) {
    for (key, value) in &section.fields {
        let Some(name) = key.strip_prefix("metadata/") else {
            continue;
        };
        if name.starts_with('_') {
            continue;
        }
        if let Some(plain) = scalar(value) {
            out.set("meta", name, plain);
        }
    }
}

/// A Godot value simple enough to be component data: TOML has no object.
fn scalar(value: &Value) -> Option<Toml> {
    match value {
        Value::Bool(b) => Some(Toml::Boolean(*b)),
        Value::Int(i) => Some(Toml::Integer(*i)),
        Value::Float(f) => Some(Toml::Float(*f)),
        Value::Str(s) => Some(Toml::String(s.clone())),
        _ => None,
    }
}

fn transform(section: &Section, out: &mut Mapped) {
    if let Some([x, y]) = section.field("position").and_then(pair) {
        out.set(
            "transform",
            "position",
            floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT, 0.0]),
        );
    }
    if let Some(angle) = section.field("rotation").and_then(Value::as_f64) {
        // y flips, so a turn one way in Godot is the other way here.
        out.set("transform", "rotation_euler", floats(&[0.0, 0.0, -angle]));
    }
    if let Some([x, y]) = section.field("scale").and_then(pair) {
        out.set("transform", "scale", floats(&[x, y, 1.0]));
    }
    // y flips, so the y axis leans the other way too.
    if let Some(skew) = section
        .field("skew")
        .and_then(Value::as_f64)
        .filter(|s| *s != 0.0)
    {
        out.set("transform", "skew", Toml::Float(-skew));
    }
}

fn sprite(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("sprite");
    let texture = section.field("texture").and_then(|t| res.path(t));
    if let Some(path) = texture {
        let texture = image_path(path, res, out);
        out.set("sprite", "texture", Toml::String(texture));
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
    if let Some(rect) = section
        .field("region_rect")
        .and_then(Value::numbers)
        .filter(|_| region)
        && let [x, y, w, h] = rect[..]
    {
        out.set("sprite", "region_origin", floats(&[x, y]));
        out.set("sprite", "region_size", floats(&[w, h]));
    }
    let frames = |key: &str| section.field(key).and_then(Value::as_i64);
    let (columns, rows) = (frames("hframes"), frames("vframes"));
    if columns.unwrap_or(1) > 1 || rows.unwrap_or(1) > 1 {
        out.set(
            "sprite",
            "columns",
            Toml::Float(columns.unwrap_or(1) as f64),
        );
        out.set("sprite", "rows", Toml::Float(rows.unwrap_or(1) as f64));
        if let Some(frame) = frames("frame") {
            out.set("sprite", "frame", Toml::Float(frame as f64));
        }
    }
    // Both engines measure `offset` in texture pixels, y down.
    if section.field("centered") == Some(&Value::Bool(false)) {
        out.set("sprite", "centered", Toml::Boolean(false));
    }
    if let Some([x, y]) = section
        .field("offset")
        .and_then(pair)
        .filter(|[x, y]| *x != 0.0 || *y != 0.0)
    {
        out.set("sprite", "offset", floats(&[x, y]));
    }
}

/// A Timer as the `timer` component, which emits `timeout` as Godot's does.
fn timer(section: &Section, out: &mut Mapped) {
    out.touch("timer");
    if let Some(wait) = section.field("wait_time").and_then(Value::as_f64) {
        out.set("timer", "wait_time", Toml::Float(wait));
    }
    for key in ["one_shot", "autostart"] {
        if let Some(Value::Bool(on)) = section.field(key) {
            out.set("timer", key, Toml::Boolean(*on));
        }
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
        let texture = image_path(path, res, out);
        out.set("polygon", "texture", Toml::String(texture));
    }
    let color = section.field("color").and_then(colour);
    let tint = section.field("self_modulate").and_then(colour);
    out.set(
        "polygon",
        "color",
        tint.or(color)
            .unwrap_or_else(|| floats(&[1.0, 1.0, 1.0, 1.0])),
    );

    let mut mesh = toml::Table::new();
    mesh.insert("type".into(), Toml::String("mesh".into()));
    let positions = points
        .iter()
        .map(|[x, y]| {
            floats(&[
                (x + offset[0]) / PIXELS_PER_UNIT,
                -(y + offset[1]) / PIXELS_PER_UNIT,
            ])
        })
        .collect();
    mesh.insert("positions".into(), Toml::Array(positions));
    if let Some(internal) = section
        .field("internal_vertex_count")
        .and_then(Value::as_i64)
        && internal > 0
    {
        mesh.insert("internal".into(), Toml::Integer(internal));
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
    let uvs = section
        .field("uv")
        .map(points_of)
        .filter(|u| u.len() == points.len());
    let size = texture.and_then(|p| res.image_size(p));
    match (size, texture) {
        (Some((w, h)), _) => {
            let texture_offset = section
                .field("texture_offset")
                .and_then(pair)
                .unwrap_or([0.0, 0.0]);
            let source = uvs.unwrap_or_else(|| points.clone());
            let uvs = source
                .iter()
                .map(|[u, v]| {
                    floats(&[
                        (u + texture_offset[0]) / f64::from(w),
                        (v + texture_offset[1]) / f64::from(h),
                    ])
                })
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
    out.assets.push(Asset {
        component: "polygon",
        key: "mesh",
        table: mesh,
    });
}

/// `bones = [NodePath, PackedFloat32Array, …]` as `skin.bones`. The paths
/// are relative to the rig in both engines, so they carry as they are.
fn skin(section: &Section, vertices: usize) -> Option<Toml> {
    let flat = section.field("bones")?.as_array()?;
    let mut bones = Vec::new();
    for pair in flat.chunks(2) {
        let [path, weights] = pair else { continue };
        let path = path
            .call("NodePath")
            .and_then(|a| a.first())
            .and_then(Value::as_str)?;
        let weights = weights.numbers()?;
        if weights.len() != vertices {
            continue;
        }
        let mut bone = toml::Table::new();
        bone.insert("path".into(), Toml::String(path.to_string()));
        bone.insert(
            "weights".into(),
            Toml::Array(weights.into_iter().map(Toml::Float).collect()),
        );
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
    let width = section
        .field("width")
        .and_then(Value::as_f64)
        .unwrap_or(10.0);
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
    let (x, y, angle) = if let Some([a, b, _, _, ox, oy]) = rest.as_deref() {
        (*ox, *oy, balaur_core::libm::atan2(*b, *a))
    } else {
        let [x, y] = section
            .field("position")
            .and_then(pair)
            .unwrap_or([0.0, 0.0]);
        (
            x,
            y,
            section
                .field("rotation")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        )
    };
    out.set(
        "bone2d",
        "rest_position",
        floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT]),
    );
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
    let number = |key: &str| shape.field(key).and_then(Value::as_f64);
    match shape.attr_str("type").unwrap_or_default() {
        "RectangleShape2D" => {
            let [w, h] = shape.field("size").and_then(pair).unwrap_or([20.0, 20.0]);
            out.set("collider2d", "kind", Toml::String("rect".into()));
            out.set(
                "collider2d",
                "half_extents",
                floats(&[w / 2.0 / PIXELS_PER_UNIT, h / 2.0 / PIXELS_PER_UNIT]),
            );
        }
        "CircleShape2D" => {
            out.set("collider2d", "kind", Toml::String("circle".into()));
            out.set(
                "collider2d",
                "radius",
                Toml::Float(number("radius").unwrap_or(10.0) / PIXELS_PER_UNIT),
            );
        }
        "CapsuleShape2D" => {
            out.set("collider2d", "kind", Toml::String("capsule".into()));
            out.set(
                "collider2d",
                "radius",
                Toml::Float(number("radius").unwrap_or(10.0) / PIXELS_PER_UNIT),
            );
            out.set(
                "collider2d",
                "height",
                Toml::Float(number("height").unwrap_or(30.0) / PIXELS_PER_UNIT),
            );
        }
        "SegmentShape2D" => {
            let a = shape.field("a").and_then(pair).unwrap_or([0.0, 0.0]);
            let b = shape.field("b").and_then(pair).unwrap_or([0.0, 10.0]);
            out.set("collider2d", "kind", Toml::String("segment".into()));
            out.set(
                "collider2d",
                "a",
                floats(&[a[0] / PIXELS_PER_UNIT, -a[1] / PIXELS_PER_UNIT]),
            );
            out.set(
                "collider2d",
                "b",
                floats(&[b[0] / PIXELS_PER_UNIT, -b[1] / PIXELS_PER_UNIT]),
            );
        }
        "WorldBoundaryShape2D" => {
            out.set("collider2d", "kind", Toml::String("halfspace".into()));
        }
        "ConvexPolygonShape2D" | "ConcavePolygonShape2D" => {
            let key = if shape.field("points").is_some() {
                "points"
            } else {
                "segments"
            };
            let points = shape.field(key).map(points_of).unwrap_or_default();
            polygon_collider(&points, "convex_hull", out);
        }
        other => out.note(format!(
            "{other}: no 2D collider of that shape; left as the default rect"
        )),
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
    out.assets.push(Asset {
        component: "collider2d",
        key: "mesh",
        table: mesh,
    });
}

fn particles(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    out.touch("particles");
    let number = |key: &str| section.field(key).and_then(Value::as_f64);
    let lifetime = number("lifetime").unwrap_or(1.0);
    out.set("particles", "lifetime", Toml::Float(lifetime));
    if let Some(amount) = number("amount") {
        out.set(
            "particles",
            "rate",
            Toml::Float(amount / lifetime.max(0.05)),
        );
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
        let texture = image_path(path, res, out);
        out.set("particles", "texture", Toml::String(texture));
    }
    if let Some(color) = section.field("color").and_then(colour) {
        out.set("particles", "color", color);
    }
    // Godot's direction is a y-down vector; here it is an angle, 90 up.
    if let Some([x, y]) = section.field("direction").and_then(pair) {
        out.set(
            "particles",
            "angle",
            Toml::Float(balaur_core::libm::atan2(-y, x).to_degrees()),
        );
    }
    if let Some(spread) = number("spread") {
        out.set("particles", "spread", Toml::Float(spread));
    }
    if let Some([x, y]) = section.field("gravity").and_then(pair) {
        out.set(
            "particles",
            "gravity",
            floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT]),
        );
    }
    let low = number("initial_velocity_min");
    let high = number("initial_velocity_max");
    if let Some(speed) = low
        .zip(high)
        .map(|(a, b)| f64::midpoint(a, b))
        .or(high)
        .or(low)
    {
        out.set("particles", "speed", Toml::Float(speed / PIXELS_PER_UNIT));
    }
    if let Some(scale) = number("scale_amount_max").or_else(|| number("scale_amount_min")) {
        out.set("particles", "size", Toml::Float((scale * 4.0).max(0.5)));
    }
    if section.field("color_ramp").is_some() || section.field("scale_amount_curve").is_some() {
        out.note("CPUParticles2D ramp or curve: only its start and end carry, as `color_end` and `size_end`");
    }
    if let Some(ramp) = section.field("color_ramp").and_then(|r| res.sub(r))
        && let Some(colors) = ramp.field("colors").and_then(Value::numbers)
        && let [.., r, g, b, a] = colors[..]
    {
        out.set("particles", "color_end", floats(&[r, g, b, a]));
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
        out.set(
            "sound",
            "volume",
            Toml::Float(balaur_core::libm::pow(10.0, db / 20.0)),
        );
    }
    if let Some(pitch) = section.field("pitch_scale").and_then(Value::as_f64) {
        out.set("sound", "pitch", Toml::Float(pitch));
    }
    if let Some(bus) = section.field("bus").and_then(Value::as_str) {
        out.set("sound", "bus", Toml::String(bus.to_string()));
    }
    out.set(
        "sound",
        "positional",
        Toml::Boolean(class == "AudioStreamPlayer2D"),
    );
}

fn light(class: &str, section: &Section, out: &mut Mapped) {
    let kind = if class == "DirectionalLight2D" {
        "directional"
    } else {
        "point"
    };
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
        out.note(
            "PointLight2D: its texture falloff is balaur's radius; set `light2d.radius` by eye",
        );
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

/// A texture's project path. An SVG is drawn from the raster Godot made of
/// it at import, which the importer writes beside it; with none, the scene
/// names the PNG it expects to find there.
pub(crate) fn image_path(path: &str, res: &Resources<'_>, out: &mut Mapped) -> String {
    if let Some(raster) = res.project.rasters.get(path) {
        return raster.clone();
    }
    match path.strip_suffix(".svg") {
        Some(stem) => {
            out.note(format!(
                "{path}: Godot kept no raster of it, so export it as {stem}.png, which the scene names"
            ));
            format!("{stem}.png")
        }
        None => path.to_string(),
    }
}

pub(crate) fn pair(value: &Value) -> Option<[f64; 2]> {
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
        return flat
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| [c[0], c[1]])
            .collect();
    }
    value
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(pair)
        .collect()
}

pub(crate) fn colour(value: &Value) -> Option<Toml> {
    let channels = value
        .call("Color")?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<_>>>()?;
    match channels[..] {
        [r, g, b] => Some(floats(&[r, g, b, 1.0])),
        [r, g, b, a] => Some(floats(&[r, g, b, a])),
        _ => None,
    }
}

pub(crate) fn hex(color: &Toml) -> String {
    let channel = |i: usize| {
        color
            .as_array()
            .and_then(|a| a.get(i))
            .and_then(Toml::as_float)
            .map_or(255, |v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        channel(0),
        channel(1),
        channel(2),
        channel(3)
    )
}

pub(crate) fn floats(values: &[f64]) -> Toml {
    Toml::Array(values.iter().map(|v| Toml::Float(*v)).collect())
}
