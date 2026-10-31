//! Audio as a Balaur plugin, backed by rodio.
//!
//! The `sound` component gives a node a configured sound: `autoplay` starts
//! it on load, `audio.play(node)` restarts it and `audio.stop(node)` silences
//! it. `audio.play` and `audio.play_cue(name)` hand back an integer handle;
//! `stop_playback`, `set_volume_linear`, `set_pitch_scale` and `is_playing`
//! address it. A sound with a place in the world (a `sound` with `positional`
//! set, or a cue played with a `position`) is heard from the `listener` node:
//! see [`spatial`].
//!
//! Audio is a pure observer of the simulation. If no output device is
//! available (CI, headless servers) the plugin logs a warning once and every
//! call still hands out the same handles: a game runs identically with and
//! without a sound card. Anything that feeds a decision (`is_playing`, the
//! `sound` component's "already started" check, `playback_time`) is therefore
//! tracked as intent on [`Voice`] and [`AudioState`], never read off a sink.
//!
//! The device is opened by the first call that needs one, not at load: the
//! open asks the OS for its default output config, and on macOS that reads
//! the directory the executable sits in. A browser defers it further, to the
//! first gesture (`UserActivation`), because it refuses audio before one.

use anyhow::{Context, Result, anyhow, bail};
use balaur_core::glamx::Vec3;
use balaur_core::hecs::Entity;
use balaur_core::{DetHashMap, Engine, Stage, scene};

mod backend;
pub mod bus;
pub mod cache;
mod component;
pub mod cue;
pub mod output;
pub mod playback;
mod script_api;
pub mod spatial;
pub mod vocabulary;

use crate::vocabulary::keys as k;
use backend::From;
use bus::Buses;
pub use component::Sound;
use output::OutputSettings;
use playback::{Clip, Program, Timeline};
use spatial::{Emitter, Listener, ListenerPose, Placement};

/// The floor `pitch_scale` is clamped to, matching the schema's `min`: rodio takes
/// a playback speed, and zero would park the sink forever.
const MIN_PITCH: f32 = 0.01;

/// What the system keeps for a node's `sound` beyond what the node asks,
/// the shape `Playback` established in `balaur_animation`: the sink is shared
/// machinery, and this is the node's hold on it.
#[derive(Clone, Debug, Default)]
pub struct Voice {
    /// The playback this node started, `None` until autoplay or `audio.play`
    /// starts one and again after `audio.stop`. A finished sink does not clear
    /// it: intent must read the same headless as with a device.
    pub handle: Option<u64>,
    /// The `file` the node named at its last write, so the next one can tell
    /// a new file, which drops the playback, from a tweak, which lands on it.
    pub file: String,
}

/// The radius a positional sound is at full volume inside, and the one it is
/// cut at. Metres, for a game whose unit is a metre; the pair is what sets a
/// sound's carry, so both are per-sound.
const DEFAULT_MIN_DISTANCE: f32 = 1.0;
const DEFAULT_MAX_DISTANCE: f32 = 50.0;

/// A time property as a play reads it: below zero, or not a finite number,
/// is none.
fn seconds(asked: f32) -> f32 {
    if asked.is_finite() {
        asked.max(0.0)
    } else {
        0.0
    }
}

pub struct AudioState {
    device: Option<backend::Device>,
    /// Whether the open has been tried. Nothing tries until a sound plays or
    /// `ready` is read, so a run that makes no sound never opens a device.
    opened: bool,
    /// The open the first touch of audio started off the frame, answered
    /// when a sound first needs the device: the platform's open is tens of
    /// milliseconds, too long for the frame a first sound plays in.
    opening: Option<std::sync::mpsc::Receiver<Result<backend::Device>>>,
    /// True while the device waits for `UserActivation`: a browser refuses
    /// to start audio before a gesture, so the open is deferred to one.
    awaiting_activation: bool,
    /// What `[audio]` asks of the device and the buses' limiters, read with
    /// the bus table and handed to the device when it opens.
    configured: bool,
    output: OutputSettings,
    routes: Vec<bus::Route>,
    /// Live sinks by handle. A handle absent here answers `is_playing` false
    /// and its setters no-op.
    playing: DetHashMap<u64, backend::Sound>,
    /// What each live handle was played at and through, so moving a bus's
    /// slider can re-apply the gain to what is already sounding. Without it a
    /// volume change would only reach sounds started after it.
    routing: DetHashMap<u64, Routed>,
    /// Stopped handles still falling to silence over their `fade_out_time`.
    releasing: DetHashMap<u64, Release>,
    /// Every `sound` node's voice, keyed the way `AnimationState` keys its
    /// players.
    pub nodes: DetHashMap<Entity, Voice>,
    /// Where each positional handle plays from. A handle absent here is
    /// flat: no attenuation, no pan, no doppler.
    spatial: DetHashMap<u64, Emitter>,
    /// Every node's `listener` component. Insertion-ordered, so "the last
    /// current one" is the same node on every run.
    listeners: DetHashMap<Entity, Listener>,
    /// Where the ears are, as of the last frame.
    listener: ListenerPose,
    /// Counts up from 1 and never reuses, so a held handle names nothing
    /// rather than something else once its sound is gone.
    next_handle: u64,
}

