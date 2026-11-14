//! What the physics worlds answer a loop about to sleep.

use balaur_core::Engine;

use crate::PhysicsState3d;
use crate::dim2::PhysicsState2d;

/// Whether either world's last step left a body moving. A paused game and a
/// paused world hold every body, so neither has anything a frame would move.
pub(crate) fn pending(eng: &Engine) -> bool {
    if eng.paused() {
        return false;
    }
    let dim3 = eng.try_resource::<PhysicsState3d>().is_some_and(|state| {
        let state = state.borrow();
        state.awake && !state.paused
    });
    dim3 || eng.try_resource::<PhysicsState2d>().is_some_and(|state| {
        let state = state.borrow();
        state.awake && !state.paused
    })
}
