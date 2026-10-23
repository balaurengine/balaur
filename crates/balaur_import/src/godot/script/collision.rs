//! A Godot collision handler fed the engine's collision record: the shim's
//! `collided` takes the other node out of it, which is what Godot handed.

use crate::godot::exports::Classes;
use crate::godot::gdscript::{self, Context};

/// The shim call that turns a collision record into Godot's node.
const COLLIDED: &str = "gd::collided";

/// The methods the project's scenes aim a collision signal at.
pub(super) fn adopt(context: &mut Context, classes: &Classes) {
    context
        .collision_handlers
        .clone_from(&classes.collision_handlers);
}

/// A scene's row hands a collision handler the record, so its first
/// parameter takes the node out of it before the body runs.
pub(super) fn prologue(param: &str) -> String {
    format!("{param} = {COLLIDED}({param});\n")
}

/// What a one-argument forwarder passes its handler for `signal`.
pub(super) fn payload(signal: &str) -> String {
    if gdscript::is_collision_event(signal) {
        format!("{COLLIDED}(payload)")
    } else {
        "payload".to_string()
    }
}
