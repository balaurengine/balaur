//! What the physics worlds answer a loop about to sleep.

use balaur_core::Engine;

use crate::dim2::state_2d;
use crate::state_3d;

/// Whether either world's last step left a body moving. A paused game and a
/// paused world hold every body, so neither has anything a frame would move.
pub(crate) fn pending(eng: &Engine) -> bool {
    if eng.paused() {
        return false;
    }
    let dim3 = state_3d(eng).is_some_and(|state| {
        let state = state.borrow();
        state.awake && !state.paused
    });
    dim3 || state_2d(eng).is_some_and(|state| {
        let state = state.borrow();
        state.awake && !state.paused
    })
}
