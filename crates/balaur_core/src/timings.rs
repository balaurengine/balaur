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
//!
//! [`Timings::stages`] covers the stages and nothing else: a windowed loop
//! mirrors the world into its scene graph, draws the UI and presents outside
//! them, which is most of a frame in the editor. That work reports itself
//! through [`record`] and [`note_wall`], and lands in the next frame's table
//! because a span filed after the stages ran misses the publish that ended
//! them.

// Measuring wall time is what this module is for; the doc above is the
// argument that none of it reaches the simulation.
#![allow(
    clippy::disallowed_methods,
    reason = "an observer, never a simulation input"
)]

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::time::Duration;

use smol_str::SmolStr;

use crate::engine::Engine;
use crate::time::Instant;
use crate::vocabulary::keys;
use crate::vocabulary::words;

const FRAME: &str = "frame";
const WALL: &str = "wall";
/// The gauges the engine samples itself while a run's timings are logged.
const NODES: &str = "nodes";
/// A run's slowest: a column of the report and a point of its spread.
const WORST: &str = "worst";
const MEMORY: &str = "memory";
/// A stage's cost no span inside it accounts for, as a spike's cause says it.
const OUTSIDE_SPANS: &str = ", outside a span";

thread_local! {
    /// What the run spent before its first frame, summed by name in the order
    /// the spans finished.
    ///
    /// Process-wide rather than a resource on the engine: the engine is one of
    /// the things being measured, and the first span is filed before there is
    /// an engine to file it in.
    static BOOT: RefCell<Vec<(SmolStr, Duration)>> = const { RefCell::new(Vec::new()) };
    /// When the process started, as the tool that owns `main` measured it.
    static STARTED: RefCell<Option<Instant>> = const { RefCell::new(None) };
    /// Whether a loop here draws, so the boot ends at a picture and not at a tick.
    static DRAWS: Cell<bool> = const { Cell::new(false) };
    /// Shaders built after the first picture: modules, pipelines and the time.
    static LATER_SHADERS: Cell<(u32, u32, Duration)> = const { Cell::new((0, 0, Duration::ZERO)) };
}

/// The name the first frame's own row carries.
const TO_FIRST_FRAME: &str = "to first frame";

/// Note that the process has started, for the row the first frame files.
pub fn mark_start() {
    STARTED.with_borrow_mut(|at| *at = Some(Instant::now()));
}

/// Time `body` and file it as boot work under `name`.
pub fn boot<T>(name: &str, body: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = body();
    note_boot(name, started.elapsed());
    out
}

/// Time `body` and file it under `name` only when it answers: a lookup that is
/// a phase of the boot when it hits and noise when it misses.
pub fn boot_hit<T>(name: &str, body: impl FnOnce() -> Option<T>) -> Option<T> {
    let started = Instant::now();
    let out = body();
    if out.is_some() {
        note_boot(name, started.elapsed());
    }
    out
}

/// A boot phase in progress, for work no closure can wrap: an awaited window,
/// or anything else between two statements rather than inside one call.
pub struct Phase(Instant);

impl Phase {
    #[must_use]
    pub fn start() -> Self {
        Self(Instant::now())
    }

    /// What has passed since [`Phase::start`].
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}

/// File boot work already measured, for a phase no closure can wrap.
pub fn note_boot(name: &str, elapsed: Duration) {
    BOOT.with_borrow_mut(
        |spans| match spans.iter_mut().find(|(n, _)| n.as_str() == name) {
            Some(slot) => slot.1 += elapsed,
            None => spans.push((name.into(), elapsed)),
        },
    );
}

/// The boot's phases in the order they ran, each summed by name.
#[must_use]
pub fn boot_phases() -> Vec<(SmolStr, Duration)> {
    BOOT.with_borrow(Clone::clone)
}

