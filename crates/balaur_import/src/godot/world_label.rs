//! A Godot `Label` under a `Node2D`: text in the world, which the camera
//! moves and zooms, so a `text2d` rather than a widget on the screen.

use std::cell::RefCell;
use std::collections::BTreeMap;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::nodes::{Mapped, PIXELS_PER_UNIT, Resources, colour, floats};
use crate::godot::{Section, Value};

thread_local! {
    /// Each converted theme a world label named, by its Godot path: a
    /// country map names one theme in every one of its labels.
    static THEMES: RefCell<BTreeMap<String, Option<toml::Table>>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// Whether a class drawn under `parent` is a label in the world: under a
/// 2D node. Under a `CanvasLayer`, a plain node or none it is on the screen.
pub(crate) fn is_world_label(class: &str, parent: &str) -> bool {
    matches!(class, "Label" | "RichTextLabel")
        && !parent.is_empty()
        && crate::godot::nodes::family(parent) == crate::godot::nodes::Family::Node2d
}

/// The label's caption, box and look as a `text2d` at the point of its box
/// that its alignment names.
pub(crate) fn world_label(
    class: &str,
    section: &Section,
    parent: &str,
    res: &Resources<'_>,
    out: &mut Mapped,
) {
    let mut widget = Mapped::default();
    crate::godot::controls::widget(class, section, parent, res, &mut widget);
    let Some(Toml::Table(table)) = widget.components.remove("widget") else {
        return;
    };
    let number = |key: &str| table.get(key).and_then(Toml::as_float).unwrap_or(0.0);
    let (x, y, width, height) = (number("x"), number("y"), number("width"), number("height"));
    let style = theme_style(&table, res);
    let styled = |key: &str| {
        table
            .get(key)
            .or_else(|| style.as_ref().and_then(|s| s.get(key)))
            .cloned()
    };
    let size = styled("font_size")
        .and_then(|v| v.as_float())
        .unwrap_or(16.0);
    let align = section
        .field("horizontal_alignment")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let valign = section
        .field("vertical_alignment")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    // One line's half height stands in for the block's where the label is
    // aligned to an edge of its box: the box height is only known here.
    let across = match align {
        1 | 3 => x + width / 2.0,
        2 => x + width,
        _ => x,
    };
    let down = match valign {
        1 | 3 => y + height / 2.0,
        2 => y + height - size * 0.6,
        _ => y + size * 0.6,
    };
    out.set(
        "transform",
        "position",
        floats(&[across / PIXELS_PER_UNIT, -down / PIXELS_PER_UNIT, 0.0]),
    );
    let text = &mut *out;
    for key in ["text", "text_key"] {
        if let Some(value) = table.get(key) {
            text.set("text2d", key, value.clone());
        }
    }
    text.set("text2d", "font_size", Toml::Float(size));
    text.set("text2d", "pixels_per_unit", Toml::Float(PIXELS_PER_UNIT));
    let word = match align {
        1 | 3 => "center",
        2 => "end",
        _ => "start",
    };
    text.set("text2d", "text_align", Toml::String(word.into()));
    if table.get("wrap").and_then(Toml::as_bool) == Some(true) && width > 0.0 {
        text.set("text2d", "max_width", Toml::Float(width));
    }
    if let Some(color) = styled("text_color") {
        text.set("text2d", "color", color);
    }
    if let Some(weight) = styled("font_weight") {
        text.set("text2d", "font_weight", weight);
    }
    outline(section, text);
}

/// Godot's outline overrides, which the widget has no key for.
fn outline(section: &Section, out: &mut Mapped) {
    let size = section
        .field("theme_override_constants/outline_size")
        .and_then(Value::as_f64)
        .filter(|size| *size > 0.0);
    if let Some(size) = size {
        out.set("text2d", "outline_size", Toml::Float(size));
        if let Some(color) = section
            .field("theme_override_colors/font_outline_color")
            .and_then(colour)
        {
            out.set("text2d", "outline_color", color);
        }
    }
}

/// The style the label's theme gives it: its role's over the theme's
/// `label` table, as the converted theme writes them.
fn theme_style(widget: &toml::Table, res: &Resources<'_>) -> Option<toml::Table> {
    let path = widget.get("theme").and_then(Toml::as_str)?;
    let theme = converted(path, res)?;
    let mut style = theme
        .get("label")
        .and_then(Toml::as_table)
        .cloned()
        .unwrap_or_default();
    let role = widget
        .get("role")
        .and_then(Toml::as_str)
        .and_then(|role| theme.get("roles")?.get(role)?.as_table());
    if let Some(role) = role {
        for (key, value) in role {
            if !value.is_table() {
                style.insert(key.clone(), value.clone());
            }
        }
    }
    Some(style)
}

/// The converted theme a widget's `theme` key names, read once.
fn converted(written: &str, res: &Resources<'_>) -> Option<toml::Table> {
    let godot = format!("{}.tres", written.strip_suffix(".toml").unwrap_or(written));
    THEMES.with(|themes| {
        themes
            .borrow_mut()
            .entry(godot.clone())
            .or_insert_with(|| {
                let text = crate::godot::io::text(&res.root.join(&godot)).ok()?;
                let document = crate::godot::parse(&text).ok()?;
                let own = crate::godot::nodes::resources_of(&document, res.root, res.project);
                let theme = crate::godot::theme::convert(&document, &own)?;
                toml::from_str(&theme.toml).ok()
            })
            .clone()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_label_on_a_canvas_layer_is_on_the_screen() {
        assert!(!super::is_world_label("Label", "CanvasLayer"));
        assert!(!super::is_world_label("Label", "Node"));
        assert!(super::is_world_label("Label", "Node2D"));
    }

    #[test]
    fn a_label_made_by_new_is_a_widget() {
        let doc = crate::godot::nodes::bare_document("Label", "made").unwrap();
        assert!(doc.contains("[nodes.widget]"), "{doc}");
        assert!(!doc.contains("text2d"), "{doc}");
    }
}
