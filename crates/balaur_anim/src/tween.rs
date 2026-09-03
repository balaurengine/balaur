//! Tweens: a clip generated on the spot, with its start values read off the
//! node it was asked for.
//!
//! This is the unification the crate is built around. A tween is not a second
//! animation system: it is a short [`Clip`] whose keys are computed when the
//! tween starts, sampled by the same [`crate::sampler`], written by the same
//! pose path, and eased by the same curves. There is one interpolator in this
//! crate and a tween does not add another — which also means a tween is
//! serialisable, so an editor can author one and it can hot reload.
//!
//! ```rune
//! let id = animation::tween(this.node, #{
//!   loops: 1, speed: 1.0,
//!   steps: [
//!     #{ property: "position",   to: [0.0, 3.0, 0.0],      duration: 0.5, ease: "out_back" },
//!     #{ property: "color/rgba", to: [1.0, 0.0, 0.0, 1.0], duration: 0.5, parallel: true },
//!     #{ interval: 0.2 },
//!     #{ call: "on_landed" },
//!     #{ property: "position", by: [0.0, -3.0, 0.0], duration: 0.4, ease: "in_quad" },
//!   ],
//! });
//! animation::stop(id);  animation::is_running(id)
//! ```
//!
//! Steps run one after another; `parallel = true` joins a step to the one
//! before it (DOTween's `Join`, Godot's `parallel()`) and the next step
//! without it waits for the whole group (Godot's `chain()`). `to` is
//! absolute, `by` is relative to wherever the property is when that step
//! begins, and `from` states the start outright. Steps on one property build
//! one track, so a tween that moves a node twice is two segments of one
//! curve rather than two writers fighting over it.
//!
//! Every start value is captured when the tween is built, so a track holds
//! its first key from the moment the tween begins — exactly as a clip holds
//! its own first key before its first segment. A tween therefore owns the
//! properties it names for its whole length, including the stretch before the
//! step that moves them; that is the price of being one generated clip rather
//! than a queue of separate writers, and it is what makes two runs of a tween
//! identical to the bit.
//!
//! Tweens ride the same fixed 1/60 step as clips and are advanced after them,
//! so a tween is what lands on a property both are driving. A tween dies with
//! the node it was made on: `queue_free` defers to `Stage::Last`, and the
//! animation system notices the node is gone on the next frame.
//!
//! There is deliberately no chainable builder: a handle with methods would
//! need new userdata in every backend and again in every future language,
//! which is exactly what declaring against `Bindings<Engine>` exists to
//! avoid. The data form needs no backend sugar at all.

use std::rc::Rc;

use anyhow::{anyhow, bail, Context, Result};
use balaur_core::components::{self, as_f64};
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::{self, Transform};
use balaur_core::Engine;
use glamx::Vec4;

use crate::clip::{Clip, Interp, Key, Property, Track, Wrap};
use crate::ease::Easing;
use crate::player::{AnimationState, FIXED_DT};
use crate::sampler::{self, euler_from_quat};
use crate::system::Effect;

/// What `animation.tween` hands back, and what `animation.stop` and
/// `animation.is_running` take. Opaque: a script holds it and gives it back.
pub type TweenId = u64;

/// Where a call at the very head of a tween is placed.
///
/// `sampler::passes` is exclusive at the start of a span, which is what stops
/// a looping key firing twice at the seam and what stops a `seek` firing what
/// it skipped. A callback authored as the first step would sit at exactly
/// zero and never be passed, so it is nudged a quarter of a millisecond in —
/// still inside the first fixed step, and a power of two, so the nudge is
/// exact.
const CALL_AT_HEAD: f32 = 1.0 / 4096.0;

