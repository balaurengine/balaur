//! A Godot `Theme` resource as a `widget_theme`.
//!
//! Godot files its look as `<Type>/<group>/<item>`: a Control class or a type
//! variation, then `styles`, `colors`, `font_sizes` or `constants`. A class
//! becomes the widget kind it converts to, and a variation becomes a role of
//! the same name, which is what `theme_type_variation` already converts to. A
//! `StyleBoxFlat` is a fill, an outline, a radius and padding; a
//! `StyleBoxTexture` is a nine-patch picture. What a widget theme has no key
//! for (fonts, icons, a second fill for a bar's progress) is reported.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::{Document, Section, Value};
use crate::godot::nodes::{Mapped, Resources, colour, hex, image_path};

/// A fill or an outline that draws nothing.
const CLEAR: &str = "#00000000";

/// The theme file, and what would not carry.
pub(crate) struct Converted {
    pub toml: String,
    pub notes: Vec<String>,
}

/// Where the theme converted from the `.tres` at `godot` is written: beside
/// it, under the same stem.
pub(crate) fn theme_path(godot: &str) -> String {
    format!("{}.toml", godot.strip_suffix(".tres").unwrap_or(godot))
}

/// Which stylebox is a kind's resting look, which its hover and which its
/// held-down one, by the Godot class it came from.
fn states(class: &str) -> (&'static str, Option<&'static str>, Option<&'static str>) {
    match class {
        "Button" | "LinkButton" | "MenuButton" | "OptionButton" | "CheckBox" | "CheckButton" => {
            ("normal", Some("hover"), Some("pressed"))
        }
        "Label" | "RichTextLabel" | "LineEdit" | "TextEdit" | "SpinBox" => ("normal", None, None),
        "TabBar" | "TabContainer" => ("tab_unselected", Some("tab_hovered"), Some("tab_selected")),
        "ProgressBar" | "TextureProgressBar" => ("background", None, None),
        "HSlider" | "VSlider" => ("slider", None, None),
        "HSeparator" | "VSeparator" => ("separator", None, None),
        "Window" => ("embedded_border", None, None),
        _ => ("panel", None, None),
    }
}

/// The kind a Godot class draws as here, or `None` for one with no widget.
fn kind_of(class: &str) -> Option<&'static str> {
    crate::godot::controls::kind_of(class)
}

/// Convert one `Theme` document.
pub(crate) fn convert(document: &Document, res: &Resources<'_>) -> Option<Converted> {
    let resource = document.first("resource")?;
    let mut notes = Vec::new();
    // Type, then group and item, in file order.
    let mut types: BTreeMap<&str, Vec<(&str, &str, &Value)>> = BTreeMap::new();
    let mut base: BTreeMap<&str, &str> = BTreeMap::new();
    for (key, value) in &resource.fields {
        let mut parts = key.splitn(3, '/');
        let (Some(ty), Some(group)) = (parts.next(), parts.next()) else {
            continue;
        };
        if group == "base_type" {
            if let Some(class) = value.as_str() {
                base.insert(ty, class);
            }
            continue;
        }
        let Some(item) = parts.next() else { continue };
        types.entry(ty).or_default().push((group, item, value));
    }
    let default_size = resource.field("default_font_size").and_then(Value::as_f64);
    let mut kinds = toml::Table::new();
    let mut roles = toml::Table::new();
    let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
    // The widget kinds' own classes first, in the order their table lists
    // them, so `Label` dresses `label` before `RichTextLabel` can.
    let mut order: Vec<&str> = types.keys().copied().collect();
    order.sort_by_key(|ty| crate::godot::controls::rank(ty).unwrap_or(usize::MAX));
    for ty in order {
        let items = &types[ty];
        let class = base.get(ty).copied().unwrap_or(ty);
        let is_class = kind_of(ty).is_some();
        let mut style = style_of(class, items, res, &mut dropped);
        if style.is_empty() {
            continue;
        }
        if is_class {
            let kind = kind_of(ty).unwrap_or("panel");
            // A kind two classes draw as keeps the first one's look and takes
            // only what it left out from the second.
            if let Some(Toml::Table(have)) = kinds.get_mut(kind) {
                for (key, value) in style {
                    have.entry(key).or_insert(value);
                }
            } else {
                if let Some(size) = default_size {
                    style.entry("size").or_insert(Toml::Float(size));
                }
                kinds.insert(kind.to_string(), Toml::Table(style));
            }
        } else {
            roles.insert(ty.to_string(), Toml::Table(style));
        }
    }
    if let Some(size) = default_size {
        for kind in [
            "label", "button", "field", "check", "dropdown", "tab", "fold",
        ] {
            let entry = kinds
                .entry(kind)
                .or_insert_with(|| Toml::Table(toml::Table::new()));
            if let Toml::Table(table) = entry {
                table.entry("size").or_insert(Toml::Float(size));
            }
        }
    }
    for (what, count) in dropped {
        notes.push(format!(
            "{count} theme `{what}` items have no widget theme key"
        ));
    }
    let mut document = toml::Table::new();
    document.insert("type".into(), Toml::String("widget_theme".into()));
    document.extend(kinds);
    if !roles.is_empty() {
        document.insert("roles".into(), Toml::Table(roles));
    }
    let mut text = String::from("# Converted from a Godot Theme by `balaur import`.\n");
    let _ = write!(text, "{}", toml::to_string(&Toml::Table(document)).ok()?);
    Some(Converted { toml: text, notes })
}

