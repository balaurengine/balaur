//! Godot's `_draw`, run on `queue_redraw` as Godot ran it: the engine's 2D
//! drawing is immediate, so the shim keeps what it drew and draws it again
//! each frame.

use std::fmt::Write as _;

use super::Function;

/// The call a frame hook opens with when the class draws.
pub(super) const DRAW_CALL: &str =
    "    let _ = (script::require(\"gd.rn\").draw_frame)(this.node);\n";

/// A class with a `_draw` and no `_process` gains the frame hook that draws.
pub(super) fn write_draw_hook(out: &mut String, functions: &[Function]) {
    let has = |name: &str| functions.iter().any(|f| f.name == name);
    if !has("_draw") || has("_process") || has("update") {
        return;
    }
    let _ = write!(
        out,
        "\n/// What `_draw` drew, drawn again every frame.\n\
         pub fn update(this, dt) {{\n{DRAW_CALL}}}\n"
    );
}
