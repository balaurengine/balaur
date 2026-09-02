//! Audio as a Balaur plugin, backed by rodio.
//!
//! `audio.play` hands back an integer handle; `stop`, `set_volume`,
//! `set_pitch` and `is_playing` address it. The `sound` component gives a
//! node a configured sound, triggered by `audio.play_on` / `audio.stop_on`.
//!
//! Audio is a pure observer of the simulation. If no output device is
//! available (CI, headless servers) the plugin logs a warning once, every
//! call still hands out the same handles, and `is_playing` answers false —
//! a game runs identically with and without a sound card. Anything that
//! feeds a decision (the `sound` component's "already started" check) is
//! therefore tracked as intent on [`Sound`], never read off a sink.
//!
//! Wasm builds are the extreme case of that rule: no audio stack compiles
//! there at all (see the backend modules below), so the whole backend is the
//! "no device" path and scripts calling `audio.*` still run.

use anyhow::{anyhow, bail, Result};
use balaur_core::components::{as_f64, ComponentDef};
use balaur_core::hecs::Entity;
use balaur_core::project::ProjectFiles;
use balaur_core::{entity_of, DetHashMap, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, NodeId, Value};

/// The rodio/cpal backend: every target with a real audio stack.
#[cfg(not(target_family = "wasm"))]
mod backend {
    use anyhow::Result;
    use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

    pub(crate) struct Device(MixerDeviceSink);
    pub(crate) struct Sound(Player);

    pub(crate) fn open_default() -> Result<Device> {
        Ok(Device(DeviceSinkBuilder::open_default_sink()?))
    }

    /// Decode sound bytes and start them on the device's mixer. Bytes, not a
    /// path: a packed game carries its audio inside the pack. rodio has no
    /// loop toggle on a live player, so looping is requested at decode time.
    pub(crate) fn play(
        device: &Device,
        bytes: Vec<u8>,
        volume: f32,
        pitch: f32,
        looped: bool,
    ) -> Result<Sound> {
        let decoder = rodio::Decoder::try_from(std::io::Cursor::new(bytes))?;
        let player = Player::connect_new(device.0.mixer());
        player.set_volume(volume);
        player.set_speed(pitch);
        if looped {
            player.append(rodio::source::Source::repeat_infinite(decoder));
        } else {
            player.append(decoder);
        }
        Ok(Sound(player))
    }

    impl Sound {
        pub(crate) fn stop(&self) {
            self.0.stop();
        }

        pub(crate) fn set_volume(&self, volume: f32) {
            self.0.set_volume(volume);
        }

        pub(crate) fn set_pitch(&self, pitch: f32) {
            self.0.set_speed(pitch);
        }

        pub(crate) fn finished(&self) -> bool {
            self.0.empty()
        }
    }
}

/// The wasm stub. cpal's emscripten host does not compile — broken upstream
/// against current wasm-bindgen and deleted outright in cpal 0.18 — and its
/// WebAudio host only runs inside a wasm-bindgen app, which an engine on
/// emscripten is not. Both types are uninhabited: opening always fails, the
/// plugin warns once, and the compiler proves the rest of this module dead.
#[cfg(target_family = "wasm")]
mod backend {
    use anyhow::{bail, Result};

    pub(crate) enum Device {}
    pub(crate) enum Sound {}

    pub(crate) fn open_default() -> Result<Device> {
        bail!("no audio backend compiles for wasm")
    }

    pub(crate) fn play(device: &Device, _: Vec<u8>, _: f32, _: f32, _: bool) -> Result<Sound> {
        match *device {}
    }

    impl Sound {
        pub(crate) fn stop(&self) {
            match *self {}
        }

        pub(crate) fn set_volume(&self, _: f32) {
            match *self {}
        }

        pub(crate) fn set_pitch(&self, _: f32) {
            match *self {}
        }

        pub(crate) fn finished(&self) -> bool {
            match *self {}
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
            handle: None,
        }
    }
}

pub struct AudioState {
    device: Option<backend::Device>,
    /// Live sinks by handle. A handle absent here answers `is_playing` false
    /// and its setters no-op.
    playing: DetHashMap<u64, backend::Sound>,
    /// Every node's `sound` component, keyed the way `AnimationState` keys
    /// its players.
    pub nodes: DetHashMap<Entity, Sound>,
    /// Counts up from 1 and never reuses, so a held handle names nothing
    /// rather than something else once its sound is gone.
    next_handle: u64,
}

impl AudioState {
    /// Stop everything currently playing and clear every node's handle.
    pub fn stop_all(&mut self) {
        for (_, sink) in self.playing.drain(..) {
            sink.stop();
        }
        for sound in self.nodes.values_mut() {
            sound.handle = None;
        }
    }

