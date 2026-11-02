//! Drawing between two fixed steps, so a body simulated at 60 Hz moves every
//! frame on a 144 Hz display.
//!
//! Render-side state and nothing else. A moving body and a node whose script
//! has a `fixed_update` keep the pose the last two fixed steps left them in,
//! and the scene sync draws `lerp(previous, current, alpha)`. `node.transform`
//! and `node.global_position` answer the tick, never the blend, so no script
//! sees a pose the simulation never held and no digest contains one.
//!
//! The blend is one step behind real time, which is what buys the smoothness:
//! there is no pose past `current` to draw. So a display at the tick's own rate
//! draws the tick, and pays neither the latency nor the per-node lookup.

use hecs::Entity;

use crate::engine::Engine;
use crate::scene::Transform;

/// How far apart the display and the tick have to be before a blend beats
/// drawing the tick. A 59.94 Hz panel under a 60 Hz tick slips one step in
/// seventeen seconds, which is not worth a tick of latency on every frame.
const SAME_RATE_HZ: f32 = 1.0;

/// The two poses a node is drawn between, both local to its parent.
#[derive(Clone, Copy)]
pub struct Interpolation {
    /// What the step before last left the node in.
    pub previous: Transform,
    /// What the last step left it in.
    pub current: Transform,
}

impl Interpolation {
    fn at(pose: Transform) -> Self {
        Self {
            previous: pose,
            current: pose,
        }
    }

    /// The pose to draw `alpha` of the way from the last step to the next.
    ///
    /// A rotation blends the short way round, so a node turning more than
    /// half a turn in one step draws the wrong way. Godot has the same limit,
    /// and a node spinning that fast has no smooth frames to show anyway.
    #[must_use]
    pub fn pose(&self, alpha: f32) -> Transform {
        Transform {
            position: self.previous.position.lerp(self.current.position, alpha),
            rotation: self.previous.rotation.slerp(self.current.rotation, alpha),
            scale: self.previous.scale.lerp(self.current.scale, alpha),
            skew: self.previous.skew + (self.current.skew - self.previous.skew) * alpha,
        }
    }
}

/// Whether this frame draws between steps: the display's rate, as measured,
/// differs from the tick's.
#[must_use]
pub fn on(eng: &Engine) -> bool {
    #[allow(clippy::cast_precision_loss, reason = "a rate bounded at 1..=480")]
    let tick = crate::app::tick_hz() as f32;
    (crate::facts::device(eng).refresh_rate - tick).abs() >= SAME_RATE_HZ
}

/// Keep this node's poses from now on: what a dynamic or kinematic body and a
/// script declaring `fixed_update` ask for. The rate is measured over a run's
/// first frames, and a window can move to another display, so a node keeps its
/// poses whichever way [`on`] answers today.
pub fn enable(eng: &Engine, entity: Entity) {
    let mut world = eng.world_mut();
    if world.get::<&Interpolation>(entity).is_ok() {
        return;
    }
    let pose = world
        .get::<&Transform>(entity)
        .map_or_else(|_| Transform::identity(), |t| *t);
    let _ = world.insert_one(entity, Interpolation::at(pose));
}

/// Stop keeping this node's poses; it goes back to the tick's own pose.
pub fn disable(eng: &Engine, entity: Entity) {
    let _ = eng.world_mut().remove_one::<Interpolation>(entity);
}

/// A script just attached to `entity`: one with a `fixed_update` moves it there.
pub(crate) fn follow_script(
    eng: &Engine,
    entity: Entity,
    host: &dyn balaur_script::ScriptHost<Engine>,
) {
    if host.has_method(crate::node_id_of(entity), "fixed_update") {
        enable(eng, entity);
    }
    crate::events::hearers_may_have_changed(eng, entity);
}

/// Whether the node is drawn between steps right now.
#[must_use]
pub fn is_on(eng: &Engine, entity: Entity) -> bool {
    eng.world().get::<&Interpolation>(entity).is_ok()
}

/// Throw both kept poses away and start again from where the node is.
///
/// What a teleport calls: without it a respawn draws the character streaking
/// across the level over the frames that blend the two positions.
pub fn reset(eng: &Engine, entity: Entity) {
    let world = eng.world();
    let Ok(pose) = world.get::<&Transform>(entity).map(|t| *t) else {
        return;
    };
    if let Ok(mut kept) = world.get::<&mut Interpolation>(entity) {
        *kept = Interpolation::at(pose);
    }
}

/// Take every kept node's pose, right after a fixed step ran.
pub fn capture(eng: &Engine) {
    let world = eng.world();
    for (kept, pose) in &mut world.query::<(&mut Interpolation, &Transform)>() {
        kept.previous = kept.current;
        kept.current = *pose;
    }
}