/// One running tween.
///
/// Everything about it that is not the generated clip: where the playhead is,
/// how many times round it has been, and which node it dies with.
pub struct Tween {
    /// The node the tween was created on. Track targets resolve against it,
    /// and the tween is dropped when it is freed.
    pub node: Entity,
    /// The generated clip. Private to this tween — its keys hold values
    /// captured from the world, so it is not shared and not in the asset
    /// cache.
    pub clip: Rc<Clip>,
    /// Seconds of playback, before wrapping.
    pub time: f32,
    /// Scales the whole sequence. Always positive: a tween that ran backwards
    /// would never reach its end and never be cleaned up.
    pub speed: f32,
    /// Whether the playhead is advancing. False only in the instant between
    /// the last step and removal.
    pub running: bool,
    /// How many times to play the sequence; zero is forever (Godot's
    /// `set_loops(0)`).
    pub loops: u32,
    /// How many times it has been played through.
    pub played: u32,
}

/// Build a tween on `node` from a specification table and start it.
///
/// The start value of every step is captured now: the first step on a
/// property reads the node, and a later step on the same property continues
/// from where the one before it left off. That is what makes `by` relative to
/// the value at that point in the sequence rather than to where the node
/// happened to be when the tween was written.
///
/// # Errors
/// If the specification is not a tween: an unknown property, a step that says
/// nothing, a target that names no node, or a component the node does not
/// have.
pub fn start(eng: &Engine, node: Entity, spec: &toml::Value) -> Result<TweenId> {
    let speed = match spec.get("speed") {
        Some(v) => {
            as_f64(v).ok_or_else(|| anyhow!("`speed` is {}, not a number", v.type_str()))? as f32
        }
        None => 1.0,
    };
    if !speed.is_finite() || speed <= 0.0 {
        bail!("`speed` scales a tween's own duration, so it has to be a positive number");
    }
    let clip = build(eng, node, spec)?;
    let state = eng.resource::<AnimationState>();
    let mut state = state.borrow_mut();
    state.next_tween += 1;
    let id = state.next_tween;
    state.tweens.insert(
        id,
        Tween {
            node,
            clip: Rc::new(clip),
            time: 0.0,
            speed,
            running: true,
            loops: loops_of(spec)?,
            played: 0,
        },
    );
    Ok(id)
}

/// The 90% case, spelled without a table: send one property somewhere over a
/// number of seconds.
///
/// # Errors
/// As [`start`].
pub fn start_to(
    eng: &Engine,
    node: Entity,
    property: &str,
    to: &toml::Value,
    duration: f32,
    ease: Option<&str>,
) -> Result<TweenId> {
    let mut step = toml::map::Map::new();
    step.insert("property".into(), property.into());
    step.insert("to".into(), to.clone());
    step.insert("duration".into(), f64::from(duration).into());
    if let Some(ease) = ease.filter(|name| !name.is_empty()) {
        step.insert("ease".into(), ease.into());
    }
    let mut spec = toml::map::Map::new();
    spec.insert(
        "steps".into(),
        toml::Value::Array(vec![toml::Value::Table(step)]),
    );
    start(eng, node, &toml::Value::Table(spec))
}

/// End a tween now, wherever it had got to. Unknown handles are a no-op, so
/// stopping a tween that already finished is not an error.
pub fn stop(eng: &Engine, id: TweenId) {
    if let Some(state) = eng.try_resource::<AnimationState>() {
        state.borrow_mut().tweens.shift_remove(&id);
    }
}

/// Whether `id` names a tween that is still going.
#[must_use]
pub fn is_running(eng: &Engine, id: TweenId) -> bool {
    let Some(state) = eng.try_resource::<AnimationState>() else {
        return false;
    };
    let state = state.borrow();
    state.tweens.get(&id).is_some_and(|tween| tween.running)
}

/// How many times a tween plays its sequence. Godot's rule: zero or fewer is
/// forever.
fn loops_of(spec: &toml::Value) -> Result<u32> {
    let Some(value) = spec.get("loops") else {
        return Ok(1);
    };
    let count = as_f64(value)
        .ok_or_else(|| anyhow!("`loops` is {}, not a count", value.type_str()))?
        as i64;
    Ok(if count <= 0 {
        0
    } else {
        u32::try_from(count).unwrap_or(u32::MAX)
    })
}

