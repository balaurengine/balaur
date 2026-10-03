//! What one play is, independent of any output device: the files it reads,
//! what is done to their samples, and the fixed-step clock that says where it
//! has got to.
//!
//! The clock is the truth a script reads (`playback_time`, `is_playing`, the
//! `finished` event). It is advanced on the fixed step from the play's own
//! numbers, never read off a sink, so a run with no sound card and a replay
//! reach every boundary on the same tick as one with speakers.

use std::sync::Arc;

use crate::FileSettings;

/// One file a play reads: its encoded bytes and its own import settings.
#[derive(Clone)]
pub struct Clip {
    pub bytes: Arc<[u8]>,
    pub file: FileSettings,
}

impl Clip {
    #[must_use]
    pub fn new(bytes: Vec<u8>, file: FileSettings) -> Self {
        Self {
            bytes: bytes.into(),
            file,
        }
    }
}

/// Automatic gain control: rodio's `automatic_gain_control` with every knob it
/// takes, and the floor its filter adds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoGain {
    pub on: bool,
    pub target: f32,
    pub attack_time: f32,
    pub release_time: f32,
    pub max: f32,
    pub floor: f32,
}

impl Default for AutoGain {
    fn default() -> Self {
        Self {
            on: false,
            target: 1.0,
            attack_time: 4.0,
            release_time: 0.0,
            max: 7.0,
            floor: 0.0,
        }
    }
}

/// What a play does to its samples between the decoder and the player. Each
/// is one of rodio's adapters, kept here as numbers so a headless run reads
/// back what a device would be given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effects {
    /// Balance of a sound that is not positional: -1 left, 1 right.
    pub pan: f32,
    /// 0 is off.
    pub low_pass_hz: f32,
    pub low_pass_q: f32,
    /// 0 is off.
    pub high_pass_hz: f32,
    pub high_pass_q: f32,
    /// 0 is off.
    pub reverb_time: f32,
    pub reverb_level: f32,
    pub distortion_gain: f32,
    /// 0 is off.
    pub distortion_threshold: f32,
    pub auto_gain: AutoGain,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            pan: 0.0,
            low_pass_hz: 0.0,
            low_pass_q: DEFAULT_Q,
            high_pass_hz: 0.0,
            high_pass_q: DEFAULT_Q,
            reverb_time: 0.0,
            reverb_level: 0.0,
            distortion_gain: 1.0,
            distortion_threshold: 0.0,
            auto_gain: AutoGain::default(),
        }
    }
}

/// rodio's own Q for `low_pass` and `high_pass` (`blt.rs`).
pub const DEFAULT_Q: f32 = 0.5;

/// Everything a play needs to be built again from any point: a seek and a
/// skip start the files over from where the clock says, rather than asking a
/// decoder that may not seek.
#[derive(Clone)]
pub struct Program {
    pub main: Clip,
    /// Mixed with `main` for its whole length.
    pub layers: Vec<Clip>,
    /// Played after `main`, in order.
    pub queue: Vec<Clip>,
    /// The caller's loop; a file's own `loop` setting loops it as well.
    pub looped: bool,
    /// Where each repeat of `main` starts; `None` takes the file's own.
    pub loop_offset: Option<f32>,
    pub start_time: f32,
    /// 0 plays to the end.
    pub end_time: f32,
    pub delay: f32,
    pub fade_in_time: f32,
    pub effects: Effects,
}

impl Program {
    #[must_use]
    pub fn new(main: Clip) -> Self {
        Self {
            main,
            layers: Vec::new(),
            queue: Vec::new(),
            looped: false,
            loop_offset: None,
            start_time: 0.0,
            end_time: 0.0,
            delay: 0.0,
            fade_in_time: 0.0,
            effects: Effects::default(),
        }
    }

    /// How many files play one after another: `main`, then the queue.
    #[must_use]
    pub fn segments(&self) -> usize {
        1 + self.queue.len()
    }

    /// The file a segment reads.
    #[must_use]
    pub fn clip(&self, index: usize) -> &Clip {
        if index == 0 {
            &self.main
        } else {
            &self.queue[index - 1]
        }
    }

    /// Whether a segment repeats until something skips past it.
    #[must_use]
    pub fn looped(&self, index: usize) -> bool {
        self.looped || self.clip(index).file.looped
    }