/// Where a live handle plays and where it has got to. The gains are
/// bookkeeping rather than a sink reading, so a headless run can assert the
/// mix.
pub(crate) struct Routed {
    bus: String,
    /// The gain its caller asked for.
    volume: f32,
    /// Its bus chain's gain times its distance gain.
    pub(crate) chain: f32,
    /// 1 until the last `fade_out_time` seconds before it plays out.
    fade: f32,
    /// What the sink was last given: `volume * chain * fade`.
    pub(crate) applied: f32,
    pitch: f32,
    paused: bool,
    fade_out_time: f32,
    positional: bool,
    program: Program,
    /// Counted on the fixed step, so a headless run and a replay end it,
    /// move between its files and report `playback_time` on the same tick.
    timeline: Timeline,
}

impl Routed {
    pub(crate) fn bus(&self) -> &str {
        &self.bus
    }

    /// Recompute the sink's gain from its parts and hand it back.
    pub(crate) fn mix(&mut self) -> f32 {
        self.applied = (self.volume * self.chain * self.fade).max(0.0);
        self.applied
    }

    /// The end fade's gain where the timeline is now.
    fn end_fade(&self) -> f32 {
        if self.fade_out_time <= 0.0 {
            return 1.0;
        }
        match self.timeline.left() {
            Some(left) if left < f64::from(self.fade_out_time) => {
                (left / f64::from(self.fade_out_time)) as f32
            }
            _ => 1.0,
        }
    }

    fn gains(&self, placed: Option<&Placement>) -> [f32; 2] {
        match placed {
            Some(placed) => spatial::stereo_gains(placed.pan),
            None => spatial::balance_gains(self.program.pan),
        }
    }
}

/// A stopped handle falling to silence from the gain it was stopped at.
struct Release {
    sink: Option<backend::Sound>,
    from: f32,
    left: f64,
    total: f64,
}

impl Release {
    fn gain(&self) -> f32 {
        (f64::from(self.from) * (self.left / self.total).clamp(0.0, 1.0)) as f32
    }
}

/// What a node announces when its sound plays out, with the handle.
pub const FINISHED_EVENT: &str = "finished";

/// One `play`: how loud and fast, looping or not, on which bus at what chain
/// gain, and, for a positional sound, where it plays from; then where it
/// starts and stops, its fades and pan, and what else plays with it.
pub struct Playback {
    pub volume: f32,
    pub pitch: f32,
    pub looped: bool,
    pub bus: String,
    /// The bus chain's gain, resolved by the caller.
    pub gain: f32,
    pub emitter: Option<Emitter>,
    /// What the file's own import settings add to every play of it.
    pub file: FileSettings,
    pub paused: bool,
    pub start_time: f32,
    pub end_time: f32,
    pub delay: f32,
    pub fade_in_time: f32,
    pub fade_out_time: f32,
    /// Where repeats start; `None` takes the file's own.
    pub loop_offset: Option<f32>,
    /// Balance when it is not positional: -1 left, 1 right.
    pub pan: f32,
    pub stems: Vec<Clip>,
    pub queue: Vec<Clip>,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            looped: false,
            bus: String::new(),
            gain: 1.0,
            emitter: None,
            file: FileSettings::default(),
            paused: false,
            start_time: 0.0,
            end_time: 0.0,
            delay: 0.0,
            fade_in_time: 0.0,
            fade_out_time: 0.0,
            loop_offset: None,
            pan: 0.0,
            stems: Vec::new(),
            queue: Vec::new(),
        }
    }
}

/// A sound file's import settings: its own `volume_linear`, whether it loops
/// wherever it is played, where each repeat starts, and how its decoder reads
/// it. Its level is baked into the samples, so a handle's volume of 1 is still
/// the file's own level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileSettings {
    pub level: f32,
    pub looped: bool,
    pub loop_offset: f32,
    /// Trim the encoder's padding, so files queued back to back have no gap.
    pub gapless: bool,
    /// Let the decoder jump in the container: a seek or a `start_time` lands
    /// without decoding up to it.
    pub seekable: bool,
}

impl Default for FileSettings {
    fn default() -> Self {
        Self {
            level: 1.0,
            looped: false,
            loop_offset: 0.0,
            gapless: true,
            seekable: false,
        }
    }
}

