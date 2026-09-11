//! An `AnimationPlayer`'s libraries as one `animation_clip` file.
//!
//! Godot keys a node property along a `NodePath("Target:property")`; a clip
//! here keys `target` and `property` apart, relative to the same root, so the
//! paths carry and only the values and the property names change. The class
//! of each target decides both, since a Sprite2D's `self_modulate` is its
//! `sprite/color` and a Polygon2D's its `polygon/color`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::import_godot::{Section, Value};
use crate::import_godot_nodes::{Family, PIXELS_PER_UNIT, Resources, colour, family, floats};

/// The clip file a player's libraries made, and what would not carry.
pub(crate) struct Clips {
    pub toml: String,
    pub notes: Vec<String>,
}

/// Convert one player. `player` is its scene path and `classes` every node's
/// class by scene path, so a track's target can be looked up.
pub(crate) fn convert(
    section: &Section,
    player: &str,
    classes: &BTreeMap<String, String>,
    res: &Resources<'_>,
) -> Option<Clips> {
    let mut notes = Vec::new();
    let root = section
        .field("root_node")
        .and_then(node_path)
        .unwrap_or_else(|| "..".to_string());
    let base = join(player, &root);
    let mut clips: Vec<(String, Toml)> = Vec::new();
    let Some(Value::Dict(libraries)) = section.field("libraries") else {
        return None;
    };
    for (name, library) in libraries {
        let prefix = name.as_str().filter(|n| !n.is_empty());
        let loaded;
        let (data, lookup): (&Section, &Resources<'_>) = if let Some(sub) = res.sub(library) {
            (sub, res)
        } else if let Some(path) = res.path(library) {
            let Some(found) = load_library(res, path) else {
                notes.push(format!("animation library {path} would not load"));
                continue;
            };
            loaded = found;
            (&loaded.0, &loaded.1)
        } else {
            continue;
        };
        let Some(Value::Dict(animations)) = data.field("_data") else {
            continue;
        };
        for (clip_name, animation) in animations {
            let Some(animation) = lookup.sub(animation) else {
                continue;
            };
            let clip_name = clip_name.as_str().unwrap_or("clip");
            let name = match prefix {
                Some(library) => format!("{library}/{clip_name}"),
                None => clip_name.to_string(),
            };
            let clip = clip(animation, &base, classes, &name, &mut notes);
            clips.push((name, clip));
        }
    }
    if clips.is_empty() {
        return None;
    }
    let mut named = toml::Table::new();
    for (name, clip) in clips {
        named.insert(name, clip);
    }
    let mut document = toml::Table::new();
    document.insert("type".into(), Toml::String("animation_clip".into()));
    document.insert("clips".into(), Toml::Table(named));
    let mut toml = String::new();
    let _ = writeln!(
        toml,
        "# Converted from a Godot AnimationPlayer by `balaur import`."
    );
    toml.push_str(&toml::to_string(&Toml::Table(document)).ok()?);
    Some(Clips { toml, notes })
}

/// A library saved as its own `.tres`: its `[resource]` section, and the
/// animations it declares as sub-resources.
fn load_library<'a>(res: &Resources<'a>, path: &str) -> Option<(Section, Resources<'a>)> {
    let text = std::fs::read_to_string(res.root.join(path)).ok()?;
    let document = crate::import_godot::parse(&text).ok()?;
    let resource = document.first("resource")?.clone();
    let internal = document
        .sections
        .into_iter()
        .filter(|s| s.kind == "sub_resource")
        .filter_map(|s| Some((s.attr_str("id")?.to_string(), s)))
        .collect();
    let lookup = Resources {
        external: BTreeMap::new(),
        internal,
        root: res.root,
        keys: res.keys,
    };
    Some((resource, lookup))
}

fn clip(
    animation: &Section,
    base: &str,
    classes: &BTreeMap<String, String>,
    name: &str,
    notes: &mut Vec<String>,
) -> Toml {
    let mut clip = toml::Table::new();
    if let Some(length) = animation.field("length").and_then(Value::as_f64) {
        clip.insert("length".into(), Toml::Float(length));
    }
    let wrap = match animation.field("loop_mode").and_then(Value::as_i64) {
        Some(1) => "loop",
        Some(2) => "pingpong",
        _ => "none",
    };
    clip.insert("loop".into(), Toml::String(wrap.into()));
    let mut tracks = Vec::new();
    let mut eased = false;
    for index in 0.. {
        let field = |key: &str| animation.field(&format!("tracks/{index}/{key}"));
        let Some(kind) = field("type").and_then(Value::as_str) else {
            break;
        };
        if field("enabled") == Some(&Value::Bool(false)) {
            continue;
        }
        let Some(path) = field("path").and_then(node_path) else {
            continue;
        };
        let keys = field("keys");
        let (target, property) = path.split_once(':').unwrap_or((path.as_str(), ""));
        let target = if target == "." { "" } else { target };
        let class = classes
            .get(&join(base, target))
            .map_or("", String::as_str);
        let track = match kind {
            "value" => value_track(target, property, class, &field, keys, &mut eased, name, notes),
            "method" => method_track(target, keys),
            other => {
                notes.push(format!("clip `{name}`: a `{other}` track has no equivalent"));
                None
            }
        };
        if let Some(track) = track {
            tracks.push(Toml::Table(track));
        }
    }
    if eased {
        notes.push(format!(
            "clip `{name}`: its keys carry Godot transition curves; they play linear here"
        ));
    }
    clip.insert("tracks".into(), Toml::Array(tracks));
    Toml::Table(clip)
}

