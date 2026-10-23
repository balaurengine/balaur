//! Godot's `_notification`, called from the engine hooks that carry the same
//! news: the app losing or regaining focus, going to the background or
//! coming back, the language changing, and the window asked to close.

use balaur_core::hooks::{
    ON_FOCUSED_CHANGED, ON_LOCALE_CHANGED, ON_QUIT_REQUESTED, ON_SUSPENDED_CHANGED,
};

use super::Function;
use super::render::{Fun, Shape};

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
pub(super) fn write_notification_hooks(shape: &mut Shape, functions: &[Function]) {
    let has = |name: &str| functions.iter().any(|f| f.name == name);
    if !has(NOTIFICATION) {
        return;
    }
    let either = |flag: &str, on: &str, off: &str| {
        format!("if {flag} {{ {} }} else {{ {} }}", number(on), number(off))
    };
    let hooks: [(&str, &[&str], String); 4] = [
        (
            ON_FOCUSED_CHANGED,
            &["focused: bool"],
            either(
                "focused",
                "NOTIFICATION_APPLICATION_FOCUS_IN",
                "NOTIFICATION_APPLICATION_FOCUS_OUT",
            ),
        ),
        (
            ON_SUSPENDED_CHANGED,
            &["suspended: bool"],
            either(
                "suspended",
                "NOTIFICATION_APPLICATION_PAUSED",
                "NOTIFICATION_APPLICATION_RESUMED",
            ),
        ),
        (
            ON_LOCALE_CHANGED,
            &["locale: str"],
            number("NOTIFICATION_TRANSLATION_CHANGED").to_string(),
        ),
        (
            ON_QUIT_REQUESTED,
            &[],
            number("NOTIFICATION_WM_CLOSE_REQUEST").to_string(),
        ),
    ];
    for (hook, params, what) in hooks {
        if has(hook) {
            continue;
        }
        shape.funs.push(
            Fun::method(
                hook,
                params,
                format!("let what: int = {what};\nself.{NOTIFICATION}(what);\n"),
            )
            .doc("Godot's notification for this news, from the engine's hook."),
        );
    }
}