impl FileSettings {
    /// The settings `path` is read with: its sidecar over `[import.audio]`.
    #[must_use]
    pub fn of(eng: &Engine, path: &str) -> Self {
        use balaur_core::import::{flag, keys, number};
        let resolved = balaur_core::import::resolved(eng, path);
        let settings = &resolved.settings;
        Self {
            level: number(settings, keys::VOLUME_LINEAR, 1.0).max(0.0) as f32,
            looped: flag(settings, keys::LOOP, false),
            loop_offset: number(settings, keys::LOOP_OFFSET, 0.0).max(0.0) as f32,
            gapless: flag(settings, k::GAPLESS, true),
            seekable: flag(settings, k::SEEKABLE, false),
        }
    }
}

impl AudioState {
    /// Stop everything currently playing and clear every node's handle.
    pub fn stop_all(&mut self) {
        for (_, sink) in self.playing.drain(..) {
            sink.stop();
        }
        for (_, release) in self.releasing.drain(..) {
            if let Some(sink) = release.sink {
                sink.stop();
            }
        }
        self.routing.clear();
        self.spatial.clear();
        for voice in self.nodes.values_mut() {
            voice.handle = None;
        }
    }

    /// Open the output device, once, unless a browser is still waiting for
    /// its gesture. Every path that needs a device calls this first.
    fn open_if_needed(&mut self) {
        if self.opened || self.awaiting_activation {
            return;
        }
        self.opened = true;
        self.device = match self.opening.take().and_then(|asked| asked.recv().ok()) {
            Some(Ok(device)) => Some(device),
            Some(Err(err)) => {
                tracing::warn!("audio disabled: {err}");
                None
            }
            None => open_device(&self.output, &self.routes),
        };
    }

    /// Start opening the device where it cannot hold up a frame, so the
    /// first sound finds it open. Not in a browser, which opens on its
    /// gesture.
    fn begin_open(&mut self) {
        if self.opened || self.awaiting_activation || self.opening.is_some() {
            return;
        }
        let (output, routes) = (self.output.clone(), self.routes.clone());
        self.opening = Some(balaur_core::task::compute(move || {
            backend::Device::open(&output, &routes)
        }));
    }

    /// Start a sound from its bytes: the `audio.*` bindings read paths
    /// through the pack-aware project reader, and hand back its handle.
    /// Never errors: no output device and bytes that will not decode both
    /// leave the handle silent, so a headless run behaves like a windowed
    /// one.
    pub fn play(&mut self, bytes: Vec<u8>, volume: f32, pitch: f32, looped: bool) -> u64 {
        self.play_on_bus(bytes, volume, pitch, looped, "", 1.0)
    }

    /// The same, routed through a bus: `gain` is the bus chain's, and the
    /// sound is started at `volume * gain`.
    ///
    /// The bus and the sound's own volume are both remembered, because a
    /// slider moved later has to be able to recompute one from the other.
    pub fn play_on_bus(
        &mut self,
        bytes: impl Into<playback::SoundBytes>,
        volume: f32,
        pitch: f32,
        looped: bool,
        bus: &str,
        gain: f32,
    ) -> u64 {
        self.play_with(
            bytes,
            Playback {
                volume,
                pitch,
                looped,
                bus: bus.to_string(),
                gain,
                ..Playback::default()
            },
        )
    }

    /// Start a whole playback, positional or not, and hand back its handle.
    ///
    /// A playback carrying an emitter is placed here rather than waiting for the
    /// next frame's pass: a sound the far side of the level must not be heard
    /// at full volume for the frame before it is placed.
    pub fn play_with(&mut self, bytes: impl Into<playback::SoundBytes>, playback: Playback) -> u64 {
        let handle = self.next_handle;
        self.next_handle += 1;
        let volume = playback.volume.max(0.0);
        let pitch = playback.pitch.max(MIN_PITCH);
        let placement = playback.emitter.map(|mut emitter| {
            emitter.pitch = pitch;
            emitter.placement = spatial::place(&self.listener, &emitter);
            let placement = emitter.placement;
            self.spatial.insert(handle, emitter);
            placement
        });
        let placed = placement.unwrap_or_default();
        let program = Program {
            main: Clip::new(bytes, playback.file),
            stems: playback.stems,
            queue: playback.queue,
            looped: playback.looped,
            loop_offset: playback.loop_offset,
            start_time: playback.start_time.max(0.0),
            end_time: playback.end_time.max(0.0),
            delay: playback.delay.max(0.0),
            fade_in_time: playback.fade_in_time.max(0.0),
            pan: playback.pan.clamp(-1.0, 1.0),
        };
        let timeline = Timeline::new(&program, &lengths(&program));
        let mut routed = Routed {
            bus: playback.bus,
            volume,
            chain: playback.gain * placed.gain,
            fade: 1.0,
            applied: 0.0,
            pitch,
            paused: playback.paused,
            fade_out_time: playback.fade_out_time.max(0.0),
            positional: placement.is_some(),
            program,
            timeline,
        };
        routed.fade = routed.end_fade();
        routed.mix();
        self.open_if_needed();
        if let Some(device) = &self.device {
            let controls = backend::Controls {
                bus: &routed.bus,
                volume: routed.applied,
                speed: (pitch * placed.pitch).max(MIN_PITCH),
                paused: routed.paused,
                positional: routed.positional,
                gains: routed.gains(placement.as_ref()),
            };
            let from = From {
                index: 0,
                at: routed.timeline.at,
                fresh: true,
            };
            match backend::Sound::start(device, &routed.program, from, controls) {
                Ok(sound) => {
                    self.playing.insert(handle, sound);
                }
                Err(err) => tracing::warn!("audio did not decode: {err}"),
            }
        }
        self.routing.insert(handle, routed);
        handle
    }

