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

/// A progress bar's fill direction and a node's translation mode: Godot's
/// enums, numbered by position.
pub(super) fn control_constant(class: &str, name: &str) -> Option<i64> {
    let names: &[&str] = match class {
        "TextureProgressBar" => &[
            "FILL_LEFT_TO_RIGHT",
            "FILL_RIGHT_TO_LEFT",
            "FILL_TOP_TO_BOTTOM",
            "FILL_BOTTOM_TO_TOP",
            "FILL_CLOCKWISE",
            "FILL_COUNTER_CLOCKWISE",
            "FILL_BILINEAR_LEFT_AND_RIGHT",
            "FILL_BILINEAR_TOP_AND_BOTTOM",
            "FILL_CLOCKWISE_AND_COUNTER_CLOCKWISE",
        ],
        "Node" => &[
            "AUTO_TRANSLATE_MODE_INHERIT",
            "AUTO_TRANSLATE_MODE_ALWAYS",
            "AUTO_TRANSLATE_MODE_DISABLED",
        ],
        _ => return None,
    };
    names
        .iter()
        .position(|n| *n == name)
        .and_then(|i| i64::try_from(i).ok())
}
