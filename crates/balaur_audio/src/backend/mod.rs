//! The rodio/cpal backend: native audio stacks, and WebAudio on wasm.
//!
//! The device's mixer is fed by a mixer of Balaur's own, the root, so the
//! whole mix can pass through the master bus's limiter and the dither before
//! it reaches the device. A bus with a limiter gets a mixer of its own that
//! feeds the nearest limited bus above it, or the root.

use std::collections::BTreeMap;
use std::num::NonZero;
use std::sync::Arc;

use anyhow::Result;
use rodio::mixer::{Mixer, MixerSource};
use rodio::source::{LimitSettings, Source, Zero};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

mod chain;
mod live;

pub(crate) use chain::{From, length_of};
use live::Live;

use crate::bus::{Limiter, MASTER, Route};
use crate::output::OutputSettings;
use crate::playback::{Effects, Program};

pub(crate) struct Device {
    /// Held for its stream: dropping it closes the device.
    _sink: MixerDeviceSink,
    root: Mixer,
    /// The mixer each declared bus's sounds join.
    routes: BTreeMap<String, Mixer>,
}

impl Device {
    /// Open the output `output` asks for, and build the bus mixers `routes`
    /// describe.
    pub(crate) fn open(output: &OutputSettings, routes: &[Route]) -> Result<Self> {
        #[cfg(windows)]
        keep_com_alive();
        let sink = if output.asks_for_nothing() {
            DeviceSinkBuilder::open_default_sink()?
        } else {
            configured(output)?
        };
        let config = sink.config();
        let (channels, rate) = (config.channel_count(), config.sample_rate());
        let fresh = || {
            let (mixer, source) = rodio::mixer::mixer(channels, rate);
            // An empty mixer ends, and its parent drops it; silence keeps it.
            mixer.add(Zero::new(channels, rate));
            (mixer, source)
        };
        let (root, root_source) = fresh();
        let mut own: BTreeMap<&str, Mixer> = BTreeMap::new();
        let mut sends: Vec<(&str, MixerSource, Limiter)> = Vec::new();
        for route in routes.iter().filter(|r| r.name != MASTER) {
            if let Some(limit) = route.limit {
                let (mixer, source) = fresh();
                own.insert(&route.name, mixer);
                sends.push((&route.parent, source, limit));
            }
        }
        let joins = |name: &str| -> Mixer {
            let mut at = name;
            for _ in 0..=routes.len() {
                if let Some(mixer) = own.get(at) {
                    return mixer.clone();
                }
                match routes.iter().find(|r| r.name == at) {
                    Some(route) if at != MASTER => at = &route.parent,
                    _ => break,
                }
            }
            root.clone()
        };
        for (parent, source, limit) in sends {
            joins(parent).add(source.limit(settings_of(limit)));
        }
        let map = routes
            .iter()
            .map(|route| (route.name.clone(), joins(&route.name)))
            .collect();
        let master = routes
            .iter()
            .find(|r| r.name == MASTER)
            .and_then(|r| r.limit);
        let limited: Box<dyn Source + Send> = match master {
            Some(limit) => Box::new(root_source.limit(settings_of(limit))),
            None => Box::new(root_source),
        };
        sink.mixer().add(dithered(limited, output.dither_bits));
        Ok(Self {
            _sink: sink,
            root,
            routes: map,
        })
    }

    /// The mixer a sound on `bus` joins: its own bus's, the nearest limited
    /// one above it, or the root.
    fn mixer(&self, bus: &str) -> &Mixer {
        let bus = if bus.is_empty() { MASTER } else { bus };
        self.routes.get(bus).unwrap_or(&self.root)
    }
}

fn settings_of(limit: Limiter) -> LimitSettings {
    let seconds = |value: f32| {
        std::time::Duration::try_from_secs_f32(value.max(0.0)).unwrap_or(std::time::Duration::MAX)
    };
    LimitSettings::default()
        .with_threshold(limit.threshold_db)
        .with_knee_width(limit.knee_db.max(0.0))
        .with_attack(seconds(limit.attack_time))
        .with_release(seconds(limit.release_time))
}

#[cfg(feature = "dither")]
fn dithered(source: Box<dyn Source + Send>, bits: u32) -> Box<dyn Source + Send> {
    match NonZero::new(bits) {
        Some(bits) => Box::new(source.dither(bits, rodio::source::DitherAlgorithm::default())),
        None => source,
    }
}