    /// Re-apply the mix to every live sound `moved` carries: the ones on it
    /// and the ones on any bus under it. What moving a slider does to what is
    /// already playing.
    ///
    /// Positional handles are left to the next frame's placement pass, which
    /// reads the same bus gain and would otherwise overwrite this.
    pub fn reroute(&mut self, buses: &Buses, moved: &str) {
        for (handle, routed) in &mut self.routing {
            if self.spatial.contains_key(handle) || !buses.feeds(&routed.bus, moved) {
                continue;
            }
            routed.chain = buses.gain(&routed.bus);
            let applied = routed.mix();
            if let Some(sink) = self.playing.get(handle) {
                sink.set_volume(applied);
            }
        }
    }

    /// The bus a live handle plays on, and the volume it was started at.
    #[must_use]
    pub fn routing_of(&self, handle: u64) -> Option<(String, f32)> {
        self.routing
            .get(&handle)
            .map(|routed| (routed.bus.clone(), routed.volume))
    }

    /// The gain a live handle's sink is at: its own volume through its bus
    /// chain, times its distance gain when it is placed and its end fade.
    #[must_use]
    pub fn effective_volume(&self, handle: u64) -> Option<f32> {
        self.routing.get(&handle).map(|routed| routed.applied)
    }

    /// The gain a stopped handle is fading out at, `None` once it is silent
    /// or when it stopped with no fade.
    #[must_use]
    pub fn releasing_volume(&self, handle: u64) -> Option<f32> {
        self.releasing.get(&handle).map(Release::gain)
    }

    /// Stop one handle's sound at once, a fading one included. A finished,
    /// stopped or unknown handle no-ops.
    pub fn stop(&mut self, handle: u64) {
        self.routing.shift_remove(&handle);
        self.spatial.shift_remove(&handle);
        if let Some(sink) = self.playing.shift_remove(&handle) {
            sink.stop();
        }
        if let Some(sink) = self.releasing.shift_remove(&handle).and_then(|r| r.sink) {
            sink.stop();
        }
    }

    /// Stop a handle over its own `fade_out_time`.
    pub fn release(&mut self, handle: u64) {
        let seconds = self
            .routing
            .get(&handle)
            .map_or(0.0, |routed| routed.fade_out_time);
        self.release_over(handle, seconds);
    }

    /// Stop a handle, falling to silence over `seconds` from where it is.
    /// It answers `is_playing` false at once: what fades is no longer the
    /// game's sound.
    pub fn release_over(&mut self, handle: u64, seconds: f32) {
        let Some(routed) = self.routing.shift_remove(&handle) else {
            return;
        };
        self.spatial.shift_remove(&handle);
        let sink = self.playing.shift_remove(&handle);
        if seconds <= 0.0 {
            if let Some(sink) = sink {
                sink.stop();
            }
            return;
        }
        let total = f64::from(seconds);
        self.releasing.insert(
            handle,
            Release {
                sink,
                from: routed.applied,
                left: total,
                total,
            },
        );
    }

    /// Set a live handle's own volume, before its bus and its distance. The
    /// stored one moves with it, so a bus slider and the next placement pass
    /// both recompute from what was asked for last.
    pub fn set_volume(&mut self, handle: u64, volume: f32, buses: &Buses) {
        let placed = self
            .spatial
            .get(&handle)
            .map_or(1.0, |emitter| emitter.placement.gain);
        let Some(routed) = self.routing.get_mut(&handle) else {
            return;
        };
        routed.volume = volume.max(0.0);
        routed.chain = buses.gain(&routed.bus) * placed;
        let applied = routed.mix();
        if let Some(sink) = self.playing.get(&handle) {
            sink.set_volume(applied);
        }
    }

