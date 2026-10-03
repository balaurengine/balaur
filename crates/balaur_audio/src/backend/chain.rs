//! A play's files turned into one rodio source: decoded, mixed with their
//! layers, trimmed, looped, queued, and passed through its effects.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use rodio::Source;
use rodio::source::AutomaticGainControlSettings;

use super::live::{self, Live, PERIOD, Pass, Side};
use crate::playback::{Clip, Effects, Program};

type Boxed = Box<dyn Source + Send>;

/// rodio caps automatic gain control's attack and release here (`source/mod.rs`).
const MAX_AUTO_GAIN_TIME: f32 = 10.0;

/// A decoder over a clip's bytes, with the file's decoder settings and level.
fn decoder(clip: &Clip) -> Result<rodio::Decoder<Cursor<Arc<[u8]>>>> {
    let len = clip.bytes.len() as u64;
    Ok(rodio::Decoder::builder()
        .with_data(Cursor::new(clip.bytes.clone()))
        .with_byte_len(len)
        .with_gapless(clip.file.gapless)
        .with_seekable(clip.file.seekable)
        .build()?)
}

/// How long a clip plays at speed 1, when its container says.
pub(crate) fn length_of(clip: &Clip) -> Option<f64> {
    decoder(clip)
        .ok()?
        .total_duration()
        .map(|length| length.as_secs_f64())
}

/// A span of seconds rodio can take: negative is none, too long is forever.
fn seconds(value: f64) -> Duration {
    Duration::try_from_secs_f64(value.max(0.0)).unwrap_or(Duration::MAX)
}

/// A clip decoded from `at` seconds in. A seekable file jumps there in its
/// container; any other is decoded up to it.
fn decoded_from(clip: &Clip, at: f64) -> Result<Boxed> {
    let level = clip.file.level;
    if at <= 0.0 {
        return Ok(Box::new(decoder(clip)?.amplify(level)));
    }
    if clip.file.seekable {
        let mut seeking = decoder(clip)?;
        if seeking.try_seek(seconds(at)).is_ok() {
            return Ok(Box::new(seeking.amplify(level)));
        }
    }
    Ok(Box::new(
        decoder(clip)?.skip_duration(seconds(at)).amplify(level),
    ))
}

/// One file of the program from `at` seconds in: `main` with its layers and
/// its end, or a queued file whole.
fn segment(program: &Program, index: usize, at: f64) -> Result<Boxed> {
    let clip = program.clip(index);
    let layers: &[Clip] = if index == 0 { &program.layers } else { &[] };
    let end = program.end(index, None);
    if program.looped(index) {
        let mut whole = decoded_from(clip, 0.0)?;
        for layer in layers {
            whole = Box::new(whole.mix(decoded_from(layer, 0.0)?));
        }
        let whole: Boxed = match end {
            Some(end) => Box::new(whole.take_duration(seconds(end))),
            None => whole,
        };
        // The first pass starts at `at`; every repeat after it at the loop's
        // own start, so an intro plays once.
        let whole = whole.buffered();
        let (queue, played) = rodio::queue::queue(false);
        queue.append(whole.clone().skip_duration(seconds(at)));
        queue.append(
            whole
                .skip_duration(seconds(f64::from(program.loop_from(index))))
                .repeat_infinite(),
        );
        return Ok(Box::new(played));
    }
    let mut mixed = decoded_from(clip, at)?;
    for layer in layers {
        mixed = Box::new(mixed.mix(decoded_from(layer, at)?));
    }
    Ok(match end {
        Some(end) => Box::new(mixed.take_duration(seconds(end - at))),
        None => mixed,
    })
}

/// Where a play starts: which file, how far into it, and whether this is the
/// play's first start, which is the only one that waits out its `delay` and
/// rises over its `fade_in_time`.
#[derive(Clone, Copy)]
pub(crate) struct From {
    pub index: usize,
    pub at: f64,
    pub fresh: bool,
}

