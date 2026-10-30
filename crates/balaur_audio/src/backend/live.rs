//! The settings a sound or a bus plays with, and the adapters that read them
//! on the audio thread.
//!
//! The frame writes a sound's [`Live`] (its pan) and the device a bus's (its
//! effects); every adapter in the chain polls the part it serves. Atomics
//! rather than a lock, because the mixer callback must never wait on the
//! frame that is writing them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use rodio::source::{ChannelVolume, SeekError, Source};
use rodio::{ChannelCount, Sample, SampleRate};

use crate::bus::{Effects, MIN_Q};

/// How often an adapter that polls looks at its knobs: the period rodio's own
/// `Player` controls run at.
pub(super) const PERIOD: Duration = Duration::from_millis(5);

/// One number the frame writes and the mixer reads.
pub(crate) struct Knob(AtomicU32);

impl Knob {
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    pub(super) fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// Every setting of one sound or one bus that its chain reads as it plays: a
/// sound's pan, a bus's effects.
pub(crate) struct Live {
    /// The left and right gains, from a positional placement or a flat pan.
    pan: [Knob; 2],
    /// Frequency and Q of each filter; 0 Hz is off.
    low_pass: [Knob; 2],
    high_pass: [Knob; 2],
    /// Moves with every filter change, so a filter retunes once per change
    /// rather than reading four knobs per sample.
    filter_revision: AtomicU32,
    pub(super) distortion_gain: Knob,
    pub(super) distortion_threshold: Knob,
    pub(super) reverb_level: Knob,
    pub(super) auto_gain_on: AtomicBool,
    pub(super) auto_gain_target: Knob,
    pub(super) auto_gain_attack_time: Knob,
    pub(super) auto_gain_release_time: Knob,
    pub(super) auto_gain_max: Knob,
    pub(super) auto_gain_floor: Knob,
}

impl Live {
    /// A sound's: its stereo gains. Its effects are its bus's.
    pub(crate) fn of_sound(gains: [f32; 2]) -> Self {
        Self::new(&Effects::default(), gains)
    }

    /// A bus's: its effects, with both channels at full, since a bus's mix is
    /// not panned.
    pub(crate) fn of_bus(effects: &Effects) -> Self {
        Self::new(effects, [1.0, 1.0])
    }

    fn new(effects: &Effects, gains: [f32; 2]) -> Self {
        let live = Self {
            pan: [Knob::new(gains[0]), Knob::new(gains[1])],
            low_pass: [Knob::new(0.0), Knob::new(0.0)],
            high_pass: [Knob::new(0.0), Knob::new(0.0)],
            filter_revision: AtomicU32::new(0),
            distortion_gain: Knob::new(1.0),
            distortion_threshold: Knob::new(0.0),
            reverb_level: Knob::new(0.0),
            auto_gain_on: AtomicBool::new(false),
            auto_gain_target: Knob::new(1.0),
            auto_gain_attack_time: Knob::new(0.0),
            auto_gain_release_time: Knob::new(0.0),
            auto_gain_max: Knob::new(1.0),
            auto_gain_floor: Knob::new(0.0),
        };
        live.set_effects(effects);
        live
    }

    pub(crate) fn set_pan(&self, gains: [f32; 2]) {
        self.pan[0].set(gains[0]);
        self.pan[1].set(gains[1]);
    }

    fn gains(&self) -> [f32; 2] {
        [self.pan[0].get(), self.pan[1].get()]
    }

    /// Everything but the pan.
    fn set_effects(&self, effects: &Effects) {
        self.low_pass[0].set(effects.low_pass_hz);
        self.low_pass[1].set(effects.low_pass_q);
        self.high_pass[0].set(effects.high_pass_hz);
        self.high_pass[1].set(effects.high_pass_q);
        self.filter_revision.fetch_add(1, Ordering::Relaxed);
        self.distortion_gain.set(effects.distortion_gain);
        self.distortion_threshold.set(effects.distortion_threshold);
        self.reverb_level.set(effects.reverb_level);
        let agc = &effects.auto_gain;
        self.auto_gain_on.store(agc.on, Ordering::Relaxed);
        self.auto_gain_target.set(agc.target);
        self.auto_gain_attack_time.set(agc.attack_time);
        self.auto_gain_release_time.set(agc.release_time);
        self.auto_gain_max.set(agc.max);
        self.auto_gain_floor.set(agc.floor);
    }
}

/// Mix a source down to mono and spread it across two channels at the gains
/// `live` holds. A positional sound has one direction, so the channels it came
/// with are given up.
pub(super) fn spread<S: Source>(source: S, live: &Arc<Live>) -> impl Source + use<S> {
    let live = live.clone();
    ChannelVolume::new(source, live.gains().to_vec()).periodic_access(PERIOD, move |channels| {
        let gains = live.gains();
        channels.set_volume(0, gains[0]);
        channels.set_volume(1, gains[1]);
    })
}

/// A flat sound's balance: a stereo file keeps its two channels and each is
/// scaled; a mono one is spread over two at the same gains.
pub(super) fn balance(source: Box<dyn Source + Send>, live: &Arc<Live>) -> Box<dyn Source + Send> {
    if source.channels().get() == 1 {
        return Box::new(spread(source, live));
    }
    Box::new(Balance {
        input: source,
        live: live.clone(),
        channel: 0,
    })
}

/// The first two channels of a source each at their own gain, the rest as
/// they are.
struct Balance<S> {
    input: S,
    live: Arc<Live>,
    channel: u16,
}

impl<S: Source> Iterator for Balance<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let sample = self.input.next()?;
        let channel = usize::from(self.channel);
        self.channel = (self.channel + 1) % self.input.channels().get();
        Some(match channel {
            0 | 1 => sample * self.live.pan[channel].get(),
            _ => sample,
        })
    }
}

