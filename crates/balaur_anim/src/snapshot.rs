//! What animation puts in a rollback snapshot and in a tick's digest.
//!
//! `Playback` and `Tween` are simulation state this plugin owns outright:
//! core cannot see them, so a rollback that restored transforms without them
//! would rewind a kinematic platform's pose and leave the playhead that wrote
//! it where it was, and every body the platform pushes diverges from there.
//!
//! Everything is keyed by `StableId`, because core's `nodes` source
//! respawns a freed node as a *new* entity and every later source has to find
//! it again; a node with no id — a tree built by hand in a test — falls back
//! to the entity it was recorded under, the same fallback core's own sources
//! make.
//!
//! The fixed-step accumulator is in the snapshot and deliberately not in the
//! digest: a rollback re-simulates on one machine and has to resume mid-step,
//! while two peers running at different frame rates hold different residuals
//! and are not thereby desynced.

use balaur_core::digest::{Entry, Hasher, node_label};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, assets, ids};
use balaur_plugin::Registry;
use glamx::Vec4;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clip::{Clip, Interp, Key, Property, Track, Wrap};
use crate::ease::Easing;
use crate::player::{AnimationState, Playback};
use crate::tween::{Tween, TweenId};

/// The name every source this module registers is filed under.
const SOURCE: &str = "animation";

pub(crate) fn register(reg: &mut Registry<'_>) {
    reg.add_digest_source(SOURCE, digest_source);
    reg.add_snapshot_source(SOURCE, capture, restore);
    // A recording starts at a frame boundary and its replay has to start at
    // the same one, or the first frames take a different number of steps.
    reg.add_replay_setup(
        SOURCE,
        |eng| Value::from(accumulator(eng)),
        |eng, value| {
            if let Some(state) = eng.try_resource::<AnimationState>() {
                let residual = value.as_f64().unwrap_or_default() as f32;
                state.borrow_mut().accumulator = residual;
            }
        },
    );
}

fn accumulator(eng: &Engine) -> f32 {
    eng.try_resource::<AnimationState>()
        .map_or(0.0, |state| state.borrow().accumulator)
}

/// One node's playhead, in a form a snapshot can hold.
#[derive(Serialize, Deserialize)]
struct PlayerFrame {
    /// The node's stable id, empty for a node that carries none.
    id: String,
    /// The entity it was captured under, for a node with no id.
    entity: u64,
    library: String,
    autoplay: String,
    clip_name: String,
    time: f32,
    speed: f32,
    playing: bool,
    paused: bool,
    root: String,
    queue: Vec<String>,
    defined: Vec<(String, String)>,
    finished: String,
}

/// One running tween, generated clip included: a tween that ended between the
/// snapshot and the rollback has to come back, and nothing else holds its
/// keys.
#[derive(Serialize, Deserialize)]
struct TweenFrame {
    handle: TweenId,
    id: String,
    entity: u64,
    time: f32,
    speed: f32,
    running: bool,
    loops: u32,
    played: u32,
    after: Option<TweenId>,
    pending: Option<String>,
    value: bool,
    clip: ClipFrame,
}

#[derive(Serialize, Deserialize)]
struct ClipFrame {
    length: f32,
    wrap: String,
    tracks: Vec<TrackFrame>,
}

#[derive(Serialize, Deserialize)]
struct TrackFrame {
    target: String,
    /// `None` is a method track, which is a clip document with no `property`.
    property: Option<String>,
    channels: usize,
    interp: String,
    keys: Vec<KeyFrame>,
}

