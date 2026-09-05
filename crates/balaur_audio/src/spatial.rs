//! Positional audio: where the ears are, where a sound is, and what the
//! distance between them does to it.
//!
//! A `listener` node is the ears; a `sound` marked `positional`, or an
//! `audio.play` given a `position`, is an emitter. Once a frame's transforms
//! have settled, every live emitter is re-placed: distance sets its gain, its
//! offset across the listener's right sets its pan, and the speed the two
//! close at bends its pitch. All of it multiplies the bus chain rather than
//! replacing it, so a positional sound still answers the `sfx` slider.
//!
//! Nothing here reaches the simulation. It runs after the tick, off poses the
//! frame already decided, and writes to sinks and to the [`Placement`] each
//! handle remembers — which is what a run with no output device can still
//! assert, the same rule the rest of this crate holds to.

use balaur_core::components::ComponentDef;
use balaur_core::glamx::Vec3;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, GlobalTransform, scene};

use crate::bus::{self, Buses};
use crate::keys as k;
use crate::{AudioState, MIN_PITCH};

/// Metres per second. A game whose unit is not a metre tunes `doppler` per
/// sound rather than this.
pub const SPEED_OF_SOUND: f32 = 343.0;

/// How far doppler may bend a pitch, either way: a node that teleports is
/// not a supersonic pass, and a screech is worse than a wrong note.
const MAX_DOPPLER: f32 = 2.0;

/// The floor `min_distance` is clamped to. At zero a sound would drop from
/// full volume to nothing the instant it left the listener's exact point.
const MIN_DISTANCE: f32 = 0.001;

/// The `listener` component: the node a positional mix is heard from.
pub struct Listener {
    pub current: bool,
}

/// Where the ears are this frame.
pub struct ListenerPose {
    pub position: Vec3,
    /// The listener's right in world space; an emitter's offset along it is
    /// the pan.
    pub right: Vec3,
    pub velocity: Vec3,
    /// False until a `listener` node or `audio.set_listener` places it, and
    /// positional sounds play flat until it is true.
    pub placed: bool,
    previous: Option<Vec3>,
}

impl Default for ListenerPose {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            right: Vec3::X,
            velocity: Vec3::ZERO,
            placed: false,
            previous: None,
        }
    }
}

impl ListenerPose {
    /// Put the ears at a point, keeping the way they face.
    pub fn place(&mut self, position: Vec3) {
        self.position = position;
        self.placed = true;
    }

    /// Put the ears at a node's pose: where it is, and which way its right is.
    pub fn follow(&mut self, at: &GlobalTransform) {
        self.place(at.position);
        self.right = at.rotation * Vec3::X;
    }

    fn track(&mut self, dt: f32) {
        self.velocity = velocity_of(&mut self.previous, self.position, dt);
    }
}

/// Where a positional sound plays from, and how far it carries.
#[derive(Clone, Debug)]
pub struct Emitter {
    pub position: Vec3,
    /// Full volume within this radius of the listener.
    pub min_distance: f32,
    /// Silent beyond it.
    pub max_distance: f32,
    /// How much the closing speed bends the pitch: 0 is off, 1 physical.
    pub doppler: f32,
    /// The sound's own pitch, which the doppler multiplier is applied to.
    /// `play` fills it in from the cue.
    pub pitch: f32,
    /// Measured from how far the emitter moved between frames.
    pub velocity: Vec3,
    /// What the last frame decided about this emitter.
    pub placement: Placement,
    previous: Option<Vec3>,
}

impl Emitter {
    #[must_use]
    pub fn new(position: Vec3, min_distance: f32, max_distance: f32, doppler: f32) -> Self {
        let min_distance = min_distance.max(MIN_DISTANCE);
        Self {
            position,
            min_distance,
            max_distance: max_distance.max(min_distance),
            doppler: doppler.max(0.0),
            pitch: 1.0,
            velocity: Vec3::ZERO,
            placement: Placement::default(),
            previous: None,
        }
    }

