//! Where the frame is seen from, for what follows the view without drawing:
//! the renderer writes it, and the ears stand there until a `listener` says
//! otherwise.

use glamx::Vec3;

use crate::engine::Engine;

/// The camera's world position and its right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub right: Vec3,
}

/// The current camera's pose; `None` until a camera node or a script says.
#[derive(Default)]
pub struct Viewpoint(pub Option<Pose>);

/// Say where the view is now.
pub fn set(eng: &Engine, position: Vec3, right: Vec3) {
    let pose = Some(Pose { position, right });
    match eng.try_resource::<Viewpoint>() {
        Some(held) => held.borrow_mut().0 = pose,
        None => eng.insert_resource(Viewpoint(pose)),
    }
}

/// Where the view is; `None` with no camera anywhere.
#[must_use]
pub fn get(eng: &Engine) -> Option<Pose> {
    eng.try_resource::<Viewpoint>()
        .and_then(|held| held.borrow().0)
}
