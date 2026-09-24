//! The `Control` verbs a script calls on a widget node, and the records a
//! script builds and hands one: theme overrides, shader materials, images.

use super::TREE;

/// A verb on a widget node, or on a record the shim keeps for the script.
pub(super) fn control_verb(receiver: &str, name: &str, args: &[String]) -> Option<String> {
    let all = args.join(", ");
    let one = args.first().cloned().unwrap_or_default();
    Some(match name {
        // A theme override lands on the widget key that carries it, if any.
        "add_theme_color_override" => format!("(gd.theme_override)({receiver}, \"colors\", {all})"),
        "add_theme_constant_override" => {
            format!("(gd.theme_override)({receiver}, \"constants\", {all})")
        }
        "add_theme_font_size_override" => {
            format!("(gd.theme_override)({receiver}, \"font_sizes\", {all})")
        }
        "remove_theme_color_override"
        | "remove_theme_constant_override"
        | "remove_theme_font_size_override"
        | "remove_theme_stylebox_override"
        | "add_theme_stylebox_override"
        | "add_theme_font_override" => "()".into(),
        // An option button's popup is the button itself here: its rows.
        "get_popup" => receiver.into(),
        "set_shader_parameter" => format!("(gd.set_shader_parameter)({receiver}, {all})"),
        "get_shader_parameter" => format!("(gd.get_shader_parameter)({receiver}, {one})"),
        // `Image.load(path)`; the global `load` is a preload by another name.
        "load" if receiver != TREE && args.len() == 1 => {
            format!("(gd.image_load)({receiver}, {one})")
        }
        _ => return None,
    })
}

/// A `Control` verb called bare, on the class's own node.
pub(super) fn own_control(name: &str, all: &str) -> Option<String> {
    Some(match name {
        "add_theme_color_override" => format!("(gd.theme_override)(this.node, \"colors\", {all})"),
        "add_theme_constant_override" => {
            format!("(gd.theme_override)(this.node, \"constants\", {all})")
        }
        "add_theme_font_size_override" => {
            format!("(gd.theme_override)(this.node, \"font_sizes\", {all})")
        }
        "remove_theme_color_override"
        | "remove_theme_constant_override"
        | "remove_theme_font_size_override"
        | "remove_theme_stylebox_override"
        | "add_theme_stylebox_override"
        | "add_theme_font_override" => "()".into(),
        "get_popup" => "this.node".into(),
        _ => return None,
    })
}
