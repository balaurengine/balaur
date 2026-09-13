//! Audio as a Balaur plugin, backed by rodio.
//!
//! `audio.play` hands back an integer handle; `stop`, `set_volume`,
//! `set_pitch` and `is_playing` address it. The `sound` component gives a
//! node a configured sound, triggered by `audio.play_on` / `audio.stop_on`.
//! A sound with a place in the world (a `positional` component, or a `play`
//! given a `position`) is heard from the `listener` node: see [`spatial`].
//!
//! Audio is a pure observer of the simulation. If no output device is
//! available (CI, headless servers) the plugin logs a warning once and every
//! call still hands out the same handles: a game runs identically with and
//! without a sound card. Anything that feeds a decision (`is_playing`, the
//! `sound` component's "already started" check) is therefore tracked as
//! intent on [`Sound`] and [`AudioState`], never read off a sink.
//!
//! The device is opened by the first call that needs one, not at load: the
//! open asks the OS for its default output config, and on macOS that reads
//! the directory the executable sits in. A browser defers it further, to the
//! first gesture (`UserActivation`), because it refuses audio before one.

use anyhow::{Result, anyhow, bail};
use balaur_core::components::{ComponentDef, as_f64};
use balaur_core::glamx::Vec3;
use balaur_core::hecs::Entity;
use balaur_core::{DetHashMap, Engine, Stage, scene};

pub mod bus;
pub mod cache;
pub mod event;
mod script_api;
pub mod spatial;

/// The `sound` and `listener` components' keys, for their schemas and readers alike.
pub(crate) mod keys {
    pub(crate) const AUTOPLAY: &str = "autoplay";
    pub(crate) const BUS: &str = "bus";
    pub(crate) const CURRENT: &str = "current";
    pub(crate) const DOPPLER: &str = "doppler";
    pub(crate) const FILE: &str = "file";
    pub(crate) const LOOP: &str = "loop";
    pub(crate) const MAX_DISTANCE: &str = "max_distance";
    pub(crate) const MIN_DISTANCE: &str = "min_distance";
    pub(crate) const PITCH: &str = "pitch";
    pub(crate) const POSITIONAL: &str = "positional";
    pub(crate) const VOLUME: &str = "volume";
}

use crate::keys as k;
use bus::Buses;
use spatial::{Emitter, Listener, ListenerPose, Placement};

/// The rodio/cpal backend: native audio stacks, and WebAudio on wasm.
mod backend {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use anyhow::Result;
    use rodio::source::{ChannelVolume, Source};
    use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

    pub(crate) struct Device(MixerDeviceSink);

    pub(crate) struct Sound {
        player: Player,
        /// Set for a positional sound, whose left and right gains the mixer
        /// re-reads as it plays.
        pan: Option<Arc<Pan>>,
    }

    /// The stereo gains a live positional sound is at. Atomics rather than a
    /// lock: the mixer callback must never block on the frame that is
    /// writing them.
    struct Pan([AtomicU32; 2]);

    impl Pan {
        fn new(gains: [f32; 2]) -> Self {
            Self([
                AtomicU32::new(gains[0].to_bits()),
                AtomicU32::new(gains[1].to_bits()),
            ])
        }

        fn store(&self, gains: [f32; 2]) {
            self.0[0].store(gains[0].to_bits(), Ordering::Relaxed);
            self.0[1].store(gains[1].to_bits(), Ordering::Relaxed);
        }

        fn load(&self) -> [f32; 2] {
            [
                f32::from_bits(self.0[0].load(Ordering::Relaxed)),
                f32::from_bits(self.0[1].load(Ordering::Relaxed)),
            ]
        }
    }

    /// Mix a source down to mono and spread it across the two channels at
    /// gains the frame can move. Positioning a stereo file means giving up
    /// the channels it came with: a sound in one place has one direction.
    fn spread<S>(source: S, pan: &Arc<Pan>) -> impl Source + use<S>
    where
        S: Source,
    {
        let gains = pan.load();
        let pan = pan.clone();
        ChannelVolume::new(source, vec![gains[0], gains[1]]).periodic_access(
            Duration::from_millis(5),
            move |channels| {
                let gains = pan.load();
                channels.set_volume(0, gains[0]);
                channels.set_volume(1, gains[1]);
            },
        )
    }

