//! A Godot `Theme` resource as a `widget_theme`.
//!
//! Godot files its look as `<Type>/<group>/<item>`: a Control class or a type
//! variation, then `styles`, `colors`, `font_sizes` or `constants`. A class
//! becomes the widget kind it converts to, and a variation becomes a role of
//! the same name, which is what `theme_type_variation` already converts to. A
//! `StyleBoxFlat` is a fill, an outline, a radius and padding; a
//! `StyleBoxTexture` is a nine-patch picture. What a widget theme has no key
//! for (fonts, icons past a fold's arrows, a second fill for a bar's progress)
//! is reported.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::nodes::{Mapped, Resources, colour, hex, image_path};
use crate::godot::{Document, Section, Value};

/// A fill or an outline that draws nothing.
const CLEAR: &str = "#00000000";

/// The theme file, and what would not carry.
pub(crate) struct Converted {
    pub toml: String,
    pub notes: Vec<String>,
    /// The font files the theme draws with, project-relative: a face a kind
    /// names is only a weight here, so the project has to ship it.
    pub fonts: Vec<String>,
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
        // A fold's resting header is its shut one; `fold_parts` adds the open.
        "FoldableContainer" => (
            "title_collapsed_panel",
            Some("title_collapsed_hover_panel"),
            None,
        ),
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
    let mut fonts: Vec<String> = Vec::new();
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
        for (group, name, value) in items {
            if *group == "fonts" && *name == "font" {
                fonts.extend(face_files(value, res));
            }
        }
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
                    style.entry("font_size").or_insert(Toml::Float(size));
                }
                kinds.insert(kind.to_string(), Toml::Table(style));
            }
        } else {
            roles.insert(ty.to_string(), Toml::Table(style));
        }
    }
    if let Some(size) = default_size {
        for kind in [
            "label",
            "button",
            "text_field",
            "checkbox",
            "dropdown",
            "tabs",
            "fold",
        ] {
            let entry = kinds
                .entry(kind)
                .or_insert_with(|| Toml::Table(toml::Table::new()));
            if let Toml::Table(table) = entry {
                table.entry("font_size").or_insert(Toml::Float(size));
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
    fonts.sort();
    fonts.dedup();
    Some(Converted {
        toml: text,
        notes,
        fonts,
    })
}

/// The font files behind one theme item: a `FontVariation` names the face it
/// varies, a `FontFile` is one itself.
fn face_files(value: &Value, res: &Resources<'_>) -> Vec<String> {
    if let Some(path) = res.path(value) {
        return vec![path.to_string()];
    }
    let Some(section) = res.sub(value) else {
        return Vec::new();
    };
    section
        .field("base_font")
        .and_then(|base| res.path(base))
        .map(str::to_string)
        .into_iter()
        .collect()
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
        style.insert("text_color".into(), color);
    }
    if let Some(color) = ink("icon_normal_color") {
        style.insert("icon_color".into(), color);
    }
    // Godot's separations are the gap between a container's children; the
    // one along the axis it stacks is the one a widget reads.
    if let Some(gap) = item("constants", "separation")
        .or_else(|| item("constants", "h_separation"))
        .or_else(|| item("constants", "v_separation"))
        .and_then(Value::as_f64)
    {
        style.insert("gap".into(), Toml::Float(gap.max(0.0)));
    }
    // A type drawn in a heavier face is bold here: the weight is the face's,
    // and the chain the project ships is what resolves it.
    if let Some(font) = item("fonts", "font")
        && res
            .sub(font)
            .and_then(|face| face.field("resource_name").and_then(Value::as_str))
            .is_some_and(|name| name.contains("bold") || name.contains("black"))
    {
        style.insert("font_weight".into(), Toml::Integer(700));
    }
    if let Some(size) = item("font_sizes", "font_size")
        .or_else(|| item("font_sizes", "normal_font_size"))
        .and_then(Value::as_f64)
    {
        style.insert("font_size".into(), Toml::Float(size));
    }
    // A MarginContainer's four margins are one padding here, their mean when
    // they differ.
    let margins: Vec<f64> = ["margin_left", "margin_top", "margin_right", "margin_bottom"]
        .iter()
        .filter_map(|name| item("constants", name).and_then(Value::as_f64))
        .collect();
    if !margins.is_empty() {
        let mean = margins.iter().sum::<f64>() / margins.len() as f64;
        style.insert("padding".into(), Toml::Float(mean.max(0.0)));
    }
    // A tab's states are named as tabs; every other kind's are plain.
    let (disabled, focus) = if rest.starts_with("tab_") {
        ("tab_disabled", "tab_focus")
    } else {
        ("disabled", "focus")
    };
    for (state, stylebox_name, font, icon) in [
        ("hover", hover, "font_hover_color", "icon_hover_color"),
        ("active", held, "font_pressed_color", "icon_pressed_color"),
        (
            "disabled",
            Some(disabled),
            "font_disabled_color",
            "icon_disabled_color",
        ),
        ("focus", Some(focus), "font_focus_color", "icon_focus_color"),
    ] {
        let mut over = stylebox_name
            .and_then(|name| item("styles", name))
            .map(|v| stylebox(v, res))
            .unwrap_or_default();
        if let Some(color) = ink(font) {
            over.insert("text_color".into(), color);
        }
        if let Some(color) = ink(icon) {
            over.insert("icon_color".into(), color);
        }
        if !over.is_empty() {
            style.insert(state.into(), Toml::Table(over));
        }
    }
    if class == "FoldableContainer" {
        fold_parts(items, res, &mut style);
    }
    let styles = [Some(rest), hover, held, Some(disabled), Some(focus)];
    for (group, name, _) in items {
        let folds = class == "FoldableContainer" && FOLD_ITEMS.contains(&(*group, *name));
        if !used(group, name, styles) && !folds {
            *dropped.entry((*group).to_string()).or_default() += 1;
        }
    }
    style
}

/// What a FoldableContainer has beyond a kind's states: the open header is
/// the fold's `checked` table, its open body the `body` frame, and its two
/// arrows pictures.
fn fold_parts(items: &[(&str, &str, &Value)], res: &Resources<'_>, style: &mut toml::Table) {
    let item = |group: &str, name: &str| {
        items
            .iter()
            .find(|(g, n, _)| *g == group && *n == name)
            .map(|(_, _, v)| *v)
    };
    let boxed = |name: &str| item("styles", name).map(|v| stylebox(v, res));
    let ink = |name: &str| {
        item("colors", name)
            .and_then(colour)
            .map(|c| Toml::String(hex(&c)))
    };
    let picture = |name: &str| {
        let path = res.path(item("icons", name)?)?;
        Some(Toml::String(image_path(path, res, &mut Mapped::default())))
    };
    let mut open = boxed("title_panel").unwrap_or_default();
    if let Some(hover) = boxed("title_hover_panel") {
        open.insert("hover".into(), Toml::Table(hover));
    }
    if let Some(color) = ink("font_color") {
        open.insert("text_color".into(), color);
    }
    if let Some(arrow) = picture("expanded_arrow") {
        open.insert("arrow".into(), arrow);
    }
    if !open.is_empty() {
        style.insert("checked".into(), Toml::Table(open));
    }
    if let Some(body) = boxed("panel") {
        style.insert("body".into(), Toml::Table(body));
    }
    if let Some(arrow) = picture("folded_arrow") {
        style.insert("arrow".into(), arrow);
    }
    if let Some(color) = ink("collapsed_font_color") {
        style.insert("text_color".into(), color);
    }
}

/// The FoldableContainer items `fold_parts` reads.
const FOLD_ITEMS: [(&str, &str); 6] = [
    ("styles", "title_panel"),
    ("styles", "title_hover_panel"),
    ("styles", "panel"),
    ("icons", "expanded_arrow"),
    ("icons", "folded_arrow"),
    ("colors", "collapsed_font_color"),
];

/// Whether a theme item is one a style table reads; `styles` names the
/// stylebox items of the type.
fn used(group: &str, name: &str, styles: [Option<&str>; 5]) -> bool {
    match group {
        "styles" => styles.contains(&Some(name)),
        "colors" => matches!(
            name,
            "font_color"
                | "default_color"
                | "title_color"
                | "font_hover_color"
                | "font_pressed_color"
                | "font_disabled_color"
                | "font_focus_color"
                | "icon_normal_color"
                | "icon_hover_color"
                | "icon_pressed_color"
                | "icon_disabled_color"
                | "icon_focus_color"
        ),
        "font_sizes" => matches!(name, "font_size" | "normal_font_size"),
        "constants" => matches!(
            name,
            "separation"
                | "h_separation"
                | "v_separation"
                | "margin_left"
                | "margin_top"
                | "margin_right"
                | "margin_bottom"
        ),
        "fonts" => name == "font",
        _ => false,
    }
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
        out.insert("corner_radius".into(), Toml::Float(radius));
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

#[cfg(test)]
mod tests {
    use balaur_plugin::toml;

    const FOLD_THEME: &str = r#"[gd_resource type="Theme" load_steps=6 format=3]

[ext_resource type="Texture2D" path="res://icons/down.png" id="1_down"]
[ext_resource type="Texture2D" path="res://icons/right.png" id="2_right"]

[sub_resource type="StyleBoxFlat" id="Shut"]
bg_color = Color(0.1, 0.1, 0.1, 1)

[sub_resource type="StyleBoxFlat" id="Open"]
bg_color = Color(0.2, 0.2, 0.2, 1)

[sub_resource type="StyleBoxFlat" id="Body"]
bg_color = Color(0.3, 0.3, 0.3, 1)

[resource]
FoldableContainer/icons/expanded_arrow = ExtResource("1_down")
FoldableContainer/icons/folded_arrow = ExtResource("2_right")
FoldableContainer/styles/panel = SubResource("Body")
FoldableContainer/styles/title_collapsed_panel = SubResource("Shut")
FoldableContainer/styles/title_panel = SubResource("Open")
"#;

    #[test]
    fn a_foldable_s_open_header_body_and_arrows_reach_the_fold_theme() {
        let document = crate::godot::parse(FOLD_THEME).unwrap();
        let project = crate::godot::nodes::Project::default();
        let res = crate::godot::nodes::resources_of(&document, std::path::Path::new(""), &project);
        let converted = super::convert(&document, &res).unwrap();
        let theme: toml::Table = toml::from_str(&converted.toml).unwrap();
        let fold = theme["fold"].as_table().unwrap();
        let fill = |table: &toml::Table| {
            table
                .get("fill")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        };
        let shut = fill(fold);
        let open = fill(fold["checked"].as_table().unwrap());
        let body = fill(fold["body"].as_table().unwrap());
        assert!(
            shut.is_some() && open.is_some() && body.is_some(),
            "{fold:?}"
        );
        assert!(shut != open && open != body, "{fold:?}");
        assert_eq!(fold["arrow"].as_str(), Some("icons/right.png"));
        assert_eq!(fold["checked"]["arrow"].as_str(), Some("icons/down.png"));
        assert!(
            !converted
                .notes
                .iter()
                .any(|note| note.contains("`styles`") || note.contains("`icons`")),
            "{:?}",
            converted.notes
        );
    }
}