/// The whole program as one source, from `from` on, through its effects and
/// its pan.
pub(super) fn source(
    program: &Program,
    from: From,
    live: &Arc<Live>,
    positional: bool,
) -> Result<Boxed> {
    let (queue, files) = rodio::queue::queue(false);
    for index in from.index..program.segments() {
        let at = if index == from.index {
            from.at
        } else {
            f64::from(program.start(index))
        };
        queue.append(segment(program, index, at)?);
    }
    let shaped = effects(Box::new(files), &program.effects, live);
    let placed: Boxed = if positional {
        Box::new(live::spread(shaped, live))
    } else {
        live::balance(shaped, live)
    };
    let faded: Boxed = if from.fresh && program.fade_in_time > 0.0 {
        Box::new(placed.fade_in(seconds(f64::from(program.fade_in_time))))
    } else {
        placed
    };
    Ok(if from.fresh && program.delay > 0.0 {
        Box::new(faded.delay(seconds(f64::from(program.delay))))
    } else {
        faded
    })
}

/// Automatic gain control, the two filters, the distortion and the reverb,
/// in that order: the level is evened out before anything shapes it, and the
/// echo carries what the rest made.
///
/// Gain control and reverb join only when on as the play starts: each holds a
/// buffer per play. The filters and the distortion are always in the chain, so
/// switching one on while it plays lands.
fn effects(source: Boxed, effects: &Effects, live: &Arc<Live>) -> Boxed {
    let levelled: Boxed = if effects.auto_gain.on {
        let agc = &effects.auto_gain;
        let settings = AutomaticGainControlSettings {
            target_level: agc.target,
            attack_time: seconds(f64::from(agc.attack_time)),
            release_time: seconds(f64::from(agc.release_time)),
            absolute_max_gain: agc.max,
        };
        let live = live.clone();
        Box::new(
            source
                .automatic_gain_control(settings)
                .periodic_access(PERIOD, move |agc| {
                    use std::sync::atomic::Ordering::Relaxed;
                    let rate = agc.sample_rate().get() as f32;
                    agc.set_enabled(live.auto_gain_on.load(Relaxed));
                    agc.set_floor(Some(live.auto_gain_floor.get()));
                    agc.get_target_level()
                        .store(live.auto_gain_target.get(), Relaxed);
                    agc.get_absolute_max_gain()
                        .store(live.auto_gain_max.get(), Relaxed);
                    agc.get_attack_coeff()
                        .store(coefficient(live.auto_gain_attack_time.get(), rate), Relaxed);
                    agc.get_release_coeff().store(
                        coefficient(live.auto_gain_release_time.get(), rate),
                        Relaxed,
                    );
                }),
        )
    } else {
        source
    };
    let filtered = Pass::new(Pass::new(levelled, live, Side::Low), live, Side::High);
    let distorted = {
        let live = live.clone();
        filtered
            .distortion(1.0, f32::MAX)
            .periodic_access(PERIOD, move |distortion| {
                let threshold = live.distortion_threshold.get();
                if threshold > 0.0 {
                    distortion.set_gain(live.distortion_gain.get());
                    distortion.set_threshold(threshold);
                } else {
                    distortion.set_gain(1.0);
                    distortion.set_threshold(f32::MAX);
                }
            })
    };
    if effects.reverb_time <= 0.0 {
        return Box::new(distorted);
    }
    // rodio's `reverb`, composed by hand so the echo's level stays live: the
    // echo is the same samples, delayed and scaled.
    let dry = distorted.buffered();
    let live = live.clone();
    let wet = dry
        .clone()
        .amplify(effects.reverb_level)
        .delay(seconds(f64::from(effects.reverb_time)))
        .periodic_access(PERIOD, move |echo| {
            echo.inner_mut().set_factor(live.reverb_level.get());
        });
    Box::new(dry.mix(wet))
}

/// The smoothing coefficient automatic gain control keeps for a time
/// constant: rodio's `duration_to_coefficient`, which it does not export.
fn coefficient(seconds: f32, rate: f32) -> f32 {
    let seconds = seconds.clamp(0.0, MAX_AUTO_GAIN_TIME);
    libm::expf(-1.0 / (seconds * rate))
}