    pub fn set_pitch(&mut self, handle: u64, pitch: f32) {
        let pitch = pitch.max(MIN_PITCH);
        if let Some(routed) = self.routing.get_mut(&handle) {
            routed.pitch = pitch;
        }
        if let Some(emitter) = self.spatial.get_mut(&handle) {
            emitter.pitch = pitch;
        }
        if let Some(sink) = self.playing.get(&handle) {
            sink.set_pitch(pitch);
        }
    }

    /// Hold a handle or let it go on. Its timeline holds with it.
    pub fn set_paused(&mut self, handle: u64, paused: bool) {
        if let Some(routed) = self.routing.get_mut(&handle) {
            routed.paused = paused;
        }
        if let Some(sink) = self.playing.get(&handle) {
            sink.set_paused(paused);
        }
    }

    /// Whether a live handle is held.
    #[must_use]
    pub fn is_paused(&self, handle: u64) -> bool {
        self.routing
            .get(&handle)
            .is_some_and(|routed| routed.paused)
    }

    /// Set a live handle's balance. A flat sound takes the pan; a positional
    /// one keeps the pan its placement gives it.
    pub fn set_pan(&mut self, handle: u64, pan: f32) {
        let Some(routed) = self.routing.get_mut(&handle) else {
            return;
        };
        routed.program.pan = pan.clamp(-1.0, 1.0);
        if let Some(sink) = self.playing.get(&handle)
            && !routed.positional
        {
            sink.set_pan(routed.gains(None));
        }
    }

    /// The `pan` a live handle was given, positional or not.
    #[must_use]
    pub fn pan_of(&self, handle: u64) -> Option<f32> {
        self.routing.get(&handle).map(|routed| routed.program.pan)
    }

    /// Where a handle has got to in the file it is playing, in seconds of
    /// that file.
    #[must_use]
    pub fn playback_time(&self, handle: u64) -> Option<f64> {
        self.routing.get(&handle).map(|routed| routed.timeline.at)
    }

    /// Which of a handle's files is playing: 0 for its own, then its queue.
    #[must_use]
    pub fn playing_index(&self, handle: u64) -> Option<usize> {
        self.routing
            .get(&handle)
            .map(|routed| routed.timeline.index)
    }

    /// Jump a live handle to `seconds` into the file it is playing. A seek is
    /// a fresh start from there, so it needs no decoder that seeks.
    pub fn seek(&mut self, handle: u64, seconds: f64) {
        if let Some(routed) = self.routing.get_mut(&handle) {
            routed.timeline.seek(seconds);
        }
        self.restart(handle);
    }

    /// Move a live handle on to the next file in its queue. True when there
    /// was none, and the handle has stopped.
    pub fn skip(&mut self, handle: u64) -> bool {
        let Some(routed) = self.routing.get_mut(&handle) else {
            return false;
        };
        if routed.timeline.next_file() {
            self.restart(handle);
            return false;
        }
        self.stop(handle);
        true
    }

    /// Build a handle's sink again from where its timeline is.
    fn restart(&mut self, handle: u64) {
        let placed = self.spatial.get(&handle).map(|emitter| emitter.placement);
        let Some(routed) = self.routing.get_mut(&handle) else {
            return;
        };
        routed.fade = routed.end_fade();
        routed.mix();
        let (Some(device), Some(sink)) = (&self.device, self.playing.get(&handle)) else {
            return;
        };
        let controls = backend::Controls {
            bus: &routed.bus,
            volume: routed.applied,
            speed: (routed.pitch * placed.map_or(1.0, |p| p.pitch)).max(MIN_PITCH),
            paused: routed.paused,
            positional: routed.positional,
            gains: routed.gains(placed.as_ref()),
        };
        let from = From {
            index: routed.timeline.index,
            at: routed.timeline.at,
            fresh: false,
        };
        match sink.restart(device, &routed.program, from, controls) {
            Ok(sound) => {
                self.playing.insert(handle, sound);
            }
            Err(err) => {
                tracing::warn!("audio did not decode: {err}");
                self.playing.shift_remove(&handle);
            }
        }
    }

    /// Land everything a node's `sound` can change while it plays: volume,
    /// pitch, pause, pan, its end fade and how far it carries.
    pub(crate) fn retune(&mut self, handle: u64, sound: &Sound, buses: &Buses) {
        self.set_volume(handle, sound.volume, buses);
        self.set_pitch(handle, sound.pitch);
        if self.is_paused(handle) != sound.paused {
            self.set_paused(handle, sound.paused);
        }
        let pan = sound.pan.clamp(-1.0, 1.0);
        if self.pan_of(handle) != Some(pan) {
            self.set_pan(handle, pan);
        }
        if let Some(routed) = self.routing.get_mut(&handle) {
            routed.fade_out_time = seconds(sound.fade_out_time);
        }
        if let Some(emitter) = self.spatial.get_mut(&handle) {
            let shaped = Emitter::new(
                emitter.position,
                sound.min_distance,
                sound.max_distance,
                sound.doppler,
            );
            emitter.min_distance = shaped.min_distance;
            emitter.max_distance = shaped.max_distance;
            emitter.doppler = shaped.doppler;
            emitter.attenuation = sound.attenuation;
        }
    }

