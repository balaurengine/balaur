//! `audio.play_file`: a sound no node carries, such as an impact or a click,
//! through a bus, and placed in the world when the caller says where.

use anyhow::Result;
use balaur_core::Engine;
use balaur_core::glamx::Vec3;

use crate::spatial::Emitter;
use crate::{AudioState, Playback, bus};

/// How a file plays, under the `sound` component's own keys.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileOptions {
    /// The bus this plays through; empty is `master`.
    pub bus: String,
    pub volume_linear: f32,
    pub pitch_scale: f32,
    #[serde(rename = "loop")]
    pub looped: bool,
    /// Where it is heard from, by the `listener`; unplaced when left out.
    pub position: Option<[f32; 3]>,
    /// Full volume inside `min_distance`, silent past `max_distance`, and
    /// `doppler_level` of the closing speed in its pitch, once placed.
    pub min_distance: f32,
    pub max_distance: f32,
    pub doppler_level: f32,
}

impl Default for FileOptions {
    fn default() -> Self {
        Self {
            bus: String::new(),
            volume_linear: 1.0,
            pitch_scale: 1.0,
            looped: false,
            position: None,
            min_distance: 1.0,
            max_distance: 50.0,
            doppler_level: 0.0,
        }
    }
}

/// Start `path` playing and answer its handle, which `stop_playback` and the
/// other playback calls take.
///
/// # Errors
/// When the file does not read as a sound.
pub fn play_file(eng: &Engine, path: &str, options: &FileOptions) -> Result<u64> {
    crate::ensure_loaded(eng);
    let bytes = crate::read_sound(eng, path)?;
    let gain = eng.resource::<bus::Buses>().borrow().gain(&options.bus);
    let emitter = options.position.map(|at| {
        Emitter::new(
            Vec3::from_array(at),
            options.min_distance,
            options.max_distance,
            options.doppler_level,
        )
    });
    let handle = eng.resource::<AudioState>().borrow_mut().play_with(
        bytes,
        Playback {
            volume: options.volume_linear,
            pitch: options.pitch_scale,
            looped: options.looped,
            bus: options.bus.clone(),
            gain,
            emitter,
            file: crate::FileSettings::of(eng, path),
            ..Playback::default()
        },
    );
    Ok(handle)
}