    pub(crate) fn open_default() -> Result<Device> {
        #[cfg(windows)]
        keep_com_alive();
        Ok(Device(DeviceSinkBuilder::open_default_sink()?))
    }

    /// cpal caches its WASAPI device enumerator process-wide but initialises
    /// COM per thread, and uninitialises it when that thread exits. Once the
    /// last such thread is gone COM unloads the audio DLLs and the cached
    /// enumerator dangles: the next open crashes with an access violation.
    /// Holding an MTA reference for the life of the process keeps COM up.
    #[cfg(windows)]
    fn keep_com_alive() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let mut cookie = std::ptr::null_mut();
            // SAFETY: a plain FFI call taking a valid out-pointer; the cookie
            // is deliberately never returned to `CoDecrementMTAUsage`.
            let result =
                unsafe { windows_sys::Win32::System::Com::CoIncrementMTAUsage(&raw mut cookie) };
            if result < 0 {
                tracing::warn!("could not keep COM alive for audio: HRESULT {result:#x}");
            }
        });
    }

    /// Decode sound bytes and start them on the device's mixer. Bytes, not a
    /// path: a packed game carries its audio inside the pack. rodio has no
    /// loop toggle on a live player, so looping is requested at decode time.
    /// `pan` is the stereo placement a positional sound starts at.
    pub(crate) fn play(
        device: &Device,
        bytes: Vec<u8>,
        volume: f32,
        pitch: f32,
        looped: bool,
        pan: Option<[f32; 2]>,
    ) -> Result<Sound> {
        let decoder = rodio::Decoder::try_from(std::io::Cursor::new(bytes))?;
        let player = Player::connect_new(device.0.mixer());
        player.set_volume(volume);
        player.set_speed(pitch);
        let Some(gains) = pan else {
            if looped {
                player.append(Source::repeat_infinite(decoder));
            } else {
                player.append(decoder);
            }
            return Ok(Sound { player, pan: None });
        };
        let pan = Arc::new(Pan::new(gains));
        if looped {
            player.append(spread(Source::repeat_infinite(decoder), &pan));
        } else {
            player.append(spread(decoder, &pan));
        }
        Ok(Sound {
            player,
            pan: Some(pan),
        })
    }

    impl Sound {
        pub(crate) fn stop(&self) {
            self.player.stop();
        }

        pub(crate) fn set_volume(&self, volume: f32) {
            self.player.set_volume(volume);
        }

        pub(crate) fn set_pitch(&self, pitch: f32) {
            self.player.set_speed(pitch);
        }

        /// Move a positional sound between the speakers. A sound started
        /// without a pan keeps the channels it came with.
        pub(crate) fn set_pan(&self, gains: [f32; 2]) {
            if let Some(pan) = &self.pan {
                pan.store(gains);
            }
        }

        pub(crate) fn finished(&self) -> bool {
            self.player.empty()
        }
    }
}

/// The floor `pitch` is clamped to, matching the schema's `min`: rodio takes
/// a playback speed, and zero would park the sink forever.
const MIN_PITCH: f32 = 0.01;

/// One node's `sound` component, the shape `Playback` established in
/// `balaur_anim`: the sink is shared machinery, the intent lives here.
pub struct Sound {
    /// Audio file, project-relative. Empty plays nothing.
    pub file: String,
    pub autoplay: bool,
    pub volume: f32,
    pub pitch: f32,
    /// The scene key is `loop`, which Rust reserves.
    pub looped: bool,
    /// The bus this plays through; empty is `master`.
    pub bus: String,
    /// Whether this sound is heard from where its node is, rather than flat.
    pub positional: bool,
    /// Full volume within this distance of the listener.
    pub min_distance: f32,
    /// Silent beyond it.
    pub max_distance: f32,
    /// How much the closing speed bends the pitch: 0 is off, 1 physical.
    pub doppler: f32,
    /// The playback this node started, `None` until autoplay or `play_on`
    /// starts one and again after `stop_on`. A finished sink does not clear
    /// it: intent must read the same headless as with a device.
    pub handle: Option<u64>,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            file: String::new(),
            autoplay: false,
            volume: 1.0,
            pitch: 1.0,
            looped: false,
            bus: String::new(),
            positional: false,
            min_distance: DEFAULT_MIN_DISTANCE,
            max_distance: DEFAULT_MAX_DISTANCE,
            doppler: 0.0,
            handle: None,
        }
    }
}

