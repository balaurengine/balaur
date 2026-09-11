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
    /// Every clip's name, for checking what the player autoplays.
    pub names: Vec<String>,
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
    // Godot 4.7 writes each library as its own key, `libraries/` for the
    // default one; earlier 4.x wrote one `libraries = { name: … }` table.
    let mut libraries: Vec<(&str, &Value)> = section
        .fields
        .iter()
        .filter_map(|(key, value)| Some((key.strip_prefix("libraries/")?, value)))
        .collect();
    if let Some(Value::Dict(table)) = section.field("libraries") {
        libraries.extend(
            table
                .iter()
                .filter_map(|(name, value)| Some((name.as_str()?, value))),
        );
    }
    if libraries.is_empty() {
        return None;
    }
    for (name, library) in libraries {
        let prefix = Some(name).filter(|n| !n.is_empty());
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
    let names = clips.iter().map(|(name, _)| name.clone()).collect();
    let mut table = toml::Table::new();
    for (name, clip) in clips {
        table.insert(name, clip);
    }
    let mut document = toml::Table::new();
    document.insert("type".into(), Toml::String("animation_clip".into()));
    document.insert("clips".into(), Toml::Table(table));
    let mut toml = String::new();
    let _ = writeln!(
        toml,
        "# Converted from a Godot AnimationPlayer by `balaur import`."
    );
    toml.push_str(&toml::to_string(&Toml::Table(document)).ok()?);
    Some(Clips { toml, names, notes })
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
        project: res.project,
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
    // Godot's default is a second, and a clip whose tracks were all dropped
    // needs it written: here a length comes from the keys when it is not.
    let length = animation
        .field("length")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    clip.insert("length".into(), Toml::Float(length));
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
        let class = classes.get(&join(base, target)).map_or("", String::as_str);
        let track = match kind {
            "value" => value_track(
                target, property, class, &field, keys, &mut eased, name, notes,
            ),
            "method" => method_track(target, keys),
            other => {
                notes.push(format!(
                    "clip `{name}`: a `{other}` track has no equivalent"
                ));
                None
            }
        };
        if let Some(track) = track {
            tracks.push(Toml::Table(track));
        }
    }
    if eased {
        notes.push(format!(
            "clip `{name}`: a Godot transition curve had no exact easing here and was approximated"
        ));
    }
    clip.insert("tracks".into(), Toml::Array(tracks));
    Toml::Table(clip)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one track's context, read once here rather than bundled for a single caller"
)]
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
        "offset" if class == "Sprite2D" => Some("sprite/offset".to_string()),
        "skew" if !control => Some("transform/skew".to_string()),
        "theme_type_variation" if control => Some("widget/role".to_string()),
        "button_pressed" if control => Some("widget/checked".to_string()),
        "text" if control => Some("widget/text".to_string()),
        "theme_override_font_sizes/font_size" if control => Some("widget/font_size".to_string()),
        "value" if control => Some("widget/value".to_string()),
        "zoom" if class == "Camera2D" => Some("camera/zoom".to_string()),
        _ => None,
    };
    let Some(here) = here else {
        notes.push(format!(
            "clip `{clip}`: `{property}` on {} has no track here",
            if class.is_empty() {
                "an unknown node"
            } else {
                class
            }
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
    let transitions = get("transitions")
        .and_then(Value::numbers)
        .unwrap_or_default();
    let discrete = get("update").and_then(Value::as_i64) == Some(1);
    let interp = match field("interp").and_then(Value::as_i64) {
        _ if discrete => "step",
        Some(0) => "step",
        Some(2 | 4) => "cubic",
        _ => "linear",
    };
    let mut out_keys = Vec::new();
    for (index, (t, value)) in times.iter().zip(values).enumerate() {
        let Some(value) = key_value(&here, value) else {
            continue;
        };
        let mut key = toml::Table::new();
        key.insert("t".into(), Toml::Float(*t));
        key.insert("value".into(), value);
        // Godot's curve shapes the segment leaving a key; here `ease` shapes
        // the one arriving, so key i's curve is key i + 1's ease.
        let leaving = index.checked_sub(1).and_then(|i| transitions.get(i));
        match leaving.map(|c| easing(*c)) {
            Some(Easing::Named(name)) => {
                key.insert("ease".into(), Toml::String(name));
            }
            Some(Easing::Approximate(name)) => {
                key.insert("ease".into(), Toml::String(name));
                *eased = true;
            }
            Some(Easing::Held) => *eased = true,
            Some(Easing::Linear) | None => {}
        }
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

/// A Godot transition as an easing here.
enum Easing {
    Linear,
    /// An exact match: Godot's curve is `x^n` for a whole `n` from 2 to 5.
    Named(String),
    /// The nearest whole power, for a curve between two of them.
    Approximate(String),
    /// Godot's 0, which holds the key and jumps at the next one.
    Held,
}

/// Godot's `ease(x, c)`: above 1 eases in as `x^c`, between 0 and 1 eases
/// out as the mirror of `x^(1/c)`, below 0 eases in and out with power `-c`.
fn easing(curve: f64) -> Easing {
    if (curve - 1.0).abs() < 1e-6 {
        return Easing::Linear;
    }
    if curve == 0.0 {
        return Easing::Held;
    }
    let (family, power) = if curve < 0.0 {
        ("in_out", -curve)
    } else if curve > 1.0 {
        ("in", curve)
    } else {
        ("out", 1.0 / curve)
    };
    if power < 1.25 {
        return Easing::Linear;
    }
    let whole = power.round().clamp(2.0, 5.0);
    let shape = match whole as i64 {
        2 => "quad",
        3 => "cubic",
        4 => "quart",
        _ => "quint",
    };
    let name = format!("{family}_{shape}");
    if (power - whole).abs() < 0.05 {
        Easing::Named(name)
    } else {
        Easing::Approximate(name)
    }
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
        "sprite/frame" | "widget/value" | "widget/font_size" => Some(Toml::Float(value.as_f64()?)),
        // Texture pixels, y down, in both engines.
        "sprite/offset" => Some(floats(&pair()?)),
        "transform/skew" => Some(Toml::Float(-value.as_f64()?)),
        // A name or a flag, held from key to key.
        "widget/role" | "widget/text" => Some(Toml::String(value.as_str()?.to_string())),
        "widget/checked" => match value {
            Value::Bool(on) => Some(Toml::Boolean(*on)),
            _ => None,
        },
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

#[cfg(test)]
mod tests {
    use super::{Easing, easing, join};

    fn name(curve: f64) -> String {
        match easing(curve) {
            Easing::Named(n) => n,
            Easing::Approximate(n) => format!("~{n}"),
            Easing::Held => "held".into(),
            Easing::Linear => "linear".into(),
        }
    }

    #[test]
    fn a_godot_curve_is_the_power_easing_it_draws() {
        assert_eq!(name(1.0), "linear");
        assert_eq!(name(2.0), "in_quad");
        assert_eq!(name(0.5), "out_quad");
        assert_eq!(name(-2.0), "in_out_quad");
        assert_eq!(name(3.0), "in_cubic");
        assert_eq!(name(2.4), "~in_quad");
        assert_eq!(name(0.0), "held");
    }

    #[test]
    fn a_path_folds_its_dots_against_the_node_it_is_read_from() {
        assert_eq!(join("Ship/Player", ".."), "Ship");
        assert_eq!(join("Ship/Player", "../Hull"), "Ship/Hull");
        assert_eq!(join("", "."), "");
    }
}