/// One fixed step of one tween. Answers whether it is finished and should be
/// forgotten.
pub(crate) fn advance(world: &World, tween: &mut Tween, effects: &mut Vec<Effect>) -> bool {
    if !tween.running {
        return true;
    }
    let clip = tween.clip.clone();
    let was = tween.time;
    tween.time += FIXED_DT * tween.speed;
    let (time, over) = sampler::clip_time(&clip, tween.time);
    let pose = sampler::sample(&clip, time);
    crate::system::write_pose(world, tween.node, "", &clip, &pose, effects);
    crate::system::collect_calls(world, tween.node, "", &clip, was, tween.time, effects);
    if !over {
        return false;
    }
    tween.played += 1;
    if tween.loops == 0 || tween.played < tween.loops {
        tween.time = 0.0;
        return false;
    }
    tween.running = false;
    true
}

/// The tracks a specification's steps add up to, in the order the steps first
/// mentioned them.
struct Builder<'a> {
    eng: &'a Engine,
    node: Entity,
    tracks: Vec<Track>,
    /// When the next sequential step starts: the end of everything so far.
    chain: f32,
    /// When the group being built started, which is what `parallel` joins.
    group: f32,
}

fn build(eng: &Engine, node: Entity, spec: &toml::Value) -> Result<Clip> {
    let steps = spec
        .get("steps")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow!("a tween needs a `steps` list"))?;
    if steps.is_empty() {
        bail!("a tween needs at least one step");
    }
    let mut builder = Builder {
        eng,
        node,
        tracks: Vec::new(),
        chain: 0.0,
        group: 0.0,
    };
    for (index, step) in steps.iter().enumerate() {
        builder
            .add(step)
            .with_context(|| format!("tween step {index}"))?;
    }
    for track in &mut builder.tracks {
        track.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
    Ok(Clip {
        // A tween of nothing but callbacks has no duration of its own, and
        // still has to live long enough for one step to deliver them.
        length: if builder.chain > 0.0 {
            builder.chain
        } else {
            FIXED_DT
        },
        // Repeats are the tween's own business: `loops` counts them, and a
        // looping clip would never end and never be cleaned up.
        wrap: Wrap::None,
        tracks: builder.tracks,
    })
}

