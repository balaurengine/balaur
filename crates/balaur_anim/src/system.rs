//! The fixed-step system: advance every playing node, write the pose, and
//! deliver what the step passed over.
//!
//! `dt` only ever feeds the accumulator; what the simulation sees is
//! [`FIXED_DT`], every time, so the same inputs give the same transforms on
//! every machine no matter how the frames fell.
//!
//! Two things happen strictly *after* the animation state and the world are
//! released. A component's `apply` hook may take the world mutably
//! (`render`'s does), and a script handler may play, stop, spawn or free
//! anything — including the node it was called on. So a step records what it
//! wants done and the frame does it once every borrow is gone, the same shape
//! `balaur_ui` uses to settle its clicks.

use balaur_core::Engine;
use balaur_core::components;
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::{self, Transform};
use glamx::Vec4;

use crate::clip::{Clip, Property, Wrap};
use crate::player::{AnimationState, FIXED_DT, MAX_SUBSTEPS, Playback};
use crate::sampler::{self, TrackValue};
use crate::tween::{self, TweenId};

/// The method a node's script is called with when its clip ends.
const FINISHED_METHOD: &str = "on_animation_finished";

/// Something a step wants done once the borrows are gone.
pub(crate) enum Effect {
    /// Write one property of one component, leaving the rest of it alone.
    Patch {
        entity: Entity,
        component: String,
        property: String,
        value: toml::Value,
    },
    /// Call a method on a node's script instance.
    Call { entity: Entity, method: String },
    /// Tell a node's script the tween it holds a handle to has run out.
    TweenFinished { entity: Entity, id: TweenId },
}

/// The method a node's script is called with when a tween on it ends.
const TWEEN_FINISHED_METHOD: &str = "on_tween_finished";

/// Re-resolve every live clip after an asset reload, keeping the playhead.
///
/// A `Playback` holds an `Rc<Clip>`, so a reload cannot pull a clip out from
/// under a frame in progress — which is exactly why it would otherwise keep
/// playing the old one forever. Saving a clip in the editor, or on disk in dev
/// mode, should be visible the next frame the way saving a script is.
///
/// Costs one integer compare on a frame where nothing was reloaded.
fn refresh_reloaded_clips(eng: &Engine) {
    let Some(state) = eng.try_resource::<AnimationState>() else {
        return;
    };
    let generation = balaur_core::assets::generation(eng);
    let references: Vec<(Entity, String)> = {
        let state = state.borrow();
        if state.asset_generation == generation {
            return;
        }
        state
            .players
            .iter()
            .filter(|(_, playback)| playback.active())
            .map(|(&entity, playback)| (entity, playback.reference(&playback.clip_name)))
            .collect()
    };
    // Loading takes the asset cache's borrow and may parse, so it happens
    // outside the animation state's.
    let reloaded: Vec<(Entity, Option<std::rc::Rc<Clip>>)> = references
        .into_iter()
        .map(|(entity, reference)| {
            let clip = balaur_core::assets::load_typed::<Clip>(eng, &reference).ok();
            if clip.is_none() {
                tracing::warn!("'{reference}' no longer loads; keeping the clip already playing");
            }
            (entity, clip)
        })
        .collect();
    let mut state = state.borrow_mut();
    for (entity, clip) in reloaded {
        // A clip that stopped loading keeps the copy it is playing: a
        // half-saved file must not blank the scene mid-frame.
        if let (Some(playback), Some(clip)) = (state.players.get_mut(&entity), clip) {
            playback.time = playback.time.min(clip.length);
            playback.clip = Some(clip);
        }
    }
    state.asset_generation = generation;
}

