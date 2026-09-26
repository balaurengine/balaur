//! Godot's `_notification`, called from the engine hooks that carry the same
//! news: the app losing or regaining focus, going to the background or
//! coming back, the language changing, and the window asked to close.

use std::fmt::Write as _;

use balaur_core::hooks::{
    ON_FOCUSED_CHANGED, ON_LOCALE_CHANGED, ON_QUIT_REQUESTED, ON_SUSPENDED_CHANGED,
};

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

/// A class with a `_notification` gains a method for each engine hook that
/// carries a notification's news, handing it Godot's number for that news.
pub(super) fn write_notification_hooks(out: &mut String, functions: &[Function]) {
    let has = |name: &str| functions.iter().any(|f| f.name == name);
    if !has(NOTIFICATION) {
        return;
    }
    let call = format!(
        "let _ = (script::require(\"{SHIM_PATH}\").invoke1)(this.node, \"{NOTIFICATION}\", what);"
    );
    let either = |flag: &str, on: &str, off: &str| {
        format!("if {flag} {{ {} }} else {{ {} }}", number(on), number(off))
    };
    let hooks = [
        (
            ON_FOCUSED_CHANGED,
            "focused",
            either(
                "focused",
                "NOTIFICATION_APPLICATION_FOCUS_IN",
                "NOTIFICATION_APPLICATION_FOCUS_OUT",
            ),
        ),
        (
            ON_SUSPENDED_CHANGED,
            "suspended",
            either(
                "suspended",
                "NOTIFICATION_APPLICATION_PAUSED",
                "NOTIFICATION_APPLICATION_RESUMED",
            ),
        ),
        (
            ON_LOCALE_CHANGED,
            "locale",
            number("NOTIFICATION_TRANSLATION_CHANGED").to_string(),
        ),
        (
            ON_QUIT_REQUESTED,
            "",
            number("NOTIFICATION_WM_CLOSE_REQUEST").to_string(),
        ),
    ];
    for (hook, takes, what) in hooks {
        if has(hook) {
            continue;
        }
        let params = if takes.is_empty() {
            String::from("this")
        } else {
            format!("this, {takes}")
        };
        let _ = write!(
            out,
            "\n/// Godot's notification for this news, from the engine's hook.\n\
             pub fn {hook}({params}) {{\n\
             \x20   let what = {what};\n\
             \x20   {call}\n\
             }}\n"
        );
    }
}