    /// Where a segment's repeats start.
    #[must_use]
    pub fn loop_from(&self, index: usize) -> f32 {
        match (index, self.loop_offset) {
            (0, Some(offset)) => offset.max(0.0),
            _ => self.clip(index).file.loop_offset,
        }
    }

    /// Where a segment starts: `start_time` for `main`, the top for the rest.
    #[must_use]
    pub fn start(&self, index: usize) -> f32 {
        if index == 0 {
            self.start_time.max(0.0)
        } else {
            0.0
        }
    }

    /// Where a segment stops, given the file's length: `end_time` for `main`
    /// when set, else the file's end. `None` when the length is not known.
    #[must_use]
    pub fn end(&self, index: usize, length: Option<f64>) -> Option<f64> {
        let end = (index == 0 && self.end_time > 0.0).then_some(f64::from(self.end_time));
        match (end, length) {
            (Some(end), Some(length)) => Some(end.min(length)),
            (Some(end), None) => Some(end),
            (None, length) => length,
        }
    }
}

/// One file's stretch of the clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    /// `None` when the file's length is not known: it ends when its sink does.
    pub end: Option<f64>,
    pub looped: bool,
    pub loop_from: f64,
}

/// Where a play has got to, counted on the fixed step.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    /// Silence still to come before the first file.
    pub delay: f64,
    /// Seconds into the current file.
    pub at: f64,
    pub index: usize,
    pub segments: Vec<Segment>,
}

impl Timeline {
    /// A clock at the start of `program`, given each segment's file length.
    #[must_use]
    pub fn new(program: &Program, lengths: &[Option<f64>]) -> Self {
        let segments = (0..program.segments())
            .map(|index| Segment {
                start: f64::from(program.start(index)),
                end: program.end(index, lengths.get(index).copied().flatten()),
                looped: program.looped(index),
                loop_from: f64::from(program.loop_from(index)),
            })
            .collect::<Vec<_>>();
        Self {
            delay: f64::from(program.delay.max(0.0)),
            at: segments.first().map_or(0.0, |segment| segment.start),
            index: 0,
            segments,
        }
    }

    fn current(&self) -> Option<&Segment> {
        self.segments.get(self.index)
    }

    /// Move the clock on by `seconds` of the file's own time. True once the
    /// last file has played out.
    pub fn advance(&mut self, seconds: f64) -> bool {
        let mut step = seconds.max(0.0);
        let held = self.delay.min(step);
        self.delay -= held;
        step -= held;
        while step > 0.0 {
            let Some(segment) = self.current().copied() else {
                return true;
            };
            let Some(end) = segment.end else {
                self.at += step;
                return false;
            };
            let room = end - self.at;
            if step < room {
                self.at += step;
                return false;
            }
            step -= room;
            if segment.looped {
                let span = end - segment.loop_from.min(end);
                if span <= 0.0 {
                    self.at = end;
                    return false;
                }
                self.at = segment.loop_from.min(end) + step % span;
                return false;
            }
            if !self.next_file() {
                return true;
            }
        }
        false
    }

    /// Move to the top of the next file. False when there is none.
    pub fn next_file(&mut self) -> bool {
        self.index += 1;
        self.delay = 0.0;
        self.at = self.current().map_or(0.0, |segment| segment.start);
        self.index < self.segments.len()
    }

    /// Jump within the current file. A known end clamps it; a time that is
    /// not a number is the top.
    pub fn seek(&mut self, seconds: f64) {
        self.delay = 0.0;
        let end = self.current().and_then(|segment| segment.end);
        let at = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        self.at = end.map_or(at, |end| at.min(end));
    }

    /// Whether the clock, rather than the sink, decides when this ends: every
    /// file from here on has a known end. A looping one never ends.
    #[must_use]
    pub fn counted(&self) -> bool {
        self.segments[self.index.min(self.segments.len())..]
            .iter()
            .all(|segment| segment.end.is_some() || segment.looped)
    }

    /// Seconds of sound left before the last file plays out, when that is
    /// known: no file from here on loops and every one has an end.
    #[must_use]
    pub fn left(&self) -> Option<f64> {
        let rest = self.segments.get(self.index..)?;
        let mut left = self.delay;
        for (offset, segment) in rest.iter().enumerate() {
            if segment.looped {
                return None;
            }
            let from = if offset == 0 { self.at } else { segment.start };
            left += (segment.end? - from).max(0.0);
        }
        Some(left)
    }
}
