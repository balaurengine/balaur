//! Godot's `_input` family, hung off the engine's own input hooks.

use std::collections::BTreeSet;

use super::Function;
use super::render::{Fun, Shape};

/// Godot's `_input` family, called from the engine's own hooks: each hook
/// builds the event, hands it to every handler the class has, and answers
/// whether one of them took it. `_unhandled_input` waits on the widget
/// layer's claim, as Godot's waited on its controls.
pub(super) fn write_input_hooks(shape: &mut Shape, functions: &[Function]) {
    let has = |name: &str| functions.iter().any(|f| f.name == name);
    let handlers = [
        "_input",
        "_gui_input",
        "_unhandled_input",
        "_unhandled_key_input",
    ];
    if !handlers.iter().any(|h| has(h)) {
        return;
    }
    let hooks: [(&str, &[&str], &str, bool); 6] = [
        (
            "on_key_down",
            &["key: str"],
            "gd::key_event(key, true)",
            false,
        ),
        (
            "on_key_up",
            &["key: str"],
            "gd::key_event(key, false)",
            false,
        ),
        (
            "on_pointer_down",
            &["button: any"],
            "gd::pointer_event(button, true)",
            true,
        ),
        (
            "on_pointer_up",
            &["button: any"],
            "gd::pointer_event(button, false)",
            true,
        ),
        (
            "on_pointer_drag",
            &["dx: float", "dy: float"],
            "gd::motion_event(dx, dy)",
            true,
        ),
        (
            "on_scroll",
            &["dx: float", "dy: float"],
            "gd::scroll_event(dx, dy)",
            true,
        ),
    ];
    for (hook, params, event, pointer) in hooks {
        if has(hook) {
            continue;
        }
        let claimed = if pointer {
            "ui::wants_pointer()"
        } else {
            "ui::wants_keyboard()"
        };
        let mut calls = String::new();
        for handler in handlers {
            let wanted = match handler {
                "_gui_input" => pointer,
                "_unhandled_key_input" => !pointer,
                _ => true,
            };
            if !wanted || !has(handler) {
                continue;
            }
            let call = format!("self.{handler}(event);");
            if handler.starts_with("_unhandled") {
                calls.push_str(&format!(
                    "if !gd::input_handled() && !{claimed} {{\n{call}\n}}\n"
                ));
            } else {
                calls.push_str(&call);
                calls.push('\n');
            }
        }
        let body = format!(
            "if !self.{}.{} {{\nreturn false;\n}}\nlet event: any = {event};\n{calls}return gd::take_input_handled();\n",
            crate::godot::gdscript::MY,
            crate::godot::gdscript::INPUT_FLAG,
        );
        shape.funs.push(
            Fun::method(hook, params, body)
                .doc("Godot's input handlers, from the engine's hook.")
                .returns("bool"),
        );
    }
}

/// A widget signal connected to something that is not a method here: the
/// widget names a forwarder, which finds the handlers by the widget's node.
pub(super) fn write_widget_forwarders(shape: &mut Shape, keys: &BTreeSet<String>) {
    for key in keys {
        let (params, args): (&[&str], &str) = if key == crate::godot::gdscript::ON_CLICK {
            (&["node: Node"], "gd::list()")
        } else {
            (&["value: any", "node: Node"], "gd::list_of([value])")
        };
        shape.funs.push(
            Fun::method(
                &format!("__widget_{key}"),
                params,
                format!("gd::widget_fire(node, \"{key}\", {args});\n"),
            )
            .doc(&format!(
                "A widget's `{key}`, for the handlers bound to its node."
            )),
        );
    }
}
