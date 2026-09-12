//! A Godot `Control` as a balaur widget: which kind it becomes, and how its
//! caption, spacing, place and per-kind properties land on the `widget`
//! component. Split from `godot::nodes`, which maps every other class.

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::{Section, Value};
use crate::godot::nodes::{
    Family, Mapped, Resources, colour, family, floats, hex, image_path, pair,
};

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
    ("Window", "window"),
    ("SpinBox", "field"),
    ("TextEdit", "field"),
];

/// The widget kind a Control class converts to.
pub(crate) fn kind_of(class: &str) -> Option<&'static str> {
    WIDGET_KINDS
        .iter()
        .find(|(godot, _)| *godot == class)
        .map(|(_, kind)| *kind)
}

/// Where a class sits in the kinds table, so the first class listed for a
/// kind is the one that speaks for it.
pub(crate) fn rank(class: &str) -> Option<usize> {
    WIDGET_KINDS.iter().position(|(godot, _)| *godot == class)
}

/// Whether `class` is a Control this importer has a widget kind for.
pub(crate) fn is_widget(class: &str) -> bool {
    WIDGET_KINDS.iter().any(|(godot, _)| *godot == class)
}

/// A Control as a widget: its kind, its caption, its place, and the
/// properties each kind reads.
pub(crate) fn widget(
    class: &str,
    section: &Section,
    parent: &str,
    res: &Resources<'_>,
    out: &mut Mapped,
) {
    let kind = WIDGET_KINDS
        .iter()
        .find(|(godot, _)| *godot == class)
        .map_or("panel", |(_, kind)| *kind);
    out.set("widget", "kind", Toml::String(kind.into()));
    caption(class, section, res, out);
    let number = |key: &str| section.field(key).and_then(Value::as_f64);
    if let Some(size) = number("theme_override_font_sizes/font_size") {
        out.set("widget", "font_size", Toml::Float(size));
    }
    if let Some(color) = section
        .field("theme_override_colors/font_color")
        .and_then(colour)
    {
        out.set("widget", "text_color", color);
    }
    spacing(section, out);
    style_override(section, res, out);
    // EXPAND is bit 2; the flag a container reads is the one along its axis.
    let flag = match parent {
        "HBoxContainer" | "HFlowContainer" => number("size_flags_horizontal"),
        "VBoxContainer" | "VFlowContainer" => number("size_flags_vertical"),
        _ => None,
    };
    if flag.is_some_and(|f| (f as i64) & 2 != 0) {
        out.set("widget", "grow", Toml::Float(1.0));
    }
    if class == "Window" {
        window(section, out);
    } else if family(parent) != Family::Control {
        placement(section, out);
    }
    // These lay out and paint nothing in Godot; a `panel` here is framed by
    // its theme, so its fill and its outline are made clear.
    if matches!(
        class,
        "Control"
            | "MarginContainer"
            | "CenterContainer"
            | "AspectRatioContainer"
            | "SubViewportContainer"
    ) {
        out.set("widget", "fill", Toml::String("#00000000".into()));
        out.set("widget", "stroke", Toml::String("#00000000".into()));
    }
    kind_properties(class, section, res, out);
    if matches!(class, "AcceptDialog" | "ConfirmationDialog") {
        dialog(class, section, out);
    }
    if matches!(kind, "button") {
        if let Some(path) = section.field("icon").and_then(|t| res.path(t)) {
            let source = image_path(path, res, out);
            out.set("widget", "source", Toml::String(source));
        }
        // A toggle Button held down is a checked button here.
        if let Some(Value::Bool(on)) = section.field("button_pressed") {
            out.set("widget", "checked", Toml::Boolean(*on));
        }
        if let Some(Value::Bool(true)) = section.field("toggle_mode") {
            out.set("widget", "toggle", Toml::Boolean(true));
            button_group(section, out);
        }
    }
    if let Some(theme) = section.field("theme") {
        match res.path(theme) {
            Some(path) => out.set(
                "widget",
                "theme",
                Toml::String(crate::godot::theme::theme_path(path)),
            ),
            None => out.note("a Theme made inside the scene: save it as a .tres to convert it"),
        }
    }
}

