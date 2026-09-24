//! Godot's `_input` family, hung off the engine's own input hooks.

use std::fmt::Write as _;

use super::Function;

/// Godot's `_input` family, called from the engine's own hooks: each hook
/// builds the event, hands it to every handler the class has, and answers
/// whether one of them took it. `_unhandled_input` waits on the widget
/// layer's claim, as Godot's waited on its controls.
pub(super) fn write_input_hooks(out: &mut String, functions: &[Function]) {
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
    let hooks = [
        (
            "on_key_down",
            "this, key",
            "(gd.key_event)(key, true)",
            false,
        ),
        (
            "on_key_up",
            "this, key",
            "(gd.key_event)(key, false)",
            false,
        ),
        (
            "on_pointer_down",
            "this, button",
            "(gd.pointer_event)(button, true)",
            true,
        ),
        (
            "on_pointer_up",
            "this, button",
            "(gd.pointer_event)(button, false)",
            true,
        ),
        (
            "on_pointer_drag",
            "this, dx, dy",
            "(gd.motion_event)(dx, dy)",
            true,
        ),
        (
            "on_scroll",
            "this, dx, dy",
            "(gd.scroll_event)(dx, dy)",
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
            let call = format!("let _ = (gd.invoke1)(this.node, \"{handler}\", event);");
            if handler.starts_with("_unhandled") {
                let _ = writeln!(
                    calls,
                    "    if !(gd.input_handled)() && !{claimed} {{\n        {call}\n    }}"
                );
            } else {
                let _ = writeln!(calls, "    {call}");
            }
        }
        let _ = write!(
            out,
            "\n/// Godot's input handlers, from the engine's hook.\n\
             pub fn {hook}({params}) {{\n\
             \x20   let gd = script::require(\"gd.rn\");\n\
             \x20   if (gd.same)((gd.get)(this, \"input_enabled\", ()), false) {{\n\
             \x20       return false;\n\
             \x20   }}\n\
             \x20   let event = {event};\n\
             {calls}\
             \x20   return (gd.take_input_handled)();\n\
             }}\n"
        );
    }
}

/// A widget signal connected to something that is not a method here: the
/// widget names a forwarder, which finds the handlers by the widget's node.
pub(super) fn write_widget_forwarders(out: &mut String, keys: &std::collections::BTreeSet<String>) {
    for key in keys {
        let (params, args) = if key == "on_click" {
            ("this, node", "[]")
        } else {
            ("this, value, node", "[value]")
        };
        let _ = write!(
            out,
            "\n/// A widget's `{key}`, for the handlers bound to its node.\n\
             pub fn __widget_{key}({params}) {{\n\
             \x20   let _ = (script::require(\"gd.rn\").widget_fire)(node, \"{key}\", {args});\n\
             }}\n"
        );
    }
}
