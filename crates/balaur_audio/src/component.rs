//! The `sound` component: what a node asks of its sound, and what a write
//! of it does to the playback the node already has.

use anyhow::Result;
use balaur_core::Engine;
use balaur_core::components::derived::{Component, Written};
use balaur_core::hecs::Entity;

use crate::bus::Buses;
use crate::spatial::Attenuation;
use crate::{
    AudioState, DEFAULT_MAX_DISTANCE, DEFAULT_MIN_DISTANCE, FINISHED_EVENT, play_node,
    publish_playback_times, seconds,
};

/// A sound on the node: `file`, `volume_linear`, `pitch_scale` and `loop`, with its `stems` mixed in and its `queue` played after it. `autoplay` starts it on load, `node.sound.play()` triggers it, `seek` and `skip` move it, `positional` plays it from the node for the `listener`, and the node announces `finished` when it plays out. What it passes through on the way out is its `bus`'s, declared in `[audio.buses]`.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a scene key of its own"
)]
#[derive(Clone, Debug, Component)]
#[component(
    name = "sound",
    on_remove = sound_removed,
    events = &[(FINISHED_EVENT, "the handle that played out")],
    tags = [balaur_core::components::tag::AUDIO],
    after_write = written,
)]
pub struct Sound {
    /// Audio file, project-relative; required to play
    pub file: String,
    /// Start playing when the node enters the scene
    pub autoplay: bool,
    /// Restart the sound when it ends; with a `queue`, each file repeats until `node.sound.skip()` moves on
    #[property(name = "loop")]
    pub looped: bool,
    /// Linear gain; 1 is the file's own level
    #[property(name = "volume_linear", default = 1.0, min = 0.0)]
    pub volume: f32,
    /// Playback speed multiplier
    #[property(name = "pitch_scale", default = 1.0, min = 0.01)]
    pub pitch: f32,
    /// Audio bus this plays through; empty is `master`
    pub bus: String,
    /// Balance of a sound that is not positional: -1 left, 0 both channels at full, 1 right. A positional sound is panned by where it is
    #[property(min = -1.0, max = 1.0)]
    pub pan: f32,
    /// Place the sound where the node is, heard from the `listener`
    #[property(group = "positional")]
    pub positional: bool,
    /// Full volume within this distance of the listener
    #[property(default = DEFAULT_MIN_DISTANCE, min = 0.001, group = "positional")]
    pub min_distance: f32,
    /// Silent beyond this distance from the listener
    #[property(default = DEFAULT_MAX_DISTANCE, min = 0.001, group = "positional")]
    pub max_distance: f32,
    /// How a positional sound falls off past `min_distance`: `inverse` halves its gain at every doubling of distance, `inverse_square` quarters it
    #[property(default = Attenuation::Inverse, group = "positional")]
    pub attenuation: Attenuation,
    /// How much the closing speed bends the pitch; 0 is off, 1 physical
    #[property(name = "doppler_level", min = 0.0, group = "positional")]
    pub doppler: f32,
    /// Hold the sound where it is; `playback_time` and the count to `finished` hold with it
    #[property(group = "playback")]
    pub paused: bool,
    /// Seconds into the file playing now, counted on the fixed step so a run with no output device reads the same; 0 when nothing plays
    #[property(readonly, group = "playback")]
    pub playback_time: f64,
    /// Seconds into `file` each play starts from. Read when it starts
    #[property(min = 0.0, group = "playback")]
    pub start_time: f32,
    /// Seconds into `file` each play stops at, and a loop turns back at; 0 plays to the end. Read when it starts
    #[property(min = 0.0, group = "playback")]
    pub end_time: f32,
    /// Seconds into `file` every repeat after the first starts from; below 0 takes the file's own `loop_offset` import setting. Read when it starts
    #[property(default = -1.0, min = -1.0, group = "playback")]
    pub loop_offset: f32,
    /// Seconds of silence before the sound starts. Read when it starts
    #[property(min = 0.0, group = "playback")]
    pub delay: f32,
    /// Files played after `file`, in order and without a gap; `node.sound.skip()` moves to the next. Read when it starts
    #[property(group = "playback")]
    pub queue: Vec<String>,
    /// Files mixed with `file` for its whole length, each at its own import level. Read when it starts
    #[property(group = "playback")]
    pub stems: Vec<String>,
    /// Seconds the sound rises from silence over when it starts
    #[property(min = 0.0, group = "fades")]
    pub fade_in_time: f32,
    /// Seconds the sound falls to silence over: the last ones before it plays out, and the first after `audio.stop`
    #[property(min = 0.0, group = "fades")]
    pub fade_out_time: f32,
    /// Seconds a playing sound fades out over when `file` changes, while the new file, if `autoplay` starts it, fades in; 0 cuts
    #[property(min = 0.0, group = "fades")]
    pub crossfade_time: f32,
}

/// What a removed `Sound` leaves behind, taken with it.
fn sound_removed(eng: &Engine, entity: Entity) {
    let Some(state) = eng.try_resource::<AudioState>() else {
        return;
    };
    let mut state = state.borrow_mut();
    let removed = state.nodes.shift_remove(&entity);
    if let Some(handle) = removed.and_then(|voice| voice.handle) {
        state.stop(handle);
    }
}

/// The `sound` scene key: the one an editor-saved `[nodes.sound]` writes.
///
/// The node holds a [`Sound`]; its [`Voice`](crate::Voice) in
/// `AudioState::nodes` holds the playback it started.
///
/// Takes the plugin `Registry` rather than `&mut App`: audio registers
/// through the plugin seam, and `Registry::register_component` is that
/// seam's spelling of the same operation.
pub(crate) fn register_sound_component(reg: &mut balaur_plugin::Registry<'_>) {
    reg.add_component::<Sound>();
}

/// What a write does to the playback the node has: another `file` drops it,
/// fading out over `crossfade_time`, and anything else lands on it as it
/// plays. `autoplay` starts one on a node with none going, so re-writing the
/// component never restarts a sound already started: the rule the
/// `animation` component holds for its autoplay clip.
#[allow(
    clippy::unnecessary_wraps,
    reason = "an after-write hook may refuse a write; this one never does"
)]
fn written(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let Some(asked) = eng
        .world()
        .get::<&Sound>(entity)
        .ok()
        .map(|held| (*held).clone())
    else {
        return Ok(());
    };
    crate::ensure_loaded(eng);
    let (start, crossfade) = {
        let buses = eng.resource::<Buses>();
        let buses = buses.borrow();
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        let voice = state.nodes.entry(entity).or_default();
        let file_changed = voice.file != asked.file;
        voice.file.clone_from(&asked.file);
        let handle = voice.handle.take();
        // A `sound` naming another file drops the old playback.
        voice.handle = handle.filter(|_| !file_changed);
        let crossfade = match handle {
            Some(handle) if file_changed => {
                let crossfade_time = seconds(asked.crossfade_time);
                state.release_over(handle, crossfade_time);
                crossfade_time > 0.0
            }
            // What can change on a sound already going lands on it.
            Some(handle) => {
                state.retune(handle, &asked, &buses);
                false
            }
            None => false,
        };
        let started = state
            .nodes
            .get(&entity)
            .is_some_and(|voice| voice.handle.is_some());
        let has_file = !asked.file.trim().is_empty();
        (asked.autoplay && has_file && !started, crossfade)
    };
    if start && let Err(why) = play_node(eng, entity, crossfade) {
        tracing::warn!("sound autoplay: {why:#}");
    }
    publish_playback_times(eng);
    Ok(())
}