/// Advance every playing node by whole fixed steps.
pub(crate) fn advance_system(eng: &Engine, dt: f32) {
    // A held game does not animate, and time spent held is dropped rather
    // than owed — what `App::run_fixed_steps` does with the simulation's own.
    if eng.frozen_root().is_some() {
        eng.resource::<AnimationState>().borrow_mut().accumulator = 0.0;
        return;
    }
    let mut effects = Vec::new();
    let mut ended: Vec<Entity> = Vec::new();
    refresh_reloaded_clips(eng);
    {
        let state = eng.resource::<AnimationState>();
        let mut state = state.borrow_mut();
        let state = &mut *state;
        let world = eng.world();
        // `Playback` and `Tween` live here, not on the entity, so this is the
        // first place a `queue_free`d node's leftovers can be dropped.
        state.players.retain(|&entity, _| world.contains(entity));
        state
            .tweens
            .retain(|_, tween| world.contains(tween.node) && (tween.running || !tween.value));
        // A frame's worth of `just_finished` expires here: the script tick
        // that could read it has already run.
        for playback in state.players.values_mut() {
            playback.finished.clear();
        }
        state.accumulator = (state.accumulator + dt).min(FIXED_DT * MAX_SUBSTEPS as f32);
        while state.accumulator >= FIXED_DT {
            for (&entity, playback) in &mut state.players {
                if advance_playback(&world, entity, playback, &mut effects) {
                    ended.push(entity);
                }
            }
            // Tweens come after the players, so a tween is what lands on a
            // property both of them drive. One waiting on another sits out
            // the step; the step after that tween is gone, it begins.
            let waiting: Vec<TweenId> = state
                .tweens
                .iter()
                .filter(|(_, tween)| tween.after.is_some_and(|on| state.tweens.contains_key(&on)))
                .map(|(&id, _)| id)
                .collect();
            let mut done: Vec<TweenId> = Vec::new();
            for (&id, tween) in &mut state.tweens {
                if waiting.contains(&id) {
                    continue;
                }
                if let Err(why) = tween::begin(eng, tween) {
                    tracing::warn!("a tween that waited its turn no longer builds: {why:#}");
                    done.push(id);
                    continue;
                }
                if tween::advance(&world, tween, &mut effects) {
                    if tween.played > 0 {
                        effects.push(Effect::TweenFinished {
                            entity: tween.node,
                            id,
                        });
                    }
                    done.push(id);
                }
            }
            for id in done {
                state.tweens.shift_remove(&id);
            }
            state.accumulator -= FIXED_DT;
        }
    }
    apply_effects(eng, &effects);
    settle_ended(eng, &ended);
}

/// One fixed step of one node. Answers whether the clip ended on this step.
fn advance_playback(
    world: &World,
    entity: Entity,
    playback: &mut Playback,
    effects: &mut Vec<Effect>,
) -> bool {
    if !playback.playing {
        return false;
    }
    let Some(clip) = playback.clip.clone() else {
        playback.playing = false;
        return false;
    };
    let was = playback.time;
    playback.time += FIXED_DT * playback.speed;
    let (time, past_end) = sampler::clip_time(&clip, playback.time);
    // Backwards off the start ends a non-looping clip too, or a negative
    // speed would leave it playing at time zero for the rest of the session.
    let backwards_off = playback.speed < 0.0 && playback.time <= 0.0 && clip.wrap == Wrap::None;
    let finished = past_end || backwards_off;
    if finished {
        // The last pose is still written: a clip that ends holds its final
        // key rather than snapping back to wherever the node was.
        playback.playing = false;
        playback.paused = false;
        playback.finished = playback.clip_name.clone();
    }
    let pose = sampler::sample(&clip, time);
    write_pose(world, entity, &playback.root, &clip, &pose, effects);
    collect_calls(
        world,
        entity,
        &playback.root,
        &clip,
        was,
        playback.time,
        effects,
    );
    finished
}

/// Pose one node at its playhead, without advancing anything.
///
/// What a `seek` shows. The fixed step is the only thing that moves time, so
/// this changes nothing a later step would compute differently: the pose is a
/// pure function of the clip and the playhead, and no method key counts as
/// passed because no span was travelled.
pub(crate) fn pose_now(eng: &Engine, entity: Entity) {
    let mut effects = Vec::new();
    {
        let state = eng.resource::<AnimationState>();
        let state = state.borrow();
        let Some(playback) = state.players.get(&entity) else {
            return;
        };
        let Some(clip) = playback.clip.as_ref() else {
            return;
        };
        let (time, _) = sampler::clip_time(clip, playback.time);
        let pose = sampler::sample(clip, time);
        let world = eng.world();
        write_pose(&world, entity, &playback.root, clip, &pose, &mut effects);
    }
    apply_effects(eng, &effects);
}

/// Write one sampled pose into the scene tree.
///
/// Targets are resolved every step rather than cached: a track may name a node
/// that is spawned, freed or reparented while the clip is running, and a
/// missing one is skipped rather than fatal.
pub(crate) fn write_pose(
    world: &World,
    entity: Entity,
    root: &str,
    clip: &Clip,
    pose: &[TrackValue],
    effects: &mut Vec<Effect>,
) {
    for (track, value) in clip.tracks.iter().zip(pose) {
        let Some(target) = target_of(world, entity, root, &track.target) else {
            continue;
        };
        if let TrackValue::Property { value, channels } = *value {
            let Property::Component {
                component,
                property,
            } = &track.property
            else {
                continue;
            };
            effects.push(Effect::Patch {
                entity: target,
                component: component.clone(),
                property: property.clone(),
                value: numbers(value, channels),
            });
            continue;
        }
        let Ok(mut transform) = world.get::<&mut Transform>(target) else {
            continue;
        };
        match *value {
            TrackValue::Position(position) => transform.position = position,
            TrackValue::Rotation(rotation) => transform.rotation = rotation,
            TrackValue::Scale(scale) => transform.scale = scale,
            TrackValue::Property { .. } | TrackValue::None => {}
        }
    }
}

