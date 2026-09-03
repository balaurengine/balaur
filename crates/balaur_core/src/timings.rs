//! What the last frame cost, per stage and per named span.
//!
//! An observer, exactly as rendering is: nothing here may feed back into the
//! simulation. Wall time is not reproducible, so a `fixed_update` that
//! branched on it would desync — and would be caught by the digest at the
//! first tick that parted, since no timing is recorded, replayed or hashed.
//! Reading them from `update`, a tool, or a profiler dock is what they exist
//! for.
//!
//! Measurement is nine `Instant::now()` calls a frame, one per stage plus the
//! frame itself, which is beneath the noise of the work being measured. Named
//! spans cost one more per span, and only a plugin that asks for them pays.

// Measuring wall time is what this module is for; the doc above is the
// argument that none of it reaches the simulation.
#![allow(
    clippy::disallowed_methods,
    reason = "an observer, never a simulation input"
)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use crate::engine::Engine;

/// The stages, in the order [`crate::app::Stage`] declares them, so a table
/// reads the way a frame runs.
pub const STAGE_NAMES: [&str; 8] = [
    "first",
    "pre_update",
    "update",
    "fixed_update",
    "post_update",
    "scene_sync",
    "render",
    "last",
];

/// The last completed frame, and what it is worth against a 60 Hz budget.
///
/// Published whole at the end of every frame rather than written as the frame
/// runs, so a reader never sees half of one.
#[derive(Default, Clone)]
pub struct Timings {
    /// Wall time of the whole frame, stages and everything between them.
    pub frame: Duration,
    /// Per stage, in `STAGE_NAMES` order.
    pub stages: [Duration; 8],
    /// How many fixed steps the accumulator drained. Zero is normal on a fast
    /// frame and the reason `fixed_update` can read as free.
    pub fixed_steps: u32,
    /// What plugins measured by name this frame, in the order they finished.
    pub spans: Vec<(String, Duration)>,
    /// Spans of the frame in progress, moved into `spans` when it ends.
    pending: Vec<(String, Duration)>,
}

impl Timings {
    /// A stage's share of one 60 Hz frame, as a fraction: 0.5 is half the
    /// budget. The number a profiler colours and a budget table compares.
    #[must_use]
    pub fn share(duration: Duration) -> f64 {
        duration.as_secs_f64() / f64::from(crate::app::FIXED_DT)
    }
}

/// Time `body` and file it under `name` for this frame.
///
/// The seam for anything finer than a stage: a plugin wraps the work it wants
/// named, and nothing else has to know the name exists. Re-using a name in one
/// frame records it twice, which is what a system that runs per fixed step
/// should look like.
pub fn measure<T>(eng: &Engine, name: &str, body: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = body();
    let elapsed = started.elapsed();
    if let Some(timings) = eng.try_resource::<Timings>() {
        timings
            .borrow_mut()
            .pending
            .push((name.to_string(), elapsed));
    }
    out
}

/// Publish the frame that just ended. Called by `App::tick` and nobody else.
pub(crate) fn publish(eng: &Engine, frame: Duration, stages: [Duration; 8], fixed_steps: u32) {
    let Some(timings) = eng.try_resource::<Timings>() else {
        return;
    };
    let mut timings = timings.borrow_mut();
    timings.frame = frame;
    timings.stages = stages;
    timings.fixed_steps = fixed_steps;
    timings.spans = std::mem::take(&mut timings.pending);
}