    /// Where a positional handle plays from, and `None` for a flat one.
    #[must_use]
    pub fn emitter_position(&self, handle: u64) -> Option<Vec3> {
        self.spatial.get(&handle).map(|emitter| emitter.position)
    }

    /// The emitter behind a positional handle.
    #[must_use]
    pub fn emitter(&self, handle: u64) -> Option<&Emitter> {
        self.spatial.get(&handle)
    }

    /// Move a positional handle's emitter. The frame's pass takes its
    /// velocity from how far it moved, so doppler follows a script that
    /// drives a sound around as it does a node that carries one.
    pub fn set_emitter_position(&mut self, handle: u64, position: Vec3) {
        if let Some(emitter) = self.spatial.get_mut(&handle) {
            emitter.position = position;
        }
    }

    /// What the last frame decided about a positional handle: its distance
    /// gain, its pan and its doppler. `None` for a flat or unknown handle.
    #[must_use]
    pub fn placement_of(&self, handle: u64) -> Option<Placement> {
        self.spatial.get(&handle).map(|emitter| emitter.placement)
    }

    /// Where the ears are, and how fast they are moving.
    #[must_use]
    pub const fn listener(&self) -> &ListenerPose {
        &self.listener
    }

    /// Put the ears somewhere by hand, for a game whose camera is not a node.
    /// A `listener` node in the scene overrides this on the next frame.
    pub fn set_listener(&mut self, position: Vec3) {
        self.listener.place(position);
    }

    /// Whether a handle's sound is still going: started and not yet stopped,
    /// swept or finished. Read off the routing rather than the sink, so a
    /// machine with no output device answers what one with a card answers.
    #[must_use]
    pub fn is_playing(&self, handle: u64) -> bool {
        self.routing.contains_key(&handle)
    }
}

/// Each file's length in a program: `main` mixed with its stems runs as
/// long as the longest of them, unknown if any one is.
fn lengths(program: &Program) -> Vec<Option<f64>> {
    let main = std::iter::once(&program.main)
        .chain(&program.stems)
        .map(backend::length_of)
        .try_fold(0.0_f64, |longest, length| length.map(|l| longest.max(l)));
    std::iter::once(main)
        .chain(program.queue.iter().map(backend::length_of))
        .collect()
}

/// Every output device's name, as `[audio] device` takes it. Empty when the
/// platform lists none.
#[must_use]
pub fn devices() -> Vec<String> {
    backend::devices()
}

/// The bytes a sound path names, cached between plays so a footstep does not
/// cost a read per step.
fn read_sound(eng: &Engine, path: &str) -> Result<playback::SoundBytes> {
    cache::read(eng, path)
}

/// Read `[audio]` and the bus table once, before anything opens a device.
pub(crate) fn ensure_loaded(eng: &Engine) {
    bus::ensure_loaded(eng);
    let state = eng.resource::<AudioState>();
    if state.borrow().configured {
        return;
    }
    let routes = eng.resource::<Buses>().borrow().routes();
    let output = OutputSettings::of(eng);
    let mut state = state.borrow_mut();
    state.configured = true;
    state.output = output;
    state.routes = routes;
    state.begin_open();
}

/// Start `entity`'s configured sound and hand back the handle. An explicit
/// trigger: a sound the node already has playing restarts.
///
/// # Errors
/// If the node has no `sound` component, its `file` is empty, or a file it
/// names does not exist.
pub fn play_on(eng: &Engine, entity: Entity) -> Result<u64> {
    play_node(eng, entity, false)
}