    /// Start a sound from its bytes — the `audio.*` bindings read paths
    /// through the pack-aware project reader — and hand back its handle.
    /// Never errors: no output device and bytes that will not decode both
    /// leave the handle silent, so a headless run behaves like a windowed
    /// one.
    pub fn play(&mut self, bytes: Vec<u8>, volume: f32, pitch: f32, looped: bool) -> u64 {
        let handle = self.next_handle;
        self.next_handle += 1;
        if let Some(device) = &self.device {
            match backend::play(device, bytes, volume.max(0.0), pitch.max(MIN_PITCH), looped) {
                Ok(sound) => {
                    self.playing.insert(handle, sound);
                }
                Err(err) => tracing::warn!("audio did not decode: {err}"),
            }
        }
        handle
    }

    /// Stop one handle's sound. A finished, stopped or unknown handle no-ops.
    pub fn stop(&mut self, handle: u64) {
        if let Some(sink) = self.playing.shift_remove(&handle) {
            sink.stop();
        }
    }

    pub fn set_volume(&mut self, handle: u64, volume: f32) {
        if let Some(sink) = self.playing.get(&handle) {
            sink.set_volume(volume.max(0.0));
        }
    }

    pub fn set_pitch(&mut self, handle: u64, pitch: f32) {
        if let Some(sink) = self.playing.get(&handle) {
            sink.set_pitch(pitch.max(MIN_PITCH));
        }
    }

    /// Whether a handle's sound is still audible. Always false headless.
    #[must_use]
    pub fn is_playing(&self, handle: u64) -> bool {
        self.playing
            .get(&handle)
            .is_some_and(|sink| !sink.finished())
    }
}

/// The bytes a sound path names, through the pack-aware project reader,
/// which says where it looked when there is nothing there.
fn read_sound(eng: &Engine, path: &str) -> Result<Vec<u8>> {
    eng.resource::<ProjectFiles>().borrow().read(path)
}

/// Start `entity`'s configured sound and hand back the handle. An explicit
/// trigger: a sound the node already has playing restarts.
///
/// # Errors
/// If the node has no `sound` component, its `file` is empty, or the file
/// does not exist.
pub fn play_on(eng: &Engine, entity: Entity) -> Result<u64> {
    let state = eng.resource::<AudioState>();
    let mut state = state.borrow_mut();
    let (file, volume, pitch, looped, current) = {
        let sound = state
            .nodes
            .get(&entity)
            .ok_or_else(|| anyhow!("this node has no `sound` component to play"))?;
        (
            sound.file.clone(),
            sound.volume,
            sound.pitch,
            sound.looped,
            sound.handle,
        )
    };
    if file.trim().is_empty() {
        bail!("the node's `sound` component names no `file`");
    }
    let bytes = read_sound(eng, &file)?;
    if let Some(current) = current {
        state.stop(current);
    }
    let handle = state.play(bytes, volume, pitch, looped);
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
fn sweep_sounds_system(eng: &Engine, _: f32) {
    let state = eng.resource::<AudioState>();
    let mut state = state.borrow_mut();
    let AudioState { nodes, playing, .. } = &mut *state;
    let world = eng.world();
    // A `Sound` lives here, not on the entity, so this is where a freed
    // node's playback stops.
    nodes.retain(|&entity, sound| {
        if world.contains(entity) {
            return true;
        }
        if let Some(handle) = sound.handle {
            if let Some(sink) = playing.shift_remove(&handle) {
                sink.stop();
            }
        }
        false
    });
    playing.retain(|_, sink| !sink.finished());
}

impl balaur_plugin::Plugin for AudioPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        let device = match backend::open_default() {
            Ok(device) => Some(device),
            Err(err) => {
                tracing::warn!("audio disabled: {err}");
                None
            }
        };
        reg.insert_resource(AudioState {
            device,
            playing: DetHashMap::default(),
            nodes: DetHashMap::default(),
            next_handle: 1,
        });

        reg.add_system(Stage::PostUpdate, sweep_sounds_system);
        register_sound_component(reg);

        let mut m = reg.script_module("audio")?;
        install_audio_api(&mut m);
        Ok(())
    }
}