    fn track(&mut self, dt: f32) {
        self.velocity = velocity_of(&mut self.previous, self.position, dt);
    }
}

/// What a frame decided about one emitter — the numbers actually applied to
/// its sink.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Distance attenuation: 1 within `min_distance`, 0 past `max_distance`.
    pub gain: f32,
    /// -1 hard left, 0 centred, 1 hard right.
    pub pan: f32,
    /// The doppler multiplier on the sound's own pitch.
    pub pitch: f32,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        }
    }
}

/// How far something moved between two frames, per second. A first frame and
/// a paused one both report standing still rather than a division by zero.
fn velocity_of(previous: &mut Option<Vec3>, position: Vec3, dt: f32) -> Vec3 {
    match previous.replace(position) {
        Some(previous) if dt > 0.0 => (position - previous) / dt,
        _ => Vec3::ZERO,
    }
}

/// How an emitter lands on the listener's ears.
///
/// Inverse-distance attenuation, clamped at both ends: a sound is at full
/// volume inside `min_distance`, halves with every doubling beyond it, and is
/// cut at `max_distance` — where, for any sane pair, it is already inaudible.
#[must_use]
pub fn place(listener: &ListenerPose, emitter: &Emitter) -> Placement {
    if !listener.placed {
        return Placement::default();
    }
    let to_emitter = emitter.position - listener.position;
    let distance = to_emitter.length();
    let min = emitter.min_distance.max(MIN_DISTANCE);
    let max = emitter.max_distance.max(min);
    let gain = if distance <= min {
        1.0
    } else if distance >= max {
        0.0
    } else {
        min / distance
    };
    // A sound at the ear has no direction to it: the pan opens up as it moves
    // out towards `min_distance`.
    let pan = if distance <= MIN_DISTANCE {
        0.0
    } else {
        (to_emitter.dot(listener.right) / distance) * (distance / min).min(1.0)
    };
    Placement {
        gain,
        pan: pan.clamp(-1.0, 1.0),
        pitch: doppler(listener, emitter, to_emitter, distance),
    }
}

/// What the two ends' speed along the line between them does to the pitch:
/// closing raises it, opening lowers it. OpenAL's model, with both speeds
/// held below the speed of sound so the ratio stays finite.
fn doppler(listener: &ListenerPose, emitter: &Emitter, to_emitter: Vec3, distance: f32) -> f32 {
    if emitter.doppler <= 0.0 || distance <= MIN_DISTANCE {
        return 1.0;
    }
    let to_listener = -to_emitter / distance;
    let ceiling = SPEED_OF_SOUND * 0.9;
    let along =
        |velocity: Vec3| (velocity.dot(to_listener) * emitter.doppler).clamp(-ceiling, ceiling);
    let heard = SPEED_OF_SOUND - along(listener.velocity);
    let sounded = SPEED_OF_SOUND - along(emitter.velocity);
    (heard / sounded).clamp(1.0 / MAX_DOPPLER, MAX_DOPPLER)
}

/// The left and right gains a pan asks for.
///
/// Equal power — the two squares sum to one — so a sound panned hard is no
/// quieter than one in the middle. Square roots rather than the usual sine
/// pair: the platform's trigonometry is not the same everywhere
/// (DETERMINISM.md), and this shape needs no exemption.
#[must_use]
pub fn stereo_gains(pan: f32) -> [f32; 2] {
    let right = f32::midpoint(pan.clamp(-1.0, 1.0), 1.0);
    [(1.0 - right).sqrt(), right.sqrt()]
}

/// Follow the listener node, then re-place every positional sound.
///
/// Runs in `SceneSync` after the core's transform propagation, so the ears
/// and the emitters are placed from the same settled poses the frame draws.
pub(crate) fn spatialize_system(eng: &Engine, dt: f32) {
    let state = eng.resource::<AudioState>();
    let idle = {
        let state = state.borrow();
        state.listeners.is_empty() && state.spatial.is_empty()
    };
    if idle {
        return;
    }
    bus::ensure_loaded(eng);
    let buses = eng.resource::<Buses>();
    let buses = buses.borrow();
    let mut state = state.borrow_mut();
    follow_nodes(&mut state, eng);
    place_live(&mut state, &buses, dt);
}

