//! Godot's `_draw`, run on `queue_redraw` as Godot ran it: the engine's 2D
//! drawing is immediate, so the shim keeps what it drew and draws it again
//! each frame.

use super::Function;
use super::render::{Fun, Shape};

/// The call a frame hook opens with when the class draws.
pub(super) const DRAW_CALL: &str = "gd::draw_frame(self.node);\n";

/// A class with a `_draw` and no `_process` gains the frame hook that draws.
pub(super) fn write_draw_hook(shape: &mut Shape, functions: &[Function]) {
    let has = |name: &str| functions.iter().any(|f| f.name == name);
    if !has("_draw") || has("_process") || has("update") {
        return;
    }
    shape.funs.push(
        Fun::method("update", &["dt: float"], DRAW_CALL.to_string())
            .doc("What `_draw` drew, drawn again every frame."),
    );
}