#[cfg(not(feature = "dither"))]
fn dithered(source: Box<dyn Source + Send>, _bits: u32) -> Box<dyn Source + Send> {
    source
}

/// An output with what the `[audio]` settings ask of it: a named device, a
/// channel count, a sample rate, a buffer size. rodio tries the device's other
/// configurations when the one asked for will not open.
fn configured(output: &OutputSettings) -> Result<MixerDeviceSink> {
    let named = (!output.device.is_empty())
        .then(|| device_named(&output.device))
        .flatten();
    if named.is_none() && !output.device.is_empty() {
        tracing::warn!(
            "audio device '{}' is not connected; opening the default one",
            output.device
        );
    }
    let mut builder = match named {
        Some(device) => DeviceSinkBuilder::from_device(device)?,
        None => DeviceSinkBuilder::from_default_device()?,
    };
    if let Some(channels) = NonZero::new(output.channels) {
        builder = builder.with_channels(channels);
    }
    if let Some(rate) = NonZero::new(output.sample_rate_hz) {
        builder = builder.with_sample_rate(rate);
    }
    if output.buffer_frames > 0 {
        builder = builder.with_buffer_size(rodio::cpal::BufferSize::Fixed(output.buffer_frames));
    }
    Ok(builder.open_sink_or_fallback()?)
}

fn device_named(name: &str) -> Option<rodio::Device> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    rodio::cpal::default_host()
        .output_devices()
        .ok()?
        .find(|device| device.description().is_ok_and(|d| d.name() == name))
}

/// Every output device's name, as `[audio] device` takes it. Empty when the
/// host lists none.
pub(crate) fn devices() -> Vec<String> {
    rodio::speakers::available_outputs()
        .map(|outputs| outputs.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
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

/// How a sound sits on its player: its bus, gain, speed, whether it is held,
/// whether it is placed in the world, and its stereo gains.
#[derive(Clone, Copy)]
pub(crate) struct Controls<'a> {
    pub bus: &'a str,
    pub volume: f32,
    pub speed: f32,
    pub paused: bool,
    pub positional: bool,
    pub gains: [f32; 2],
}

/// One playing sound on the device.
pub(crate) struct Sound {
    player: Player,
    live: Arc<Live>,
}

impl Sound {
    /// Decode `program` from `from` and start it.
    pub(crate) fn start(
        device: &Device,
        program: &Program,
        from: From,
        controls: Controls<'_>,
    ) -> Result<Self> {
        let live = Arc::new(Live::new(&program.effects, controls.gains));
        Self::with_live(device, program, from, controls, live)
    }

    /// The same sound from somewhere else in its program, keeping its live
    /// settings. The old player stops; a seek and a skip are this.
    pub(crate) fn restart(
        &self,
        device: &Device,
        program: &Program,
        from: From,
        controls: Controls<'_>,
    ) -> Result<Self> {
        self.player.stop();
        Self::with_live(device, program, from, controls, self.live.clone())
    }

    fn with_live(
        device: &Device,
        program: &Program,
        from: From,
        controls: Controls<'_>,
        live: Arc<Live>,
    ) -> Result<Self> {
        let source = chain::source(program, from, &live, controls.positional)?;
        let player = Player::connect_new(device.mixer(controls.bus));
        player.set_volume(controls.volume);
        player.set_speed(controls.speed);
        if controls.paused {
            player.pause();
        }
        player.append(source);
        Ok(Self { player, live })
    }

    pub(crate) fn stop(&self) {
        self.player.stop();
    }

    pub(crate) fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume);
    }

    pub(crate) fn set_pitch(&self, pitch: f32) {
        self.player.set_speed(pitch);
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        if paused {
            self.player.pause();
        } else {
            self.player.play();
        }
    }

    /// Move the sound between the speakers: a positional placement's gains,
    /// or a flat sound's balance.
    pub(crate) fn set_pan(&self, gains: [f32; 2]) {
        self.live.set_pan(gains);
    }

    /// Retune the filters, the distortion, the reverb's level and automatic
    /// gain control. A reverb or a gain control the play started without
    /// joins at the next play.
    pub(crate) fn set_effects(&self, effects: &Effects) {
        self.live.set_effects(effects);
    }

    pub(crate) fn finished(&self) -> bool {
        self.player.empty()
    }
}
