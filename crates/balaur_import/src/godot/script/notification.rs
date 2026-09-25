//! Godot's `_notification`, called from the engine hooks that carry the same
//! news: the app losing or regaining focus, and the window asked to close.

use std::fmt::Write as _;

use super::Function;

/// Godot's number for a notification, from the constants table the
/// translator reads `what == NOTIFICATION_…` through, so the two agree.
fn number(name: &str) -> &'static str {
    crate::godot::gdscript::global_constant(name).unwrap_or("0")
}

/// A class with a `_notification` gains `on_focus_changed` and
/// `on_quit_requested`, each handing it Godot's number for that news.
pub(super) fn write_notification_hooks(out: &mut String, functions: &[Function]) {
    let has = |name: &str| functions.iter().any(|f| f.name == name);
    if !has("_notification") {
        return;
    }
    let call = "let _ = (script::require(\"gd.rn\").invoke1)(this.node, \"_notification\", what);";
    if !has("on_focus_changed") {
        let (focus_in, focus_out) = (
            number("NOTIFICATION_APPLICATION_FOCUS_IN"),
            number("NOTIFICATION_APPLICATION_FOCUS_OUT"),
        );
        let _ = write!(
            out,
            "\n/// Godot's focus notifications, from the engine's hook.\n\
             pub fn on_focus_changed(this, focused) {{\n\
             \x20   let what = if focused {{ {focus_in} }} else {{ {focus_out} }};\n\
             \x20   {call}\n\
             }}\n"
        );
    }
    if !has("on_quit_requested") {
        let close = number("NOTIFICATION_WM_CLOSE_REQUEST");
        let _ = write!(
            out,
            "\n/// Godot's close request, from the engine's hook.\n\
             pub fn on_quit_requested(this) {{\n\
             \x20   let what = {close};\n\
             \x20   {call}\n\
             }}\n"
        );
    }
}