/// The radius a positional sound is at full volume inside, and the one it is
/// cut at. Metres, for a game whose unit is a metre; the pair is what sets a
/// sound's carry, so both are per-sound.
const DEFAULT_MIN_DISTANCE: f32 = 1.0;
const DEFAULT_MAX_DISTANCE: f32 = 50.0;

pub struct AudioState {
    device: Option<backend::Device>,
    /// Whether the open has been tried. Nothing tries until a sound plays or
    /// `ready` is read, so a run that makes no sound never opens a device.
    opened: bool,
    /// True while the device waits for `UserActivation`: a browser refuses
    /// to start audio before a gesture, so the open is deferred to one.
    awaiting_activation: bool,
    /// Live sinks by handle. A handle absent here answers `is_playing` false
    /// and its setters no-op.
    playing: DetHashMap<u64, backend::Sound>,
    /// What each live handle was played at and through, so moving a bus's
    /// slider can re-apply the gain to what is already sounding. Without it a
    /// volume change would only reach sounds started after it.
    routing: DetHashMap<u64, Routed>,
    /// Every node's `sound` component, keyed the way `AnimationState` keys
    /// its players.
    pub nodes: DetHashMap<Entity, Sound>,
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

/// Where a live handle plays: its bus, the volume its caller asked for, and
/// the gain last handed to its sink. The applied gain is bookkeeping rather
/// than a sink reading, so a headless run can assert the mix.
struct Routed {
    bus: String,
    volume: f32,
    applied: f32,
}

/// One `play`: how loud and fast, looping or not, on which bus at what chain
/// gain, and, for a positional sound, where it plays from.
pub struct Cue {
    pub volume: f32,
    pub pitch: f32,
    pub looped: bool,
    pub bus: String,
    /// The bus chain's gain, resolved by the caller.
    pub gain: f32,
    pub emitter: Option<Emitter>,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            looped: false,
            bus: String::new(),
            gain: 1.0,
            emitter: None,
        }
    }
}

impl AudioState {
    /// Stop everything currently playing and clear every node's handle.
    pub fn stop_all(&mut self) {
        for (_, sink) in self.playing.drain(..) {
            sink.stop();
        }
        self.routing.clear();
        self.spatial.clear();
        for sound in self.nodes.values_mut() {
            sound.handle = None;
        }
    }

    /// Open the output device, once, unless a browser is still waiting for
    /// its gesture. Every path that needs a device calls this first.
    fn open_if_needed(&mut self) {
        if self.opened || self.awaiting_activation {
            return;
        }
        self.opened = true;
        self.device = open_device();
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
        bytes: Vec<u8>,
        volume: f32,
        pitch: f32,
        looped: bool,
        bus: &str,
        gain: f32,
    ) -> u64 {
        self.play_cue(
            bytes,
            Cue {
                volume,
                pitch,
                looped,
                bus: bus.to_string(),
                gain,
                emitter: None,
            },
        )
    }