impl Builder<'_> {
    /// Place one step on the timeline and record what it does.
    fn add(&mut self, step: &toml::Value) -> Result<()> {
        let parallel = step
            .get("parallel")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        let start = if parallel {
            self.group
        } else {
            self.group = self.chain;
            self.chain
        };
        let target = match step.get("target") {
            Some(v) => v
                .as_str()
                .ok_or_else(|| anyhow!("`target` is {}, not a node path", v.type_str()))?,
            None => "",
        };
        let duration = match (step.get("call"), step.get("interval")) {
            (Some(_), Some(_)) => bail!("a step waits or calls a method, not both"),
            (Some(call), None) => {
                let method = call
                    .as_str()
                    .ok_or_else(|| anyhow!("`call` is {}, not a method name", call.type_str()))?;
                self.add_call(target, start, method)?;
                0.0
            }
            (None, Some(interval)) => as_f64(interval)
                .ok_or_else(|| anyhow!("`interval` is {}, not seconds", interval.type_str()))?
                as f32,
            (None, None) => self.add_property(step, target, start)?,
        };
        if !duration.is_finite() || duration < 0.0 {
            bail!("a step lasts a number of seconds, and cannot last fewer than none");
        }
        self.chain = self.chain.max(start + duration);
        Ok(())
    }

    /// A callback step: a moment on a method track, and no time of its own.
    fn add_call(&mut self, target: &str, start: f32, method: &str) -> Result<()> {
        target_of(self.eng, self.node, target)?;
        let index = self.track_for(target, &Property::Call, 0);
        self.tracks[index].keys.push(Key {
            t: start.max(CALL_AT_HEAD),
            value: Vec4::ZERO,
            call: Some(method.to_string()),
            ease: None,
        });
        Ok(())
    }

    /// A property step: two keys on the property's track, and the seconds it
    /// takes to get from the first to the second.
    fn add_property(&mut self, step: &toml::Value, target: &str, start: f32) -> Result<f32> {
        let name = step
            .get("property")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                anyhow!("a step needs a `property` to animate, an `interval` to wait or a `call`")
            })?;
        let property = Property::parse(name)?;
        let duration = match step.get("duration") {
            Some(v) => as_f64(v)
                .ok_or_else(|| anyhow!("`duration` is {}, not seconds", v.type_str()))?
                as f32,
            None => bail!("a property step needs a `duration` in seconds"),
        };
        let ease = match step.get("ease") {
            Some(v) => Some(Easing::parse(v.as_str().ok_or_else(|| {
                anyhow!("`ease` is {}, not a curve name", v.type_str())
            })?)?),
            None => None,
        };
        let (channels, from, to) = self.endpoints(step, target, &property, name)?;
        let index = self.track_for(target, &property, channels);
        push_segment(&mut self.tracks[index], start, duration, from, to, ease);
        Ok(duration)
    }

    /// Where a step starts and where it ends, and how many numbers wide both
    /// of those are.
    fn endpoints(
        &mut self,
        step: &toml::Value,
        target: &str,
        property: &Property,
        name: &str,
    ) -> Result<(usize, Vec4, Vec4)> {
        let (destination, relative) = match (step.get("to"), step.get("by")) {
            (Some(_), Some(_)) => bail!("a step goes `to` a value or `by` one, not both"),
            (Some(v), None) => (v, false),
            (None, Some(v)) => (v, true),
            (None, None) => bail!("a property step needs a `to` value or a `by` offset"),
        };
        let (destination, count) = numbers(destination)?;
        let channels = match property.channels() {
            Some(fixed) if fixed != count => {
                bail!("`{name}` takes {fixed} numbers, and this step gives {count}")
            }
            Some(fixed) => fixed,
            None => count,
        };
        let from = match step.get("from") {
            Some(v) => {
                let (value, given) = numbers(v)?;
                if given != channels {
                    bail!(
                        "`from` gives {given} numbers where the rest of the step gives {channels}"
                    )
                }
                value
            }
            None => self.captured(target, property, channels)?,
        };
        Ok((
            channels,
            from,
            if relative {
                from + destination
            } else {
                destination
            },
        ))
    }

    /// The value a step starts from when it does not say: whatever an earlier
    /// step in this tween left on the same track, or else what the node
    /// holds right now.
    fn captured(&self, target: &str, property: &Property, channels: usize) -> Result<Vec4> {
        if let Some(track) = self.find(target, property) {
            if let Some(key) = self.tracks[track].keys.last() {
                return Ok(key.value);
            }
        }
        let entity = target_of(self.eng, self.node, target)?;
        current_value(self.eng, entity, property, channels)
    }

    fn find(&self, target: &str, property: &Property) -> Option<usize> {
        self.tracks
            .iter()
            .position(|track| track.target == target && track.property == *property)
    }

    /// The track this step writes to, made if this is the first step to
    /// mention it.
    fn track_for(&mut self, target: &str, property: &Property, channels: usize) -> usize {
        if let Some(index) = self.find(target, property) {
            return index;
        }
        self.tracks.push(Track {
            target: target.to_string(),
            property: property.clone(),
            channels,
            // The shaping is the key's own `ease`; the track between keys is
            // the straight line every curve is measured against.
            interp: Interp::Linear,
            keys: Vec::new(),
        });
        self.tracks.len() - 1
    }
}