#[derive(Serialize, Deserialize)]
struct KeyFrame {
    t: f32,
    value: [f32; 4],
    call: Option<String>,
    ease: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct AnimationFrame {
    accumulator: f32,
    next_tween: TweenId,
    players: Vec<PlayerFrame>,
    tweens: Vec<TweenFrame>,
}

fn capture(eng: &Engine) -> Value {
    let Some(state) = eng.try_resource::<AnimationState>() else {
        return Value::Null;
    };
    let state = state.borrow();
    let world = eng.world();
    let id_of = |entity: Entity| ids::of(&world, entity).unwrap_or_default();
    let frame = AnimationFrame {
        accumulator: state.accumulator,
        next_tween: state.next_tween,
        players: state
            .players
            .iter()
            .map(|(&entity, playback)| PlayerFrame {
                id: id_of(entity),
                entity: entity.to_bits().get(),
                library: playback.library.clone(),
                autoplay: playback.autoplay.clone(),
                clip_name: playback.clip_name.clone(),
                time: playback.time,
                speed: playback.speed,
                playing: playback.playing,
                paused: playback.paused,
                root: playback.root.clone(),
                queue: playback.queue.clone(),
                defined: playback
                    .defined
                    .iter()
                    .map(|(name, reference)| (name.clone(), reference.clone()))
                    .collect(),
                finished: playback.finished.clone(),
            })
            .collect(),
        tweens: state
            .tweens
            .iter()
            .map(|(&handle, tween)| TweenFrame {
                handle,
                id: id_of(tween.node),
                entity: tween.node.to_bits().get(),
                time: tween.time,
                speed: tween.speed,
                running: tween.running,
                loops: tween.loops,
                played: tween.played,
                after: tween.after,
                pending: tween.pending.clone(),
                value: tween.value,
                clip: clip_frame(&tween.clip),
            })
            .collect(),
    };
    serde_json::to_value(frame).unwrap_or(Value::Null)
}

fn restore(eng: &Engine, value: &Value) {
    let frame: AnimationFrame = match serde_json::from_value(value.clone()) {
        Ok(frame) => frame,
        Err(why) => {
            tracing::warn!("animation snapshot: {why}");
            return;
        }
    };
    let resolved: Vec<(Entity, PlayerFrame)> = {
        let world = eng.world();
        let root = eng.root();
        frame
            .players
            .into_iter()
            .filter_map(|player| {
                Some((entity_of(&world, root, &player.id, player.entity)?, player))
            })
            .collect()
    };
    // Loading takes the asset cache's borrow and may parse, so every clip a
    // restored playhead needs is resolved before the state is written.
    let clips: Vec<Option<std::rc::Rc<Clip>>> = resolved
        .iter()
        .map(|(_, player)| clip_for(eng, player))
        .collect();
    let tweens: Vec<(TweenId, Tween)> = {
        let world = eng.world();
        let root = eng.root();
        frame
            .tweens
            .into_iter()
            .filter_map(|tween| {
                let node = entity_of(&world, root, &tween.id, tween.entity)?;
                Some((
                    tween.handle,
                    Tween {
                        node,
                        clip: std::rc::Rc::new(clip_of(&tween.clip)),
                        time: tween.time,
                        speed: tween.speed,
                        running: tween.running,
                        loops: tween.loops,
                        played: tween.played,
                        after: tween.after,
                        pending: tween.pending,
                        value: tween.value,
                    },
                ))
            })
            .collect()
    };
    let state = eng.resource::<AnimationState>();
    let mut state = state.borrow_mut();
    state.accumulator = frame.accumulator;
    state.next_tween = frame.next_tween;
    state.players.clear();
    for ((entity, player), clip) in resolved.into_iter().zip(clips) {
        state.players.insert(entity, playback_of(player, clip));
    }
    state.tweens.clear();
    for (handle, tween) in tweens {
        state.tweens.insert(handle, tween);
    }
}

/// The node a frame belongs to: its stable id where it has one, else the
/// entity it was captured under.
fn entity_of(
    world: &balaur_core::hecs::World,
    root: Entity,
    id: &str,
    entity: u64,
) -> Option<Entity> {
    if !id.is_empty() {
        return ids::find(world, root, id);
    }
    let entity = Entity::from_bits(entity)?;
    world.contains(entity).then_some(entity)
}

/// The clip a restored playhead is sampling, re-resolved through the cache.
///
/// A reference that no longer loads leaves the playhead where it was with no
/// clip, which is what a stopped player already looks like.
fn clip_for(eng: &Engine, player: &PlayerFrame) -> Option<std::rc::Rc<Clip>> {
    if !player.playing && !player.paused {
        return None;
    }
    let reference = if let Some(defined) = player
        .defined
        .iter()
        .find(|(name, _)| *name == player.clip_name)
    {
        defined.1.clone()
    } else if player.clip_name.is_empty() {
        player.library.clone()
    } else {
        format!("{}#{}", player.library, player.clip_name)
    };
    assets::load_typed::<Clip>(eng, &reference).ok()
}

fn playback_of(player: PlayerFrame, clip: Option<std::rc::Rc<Clip>>) -> Playback {
    let mut playback = Playback {
        library: player.library,
        autoplay: player.autoplay,
        clip_name: player.clip_name,
        clip,
        time: player.time,
        speed: player.speed,
        playing: player.playing,
        paused: player.paused,
        root: player.root,
        queue: player.queue,
        finished: player.finished,
        ..Playback::default()
    };
    for (name, reference) in player.defined {
        playback.defined.insert(name, reference);
    }
    playback
}

fn clip_frame(clip: &Clip) -> ClipFrame {
    ClipFrame {
        length: clip.length,
        wrap: clip.wrap.name().to_string(),
        tracks: clip
            .tracks
            .iter()
            .map(|track| TrackFrame {
                target: track.target.clone(),
                property: track.property.name(),
                channels: track.channels,
                interp: track.interp.name().to_string(),
                keys: track
                    .keys
                    .iter()
                    .map(|key| KeyFrame {
                        t: key.t,
                        value: key.value.to_array(),
                        call: key.call.clone(),
                        ease: key.ease.map(|ease| ease.name().to_string()),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// A frame back into the generated clip it came from. Every spelling here was
/// written by [`clip_frame`], so an unreadable one falls back rather than
/// failing: a snapshot is not content a stranger authored.
fn clip_of(frame: &ClipFrame) -> Clip {
    Clip {
        length: frame.length,
        wrap: Wrap::parse(&frame.wrap).unwrap_or(Wrap::None),
        tracks: frame
            .tracks
            .iter()
            .map(|track| Track {
                target: track.target.clone(),
                property: track.property.as_deref().map_or(Property::Call, |name| {
                    Property::parse(name).unwrap_or(Property::Call)
                }),
                channels: track.channels,
                interp: Interp::parse(&track.interp).unwrap_or(Interp::Linear),
                keys: track
                    .keys
                    .iter()
                    .map(|key| Key {
                        t: key.t,
                        value: Vec4::from(key.value),
                        call: key.call.clone(),
                        ease: key
                            .ease
                            .as_deref()
                            .and_then(|name| Easing::parse(name).ok()),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// What animation contributes to a tick's digest: the playhead, not the pose.
///
/// The pose is already hashed as the transform core walks; what no other
/// source can see is that two nodes at the same transform are one paused and
/// one playing, and diverge on the next tick.
fn digest_source(eng: &Engine, out: &mut Vec<Entry>) {
    let Some(state) = eng.try_resource::<AnimationState>() else {
        return;
    };
    let state = state.borrow();
    let world = eng.world();
    for (&entity, playback) in &state.players {
        if !world.contains(entity) {
            continue;
        }
        let mut h = Hasher::new();
        h.write_f32(playback.time);
        h.write_f32(playback.speed);
        h.write(&[u8::from(playback.playing), u8::from(playback.paused)]);
        h.write_str(&playback.clip_name);
        h.write_str(&playback.finished);
        for name in &playback.queue {
            h.write_str(name);
        }
        out.push(Entry {
            label: format!("{}/animation", node_label(&world, entity)),
            digest: h.finish(),
        });
    }
    for (&handle, tween) in &state.tweens {
        if !world.contains(tween.node) {
            continue;
        }
        let mut h = Hasher::new();
        h.write_u64(handle);
        h.write_f32(tween.time);
        h.write_f32(tween.speed);
        h.write(&[u8::from(tween.running)]);
        h.write_u64(u64::from(tween.played));
        out.push(Entry {
            label: format!("{}/tween", node_label(&world, tween.node)),
            digest: h.finish(),
        });
    }
}