    /// Start a whole cue, positional or not, and hand back its handle.
    ///
    /// A cue carrying an emitter is placed here rather than waiting for the
    /// next frame's pass: a sound the far side of the level must not be heard
    /// at full volume for the frame before it is placed.
    pub fn play_cue(&mut self, bytes: Vec<u8>, cue: Cue) -> u64 {
        let handle = self.next_handle;
        self.next_handle += 1;
        let volume = cue.volume.max(0.0);
        let pitch = cue.pitch.max(MIN_PITCH);
        let placement = match cue.emitter {
            Some(mut emitter) => {
                emitter.pitch = pitch;
                emitter.placement = spatial::place(&self.listener, &emitter);
                let placement = emitter.placement;
                self.spatial.insert(handle, emitter);
                Some(placement)
            }
            None => None,
        };
        let placed = placement.unwrap_or_default();
        let applied = (volume * cue.gain * placed.gain).max(0.0);
        self.routing.insert(
            handle,
            Routed {
                bus: cue.bus,
                volume,
                applied,
            },
        );
        self.open_if_needed();
        if let Some(device) = &self.device {
            let started = backend::play(
                device,
                bytes,
                applied,
                (pitch * placed.pitch).max(MIN_PITCH),
                cue.looped,
                placement.map(|placed| spatial::stereo_gains(placed.pan)),
            );
            match started {
                Ok(sound) => {
                    self.playing.insert(handle, sound);
                }
                Err(err) => tracing::warn!("audio did not decode: {err}"),
            }
        }
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
            routed.applied = (routed.volume * buses.gain(&routed.bus)).max(0.0);
            if let Some(sink) = self.playing.get(handle) {
                sink.set_volume(routed.applied);
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
    /// chain, times its distance gain when it is placed.
    #[must_use]
    pub fn effective_volume(&self, handle: u64) -> Option<f32> {
        self.routing.get(&handle).map(|routed| routed.applied)
    }

    /// Stop one handle's sound. A finished, stopped or unknown handle no-ops.
    pub fn stop(&mut self, handle: u64) {
        self.routing.shift_remove(&handle);
        self.spatial.shift_remove(&handle);
        if let Some(sink) = self.playing.shift_remove(&handle) {
            sink.stop();
        }
    }

    /// Set a live handle's own volume, before its bus and its distance. The
    /// stored one moves with it, so a bus slider and the next placement pass
    /// both recompute from what was asked for last.
    pub fn set_volume(&mut self, handle: u64, volume: f32, buses: &Buses) {
        let volume = volume.max(0.0);
        let placed = self
            .spatial
            .get(&handle)
            .map_or(1.0, |emitter| emitter.placement.gain);
        let Some(routed) = self.routing.get_mut(&handle) else {
            return;
        };
        routed.volume = volume;
        routed.applied = (volume * buses.gain(&routed.bus) * placed).max(0.0);
        let applied = routed.applied;
        if let Some(sink) = self.playing.get(&handle) {
            sink.set_volume(applied);
        }
    }

    pub fn set_pitch(&mut self, handle: u64, pitch: f32) {
        let pitch = pitch.max(MIN_PITCH);
        if let Some(emitter) = self.spatial.get_mut(&handle) {
            emitter.pitch = pitch;
        }
        if let Some(sink) = self.playing.get(&handle) {
            sink.set_pitch(pitch);
        }
    }

    /// Where a positional handle plays from, and `None` for a flat one.
    #[must_use]
    pub fn emitter_position(&self, handle: u64) -> Option<Vec3> {
        self.spatial.get(&handle).map(|emitter| emitter.position)
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

/// The bytes a sound path names, cached between plays so a footstep does not
/// cost a read per step.
fn read_sound(eng: &Engine, path: &str) -> Result<Vec<u8>> {
    cache::read(eng, path)
}

/// Start `entity`'s configured sound and hand back the handle. An explicit
/// trigger: a sound the node already has playing restarts.
///
/// # Errors
/// If the node has no `sound` component, its `file` is empty, or the file
/// does not exist.
pub fn play_on(eng: &Engine, entity: Entity) -> Result<u64> {
    bus::ensure_loaded(eng);
    let state = eng.resource::<AudioState>();
    let mut state = state.borrow_mut();
    let (file, current, mut cue) = {
        let sound = state
            .nodes
            .get(&entity)
            .ok_or_else(|| anyhow!("this node has no `sound` component to play"))?;
        (
            sound.file.clone(),
            sound.handle,
            Cue {
                volume: sound.volume,
                pitch: sound.pitch,
                looped: sound.looped,
                bus: sound.bus.clone(),
                gain: 1.0,
                emitter: sound.positional.then(|| {
                    Emitter::new(
                        Vec3::ZERO,
                        sound.min_distance,
                        sound.max_distance,
                        sound.doppler,
                    )
                }),
            },
        )
    };
    if file.trim().is_empty() {
        bail!("the node's `sound` component names no `file`");
    }
    let bytes = read_sound(eng, &file)?;
    if let Some(emitter) = &mut cue.emitter {
        // Composed here rather than read off `GlobalTransform`: a node that
        // entered the scene this frame has not been through a scene sync, and
        // a sound must not start from the origin and jump.
        emitter.position = scene::composed_global(&eng.world(), entity).position;
    }
    if let Some(current) = current {
        state.stop(current);
    }
    cue.gain = eng.resource::<bus::Buses>().borrow().gain(&cue.bus);
    let handle = state.play_cue(bytes, cue);
    if let Some(sound) = state.nodes.get_mut(&entity) {
        sound.handle = Some(handle);
    }
    Ok(handle)
}

/// Silence `entity`'s sound. A node without one is left alone.
pub fn stop_on(eng: &Engine, entity: Entity) {
    let Some(state) = eng.try_resource::<AudioState>() else {
        return;
    };
    let mut state = state.borrow_mut();
    let stopped = state
        .nodes
        .get_mut(&entity)
        .and_then(|sound| sound.handle.take());
    if let Some(handle) = stopped {
        state.stop(handle);
    }
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

/// Drop finished sinks, and stop the sounds of nodes that were freed.
fn open_device() -> Option<backend::Device> {
    match backend::open_default() {
        Ok(device) => Some(device),
        Err(err) => {
            tracing::warn!("audio disabled: {err}");
            None
        }
    }
}

/// Open the device the first tick after the page has seen a gesture.
fn open_on_activation_system(eng: &Engine, _: f32) {
    let state = eng.resource::<AudioState>();
    let mut state = state.borrow_mut();
    if !state.awaiting_activation || eng.try_resource::<balaur_core::UserActivation>().is_none() {
        return;
    }
    state.awaiting_activation = false;
    state.opened = true;
    state.device = open_device();
}

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
        ..
    } = &mut *state;
    // A `Sound` lives here, not on the entity, so this is where a freed
    // node's playback stops.
    nodes.retain(|&entity, sound| {
        if world.contains(entity) {
            return true;
        }
        if let Some(handle) = sound.handle {
            spatial.shift_remove(&handle);
            routing.shift_remove(&handle);
            if let Some(sink) = playing.shift_remove(&handle) {
                sink.stop();
            }
        }
        false
    });
    // A sink that has played out ends its handle's bookkeeping too. With no
    // device nothing plays out, so a handle there lasts until it is stopped.
    playing.retain(|handle, sink| {
        if sink.finished() {
            spatial.shift_remove(handle);
            routing.shift_remove(handle);
            return false;
        }
        true
    });
}

impl balaur_plugin::Plugin for AudioPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(AudioState {
            device: None,
            opened: false,
            awaiting_activation: cfg!(target_family = "wasm"),
            playing: DetHashMap::default(),
            routing: DetHashMap::default(),
            nodes: DetHashMap::default(),
            spatial: DetHashMap::default(),
            listeners: DetHashMap::default(),
            listener: ListenerPose::default(),
            next_handle: 1,
        });
        reg.insert_resource(bus::Buses::default());
        reg.insert_resource(event::Events::default());
        reg.insert_resource(cache::SoundCache::default());

        reg.add_system(Stage::First, open_on_activation_system);
        reg.add_system(Stage::PostUpdate, sweep_sounds_system);
        reg.add_system(Stage::SceneSync, spatial::spatialize_system);
        register_sound_component(reg);
        spatial::register_listener_component(reg);

        let mut m = reg.script_module("audio")?;
        script_api::install_audio_api(&mut m);
        Ok(())
    }
}

/// The `sound` scene key: the one an editor-saved `[nodes.sound]` writes.
///
/// Takes the plugin `Registry` rather than `&mut App`: audio registers
/// through the plugin seam, and `Registry::register_component` is that
/// seam's spelling of the same operation.
fn register_sound_component(reg: &mut balaur_plugin::Registry<'_>) {
    reg.register_component(
        "sound",
        ComponentDef {
            doc: "A sound on the node: `file`, `volume`, `pitch` and `loop`. `autoplay` starts it on load, `audio.play_on` triggers it, and `positional` plays it from the node for the `listener`.",
            schema: ComponentDef::parse_schema(
                "sound",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::FILE, r#"{ type = "string", default = "", description = "Audio file, project-relative; required to play" }"#),
                    (k::AUTOPLAY, r#"{ type = "bool", default = false, description = "Start playing when the node enters the scene" }"#),
                    (k::VOLUME, r#"{ type = "float", default = 1.0, min = 0.0, description = "Linear gain; 1 is the file's own level" }"#),
                    (k::PITCH, r#"{ type = "float", default = 1.0, min = 0.01, description = "Playback speed multiplier" }"#),
                    (k::LOOP, r#"{ type = "bool", default = false, description = "Restart the sound when it ends" }"#),
                    (k::BUS, r#"{ type = "string", default = "", description = "Audio bus this plays through; empty is `master`" }"#),
                    (k::POSITIONAL, r#"{ type = "bool", default = false, description = "Place the sound where the node is, heard from the `listener`" }"#),
                    (k::MIN_DISTANCE, r#"{ type = "float", default = 1.0, min = 0.001, description = "Full volume within this distance of the listener" }"#),
                    (k::MAX_DISTANCE, r#"{ type = "float", default = 50.0, min = 0.001, description = "Silent beyond this distance from the listener" }"#),
                    (k::DOPPLER, r#"{ type = "float", default = 0.0, min = 0.0, description = "How much the closing speed bends the pitch; 0 is off, 1 physical" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::AUDIO],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                apply_sound(eng, entity, params);
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                remove_sound(eng, entity);
                Ok(())
            }),
            get: Box::new(sound_of),
        },
    );
}

fn apply_sound(eng: &Engine, entity: Entity, params: &toml::Value) {
    let file = params
        .get(k::FILE)
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let flag = |key: &str| params.get(key).and_then(toml::Value::as_bool) == Some(true);
    let level =
        |key: &str, default: f64| params.get(key).and_then(as_f64).unwrap_or(default) as f32;
    let (autoplay, volume, pitch) = (
        flag(k::AUTOPLAY),
        level(k::VOLUME, 1.0),
        level(k::PITCH, 1.0),
    );
    let has_file = !file.trim().is_empty();
    bus::ensure_loaded(eng);
    let start = {
        let buses = eng.resource::<Buses>();
        let buses = buses.borrow();
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        let (file_changed, handle) = {
            let sound = state.nodes.entry(entity).or_default();
            let file_changed = sound.file != file;
            sound.file = file;
            sound.autoplay = autoplay;
            sound.volume = volume;
            sound.pitch = pitch;
            sound.looped = flag(k::LOOP);
            sound.bus = params
                .get(k::BUS)
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_string();
            sound.positional = flag(k::POSITIONAL);
            sound.min_distance = level(k::MIN_DISTANCE, f64::from(DEFAULT_MIN_DISTANCE));
            sound.max_distance = level(k::MAX_DISTANCE, f64::from(DEFAULT_MAX_DISTANCE));
            sound.doppler = level(k::DOPPLER, 0.0);
            // A `sound` now naming another file drops the old playback.
            if file_changed {
                (true, sound.handle.take())
            } else {
                (false, sound.handle)
            }
        };
        match handle {
            Some(handle) if file_changed => state.stop(handle),
            // Volume and pitch land live on a sound already going.
            Some(handle) => {
                state.set_volume(handle, volume, &buses);
                state.set_pitch(handle, pitch);
            }
            None => {}
        }
        let started = state.nodes.get(&entity).is_some_and(|s| s.handle.is_some());
        autoplay && has_file && !started
    };
    // Re-applying the component must not restart a sound already started:
    // the same rule the `animation` component holds for its autoplay clip.
    if start && let Err(why) = play_on(eng, entity) {
        tracing::warn!("sound autoplay: {why:#}");
    }
}

fn remove_sound(eng: &Engine, entity: Entity) {
    let Some(state) = eng.try_resource::<AudioState>() else {
        return;
    };
    let mut state = state.borrow_mut();
    let removed = state.nodes.shift_remove(&entity);
    if let Some(handle) = removed.and_then(|sound| sound.handle) {
        state.stop(handle);
    }
}

fn sound_of(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.try_resource::<AudioState>()?;
    let state = state.borrow();
    let sound = state.nodes.get(&entity)?;
    let mut out = toml::map::Map::new();
    out.insert(k::FILE.into(), sound.file.clone().into());
    out.insert(k::AUTOPLAY.into(), sound.autoplay.into());
    out.insert(k::VOLUME.into(), f64::from(sound.volume).into());
    out.insert(k::PITCH.into(), f64::from(sound.pitch).into());
    out.insert(k::LOOP.into(), sound.looped.into());
    out.insert(k::BUS.into(), sound.bus.clone().into());
    out.insert(k::POSITIONAL.into(), sound.positional.into());
    out.insert(k::MIN_DISTANCE.into(), f64::from(sound.min_distance).into());
    out.insert(k::MAX_DISTANCE.into(), f64::from(sound.max_distance).into());
    out.insert(k::DOPPLER.into(), f64::from(sound.doppler).into());
    Some(toml::Value::Table(out))
}