#[allow(clippy::too_many_arguments)]
fn value_track<'a>(
    target: &str,
    property: &str,
    class: &str,
    field: &dyn Fn(&str) -> Option<&'a Value>,
    keys: Option<&'a Value>,
    eased: &mut bool,
    clip: &str,
    notes: &mut Vec<String>,
) -> Option<toml::Table> {
    let control = family(class) == Family::Control;
    let here = match property {
        "position" | "rotation" | "scale" if control => None,
        "position" => Some("position".to_string()),
        "rotation" => Some("rotation_euler".to_string()),
        "scale" => Some("scale".to_string()),
        "modulate" => Some("tint".to_string()),
        "visible" => Some("visible".to_string()),
        "self_modulate" | "color" => match class {
            "Sprite2D" => Some("sprite/color".to_string()),
            "Polygon2D" => Some("polygon/color".to_string()),
            "Line2D" => Some("shape2d/color".to_string()),
            _ => None,
        },
        "frame" if class == "Sprite2D" => Some("sprite/frame".to_string()),
        "value" if control => Some("widget/value".to_string()),
        "zoom" if class == "Camera2D" => Some("camera/zoom".to_string()),
        _ => None,
    };
    let Some(here) = here else {
        notes.push(format!(
            "clip `{clip}`: `{property}` on {} has no track here",
            if class.is_empty() { "an unknown node" } else { class }
        ));
        return None;
    };
    let Some(Value::Dict(pairs)) = keys else {
        return None;
    };
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k.as_str() == Some(key))
            .map(|(_, v)| v)
    };
    let times = get("times").and_then(Value::numbers).unwrap_or_default();
    let values = get("values").and_then(Value::as_array).unwrap_or_default();
    if get("transitions")
        .and_then(Value::numbers)
        .is_some_and(|t| t.iter().any(|v| (v - 1.0).abs() > 1e-6))
    {
        *eased = true;
    }
    let discrete = get("update").and_then(Value::as_i64) == Some(1);
    let interp = match field("interp").and_then(Value::as_i64) {
        _ if discrete => "step",
        Some(0) => "step",
        Some(2 | 4) => "cubic",
        _ => "linear",
    };
    let mut out_keys = Vec::new();
    for (t, value) in times.iter().zip(values) {
        let Some(value) = key_value(&here, value) else {
            continue;
        };
        let mut key = toml::Table::new();
        key.insert("t".into(), Toml::Float(*t));
        key.insert("value".into(), value);
        out_keys.push(Toml::Table(key));
    }
    if out_keys.is_empty() {
        return None;
    }
    let mut track = toml::Table::new();
    track.insert("target".into(), Toml::String(target.to_string()));
    track.insert("property".into(), Toml::String(here));
    track.insert("interp".into(), Toml::String(interp.into()));
    track.insert("keys".into(), Toml::Array(out_keys));
    Some(track)
}

/// One key's value in the units and the handedness the property takes here.
fn key_value(property: &str, value: &Value) -> Option<Toml> {
    let pair = || match value.numbers()?.as_slice() {
        [x, y] => Some([*x, *y]),
        _ => None,
    };
    match property {
        "position" => {
            let [x, y] = pair()?;
            Some(floats(&[x / PIXELS_PER_UNIT, -y / PIXELS_PER_UNIT, 0.0]))
        }
        "rotation_euler" => Some(floats(&[0.0, 0.0, -value.as_f64()?])),
        "scale" => {
            let [x, y] = pair()?;
            Some(floats(&[x, y, 1.0]))
        }
        "visible" => match value {
            Value::Bool(on) => Some(Toml::Float(if *on { 1.0 } else { 0.0 })),
            _ => None,
        },
        "camera/zoom" => Some(Toml::Float(pair()?[0])),
        "sprite/frame" | "widget/value" => Some(Toml::Float(value.as_f64()?)),
        _ => colour(value),
    }
}

fn method_track(target: &str, keys: Option<&Value>) -> Option<toml::Table> {
    let Some(Value::Dict(pairs)) = keys else {
        return None;
    };
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k.as_str() == Some(key))
            .map(|(_, v)| v)
    };
    let times = get("times").and_then(Value::numbers).unwrap_or_default();
    let values = get("values").and_then(Value::as_array).unwrap_or_default();
    let mut out_keys = Vec::new();
    for (t, value) in times.iter().zip(values) {
        let Value::Dict(call) = value else { continue };
        let Some(method) = call
            .iter()
            .find(|(k, _)| k.as_str() == Some("method"))
            .and_then(|(_, v)| v.as_str())
        else {
            continue;
        };
        let mut key = toml::Table::new();
        key.insert("t".into(), Toml::Float(*t));
        key.insert("call".into(), Toml::String(method.to_string()));
        out_keys.push(Toml::Table(key));
    }
    if out_keys.is_empty() {
        return None;
    }
    let mut track = toml::Table::new();
    track.insert("target".into(), Toml::String(target.to_string()));
    track.insert("keys".into(), Toml::Array(out_keys));
    Some(track)
}

/// A `NodePath("…")` or `^"…"` as its text.
pub(crate) fn node_path(value: &Value) -> Option<String> {
    value
        .call("NodePath")
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .or_else(|| value.as_str())
        .map(str::to_string)
}

/// A path under a scene path, with `.` and `..` folded: the scene path of the
/// node `relative` names when read from `from`. The scene root is `""`.
pub(crate) fn join(from: &str, relative: &str) -> String {
    let mut parts: Vec<&str> = from.split('/').filter(|p| !p.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    parts.join("/")
}
