//! `[audio]`: the output device a run opens, and what the whole mix passes
//! through on its way there.

use balaur_core::Engine;
use balaur_core::components::as_f64;
use balaur_core::settings::{self, Scope};

use crate::vocabulary::settings as path;

/// What `[audio]` asks of the output. Zero and empty mean "the device's own".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputSettings {
    pub device: String,
    pub channels: u16,
    pub sample_rate_hz: u32,
    pub buffer_frames: u32,
    /// The bit depth the mix is dithered to; 0 is off.
    pub dither_bits: u32,
}

impl OutputSettings {
    /// Whether the device can be opened exactly as rodio opens a default one,
    /// with every fallback it tries.
    #[must_use]
    pub fn asks_for_nothing(&self) -> bool {
        self.device.is_empty()
            && self.channels == 0
            && self.sample_rate_hz == 0
            && self.buffer_frames == 0
    }

    /// The settings as this run resolves them.
    #[must_use]
    pub fn of(eng: &Engine) -> Self {
        let whole = |key: &str| {
            settings::get(eng, key)
                .as_ref()
                .and_then(as_f64)
                .unwrap_or_default()
                .max(0.0) as u32
        };
        Self {
            device: settings::get(eng, path::DEVICE)
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default(),
            channels: u16::try_from(whole(path::CHANNELS)).unwrap_or(u16::MAX),
            sample_rate_hz: whole(path::SAMPLE_RATE_HZ),
            buffer_frames: whole(path::BUFFER_FRAMES),
            dither_bits: whole(path::DITHER_BITS),
        }
    }
}

/// The `[audio]` keys, declared so the settings screen lists them.
/// `[audio.buses]` is the game's own names and is not declared.
pub(crate) fn declare_settings(eng: &Engine) {
    settings::define_group(
        eng,
        "audio",
        Scope::Project,
        &balaur_core::ComponentDef::parse_schema(
            "settings.audio",
            r#"
device = { type = "string", default = "", order = 1, applies = "restart", help = "The output device, by a name `audio.devices()` lists. Empty, or a device that is not connected, opens the system's default." }
channels = { type = "int", default = 0, min = 0, max = 32, order = 2, applies = "restart", help = "Output channels. 0 takes the device's own." }
sample_rate_hz = { type = "int", default = 0, min = 0, max = 384000, order = 3, applies = "restart", help = "Output sample rate. 0 takes the device's own." }
buffer_frames = { type = "int", default = 0, min = 0, max = 65536, order = 4, applies = "restart", help = "Frames per output buffer. Fewer answers sooner and risks dropouts; 0 takes 50 ms of frames, rodio's default." }
dither_bits = { type = "int", default = 0, min = 0, max = 32, order = 5, applies = "restart", help = "The bit depth the whole mix is dithered to before the device. 0 is off; a build without the `dither` feature leaves the mix alone." }
"#,
        ),
    );
}