/// The table `--timings` prints for the boot, empty when nothing measured one.
///
/// A sequence rather than a ranking: boot phases run in order, and what a
/// reader wants is where the time went on the way to the first frame.
#[must_use]
pub fn boot_report() -> String {
    let spans = BOOT.with_borrow(Clone::clone);
    if spans.is_empty() {
        return String::new();
    }
    let width = spans.iter().map(|(n, _)| n.len()).max().unwrap_or(0).max(5);
    let mut out = format!("\n{:width$}  {:>9}\n", "boot", "cost");
    for (name, cost) in &spans {
        let _ = writeln!(out, "{name:width$}  {:>9}", millis(*cost));
    }
    let (modules, pipelines, time) = LATER_SHADERS.get();
    if modules + pipelines > 0 {
        let _ = writeln!(
            out,
            "later shaders: {modules} modules, {pipelines} pipelines, {}",
            millis(time)
        );
    }
    out
}

/// Say a loop on this thread draws. Its boot then ends when the first frame is on
/// the screen, which it says with [`note_first_frame`]: a frame's shaders are built
/// as it draws, and on a first launch those are the boot's largest cost.
pub fn draws_pictures() {
    DRAWS.set(true);
}

/// Add shaders built after the first picture to the boot table's last row. Each
/// one was a stall on the frame that asked for it.
pub fn note_later_shaders(modules: u32, pipelines: u32, time: Duration) {
    let (modules_so_far, pipelines_so_far, time_so_far) = LATER_SHADERS.get();
    LATER_SHADERS.set((
        modules_so_far + modules,
        pipelines_so_far + pipelines,
        time_so_far + time,
    ));
}

/// File the first frame's own row, once: when a loop that draws has drawn it, or
/// when a frame publishes where nothing draws.
pub fn note_first_frame() {
    let Some(started) = STARTED.with_borrow(|at| *at) else {
        return;
    };
    let filed = BOOT.with_borrow(|spans| spans.iter().any(|(n, _)| n == TO_FIRST_FRAME));
    if !filed {
        note_boot(TO_FIRST_FRAME, started.elapsed());
    }
}

/// The stages, in the order [`crate::app::Stage`] declares them, so a table
/// reads the way a frame runs.
pub const STAGE_NAMES: [&str; 8] = [
    keys::FIRST,
    "pre_update",
    "update",
    crate::hooks::FIXED_UPDATE,
    "post_update",
    "scene_sync",
    "render",
    keys::LAST,
];

/// The last completed frame, and what it is worth against a 60 Hz budget.
///
/// Published whole at the end of every frame rather than written as the frame
/// runs, so a reader never sees half of one.
#[derive(Default, Clone)]
pub struct Timings {
    /// Wall time of the whole frame, stages and everything between them.
    pub frame: Duration,
    /// Frame to frame as the loop driving the window measured it: the stages,
    /// the syncs, the UI pass, present and the wait for the next frame. Noted
    /// before a tick, so inside one it is the period [`Timings::frame`] ran in.
    /// Zero under a driver that reports none, such as a headless run.
    pub wall: Duration,
    /// Per stage, in `STAGE_NAMES` order.
    pub stages: [Duration; 8],
    /// How many fixed steps the accumulator drained. Zero is normal on a fast
    /// frame and the reason `fixed_update` can read as free.
    pub fixed_steps: u32,
    /// What plugins measured by name this frame, in the order they finished.
    /// Inline strings: every name is a short literal, and building one a span
    /// a frame was an allocation for a number nothing reads most frames.
    pub spans: Vec<(SmolStr, Duration)>,
    /// Spans of the frame in progress, moved into `spans` when it ends.
    pending: Vec<(SmolStr, Duration)>,
    /// The CPU time every thread of the process spent since the last frame
    /// ended; against [`Timings::interval`], how many cores the frame kept busy. Zero where
    /// the platform does not say ([`process_cpu`]).
    pub cpu: Duration,
    /// Each span's CPU time, every thread's, while [`count_cpu`] is on: what a
    /// span that fans out to the thread pool spent against its wall time.
    pub span_cpu: Vec<(SmolStr, Duration)>,
    pending_cpu: Vec<(SmolStr, Duration)>,
    /// Levels read as the frame ended, by name, the last one filed winning:
    /// the nodes alive and the memory the process holds, while [`sample_gauges`]
    /// is on, and whatever a plugin counts ([`gauge`]).
    pub gauges: Vec<(SmolStr, f64, Unit)>,
    pending_gauges: Vec<(SmolStr, f64, Unit)>,
    /// The process's CPU time when the last frame ended.
    cpu_at: Option<Duration>,
    /// Wall time since the frame before was published: the window
    /// [`Timings::cpu`] was counted over.
    pub interval: Duration,
    /// When the last frame was published.
    published_at: Option<Instant>,
}