/// Add one step's pair of keys to a track, holding the previous value across
/// any gap before it.
fn push_segment(
    track: &mut Track,
    start: f32,
    duration: f32,
    from: Vec4,
    to: Vec4,
    ease: Option<Easing>,
) {
    // An explicit `from` after a pause is a jump, not a slow drift across the
    // pause: the previous value is held right up to the moment this step
    // begins, and the two keys at the same time are what say so.
    if let Some(last) = track.keys.last() {
        if last.t < start && last.value != from {
            let held = last.value;
            track.keys.push(Key {
                t: start,
                value: held,
                call: None,
                ease: None,
            });
        }
    }
    track.keys.push(Key {
        t: start,
        value: from,
        call: None,
        ease: None,
    });
    track.keys.push(Key {
        t: start + duration,
        value: to,
        call: None,
        ease,
    });
}

/// The node a step's `target` names, relative to the node the tween is on.
fn target_of(eng: &Engine, node: Entity, target: &str) -> Result<Entity> {
    if target.is_empty() {
        return Ok(node);
    }
    scene::find_node(&eng.world(), node, target)
        .ok_or_else(|| anyhow!("`target = \"{target}\"` names no node under this one"))
}

/// What `entity` holds for `property` right now — the tween's start value
/// when the step does not state one.
fn current_value(
    eng: &Engine,
    entity: Entity,
    property: &Property,
    channels: usize,
) -> Result<Vec4> {
    if let Property::Component {
        component,
        property,
    } = property
    {
        return component_value(eng, entity, component, property, channels);
    }
    let world = eng.world();
    let transform = world
        .get::<&Transform>(entity)
        .map_err(|_| anyhow!("this node has no transform to tween"))?;
    Ok(match property {
        Property::Position => transform.position.extend(0.0),
        Property::Scale => transform.scale.extend(0.0),
        // Rotation keys are euler, so the rotation a tween starts from has to
        // be read back as one.
        Property::RotationEuler => euler_from_quat(transform.rotation).extend(0.0),
        Property::Rotation => Vec4::from(transform.rotation),
        Property::Component { .. } | Property::Call => Vec4::ZERO,
    })
}

/// One property of one registered component, read through the registry — the
/// same indirection the pose write uses, and the reason this crate can tween
/// `color/rgba` without depending on the crate that registered it.
fn component_value(
    eng: &Engine,
    entity: Entity,
    component: &str,
    property: &str,
    channels: usize,
) -> Result<Vec4> {
    let table = components::get(eng, entity, component).ok_or_else(|| {
        anyhow!("this node has no `{component}` component to tween `{property}` on")
    })?;
    let value = table
        .get(property)
        .ok_or_else(|| anyhow!("the `{component}` component has no `{property}` to tween"))?;
    let (value, count) = numbers(value)?;
    if count != channels {
        bail!("`{component}/{property}` is {count} numbers wide, and this step gives {channels}");
    }
    Ok(value)
}

/// A number or a list of up to four, as the sampler's four channels and the
/// count of how many of them mean anything.
fn numbers(value: &toml::Value) -> Result<(Vec4, usize)> {
    let items: Vec<&toml::Value> = match value {
        toml::Value::Array(items) => items.iter().collect(),
        scalar if as_f64(scalar).is_some() => vec![scalar],
        other => bail!(
            "a value is a number or a list of numbers, not {}",
            other.type_str()
        ),
    };
    if items.is_empty() || items.len() > 4 {
        bail!(
            "a value holds {} numbers; it takes one to four",
            items.len()
        );
    }
    let mut xyzw = [0.0_f32; 4];
    for (slot, item) in xyzw.iter_mut().zip(&items) {
        *slot = as_f64(item)
            .ok_or_else(|| anyhow!("a value holds {}, not a number", item.type_str()))?
            as f32;
    }
    Ok((Vec4::from(xyzw), items.len()))
}