/// One type's items as a style table: its resting stylebox, the states over
/// it, and its caption's colour and size.
fn style_of(
    class: &str,
    items: &[(&str, &str, &Value)],
    res: &Resources<'_>,
    dropped: &mut BTreeMap<String, usize>,
) -> toml::Table {
    let item = |group: &str, name: &str| {
        items
            .iter()
            .find(|(g, n, _)| *g == group && *n == name)
            .map(|(_, _, v)| *v)
    };
    let (rest, hover, held) = states(class);
    let mut style = item("styles", rest)
        .map(|v| stylebox(v, res))
        .unwrap_or_default();
    let ink = |name: &str| {
        item("colors", name)
            .and_then(colour)
            .map(|c| Toml::String(hex(&c)))
    };
    if let Some(color) = ink("font_color")
        .or_else(|| ink("default_color"))
        .or_else(|| ink("title_color"))
    {
        style.insert("color".into(), color);
    }
    if let Some(size) = item("font_sizes", "font_size")
        .or_else(|| item("font_sizes", "normal_font_size"))
        .and_then(Value::as_f64)
    {
        style.insert("size".into(), Toml::Float(size));
    }
    for (state, stylebox_name, font) in [
        ("hover", hover, "font_hover_color"),
        ("active", held, "font_pressed_color"),
    ] {
        let mut over = stylebox_name
            .and_then(|name| item("styles", name))
            .map(|v| stylebox(v, res))
            .unwrap_or_default();
        if let Some(color) = ink(font) {
            over.insert("color".into(), color);
        }
        if !over.is_empty() {
            style.insert(state.into(), Toml::Table(over));
        }
    }
    let used = |group: &str, name: &str| match group {
        "styles" => [Some(rest), hover, held].contains(&Some(name)),
        "colors" => matches!(
            name,
            "font_color"
                | "default_color"
                | "title_color"
                | "font_hover_color"
                | "font_pressed_color"
        ),
        "font_sizes" => matches!(name, "font_size" | "normal_font_size"),
        _ => false,
    };
    for (group, name, _) in items {
        if !used(group, name) {
            *dropped.entry((*group).to_string()).or_default() += 1;
        }
    }
    style
}

/// A `StyleBoxFlat`, `StyleBoxTexture`, `StyleBoxEmpty` or `StyleBoxLine`
/// as the style keys it draws with.
pub(crate) fn stylebox(value: &Value, res: &Resources<'_>) -> toml::Table {
    let mut out = toml::Table::new();
    let Some(section) = res.sub(value) else {
        return out;
    };
    let number = |key: &str| section.field(key).and_then(Value::as_f64);
    let most = |sides: &[&str]| {
        sides
            .iter()
            .filter_map(|side| number(&format!("content_margin_{side}")))
            .reduce(f64::max)
    };
    if let Some(across) = most(&["left", "right"]) {
        out.insert("padding_x".into(), Toml::Float(across));
    }
    if let Some(down) = most(&["top", "bottom"]) {
        out.insert("padding".into(), Toml::Float(down));
    }
    match section.attr_str("type").unwrap_or_default() {
        "StyleBoxFlat" => flat(section, &mut out),
        "StyleBoxTexture" => textured(section, res, &mut out),
        "StyleBoxLine" => {
            if let Some(color) = section.field("color").and_then(colour) {
                out.insert("stroke".into(), Toml::String(hex(&color)));
            }
            out.insert(
                "stroke_width".into(),
                Toml::Float(number("thickness").unwrap_or(1.0)),
            );
        }
        // StyleBoxEmpty: draws nothing, which a clear fill and no outline say.
        _ => {
            out.insert("fill".into(), Toml::String(CLEAR.into()));
            out.insert("stroke".into(), Toml::String(CLEAR.into()));
        }
    }
    out
}

fn flat(section: &Section, out: &mut toml::Table) {
    let number = |key: &str| section.field(key).and_then(Value::as_f64);
    // Godot's default fill is a mid grey, drawn unless `draw_center` is off.
    let fill = section
        .field("bg_color")
        .and_then(colour)
        .map_or_else(|| "#999999ff".to_string(), |c| hex(&c));
    let drawn = section.field("draw_center") != Some(&Value::Bool(false));
    out.insert(
        "fill".into(),
        Toml::String(if drawn { fill } else { CLEAR.into() }),
    );
    let border = ["left", "top", "right", "bottom"]
        .iter()
        .filter_map(|side| number(&format!("border_width_{side}")))
        .reduce(f64::max)
        .unwrap_or(0.0);
    if border > 0.0 {
        let color = section
            .field("border_color")
            .and_then(colour)
            .map_or_else(|| "#cccccc".to_string(), |c| hex(&c));
        out.insert("stroke".into(), Toml::String(color));
        out.insert("stroke_width".into(), Toml::Float(border));
    } else {
        out.insert("stroke".into(), Toml::String(CLEAR.into()));
    }
    if let Some(radius) = ["top_left", "top_right", "bottom_right", "bottom_left"]
        .iter()
        .filter_map(|corner| number(&format!("corner_radius_{corner}")))
        .reduce(f64::max)
    {
        out.insert("radius".into(), Toml::Float(radius));
    }
}

fn textured(section: &Section, res: &Resources<'_>, out: &mut toml::Table) {
    let Some(path) = section.field("texture").and_then(|t| res.path(t)) else {
        return;
    };
    // Where a raster lives is the scene converter's concern; its notes are
    // for a node, and a theme has none.
    let mut scratch = Mapped::default();
    out.insert(
        "image".into(),
        Toml::String(image_path(path, res, &mut scratch)),
    );
    let slice: Vec<Toml> = ["left", "top", "right", "bottom"]
        .iter()
        .map(|side| {
            Toml::Float(
                section
                    .field(&format!("texture_margin_{side}"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            )
        })
        .collect();
    out.insert("slice".into(), Toml::Array(slice));
}
