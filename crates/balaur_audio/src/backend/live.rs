//! The settings a playing sound takes while it plays, and the adapters that
//! read them on the audio thread.
//!
//! The frame writes a [`Live`]; every adapter in the sound's chain polls the
//! part it serves. Atomics rather than a lock, because the mixer callback must
//! never wait on the frame that is writing them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use rodio::source::{BltFilter, ChannelVolume, SeekError, Source};
use rodio::{ChannelCount, Sample, SampleRate};

use crate::playback::Effects;

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

/// Every setting of one sound that lands while it plays.
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
    pub(crate) fn new(effects: &Effects, gains: [f32; 2]) -> Self {
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

    /// Everything but the pan, which a placement owns for a positional sound.
    pub(crate) fn set_effects(&self, effects: &Effects) {
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

/// A `BltFilter` that can be switched off and on while it plays. Retuning a
/// running filter keeps its history (`to_low_pass_with_q`); switching it off
/// hands the source straight through.
pub(super) struct Pass<S> {
    stage: Option<Stage<S>>,
    live: Arc<Live>,
    side: Side,
    revision: u32,
}

enum Stage<S> {
    Bare(S),
    On(BltFilter<S>),
}

impl<S: Source> Pass<S> {
    pub(super) fn new(source: S, live: &Arc<Live>, side: Side) -> Self {
        let mut pass = Self {
            stage: Some(Stage::Bare(source)),
            live: live.clone(),
            side,
            revision: u32::MAX,
        };
        pass.retune();
        pass
    }

    fn retune(&mut self) {
        let revision = self.live.filter_revision.load(Ordering::Relaxed);
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        let knobs = match self.side {
            Side::Low => &self.live.low_pass,
            Side::High => &self.live.high_pass,
        };
        let (hz, q) = (knobs[0].get(), knobs[1].get().max(0.01));
        let Some(stage) = self.stage.take() else {
            return;
        };
        let rate = match &stage {
            Stage::Bare(source) => source.sample_rate(),
            Stage::On(filter) => filter.sample_rate(),
        };
        // Past half the sample rate the bilinear transform has no stable
        // filter to give.
        let hz = hz.min(rate.get() as f32 * 0.49);
        self.stage = Some(match (stage, hz >= 1.0, self.side) {
            (Stage::Bare(source), true, Side::Low) => {
                Stage::On(source.low_pass_with_q(hz as u32, q))
            }
            (Stage::Bare(source), true, Side::High) => {
                Stage::On(source.high_pass_with_q(hz as u32, q))
            }
            (Stage::On(mut filter), true, Side::Low) => {
                filter.to_low_pass_with_q(hz as u32, q);
                Stage::On(filter)
            }
            (Stage::On(mut filter), true, Side::High) => {
                filter.to_high_pass_with_q(hz as u32, q);
                Stage::On(filter)
            }
            (Stage::On(filter), false, _) => Stage::Bare(filter.into_inner()),
            (bare @ Stage::Bare(_), false, _) => bare,
        });
    }

    fn source(&self) -> &dyn Source {
        match self.stage.as_ref() {
            Some(Stage::Bare(source)) => source,
            Some(Stage::On(filter)) => filter,
            None => unreachable!("a stage is only taken while it is retuned"),
        }
    }
}

impl<S: Source> Iterator for Pass<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        self.retune();
        match self.stage.as_mut()? {
            Stage::Bare(source) => source.next(),
            Stage::On(filter) => filter.next(),
        }
    }
}

impl<S: Source> Source for Pass<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source().current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.source().channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.source().sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.source().total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        match self.stage.as_mut() {
            Some(Stage::Bare(source)) => source.try_seek(pos),
            Some(Stage::On(filter)) => filter.try_seek(pos),
            None => Ok(()),
        }
    }
}