impl<S: Source> Source for Balance<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.input.try_seek(pos)
    }
}

/// Which side of the cutoff a [`Pass`] keeps.
#[derive(Clone, Copy)]
pub(super) enum Side {
    Low,
    High,
}

/// rodio's `low_pass_with_q` and `high_pass_with_q` with a history per
/// channel. rodio's `BltFilter` keeps one history across the interleaved
/// samples, so on a stereo mix each channel is filtered with the other's
/// samples; a bus's mix is as wide as the device. Retuned in place when its
/// knobs move, keeping its history; below 1 Hz it hands the samples through.
pub(super) struct Pass<S> {
    input: S,
    live: Arc<Live>,
    side: Side,
    revision: u32,
    /// `None` while the filter is off.
    coefficients: Option<Coefficients>,
    histories: Vec<History>,
    channel: usize,
}

/// One biquad's coefficients, rodio's own formulas (`blt.rs`) divided
/// through by `a0`.
#[derive(Clone, Copy)]
struct Coefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coefficients {
    /// Sine and cosine through `libm`: a platform's own library may round
    /// them differently (DETERMINISM.md).
    fn new(side: Side, hz: f32, q: f32, rate: f32) -> Self {
        let angle = 2.0 * std::f32::consts::PI * hz / rate;
        let cosine = libm::cosf(angle);
        let alpha = libm::sinf(angle) / (2.0 * q);
        let (b0, b1) = match side {
            Side::Low => (f32::midpoint(1.0, -cosine), 1.0 - cosine),
            Side::High => (f32::midpoint(1.0, cosine), -1.0 - cosine),
        };
        let a0 = 1.0 + alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b0 / a0,
            a1: -2.0 * cosine / a0,
            a2: (1.0 - alpha) / a0,
        }
    }
}

/// One channel's last two samples in and out.
#[derive(Clone, Copy, Default)]
struct History {
    inputs: [f32; 2],
    outputs: [f32; 2],
}

impl History {
    fn apply(&mut self, coefficients: &Coefficients, sample: f32) -> f32 {
        let Coefficients { b0, b1, b2, a1, a2 } = *coefficients;
        let output = b0 * sample + b1 * self.inputs[0] + b2 * self.inputs[1]
            - a1 * self.outputs[0]
            - a2 * self.outputs[1];
        self.inputs = [sample, self.inputs[0]];
        self.outputs = [output, self.outputs[0]];
        output
    }
}

impl<S: Source> Pass<S> {
    pub(super) fn new(input: S, live: &Arc<Live>, side: Side) -> Self {
        let mut pass = Self {
            input,
            live: live.clone(),
            side,
            revision: u32::MAX,
            coefficients: None,
            histories: Vec::new(),
            channel: 0,
        };
        pass.retune();
        pass
    }

    /// Read the knobs when they have moved. Called between frames, so every
    /// channel of one frame is filtered alike.
    fn retune(&mut self) {
        let channels = usize::from(self.input.channels().get());
        if self.histories.len() != channels {
            self.histories = vec![History::default(); channels];
        }
        let revision = self.live.filter_revision.load(Ordering::Relaxed);
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        let knobs = match self.side {
            Side::Low => &self.live.low_pass,
            Side::High => &self.live.high_pass,
        };
        let rate = self.input.sample_rate().get() as f32;
        // Past half the sample rate the bilinear transform has no stable
        // filter to give.
        let hz = knobs[0].get().min(rate * 0.49);
        let q = knobs[1].get().max(MIN_Q);
        self.coefficients = (hz >= 1.0).then(|| Coefficients::new(self.side, hz, q, rate));
        if self.coefficients.is_none() {
            // Switched on again later, it starts from silence, as a fresh
            // rodio filter would.
            self.histories.fill(History::default());
        }
    }
}

impl<S: Source> Iterator for Pass<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.channel == 0 {
            self.retune();
        }
        let sample = self.input.next()?;
        let channel = self.channel;
        self.channel = (self.channel + 1) % self.histories.len();
        Some(match &self.coefficients {
            Some(coefficients) => self.histories[channel].apply(coefficients, sample),
            None => sample,
        })
    }
}

impl<S: Source> Source for Pass<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.input.try_seek(pos)
    }
}

/// rodio's `reverb` without its `buffered`: the echo is the same samples,
/// held back `delay` and added at the level `live` holds. `buffered` reads a
/// source that names no span 32768 samples at a time, and a mixer names
/// none, so a sound joining a bus with a reverb would wait that long to be
/// heard.
pub(super) struct Echo<S> {
    input: S,
    live: Arc<Live>,
    /// The last `delay` of samples, every channel interleaved; the oldest is
    /// at `oldest`, next to be heard and overwritten.
    held: Vec<Sample>,
    oldest: usize,
}

impl<S: Source> Echo<S> {
    pub(super) fn new(input: S, delay: Duration, live: &Arc<Live>) -> Self {
        let channels = usize::from(input.channels().get());
        let frames = (delay.as_secs_f64() * f64::from(input.sample_rate().get())).round() as usize;
        Self {
            held: vec![0.0; frames.max(1) * channels],
            oldest: 0,
            input,
            live: live.clone(),
        }
    }
}

impl<S: Source> Iterator for Echo<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let sample = self.input.next()?;
        let echo = std::mem::replace(&mut self.held[self.oldest], sample);
        self.oldest = (self.oldest + 1) % self.held.len();
        Some(sample + echo * self.live.reverb_level.get())
    }
}

impl<S: Source> Source for Echo<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.input.try_seek(pos)
    }
}