/// The `sound` scene key — the one an editor-saved `[nodes.sound]` writes.
///
/// Takes the plugin `Registry` rather than `&mut App`: audio registers
/// through the plugin seam, and `Registry::register_component` is that
/// seam's spelling of the same operation.
fn register_sound_component(reg: &mut balaur_plugin::Registry<'_>) {
    reg.register_component(
        "sound",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "sound",
                r#"file = { type = "string", default = "", description = "Audio file, project-relative; required to play" }
autoplay = { type = "bool", default = false, description = "Start playing when the node enters the scene" }
volume = { type = "float", default = 1.0, min = 0.0, description = "Linear gain; 1 is the file's own level" }
pitch = { type = "float", default = 1.0, min = 0.01, description = "Playback speed multiplier" }
loop = { type = "bool", default = false, description = "Restart the sound when it ends" }"#,
            ),
            tags: &["audio"],
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
        .get("file")
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let flag = |key: &str| params.get(key).and_then(toml::Value::as_bool) == Some(true);
    let level = |key: &str| params.get(key).and_then(as_f64).unwrap_or(1.0) as f32;
    let (autoplay, volume, pitch) = (flag("autoplay"), level("volume"), level("pitch"));
    let has_file = !file.trim().is_empty();
    let start = {
        let state = eng.resource::<AudioState>();
        let mut state = state.borrow_mut();
        let (file_changed, handle) = {
            let sound = state.nodes.entry(entity).or_default();
            let file_changed = sound.file != file;
            sound.file = file;
            sound.autoplay = autoplay;
            sound.volume = volume;
            sound.pitch = pitch;
            sound.looped = flag("loop");
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
                state.set_volume(handle, volume);
                state.set_pitch(handle, pitch);
            }
            None => {}
        }
        let started = state.nodes.get(&entity).is_some_and(|s| s.handle.is_some());
        autoplay && has_file && !started
    };
    // Re-applying the component must not restart a sound already started —
    // the same rule the `animation` component holds for its autoplay clip.
    if start {
        if let Err(why) = play_on(eng, entity) {
            tracing::warn!("sound autoplay: {why:#}");
        }
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
    out.insert("file".into(), sound.file.clone().into());
    out.insert("autoplay".into(), sound.autoplay.into());
    out.insert("volume".into(), f64::from(sound.volume).into());
    out.insert("pitch".into(), f64::from(sound.pitch).into());
    out.insert("loop".into(), sound.looped.into());
    Some(toml::Value::Table(out))
}

/// One key out of a script options table, or `None` if the table, the key or
/// its type is missing. A typo in an options table should not stop the frame.
fn opt<'a>(opts: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    match opts? {
        Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// A number from an options entry, whichever way the language spelled it.
fn number(value: Option<&Value>) -> Option<f32> {
    match value {
        Some(Value::Num(n)) => Some(*n as f32),
        Some(Value::Int(i)) => Some(*i as f32),
        _ => None,
    }
}

/// A script-supplied handle. Negative numbers wrap to values `play` never
/// hands out, so they answer false and no-op rather than erroring.
const fn handle_of(raw: i64) -> u64 {
    raw as u64
}

/// `audio.*`. Declared against the neutral seam, so it works on any backend.
fn install_audio_api(m: &mut dyn Bindings<Engine>) {
    // `audio.play(path, { volume = 1.0, pitch = 1.0, loop = true })` hands
    // back the handle the other functions take. Flags live in the options
    // table rather than in the name, so fade/bus can join them (N9).
    m.function(
        "play",
        |eng: &Engine, (path, opts): (String, Option<Value>)| {
            let opts = opts.as_ref();
            let volume = number(opt(opts, "volume")).unwrap_or(1.0);
            let pitch = number(opt(opts, "pitch")).unwrap_or(1.0);
            let looped = matches!(opt(opts, "loop"), Some(Value::Bool(true)));
            let bytes = read_sound(eng, &path)?;
            let state = eng.resource::<AudioState>();
            let handle = state.borrow_mut().play(bytes, volume, pitch, looped);
            Ok(handle)
        },
    );
    m.function("stop", |eng: &Engine, handle: i64| {
        eng.resource::<AudioState>()
            .borrow_mut()
            .stop(handle_of(handle));
        Ok(())
    });
    m.function(
        "set_volume",
        |eng: &Engine, (handle, volume): (i64, f32)| {
            eng.resource::<AudioState>()
                .borrow_mut()
                .set_volume(handle_of(handle), volume);
            Ok(())
        },
    );
    m.function("set_pitch", |eng: &Engine, (handle, pitch): (i64, f32)| {
        eng.resource::<AudioState>()
            .borrow_mut()
            .set_pitch(handle_of(handle), pitch);
        Ok(())
    });
    m.function("is_playing", |eng: &Engine, handle: i64| {
        Ok(eng
            .resource::<AudioState>()
            .borrow()
            .is_playing(handle_of(handle)))
    });
    m.function("stop_all", |eng: &Engine, ()| {
        eng.resource::<AudioState>().borrow_mut().stop_all();
        Ok(())
    });
    m.function("play_on", |eng: &Engine, node: NodeId| {
        play_on(eng, entity_of(node)?)
    });
    m.function("stop_on", |eng: &Engine, node: NodeId| {
        stop_on(eng, entity_of(node)?);
        Ok(())
    });
}
