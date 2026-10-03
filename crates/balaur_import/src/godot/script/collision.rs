//! A Godot collision handler fed the engine's collision record: the shim's
//! `collided` takes the other node out of it, which is what Godot handed.

use std::fmt::Write as _;

use super::Function;
use crate::godot::exports::Classes;
use crate::godot::gdscript::{self, Context, safe};

/// The shim call that turns a collision record into Godot's node.
const COLLIDED: &str = "(script::require(\"gd.rn\").collided)";

/// The methods the project's scenes aim a collision signal at.
pub(super) fn adopt(context: &mut Context, classes: &Classes) {
    context
        .collision_handlers
        .clone_from(&classes.collision_handlers);
}

/// A scene's row hands a collision handler the record, so its first
/// parameter takes the node out of it before the body runs.
pub(super) fn prologue(out: &mut String, function: &Function, context: &Context) {
    if !context.collision_handlers.contains(&function.name) {
        return;
    }
    if let Some(first) = function.params.first() {
        let bound = safe(first);
        let _ = writeln!(out, "    let {bound} = {COLLIDED}({bound});");
    }
}

/// What a one-argument forwarder passes its handler for `signal`.
pub(super) fn payload(signal: &str) -> String {
    if gdscript::is_collision_event(signal) {
        format!(", {COLLIDED}(payload)")
    } else {
        ", payload".to_string()
    }
}
