//! Drawing between two fixed steps, so a body simulated at 60 Hz moves every
//! frame on a 144 Hz display.
//!
//! Render-side state and nothing else. A node that opts in keeps the pose the
//! last two fixed steps left it in, and the scene sync draws
//! `lerp(previous, current, alpha)`. `node.transform` and
//! `node.global_position` answer the tick, never the blend, so no script sees
//! a pose the simulation never held and no digest contains one.
//!
//! The blend is one step behind real time, which is what buys the smoothness:
//! there is no pose past `current` to draw. That is also why it is off until a
//! project asks — `[time] interpolate = true` — since a run taking exactly one
//! step per frame gains nothing and would only pay the tick of latency.

use hecs::Entity;

use crate::engine::Engine;
use crate::scene::Transform;

/// The scene key and the node op both spell it this way.
pub const KEY: &str = "interpolate";

/// Whether this project draws between steps at all: `[time] interpolate`.
pub struct Interpolating(pub bool);

/// The two poses a node is drawn between, both local to its parent.
#[derive(Clone, Copy)]
pub struct Interpolation {
    /// What the step before last left the node in.
    pub previous: Transform,
    /// What the last step left it in.
    pub current: Transform,
}

/// What the node's own `interpolate` key said, so a body or a `fixed_update`
/// cannot overrule the author either way.
pub struct Pinned(pub bool);

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

/// Whether the project draws between steps. False turns every `enable` into
/// nothing, which is what keeps a project that wants the old frames paying
/// for none of this.
#[must_use]
pub fn on(eng: &Engine) -> bool {
    eng.try_resource::<Interpolating>()
        .is_some_and(|flag| flag.borrow().0)
}

/// Read `[time] interpolate` and file it where every caller reads it.
pub fn apply_setting(eng: &Engine) {
    let on = crate::settings::get(eng, "time/interpolate")
        .as_ref()
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    eng.insert_resource(Interpolating(on));
}

/// Draw this node between steps from now on: what a dynamic or kinematic body
/// and a script declaring `fixed_update` ask for. Does nothing for a node
/// whose `interpolate = false`, or in a project that has it off.
pub fn enable(eng: &Engine, entity: Entity) {
    if !on(eng) {
        return;
    }
    let mut world = eng.world_mut();
    if world.get::<&Pinned>(entity).is_ok_and(|p| !p.0)
        || world.get::<&Interpolation>(entity).is_ok()
    {
        return;
    }
    let pose = world
        .get::<&Transform>(entity)
        .map_or_else(|_| Transform::identity(), |t| *t);
    let _ = world.insert_one(entity, Interpolation::at(pose));
}

/// Stop drawing this node between steps; it goes back to the tick's own pose.
/// A node whose own `interpolate = true` keeps it: the key is the author's
/// answer, and a component changing underneath must not silently undo it.
pub fn disable(eng: &Engine, entity: Entity) {
    let world = eng.world();
    if world.get::<&Pinned>(entity).is_ok_and(|p| p.0) {
        return;
    }
    drop(world);
    let _ = eng.world_mut().remove_one::<Interpolation>(entity);
}

/// What the node's own `interpolate` key says: `Some(true)` draws it between
/// steps whatever it carries, `Some(false)` never does, `None` leaves it to
/// what the node is made of.
pub fn set(eng: &Engine, entity: Entity, want: Option<bool>) {
    match want {
        Some(on) => {
            let _ = eng.world_mut().insert_one(entity, Pinned(on));
            if on {
                enable(eng, entity);
            } else {
                let _ = eng.world_mut().remove_one::<Interpolation>(entity);
            }
        }
        None => {
            let _ = eng.world_mut().remove_one::<Pinned>(entity);
        }
    }
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

/// Take every opted-in node's pose, right after a fixed step ran.
pub fn capture(eng: &Engine) {
    let world = eng.world();
    for (kept, pose) in &mut world.query::<(&mut Interpolation, &Transform)>() {
        kept.previous = kept.current;
        kept.current = *pose;
    }
}