/// Whether spans count their CPU time as well, which costs a system call
/// each: on while a run's timings are being logged.
static COUNT_CPU: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether each frame reads the engine's own gauges, the nodes and the
/// memory: a count and a system call, on while a run's timings are logged.
static SAMPLE_GAUGES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What a gauge's number counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Things: nodes, widgets, draw calls.
    Count,
    /// Bytes, printed in megabytes.
    Bytes,
}

impl Unit {
    fn word(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Bytes => "bytes",
        }
    }

    fn show(self, value: f64) -> String {
        match self {
            Self::Count => format!("{value:.0}"),
            Self::Bytes => format!("{:.1} MB", value / (1024.0 * 1024.0)),
        }
    }
}

/// Read the engine's own gauges every frame from now on, for [`TimingLog`]'s
/// report: the nodes alive and the memory the process holds.
pub fn sample_gauges() {
    SAMPLE_GAUGES.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// File `value` as this frame's level of `name`: a count a plugin keeps,
/// widgets drawn or draw calls. Filed twice in a frame, the last one stands.
pub fn gauge(eng: &Engine, name: &str, value: f64, unit: Unit) {
    if let Some(timings) = eng.try_resource::<Timings>() {
        file_gauge(&mut timings.borrow_mut().pending_gauges, name, value, unit);
    }
}

fn file_gauge(gauges: &mut Vec<(SmolStr, f64, Unit)>, name: &str, value: f64, unit: Unit) {
    match gauges.iter_mut().find(|g| g.0 == name) {
        Some(slot) => {
            slot.1 = value;
            slot.2 = unit;
        }
        None => gauges.push((name.into(), value, unit)),
    }
}

/// The memory the process holds now, its resident set, in bytes; `None`
/// where the platform does not say.
#[must_use]
pub fn process_memory() -> Option<u64> {
    memory::resident()
}

/// Count every span's CPU time from now on, for [`TimingLog`]'s report.
pub fn count_cpu() {
    COUNT_CPU.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The CPU time every thread of the process has spent so far, as the
/// operating system counts it (`cpu-time`); `None` where it does not.
#[must_use]
pub fn process_cpu() -> Option<Duration> {
    #[cfg(not(target_family = "wasm"))]
    {
        cpu_time::ProcessTime::try_now()
            .ok()
            .map(|time| time.as_duration())
    }
    #[cfg(target_family = "wasm")]
    {
        None
    }
}

impl Timings {
    /// A stage's share of one 60 Hz frame, as a fraction: 0.5 is half the
    /// budget. The number a profiler colours and a budget table compares.
    #[must_use]
    pub fn share(duration: Duration) -> f64 {
        duration.as_secs_f64() / f64::from(crate::fixed_dt())
    }
}

/// File `elapsed` under `name`, for work timed somewhere a closure cannot
/// wrap — an awaited render, or a cost the window reports after the fact.
pub fn record(eng: &Engine, name: &str, elapsed: Duration) {
    if let Some(timings) = eng.try_resource::<Timings>() {
        timings.borrow_mut().pending.push((name.into(), elapsed));
    }
}

/// Note the frame period the loop measured, for [`Timings::wall`].
pub fn note_wall(eng: &Engine, frame: Duration) {
    if let Some(timings) = eng.try_resource::<Timings>() {
        timings.borrow_mut().wall = frame;
    }
}

/// Time `body` and file it under `name` for this frame.
///
/// The seam for anything finer than a stage: a plugin wraps the work it wants
/// named, and nothing else has to know the name exists. Re-using a name in one
/// frame records it twice, which is what a system that runs per fixed step
/// should look like.
pub fn measure<T>(eng: &Engine, name: &str, body: impl FnOnce() -> T) -> T {
    let counting = COUNT_CPU.load(std::sync::atomic::Ordering::Relaxed);
    let cpu = counting.then(process_cpu).flatten();
    let started = Instant::now();
    let out = body();
    record(eng, name, started.elapsed());
    if let (Some(before), Some(after)) = (cpu, counting.then(process_cpu).flatten())
        && let Some(timings) = eng.try_resource::<Timings>()
    {
        timings
            .borrow_mut()
            .pending_cpu
            .push((name.into(), after.saturating_sub(before)));
    }
    out
}

/// Publish the frame that just ended. Called by `App::tick` and nobody else.
pub(crate) fn publish(eng: &Engine, frame: Duration, stages: [Duration; 8], fixed_steps: u32) {
    if !DRAWS.get() {
        note_first_frame();
    }
    let Some(timings) = eng.try_resource::<Timings>() else {
        return;
    };
    // Read before the table is borrowed: a count of the world is a borrow
    // of its own.
    let sampled = SAMPLE_GAUGES
        .load(std::sync::atomic::Ordering::Relaxed)
        .then(|| (eng.world().len(), process_memory()));
    let mut timings = timings.borrow_mut();
    if let Some((nodes, memory)) = sampled {
        file_gauge(
            &mut timings.pending_gauges,
            NODES,
            f64::from(nodes),
            Unit::Count,
        );
        if let Some(bytes) = memory {
            file_gauge(
                &mut timings.pending_gauges,
                MEMORY,
                bytes as f64,
                Unit::Bytes,
            );
        }
    }
    timings.gauges = std::mem::take(&mut timings.pending_gauges);
    timings.frame = frame;
    // Not `wall`: the loop writes that, and the last one it measured stands
    // until it measures another.
    timings.stages = stages;
    timings.fixed_steps = fixed_steps;
    timings.spans = std::mem::take(&mut timings.pending);
    timings.span_cpu = std::mem::take(&mut timings.pending_cpu);
    let now = process_cpu();
    timings.cpu = match (timings.cpu_at, now) {
        (Some(before), Some(now)) => now.saturating_sub(before),
        _ => Duration::ZERO,
    };
    timings.cpu_at = now;
    let at = Instant::now();
    timings.interval = timings
        .published_at
        .map_or(Duration::ZERO, |before| at.duration_since(before));
    timings.published_at = Some(at);
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
    let mut spans: Vec<(SmolStr, Duration)> = Vec::new();
    for (name, elapsed) in &timings.spans {
        match spans.iter_mut().find(|(n, _)| n == name.as_str()) {
            Some(slot) => slot.1 += *elapsed,
            None => spans.push((SmolStr::new(name), *elapsed)),
        }
    }
    Value::Map(vec![
        (FRAME.to_string(), seconds(timings.frame)),
        (WALL.to_string(), seconds(timings.wall)),
        (
            "fixed_steps".to_string(),
            Value::Int(i64::from(timings.fixed_steps)),
        ),
        ("stages".to_string(), Value::Map(stages)),
        (
            "spans".to_string(),
            Value::List(
                spans
                    .into_iter()
                    .map(|(name, d)| {
                        Value::Map(vec![
                            (
                                words::scene_toml::NAME.to_string(),
                                Value::text(name.as_str()),
                            ),
                            ("seconds".to_string(), seconds(d)),
                        ])
                    })
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
    wall_total: Duration,
    wall_worst: Duration,
    stage_totals: [Duration; 8],
    stage_worst: [Duration; 8],
    spans: Vec<SpanTotal>,
    /// Every frame, for the distribution and the spikes in it.
    marks: Vec<Mark>,
    /// CPU time and wall time of every span that counted its CPU, by name.
    span_cpu: Vec<(String, Duration, Duration)>,
    /// Every gauge's run, by name, in the order each was first seen.
    gauges: Vec<GaugeRun>,
}

/// One gauge over a run: where it started, its peak and when, where it ended,
/// and its steepest one-frame climb, which is where a leak or a burst is.
struct GaugeRun {
    name: String,
    unit: Unit,
    first: f64,
    peak: (f64, u64),
    last: f64,
    rise: (f64, u64),
}

/// A frame that stood out of a run's spread: its number, how long it took,
/// the CPU the process spent in it (a frame it mostly waited through was the
/// machine's, not the game's), and what cost it the most.
struct Spike {
    frame: usize,
    period: Duration,
    cpu: Duration,
    cause: String,
    cost: Duration,
}

/// One frame as the spike report reads it: how long it took, how much CPU
/// it kept busy, and what in it cost the most.
struct Mark {
    period: Duration,
    /// [`Timings::cpu`] and the [`Timings::interval`] it was spent over.
    cpu: Duration,
    interval: Duration,
    costliest: (SmolStr, Duration),
}

/// One span's totals over a run. `calls` is what tells a pass that runs twice
/// a frame from one that costs twice as much.
struct SpanTotal {
    name: String,
    total: Duration,
    worst: Duration,
    calls: u64,
}

impl TimingLog {
    /// Fold the last frame in. Called once per frame while `--timings` is on.
    pub fn observe(&mut self, timings: &Timings) {
        // The loop notes the period these stages ran in before the next tick:
        // frame to frame where it measured one, the stages where nothing did.
        self.marks.push(Mark {
            period: timings.frame.max(timings.wall),
            cpu: timings.cpu,
            interval: timings.interval,
            costliest: costliest(timings),
        });
        for (name, cpu) in &timings.span_cpu {
            let wall = timings
                .spans
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, d)| *d)
                .sum();
            match self.span_cpu.iter_mut().find(|s| s.0 == name.as_str()) {
                Some(slot) => {
                    slot.1 += *cpu;
                    slot.2 += wall;
                }
                None => self.span_cpu.push((name.to_string(), *cpu, wall)),
            }
        }
        for (name, value, unit) in &timings.gauges {
            let frame = self.frames;
            match self.gauges.iter_mut().find(|g| g.name == name.as_str()) {
                Some(run) => {
                    if *value > run.peak.0 {
                        run.peak = (*value, frame);
                    }
                    if *value - run.last > run.rise.0 {
                        run.rise = (*value - run.last, frame);
                    }
                    run.last = *value;
                }
                None => self.gauges.push(GaugeRun {
                    name: name.to_string(),
                    unit: *unit,
                    first: *value,
                    peak: (*value, frame),
                    last: *value,
                    rise: (0.0, frame),
                }),
            }
        }
        self.frames += 1;
        self.frame_total += timings.frame;
        self.frame_worst = self.frame_worst.max(timings.frame);
        self.wall_total += timings.wall;
        self.wall_worst = self.wall_worst.max(timings.wall);
        for (i, stage) in timings.stages.iter().enumerate() {
            self.stage_totals[i] += *stage;
            self.stage_worst[i] = self.stage_worst[i].max(*stage);
        }
        for (name, elapsed) in &timings.spans {
            match self.spans.iter_mut().find(|s| s.name == name.as_str()) {
                Some(slot) => {
                    slot.total += *elapsed;
                    slot.worst = slot.worst.max(*elapsed);
                    slot.calls += 1;
                }
                None => self.spans.push(SpanTotal {
                    name: name.to_string(),
                    total: *elapsed,
                    worst: *elapsed,
                    calls: 1,
                }),
            }
        }
    }

    /// The table `--timings` prints: mean, worst and the mean's share of a
    /// 60 Hz frame, per stage and per span, ordered by cost, and how many
    /// times a frame each span ran.
    #[must_use]
    pub fn report(&self) -> String {
        if self.frames == 0 {
            return String::from("no frames ran, so there is nothing to report\n");
        }
        let frames = u32::try_from(self.frames).unwrap_or(u32::MAX);
        let mut rows: Vec<(String, Duration, Duration, String)> = STAGE_NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                (
                    (*name).to_string(),
                    self.stage_totals[i] / frames,
                    self.stage_worst[i],
                    String::new(),
                )
            })
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.1));
        for span in &self.spans {
            let calls = span.calls as f64 / self.frames as f64;
            rows.push((
                format!("  {}", span.name),
                span.total / frames,
                span.worst,
                format!("{calls:.2}"),
            ));
        }
        let width = rows.iter().map(|r| r.0.len()).max().unwrap_or(0).max(6);
        let mut out = format!(
            "\n{:width$}  {:>9}  {:>9}  {:>9}  {:>7}\n",
            "stage", "mean", WORST, "of frame", "a frame"
        );
        for (name, mean, worst, calls) in &rows {
            let line = format!(
                "{:width$}  {:>9}  {:>9}  {:>8.1}%  {:>7}",
                name,
                millis(*mean),
                millis(*worst),
                100.0 * Timings::share(*mean),
                calls,
            );
            let _ = writeln!(out, "{}", line.trim_end());
        }
        let _ = writeln!(
            out,
            "{:width$}  {:>9}  {:>9}  {:>8.1}%   over {} frames",
            FRAME,
            millis(self.frame_total / frames),
            millis(self.frame_worst),
            100.0 * Timings::share(self.frame_total / frames),
            self.frames,
        );
        if self.wall_total > Duration::ZERO {
            let _ = writeln!(
                out,
                "{:width$}  {:>9}  {:>9}  {:>8.1}%   frame to frame",
                WALL,
                millis(self.wall_total / frames),
                millis(self.wall_worst),
                100.0 * Timings::share(self.wall_total / frames),
            );
        }
        out.push_str(&self.distribution());
        out.push_str(&self.gauge_table());
        out
    }

    /// Each gauge's first, peak and last levels, and its steepest climb.
    fn gauge_table(&self) -> String {
        if self.gauges.is_empty() {
            return String::new();
        }
        let width = self
            .gauges
            .iter()
            .map(|g| g.name.len())
            .max()
            .unwrap_or(0)
            .max(6);
        let mut out = format!(
            "\n{:width$}  {:>10}  {:>22}  {:>10}  {:>22}\n",
            "gauge", "first", "peak", "last", "steepest rise"
        );
        for g in &self.gauges {
            let line = format!(
                "{:width$}  {:>10}  {:>22}  {:>10}  {:>22}",
                g.name,
                g.unit.show(g.first),
                format!("{} (frame {})", g.unit.show(g.peak.0), g.peak.1),
                g.unit.show(g.last),
                format!("+{} (frame {})", g.unit.show(g.rise.0), g.rise.1),
            );
            let _ = writeln!(out, "{}", line.trim_end());
        }
        out
    }

    /// The run as data, for a tool that holds a budget: the boot's phases,
    /// each stage's and span's mean and worst, the frames' spread and spikes,
    /// the cores kept busy and every gauge, durations in milliseconds.
    #[must_use]
    pub fn json(&self) -> serde_json::Value {
        use serde_json::{Map, Value, json};
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let frames = u32::try_from(self.frames).unwrap_or(u32::MAX).max(1);
        let boot: Map<String, Value> = boot_phases()
            .into_iter()
            .map(|(name, cost)| (name.to_string(), json!(ms(cost))))
            .collect();
        let mut stages = Map::new();
        for (i, name) in STAGE_NAMES.iter().enumerate() {
            stages.insert(
                (*name).to_string(),
                json!({ "mean": ms(self.stage_totals[i] / frames), "worst": ms(self.stage_worst[i]) }),
            );
        }
        let spans: Map<String, Value> = self
            .spans
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    json!({
                        "mean": ms(s.total / frames),
                        "worst": ms(s.worst),
                        "calls": s.calls as f64 / f64::from(frames),
                    }),
                )
            })
            .collect();
        let gauges: Map<String, Value> = self
            .gauges
            .iter()
            .map(|g| {
                (
                    g.name.clone(),
                    json!({
                        "unit": g.unit.word(),
                        "first": g.first,
                        "peak": g.peak.0,
                        "peak_frame": g.peak.1,
                        "last": g.last,
                        "rise": g.rise.0,
                        "rise_frame": g.rise.1,
                    }),
                )
            })
            .collect();
        let (spread, spikes) = self.spread();
        json!({
            "frames": self.frames,
            "boot": boot,
            "frame": { "mean": ms(self.frame_total / frames), "worst": ms(self.frame_worst) },
            "spread": spread.iter().map(|(k, d)| ((*k).to_string(), json!(ms(*d)))).collect::<Map<String, Value>>(),
            "spikes": spikes.iter().map(|spike| json!({
                "frame": spike.frame, "ms": ms(spike.period), "cpu_ms": ms(spike.cpu),
                "cause": spike.cause, "cause_ms": ms(spike.cost),
            })).collect::<Vec<Value>>(),
            "cpu": self.cores().map_or(Value::Null, |(average, busiest)| json!({ "average": average, "busiest": busiest })),
            "stages": stages,
            "spans": spans,
            "gauges": gauges,
        })
    }

    /// The frame times' spread, the frames that stood out of it and what each
    /// spent its time on, and how many cores the run kept busy.
    fn distribution(&self) -> String {
        let (spread, spikes) = self.spread();
        let mut out = String::new();
        let _ = writeln!(
            out,
            "\nframes  p50 {}  p95 {}  p99 {}  worst {}",
            millis(spread[0].1),
            millis(spread[1].1),
            millis(spread[2].1),
            millis(spread[3].1),
        );
        let _ = writeln!(out, "spikes  {} over twice the median", spikes.len());
        for spike in spikes.iter().take(8) {
            // A frame the process sat out most of was the machine's, not the
            // game's: its CPU says how much of it was work.
            let waited = if !spike.cpu.is_zero() && spike.cpu * 2 < spike.period {
                format!(", {} of it on the CPU", millis(spike.cpu))
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "  frame {}: {}{waited}, most in {} ({})",
                spike.frame,
                millis(spike.period),
                spike.cause,
                millis(spike.cost),
            );
        }
        if let Some((average, busiest)) = self.cores() {
            let _ = writeln!(
                out,
                "cpu     {average:.2} cores busy on average, {busiest:.2} in the busiest frame",
            );
        }
        let step = self.cpu_step();
        for (name, cpu, wall) in &self.span_cpu {
            if !wall.is_zero() && *wall >= step * 20 {
                let _ = writeln!(
                    out,
                    "  {name}: {:.2} cores busy while it ran",
                    cpu.as_secs_f64() / wall.as_secs_f64()
                );
            }
        }
        out
    }

    /// The frame times at p50, p95, p99 and worst, and every frame over twice
    /// the median, slowest first: its number, its period, and what cost it
    /// the most with that cost.
    fn spread(&self) -> ([(&'static str, Duration); 4], Vec<Spike>) {
        let mut periods: Vec<Duration> = self.marks.iter().map(|m| m.period).collect();
        periods.sort_unstable();
        let at = |share: f64| {
            let last = periods.len().saturating_sub(1);
            periods
                .get(((last as f64) * share).round() as usize)
                .copied()
                .unwrap_or_default()
        };
        let median = at(0.5);
        let spread = [
            ("p50", median),
            ("p95", at(0.95)),
            ("p99", at(0.99)),
            (WORST, at(1.0)),
        ];
        // Twice the median and a millisecond over it: a frame a player could
        // feel, not one a fast frame's noise doubles.
        let over = median * 2;
        let mut spikes: Vec<(usize, &Mark)> = self
            .marks
            .iter()
            .enumerate()
            .filter(|(_, m)| m.period > over && m.period > median + Duration::from_millis(1))
            .collect();
        spikes.sort_by_key(|(_, m)| std::cmp::Reverse(m.period));
        let spikes = spikes
            .into_iter()
            .map(|(frame, mark)| {
                // What the loop spent drawing a frame is published with the next
                // frame's stages, and a shader it built after them with the one after
                // that, so a frame's cause can be two tables behind it.
                let (name, cost) = self.marks[frame..]
                    .iter()
                    .take(3)
                    .map(|m| &m.costliest)
                    // No more than the frame cost: a slow neighbour's is not its.
                    .filter(|(_, cost)| *cost <= mark.period)
                    .max_by_key(|(_, cost)| *cost)
                    .unwrap_or(&mark.costliest);
                Spike {
                    frame,
                    period: mark.period,
                    cpu: mark.cpu,
                    cause: name.to_string(),
                    cost: *cost,
                }
            })
            .collect();
        (spread, spikes)
    }

    /// The CPU clock's step, as small as a frame saw it: a frame or span only
    /// a few steps long reads its slices as cores it never had.
    fn cpu_step(&self) -> Duration {
        self.marks
            .iter()
            .map(|m| m.cpu)
            .filter(|cpu| !cpu.is_zero())
            .min()
            .unwrap_or_default()
    }

    /// The cores the run kept busy on average and in its busiest frame;
    /// `None` where the platform counts no CPU time.
    fn cores(&self) -> Option<(f64, f64)> {
        let (cpu, period): (Duration, Duration) = self
            .marks
            .iter()
            .fold((Duration::ZERO, Duration::ZERO), |(c, p), m| {
                (c + m.cpu, p + m.interval)
            });
        if cpu.is_zero() || period.is_zero() {
            return None;
        }
        let step = self.cpu_step();
        let busiest = self
            .marks
            .iter()
            .filter(|m| !m.interval.is_zero() && m.interval >= step * 2)
            .map(|m| m.cpu.as_secs_f64() / m.interval.as_secs_f64())
            .fold(0.0, f64::max);
        Some((cpu.as_secs_f64() / period.as_secs_f64(), busiest))
    }
}

/// What in a frame cost the most: its costliest span, which names the work,
/// where it is most of the costliest stage; else that stage, said to have
/// spent it outside any span, so a report never names a span for a cost it
/// holds a sliver of.
fn costliest(timings: &Timings) -> (SmolStr, Duration) {
    let span = timings.spans.iter().max_by_key(|(_, d)| *d).cloned();
    let stage = STAGE_NAMES
        .iter()
        .zip(timings.stages)
        .max_by_key(|(_, d)| *d)
        .map(|(n, d)| (SmolStr::new(n), d));
    match (span, stage) {
        (Some(span), Some(stage)) if span.1 * 2 < stage.1 => {
            (SmolStr::new(format!("{}{OUTSIDE_SPANS}", stage.0)), stage.1)
        }
        (Some(span), _) => span,
        (None, Some(stage)) => stage,
        (None, None) => (SmolStr::default(), Duration::ZERO),
    }
}

fn millis(d: Duration) -> String {
    format!("{:.3} ms", d.as_secs_f64() * 1000.0)
}

/// The process's resident set, per platform.
mod memory {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(super) fn resident() -> Option<u64> {
        // `statm`'s second field is the resident set, in pages.
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?; // os files: the kernel's, never a game's
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        // SAFETY: `sysconf` reads a constant of the system and touches no memory of ours.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        Some(pages * u64::try_from(page).ok()?)
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(super) fn resident() -> Option<u64> {
        let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
        let size = i32::try_from(std::mem::size_of::<libc::proc_taskinfo>()).ok()?;
        // SAFETY: the buffer is a zeroed `proc_taskinfo` of the size passed,
        // which is what `PROC_PIDTASKINFO` fills; the call writes nothing else.
        let wrote = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDTASKINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if wrote != size {
            return None;
        }
        // SAFETY: `proc_pidinfo` filled the whole struct, as its return says.
        Some(unsafe { info.assume_init() }.pti_resident_size)
    }

    #[cfg(windows)]
    pub(super) fn resident() -> Option<u64> {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut counters = std::mem::MaybeUninit::<PROCESS_MEMORY_COUNTERS>::zeroed();
        let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
        // SAFETY: the buffer is a zeroed `PROCESS_MEMORY_COUNTERS` of the size
        // passed, and the pseudo handle of the current process needs no closing.
        let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), counters.as_mut_ptr(), size) };
        if ok == 0 {
            return None;
        }
        // SAFETY: the call succeeded, so it filled the struct.
        let counters = unsafe { counters.assume_init() };
        u64::try_from(counters.WorkingSetSize).ok()
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        windows
    )))]
    pub(super) fn resident() -> Option<u64> {
        None
    }
}