/// A Control's caption and how it is set: its text or translation key, its
/// role, tooltip and alignment.
fn caption(class: &str, section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    let text = |key: &str| {
        section
            .field(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    // Written even when empty: a caption a script fills in at run time would
    // otherwise show the schema's default one until it does.
    let caption = text("text").or_else(|| text("title")).unwrap_or_default();
    if res.project.keys.contains(&caption) {
        out.set("widget", "text_key", Toml::String(caption.clone()));
    }
    out.set("widget", "text", Toml::String(caption));
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
    if section
        .field("autowrap_mode")
        .and_then(Value::as_i64)
        .is_some_and(|m| m != 0)
    {
        out.set("widget", "wrap", Toml::Boolean(true));
    }
    if let Some(align) = section
        .field("horizontal_alignment")
        .and_then(Value::as_i64)
    {
        let align = match align {
            1 => "center",
            2 => "end",
            _ => "start",
        };
        out.set("widget", "text_align", Toml::String(align.into()));
    }
}

/// A stylebox set on the node itself, as the widget's own paint: the keys a
/// widget carries of what a theme's style has. Its hover and pressed looks
/// would need a role of their own and are reported.
fn style_override(section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    let own = ["panel", "normal", "background"]
        .iter()
        .find_map(|name| section.field(&format!("theme_override_styles/{name}")));
    if let Some(value) = own {
        for (key, value) in crate::godot::theme::stylebox(value, res) {
            match key.as_str() {
                "fill" | "stroke" | "radius" | "padding_x" => out.set("widget", &key, value),
                "padding" => {
                    let unset = out
                        .components
                        .get("widget")
                        .and_then(|w| w.get("padding"))
                        .is_none();
                    if unset {
                        out.set("widget", "padding", value);
                    }
                }
                _ => {}
            }
        }
    }
    let states = section
        .fields
        .iter()
        .filter(|(key, _)| {
            key.strip_prefix("theme_override_styles/")
                .is_some_and(|state| matches!(state, "hover" | "pressed" | "hover_pressed"))
        })
        .count();
    if states > 0 {
        out.note("a hover or pressed stylebox set on the node: dress it through a theme role");
    }
}

/// A Control's gap, padding and minimum size: what it asks of the space
/// around and inside it.
fn spacing(section: &Section, out: &mut Mapped) {
    let number = |key: &str| section.field(key).and_then(Value::as_f64);
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
}

/// A `ButtonGroup` as the widget's `group`, named by its resource id.
fn button_group(section: &Section, out: &mut Mapped) {
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

/// The properties one kind of Control has beyond what every widget does: a
/// check's tick, a field's hint, a range's bounds, a picture's source.
fn kind_properties(class: &str, section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    let text = |key: &str| {
        section
            .field(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let number = |key: &str| section.field(key).and_then(Value::as_f64);
    match class {
        "CheckBox" | "CheckButton" => {
            if let Some(Value::Bool(on)) = section.field("button_pressed") {
                out.set("widget", "checked", Toml::Boolean(*on));
            }
            button_group(section, out);
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
            for (godot, here) in [
                ("min_value", "min"),
                ("max_value", "max"),
                ("step", "step"),
                ("value", "value"),
            ] {
                if let Some(n) = number(godot) {
                    out.set("widget", here, Toml::Float(n));
                }
            }
            if !section.fields.iter().any(|(k, _)| k == "max_value") {
                out.set("widget", "max", Toml::Float(100.0));
            }
        }
        "TextureRect" | "TextureButton" | "NinePatchRect" => picture(class, section, res, out),
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
        _ => {}
    }
}

/// A picture Control's source, and a nine-patch's slice.
fn picture(class: &str, section: &Section, res: &Resources<'_>, out: &mut Mapped) {
    let key = if class == "TextureButton" {
        "texture_normal"
    } else {
        "texture"
    };
    if let Some(path) = section.field(key).and_then(|t| res.path(t)) {
        let source = image_path(path, res, out);
        out.set("widget", "source", Toml::String(source));
    }
    if class == "NinePatchRect" {
        let slice: Vec<f64> = ["left", "top", "right", "bottom"]
            .iter()
            .map(|side| {
                section
                    .field(&format!("patch_margin_{side}"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
            })
            .collect();
        out.set("widget", "slice", floats(&slice));
    }
    if class == "TextureButton" {
        out.note("TextureButton: name its `on_click` handler, which is what makes a picture a button here");
    }
}

/// The node a dialog's OK button becomes, under its `Buttons` row, and its
/// Cancel button: where `confirmed` and `canceled` are connected here.
pub(crate) const DIALOG_OK: &str = "Buttons/Ok";
pub(crate) const DIALOG_CANCEL: &str = "Buttons/Cancel";

/// What Godot builds inside an AcceptDialog: its text, then a row of OK and,
/// for a ConfirmationDialog, Cancel, each of which closes it. Hidden until a
/// script shows it, as Godot's is.
fn dialog(class: &str, section: &Section, out: &mut Mapped) {
    out.keys.entry("visible").or_insert(Toml::Boolean(false));
    let text = |key: &str, default: &str| {
        section
            .field(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_string()
    };
    let label = |kind: &str, caption: String| {
        let mut part = Mapped::default();
        part.set("widget", "kind", Toml::String(kind.into()));
        part.set("widget", "text", Toml::String(caption));
        part
    };
    let body = text("dialog_text", "");
    if !body.is_empty() {
        let mut part = label("label", body);
        part.set("widget", "wrap", Toml::Boolean(true));
        out.children.push(("Text".into(), part));
    }
    let button = |caption: String| {
        let mut part = label("button", caption);
        let mut close = toml::Table::new();
        close.insert("event".into(), Toml::String("pointer_click".into()));
        close.insert("action".into(), Toml::String("visible".into()));
        close.insert("target".into(), Toml::String("../..".into()));
        close.insert("value".into(), Toml::Boolean(false));
        part.keys
            .insert("bindings".into(), Toml::Array(vec![Toml::Table(close)]));
        part
    };
    let mut row = label("row", String::new());
    row.set("widget", "justify", Toml::String("center".into()));
    let (ok, cancel) = (
        DIALOG_OK.rsplit('/').next().unwrap_or(DIALOG_OK),
        DIALOG_CANCEL.rsplit('/').next().unwrap_or(DIALOG_CANCEL),
    );
    row.children
        .push((ok.into(), button(text("ok_button_text", "OK"))));
    if class == "ConfirmationDialog" {
        row.children
            .push((cancel.into(), button(text("cancel_button_text", "Cancel"))));
    }
    let buttons = DIALOG_OK.split('/').next().unwrap_or(DIALOG_OK);
    out.children.push((buttons.into(), row));
}

/// An embedded Window: placed by its own `position` and `size` in pixels,
/// shown or hidden by `open`, which its cross turns off.
fn window(section: &Section, out: &mut Mapped) {
    out.set("widget", "anchor", Toml::String("top_left".into()));
    let [x, y] = section
        .field("position")
        .and_then(pair)
        .unwrap_or([0.0, 0.0]);
    out.set("widget", "x", Toml::Float(x));
    out.set("widget", "y", Toml::Float(y));
    // Godot's default Window is 100 pixels square.
    let [w, h] = section
        .field("size")
        .and_then(pair)
        .unwrap_or([100.0, 100.0]);
    out.set("widget", "width", Toml::Float(w));
    out.set("widget", "height", Toml::Float(h));
    if let Some(Toml::Boolean(shown)) = out.keys.remove("visible") {
        out.set("widget", "open", Toml::Boolean(shown));
    }
}

/// A root Control's anchor preset and offsets as a widget's anchor, `x`, `y`
/// and size. Inside a container a Control is placed by it, so only a Control
/// whose parent is not one reaches here.
fn placement(section: &Section, out: &mut Mapped) {
    let number = |key: &str| section.field(key).and_then(Value::as_f64).unwrap_or(0.0);
    let preset = section
        .field("anchors_preset")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let anchor = match preset {
        1 => "top_right",
        2 => "bottom_left",
        3 => "bottom_right",
        4 => "center_left",
        5 => "center_top",
        6 => "center_right",
        7 => "center_bottom",
        8 => "center",
        9 => "fill_left",
        10 => "fill_top",
        11 => "fill_right",
        12 => "fill_bottom",
        13 => "fill_across",
        14 => "fill_down",
        15 => "fill",
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
    // A wide preset spans one axis less its insets and is placed on the
    // other, measured the way the matching corner or middle anchor is.
    let (width, height) = (right - left, bottom - top);
    let spans_width = match anchor {
        "fill_top" | "fill_bottom" | "fill_across" => Some(true),
        "fill_left" | "fill_right" | "fill_down" => Some(false),
        _ => None,
    };
    if let Some(spans_width) = spans_width {
        if spans_width {
            out.set("widget", "inset", floats(&[left, 0.0, -right, 0.0]));
            if height > 0.0 {
                out.set("widget", "height", Toml::Float(height));
            }
            let y = match anchor {
                "fill_bottom" => -bottom,
                "fill_across" => f64::midpoint(top, bottom),
                _ => top,
            };
            out.set("widget", "y", Toml::Float(y));
        } else {
            out.set("widget", "inset", floats(&[0.0, top, 0.0, -bottom]));
            if width > 0.0 {
                out.set("widget", "width", Toml::Float(width));
            }
            let x = match anchor {
                "fill_right" => -right,
                "fill_down" => f64::midpoint(left, right),
                _ => left,
            };
            out.set("widget", "x", Toml::Float(x));
        }
        return;
    }
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
        "center" | "center_top" | "center_bottom" => f64::midpoint(left, right),
        _ => left,
    };
    let y = match anchor {
        "bottom_left" | "bottom_right" | "center_bottom" => -bottom,
        "center" | "center_left" | "center_right" => f64::midpoint(top, bottom),
        _ => top,
    };
    out.set("widget", "x", Toml::Float(x));
    out.set("widget", "y", Toml::Float(y));
}