/// [`play_on`], fading in over the node's `crossfade_time` rather than its
/// `fade_in_time` when `crossfade` is set: the new half of a changed `file`.
pub(crate) fn play_node(eng: &Engine, entity: Entity, crossfade: bool) -> Result<u64> {
    ensure_loaded(eng);
    let sound = eng
        .world()
        .get::<&Sound>(entity)
        .ok()
        .map(|held| (*held).clone())
        .ok_or_else(|| anyhow!("this node has no `sound` component to play"))?;
    if sound.file.trim().is_empty() {
        bail!("the node's `sound` component names no `file`");
    }
    let clip = |path: &String| -> Result<Clip> {
        let bytes = read_sound(eng, path).with_context(|| format!("sound file `{path}`"))?;
        Ok(Clip::new(bytes, FileSettings::of(eng, path)))
    };
    let stems = sound.stems.iter().map(clip).collect::<Result<Vec<_>>>()?;
    let queue = sound.queue.iter().map(clip).collect::<Result<Vec<_>>>()?;
    let bytes = read_sound(eng, &sound.file)?;
    let emitter = sound.positional.then(|| {
        let mut emitter = Emitter::new(
            // Composed here rather than read off `GlobalTransform`: a node
            // that entered the scene this frame has not been through a scene
            // sync, and a sound must not start from the origin and jump.
            scene::composed_global(&eng.world(), entity).position,
            sound.min_distance,
            sound.max_distance,
            sound.doppler,
        );
        emitter.attenuation = sound.attenuation;
        emitter
    });
    let gain = eng.resource::<Buses>().borrow().gain(&sound.bus);
    let playback = Playback {
        volume: sound.volume,
        pitch: sound.pitch,
        looped: sound.looped,
        bus: sound.bus.clone(),
        gain,
        emitter,
        file: FileSettings::of(eng, &sound.file),
        paused: sound.paused,
        start_time: seconds(sound.start_time),
        end_time: seconds(sound.end_time),
        delay: seconds(sound.delay),
        fade_in_time: seconds(if crossfade {
            sound.crossfade_time
        } else {
            sound.fade_in_time
        }),
        fade_out_time: seconds(sound.fade_out_time),
        loop_offset: (sound.loop_offset >= 0.0).then_some(sound.loop_offset),
        pan: sound.pan.clamp(-1.0, 1.0),
        stems,
        queue,
    };
    let handle = {
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        let voice = state.nodes.entry(entity).or_default();
        let current = voice.handle.take();
        if let Some(current) = current {
            state.stop(current);
        }
        let handle = state.play_with(bytes, playback);
        if let Some(voice) = state.nodes.get_mut(&entity) {
            voice.handle = Some(handle);
        }
        handle
    };
    publish_playback_times(eng);
    Ok(handle)
}

/// Silence `entity`'s sound, over its `fade_out_time`. A node without one is
/// left alone.
pub fn stop_on(eng: &Engine, entity: Entity) {
    let Some(state) = eng.try_resource::<AudioState>() else {
        return;
    };
    {
        let mut state = state.borrow_mut();
        let stopped = state
            .nodes
            .get_mut(&entity)
            .and_then(|voice| voice.handle.take());
        if let Some(handle) = stopped {
            state.release(handle);
        }
    }
    publish_playback_times(eng);
}

/// Jump `entity`'s sound to `seconds` into the file it is playing. A node
/// with nothing playing is left alone.
pub fn seek_on(eng: &Engine, entity: Entity, seconds: f64) {
    {
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        if let Some(handle) = state.nodes.get(&entity).and_then(|voice| voice.handle) {
            state.seek(handle, seconds);
        }
    }
    publish_playback_times(eng);
}

/// Move `entity`'s sound on to the next file in its queue. Past the last one
/// it has played out, and the node announces `finished`.
pub fn skip_on(eng: &Engine, entity: Entity) {
    let ended = {
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        state
            .nodes
            .get(&entity)
            .and_then(|voice| voice.handle)
            .filter(|handle| state.skip(*handle))
    };
    if let Some(handle) = ended {
        announce_finished(eng, &[handle]);
    }
    publish_playback_times(eng);
}

pub struct AudioPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for AudioPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("audio", env!("CARGO_PKG_VERSION")),
        }
    }
}

fn open_device(output: &OutputSettings, routes: &[bus::Route]) -> Option<backend::Device> {
    match backend::Device::open(output, routes) {
        Ok(device) => Some(device),
        Err(err) => {
            tracing::warn!("audio disabled: {err}");
            None
        }
    }
}

/// Open the device the first tick after the page has seen a gesture.
fn open_on_activation_system(eng: &Engine, _: f32) {
    let waiting = eng.resource::<AudioState>().borrow().awaiting_activation;
    if !waiting || eng.try_resource::<balaur_core::UserActivation>().is_none() {
        return;
    }
    ensure_loaded(eng);
    let state = eng.resource::<AudioState>();
    let mut state = state.borrow_mut();
    state.awaiting_activation = false;
    state.opened = true;
    state.device = open_device(&state.output, &state.routes);
}

/// Drop finished sinks, and stop the sounds of nodes that were freed.
fn sweep_sounds_system(eng: &Engine, _: f32) {
    let state = eng.resource::<AudioState>();
    let mut state = state.borrow_mut();
    let world = eng.world();
    spatial::sweep_listeners(&mut state, &world);
    let AudioState {
        nodes,
        playing,
        routing,
        spatial,
        releasing,
        ..
    } = &mut *state;
    // A voice lives here, not on the entity, so this is where a freed node's
    // playback stops.
    nodes.retain(|&entity, voice| {
        if world.contains(entity) {
            return true;
        }
        if let Some(handle) = voice.handle {
            spatial.shift_remove(&handle);
            routing.shift_remove(&handle);
            if let Some(sink) = playing.shift_remove(&handle) {
                sink.stop();
            }
        }
        false
    });
    // A sink that has played out is dropped. A sound whose length is known
    // ends on the fixed step's count instead, which a machine with no device
    // reaches on the same tick; only one of unknown length ends here.
    let mut ended = Vec::new();
    playing.retain(|handle, sink| {
        if !sink.finished() {
            return true;
        }
        if routing
            .get(handle)
            .is_some_and(|routed| !routed.timeline.counted())
        {
            spatial.shift_remove(handle);
            routing.shift_remove(handle);
            ended.push(*handle);
        }
        false
    });
    releasing.retain(|_, release| release.sink.as_ref().is_none_or(|sink| !sink.finished()));
    drop(world);
    drop(state);
    if !ended.is_empty() {
        announce_finished(eng, &ended);
        publish_playback_times(eng);
    }
}