/// The last frame, as a script sees it: seconds, because every other duration
/// a script reads is in seconds.
pub fn table(eng: &Engine) -> balaur_script::Value {
    use balaur_script::Value;
    let Some(timings) = eng.try_resource::<Timings>() else {
        return Value::Nil;
    };
    let timings = timings.borrow();
    let seconds = |d: Duration| Value::Num(d.as_secs_f64());
    let stages = STAGE_NAMES
        .iter()
        .zip(timings.stages)
        .map(|(name, d)| ((*name).to_string(), seconds(d)))
        .collect();
    // Spans repeat when a system ran more than once, so they are summed by
    // name: a caller wants "physics cost 4 ms", not four rows of one.
    let mut spans: Vec<(String, Duration)> = Vec::new();
    for (name, elapsed) in &timings.spans {
        match spans.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 += *elapsed,
            None => spans.push((name.clone(), *elapsed)),
        }
    }
    Value::Map(vec![
        ("frame".to_string(), seconds(timings.frame)),
        (
            "fixed_steps".to_string(),
            Value::Int(i64::from(timings.fixed_steps)),
        ),
        ("stages".to_string(), Value::Map(stages)),
        (
            "spans".to_string(),
            Value::Map(
                spans
                    .into_iter()
                    .map(|(name, d)| (name, seconds(d)))
                    .collect(),
            ),
        ),
    ])
}

/// Running totals over a whole run, for the summary `--timings` prints.
///
/// A single frame says nothing: the first is cold, and a stall shows up in one
/// frame out of six hundred. The mean is what a budget is set against and the
/// worst is what a player feels.
#[derive(Default)]
pub struct TimingLog {
    frames: u64,
    frame_total: Duration,
    frame_worst: Duration,
    stage_totals: [Duration; 8],
    stage_worst: [Duration; 8],
    spans: Vec<(String, Duration, Duration)>,
}

impl TimingLog {
    /// Fold the last frame in. Called once per frame while `--timings` is on.
    pub fn observe(&mut self, timings: &Timings) {
        self.frames += 1;
        self.frame_total += timings.frame;
        self.frame_worst = self.frame_worst.max(timings.frame);
        for (i, stage) in timings.stages.iter().enumerate() {
            self.stage_totals[i] += *stage;
            self.stage_worst[i] = self.stage_worst[i].max(*stage);
        }
        for (name, elapsed) in &timings.spans {
            match self.spans.iter_mut().find(|(n, _, _)| n == name) {
                Some(slot) => {
                    slot.1 += *elapsed;
                    slot.2 = slot.2.max(*elapsed);
                }
                None => self.spans.push((name.clone(), *elapsed, *elapsed)),
            }
        }
    }

    /// The table `--timings` prints: mean, worst and the mean's share of a
    /// 60 Hz frame, per stage and per span, ordered by cost.
    #[must_use]
    pub fn report(&self) -> String {
        if self.frames == 0 {
            return String::from("no frames ran, so there is nothing to report\n");
        }
        let frames = u32::try_from(self.frames).unwrap_or(u32::MAX);
        let mut rows: Vec<(String, Duration, Duration)> = STAGE_NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                (
                    (*name).to_string(),
                    self.stage_totals[i] / frames,
                    self.stage_worst[i],
                )
            })
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.1));
        for (name, total, worst) in &self.spans {
            rows.push((format!("  {name}"), *total / frames, *worst));
        }
        let width = rows.iter().map(|r| r.0.len()).max().unwrap_or(0).max(6);
        let mut out = format!(
            "\n{:width$}  {:>9}  {:>9}  {:>9}\n",
            "stage", "mean", "worst", "of frame"
        );
        for (name, mean, worst) in &rows {
            let _ = writeln!(
                out,
                "{:width$}  {:>9}  {:>9}  {:>8.1}%",
                name,
                millis(*mean),
                millis(*worst),
                100.0 * Timings::share(*mean),
            );
        }
        let _ = writeln!(
            out,
            "{:width$}  {:>9}  {:>9}  {:>8.1}%   over {} frames",
            "frame",
            millis(self.frame_total / frames),
            millis(self.frame_worst),
            100.0 * Timings::share(self.frame_total / frames),
            self.frames,
        );
        out
    }
}

fn millis(d: Duration) -> String {
    format!("{:.3} ms", d.as_secs_f64() * 1000.0)
}
