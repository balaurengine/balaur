//! Godot's `_notification`, called from the engine hooks that carry the same
//! news: the app losing or regaining focus, and the window asked to close.

use std::fmt::Write as _;

use balaur_core::hooks::{ON_FOCUS_CHANGED, ON_QUIT_REQUESTED};

use super::Function;
use crate::godot::gdscript::SHIM_PATH;

/// The Godot method the hooks call.
const NOTIFICATION: &str = "_notification";

/// Godot's number for a notification, from the constants table the
/// translator reads `what == NOTIFICATION_…` through, so the two agree.
fn number(name: &str) -> &'static str {
    crate::godot::gdscript::global_constant(name)
        .expect("every notification a hook hands over is in the constants table")
}

/// A class with a `_notification` gains `on_focus_changed` and
/// `on_quit_requested`, each handing it Godot's number for that news.
pub(super) fn write_notification_hooks(out: &mut String, functions: &[Function]) {
    let has = |name: &str| functions.iter().any(|f| f.name == name);
    if !has(NOTIFICATION) {
        return;
    }
    let call = format!(
        "let _ = (script::require(\"{SHIM_PATH}\").invoke1)(this.node, \"{NOTIFICATION}\", what);"
    );
    if !has(ON_FOCUS_CHANGED) {
        let (focus_in, focus_out) = (
            number("NOTIFICATION_APPLICATION_FOCUS_IN"),
            number("NOTIFICATION_APPLICATION_FOCUS_OUT"),
        );
        let _ = write!(
            out,
            "\n/// Godot's focus notifications, from the engine's hook.\n\
             pub fn {ON_FOCUS_CHANGED}(this, focused) {{\n\
             \x20   let what = if focused {{ {focus_in} }} else {{ {focus_out} }};\n\
             \x20   {call}\n\
             }}\n"
        );
    }
    if !has(ON_QUIT_REQUESTED) {
        let close = number("NOTIFICATION_WM_CLOSE_REQUEST");
        let _ = write!(
            out,
            "\n/// Godot's close request, from the engine's hook.\n\
             pub fn {ON_QUIT_REQUESTED}(this) {{\n\
             \x20   let what = {close};\n\
             \x20   {call}\n\
             }}\n"
        );
    }
}
