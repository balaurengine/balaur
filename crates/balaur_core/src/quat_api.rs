//! The `quat` script module's record: rotations as quaternions `{ x, y, z,
//! w }`, for what a transform's `rotation_euler` cannot compose. Its functions
//! are Balaur Script in the language's prelude, over glam's formulas and the
//! Euler order a transform's `rotation_euler` is read in.

use balaur_script::Module;

use crate::engine::Engine;

const RECORD: &str = "Quat";

/// Declare the module.
pub fn install_quat_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Rotations as quaternions `{ x, y, z, w }`: from an axis and an angle, from one direction to another or from a `rotation_euler`, composed, inverted and applied to a vector. Balaur Script over glam's formulas, so every platform turns alike.",
    );
    m.records(&[(
        RECORD,
        "{ x: float, y: float, z: float, w: float }",
        "A rotation as a unit quaternion.",
    )]);
}