/// Every method key this step passed over, in track order.
///
/// A key fires once per pass over its time — including the pass a looping
/// clip makes when it wraps, which is two spans in one step — and never for
/// the stretch a `seek` jumped, because a seek moves the playhead without a
/// step ever running over what it skipped.
pub(crate) fn collect_calls(
    world: &World,
    entity: Entity,
    root: &str,
    clip: &Clip,
    was: f32,
    now: f32,
    effects: &mut Vec<Effect>,
) {
    let spans = sampler::spans(clip, was, now);
    if spans.is_empty() {
        return;
    }
    for track in &clip.tracks {
        if track.property != Property::Call {
            continue;
        }
        let Some(target) = target_of(world, entity, root, &track.target) else {
            continue;
        };
        for key in &track.keys {
            let Some(method) = key.call.as_ref() else {
                continue;
            };
            if spans.iter().any(|&span| sampler::passes(span, key.t)) {
                effects.push(Effect::Call {
                    entity: target,
                    method: method.clone(),
                });
            }
        }
    }
}

/// The node a track's `target` names, resolved against the player's `root`.
fn target_of(world: &World, entity: Entity, root: &str, target: &str) -> Option<Entity> {
    let base = if root.is_empty() {
        Some(entity)
    } else {
        scene::find_node(world, entity, root)
    };
    let root = base.or_else(|| {
        tracing::debug!(root, "animation root path names no node");
        None
    })?;
    if target.is_empty() {
        return Some(root);
    }
    scene::find_node(world, root, target).or_else(|| {
        tracing::debug!(target, "animation track targets no node");
        None
    })
}

/// A sampled component value as the property table `patch` takes.
///
/// One channel is a number and the rest are a list, which is how a component
/// schema spells `radius = 0.5` against `rgba = [1, 0, 0, 1]`.
fn numbers(value: Vec4, channels: usize) -> toml::Value {
    if channels == 1 {
        return toml::Value::Float(f64::from(value.x));
    }
    toml::Value::Array(
        value
            .to_array()
            .into_iter()
            .take(channels)
            .map(|n| toml::Value::Float(f64::from(n)))
            .collect(),
    )
}

/// Do what the steps asked for, now that nothing is borrowed.
fn apply_effects(eng: &Engine, effects: &[Effect]) {
    let host = eng.script_host();
    for effect in effects {
        match effect {
            Effect::Patch {
                entity,
                component,
                property,
                value,
            } => {
                let params = toml::Value::Table(toml::map::Map::from_iter([(
                    property.clone(),
                    value.clone(),
                )]));
                if let Err(why) = components::patch(eng, *entity, component, &params) {
                    tracing::debug!(
                        component = component.as_str(),
                        property = property.as_str(),
                        "animation track: {why:#}"
                    );
                }
            }
            Effect::Call { entity, method } => {
                if let Some(host) = host.as_ref() {
                    host.call_on(balaur_core::node_id_of(*entity), method, &[]);
                }
            }
            Effect::TweenFinished { entity, id } => {
                if let Some(host) = host.as_ref() {
                    host.call_on(
                        balaur_core::node_id_of(*entity),
                        TWEEN_FINISHED_METHOD,
                        &[balaur_script::Value::Int(
                            i64::try_from(*id).unwrap_or(i64::MAX),
                        )],
                    );
                }
            }
        }
    }
}

/// Start whatever was queued behind a clip that just ended, then tell its
/// script.
///
/// The signal goes out after the queue moves on, so a handler asking
/// `animation.current(node)` sees what is playing now rather than what just
/// stopped. The clip that ended is the handler's argument —
/// `animation.just_finished(node)` still answers for this frame, for a script
/// that would rather poll than declare a method.
fn settle_ended(eng: &Engine, ended: &[Entity]) {
    for &entity in ended {
        let next = {
            let state = eng.resource::<AnimationState>();
            let mut state = state.borrow_mut();
            state
                .players
                .get_mut(&entity)
                .filter(|playback| !playback.queue.is_empty())
                .map(|playback| playback.queue.remove(0))
        };
        if let Some(name) = next
            && let Err(why) = crate::play(eng, entity, &name) {
                tracing::warn!("queued animation '{name}': {why:#}");
            }
        let finished = {
            let state = eng.resource::<AnimationState>();
            let state = state.borrow();
            state
                .players
                .get(&entity)
                .map(|playback| playback.finished.clone())
                .unwrap_or_default()
        };
        if let Some(host) = eng.script_host() {
            host.call_on(
                balaur_core::node_id_of(entity),
                FINISHED_METHOD,
                &[balaur_script::Value::Str(finished)],
            );
        }
    }
}