/// The ears go where the current listener node is, and a positional `sound`
/// emits from wherever its own node is now.
fn follow_nodes(state: &mut AudioState, eng: &Engine) {
    let world = eng.world();
    let AudioState {
        nodes,
        spatial,
        listeners,
        listener,
        ..
    } = state;
    let current = listeners
        .iter()
        .rev()
        .find(|(_, node)| node.current)
        .map(|(entity, _)| *entity);
    if let Some(entity) = current
        && let Ok(global) = world.get::<&GlobalTransform>(entity)
    {
        listener.follow(&global);
    }
    for (entity, sound) in nodes.iter() {
        let Some(handle) = sound.handle else { continue };
        let Some(emitter) = spatial.get_mut(&handle) else {
            continue;
        };
        if let Ok(global) = world.get::<&GlobalTransform>(*entity) {
            emitter.position = global.position;
        }
    }
}

/// Re-place every emitter and hand the result to its sink.
fn place_live(state: &mut AudioState, buses: &Buses, dt: f32) {
    let AudioState {
        playing,
        routing,
        spatial,
        listener,
        ..
    } = state;
    listener.track(dt);
    for (handle, emitter) in spatial.iter_mut() {
        emitter.track(dt);
        emitter.placement = place(listener, emitter);
        let Some(routed) = routing.get_mut(handle) else {
            continue;
        };
        routed.applied = routed.volume * buses.gain(&routed.bus) * emitter.placement.gain;
        let Some(sink) = playing.get(handle) else {
            continue;
        };
        sink.set_volume(routed.applied);
        sink.set_pitch((emitter.pitch * emitter.placement.pitch).max(MIN_PITCH));
        sink.set_pan(stereo_gains(emitter.placement.pan));
    }
}

/// The `listener` scene key. Kept in `AudioState` beside the `sound` nodes
/// rather than on the entity, so finding the current one costs no query.
///
/// Takes the plugin `Registry` rather than `&mut App`, as the `sound`
/// component beside it does: audio registers through the plugin seam.
pub(crate) fn register_listener_component(reg: &mut balaur_plugin::Registry<'_>) {
    reg.register_component(
        "listener",
        ComponentDef {
            doc: "The ears a positional sound is heard from: its distance to this node sets \
                  its volume, and its offset across this node's right sets its pan. The last \
                  `current` listener applied wins; with no listener in the scene at all, every \
                  sound plays flat.",
            schema: ComponentDef::parse_schema(
                "listener",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::CURRENT, r#"{ type = "bool", default = true, description = "Whether the mix is heard from this node; the last current one wins" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::AUDIO],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let current = params
                    .get(k::CURRENT)
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true);
                // Composed and placed as the component is applied: a sound
                // started before the first scene sync would hear the origin.
                let pose = current.then(|| scene::composed_global(&eng.world(), entity));
                let state = eng.resource::<AudioState>();
                let mut state = state.borrow_mut();
                state.listeners.insert(entity, Listener { current });
                if let Some(pose) = pose {
                    state.listener.follow(&pose);
                }
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                if let Some(state) = eng.try_resource::<AudioState>() {
                    state.borrow_mut().listeners.shift_remove(&entity);
                }
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let state = eng.try_resource::<AudioState>()?;
                let state = state.borrow();
                let node = state.listeners.get(&entity)?;
                let mut out = toml::map::Map::new();
                out.insert(k::CURRENT.into(), node.current.into());
                Some(toml::Value::Table(out))
            }),
        },
    );
}

/// Forget the listeners of nodes that have been freed. Called from the same
/// sweep that drops finished sounds.
pub(crate) fn sweep_listeners(state: &mut AudioState, world: &balaur_core::hecs::World) {
    state
        .listeners
        .retain(|&entity: &Entity, _| world.contains(entity));
}