/// Count every timed sound on by a fixed step, end the ones that ran out, and
/// move every fade along.
fn count_down_system(eng: &Engine, dt: f32) {
    let ended: Vec<u64> = {
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        let AudioState {
            routing,
            playing,
            releasing,
            ..
        } = &mut *state;
        let mut ended = Vec::new();
        for (handle, routed) in routing.iter_mut() {
            if routed.paused {
                continue;
            }
            if routed
                .timeline
                .advance(f64::from(dt) * f64::from(routed.pitch))
            {
                ended.push(*handle);
                continue;
            }
            let fade = routed.end_fade();
            if (fade - routed.fade).abs() > f32::EPSILON {
                routed.fade = fade;
                let applied = routed.mix();
                if let Some(sink) = playing.get(handle) {
                    sink.set_volume(applied);
                }
            }
        }
        releasing.retain(|_, release| {
            release.left -= f64::from(dt);
            let Some(sink) = &release.sink else {
                return release.left > 0.0;
            };
            if release.left > 0.0 {
                sink.set_volume(release.gain());
                return true;
            }
            sink.stop();
            false
        });
        for handle in &ended {
            state.stop(*handle);
        }
        ended
    };
    announce_finished(eng, &ended);
    publish_playback_times(eng);
}

/// Every `sound` node's `playback_time`, from its voice's clock: engine
/// output the component holds, so a read, a save and a snapshot all see it.
/// Written wherever the clock moves -- the fixed step, a play, a stop, a seek
/// and a skip -- and 0 on a node with nothing playing.
pub(crate) fn publish_playback_times(eng: &Engine) {
    let Some(state) = eng.try_resource::<AudioState>() else {
        return;
    };
    let state = state.borrow();
    let world = eng.world();
    for (entity, voice) in &state.nodes {
        if let Ok(mut sound) = world.get::<&mut Sound>(*entity) {
            sound.playback_time = voice
                .handle
                .and_then(|handle| state.playback_time(handle))
                .unwrap_or_default();
        }
    }
}

/// Tell the node whose `sound` held each handle that it played out.
fn announce_finished(eng: &Engine, ended: &[u64]) {
    if ended.is_empty() {
        return;
    }
    let owners: Vec<(Entity, u64)> = {
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        state
            .nodes
            .iter_mut()
            .filter_map(|(entity, voice)| {
                let handle = voice.handle.filter(|h| ended.contains(h))?;
                voice.handle = None;
                Some((*entity, handle))
            })
            .collect()
    };
    for (entity, handle) in owners {
        balaur_core::events::announce(
            eng,
            entity,
            FINISHED_EVENT,
            balaur_script::Value::Int(i64::try_from(handle).unwrap_or(i64::MAX)),
        );
    }
}

impl balaur_plugin::Plugin for AudioPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        output::declare_settings(reg.engine());
        reg.insert_resource(AudioState {
            device: None,
            opened: false,
            opening: None,
            awaiting_activation: cfg!(target_family = "wasm"),
            configured: false,
            output: OutputSettings::default(),
            routes: Vec::new(),
            playing: DetHashMap::default(),
            routing: DetHashMap::default(),
            releasing: DetHashMap::default(),
            nodes: DetHashMap::default(),
            spatial: DetHashMap::default(),
            listeners: DetHashMap::default(),
            listener: ListenerPose::default(),
            next_handle: 1,
        });
        reg.insert_resource(bus::Buses::default());
        reg.insert_resource(cue::Cues::default());
        reg.insert_resource(cache::SoundCache::default());

        reg.add_system(Stage::First, open_on_activation_system);
        reg.add_system(Stage::PostUpdate, sweep_sounds_system);
        reg.add_system(Stage::FixedUpdate, count_down_system);
        reg.add_system(Stage::SceneSync, spatial::spatialize_system);
        component::register_sound_component(reg);
        spatial::register_listener_component(reg);

        let mut m = reg.script_module("audio")?;
        script_api::install_audio_api(&mut m);
        Ok(())
    }
}
