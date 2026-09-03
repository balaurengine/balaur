//! Record a session and play it back: the determinism debugger, and the
//! editor's session recorder.
//!
//! A digest says two runs diverged; a replay says *what they were fed* when
//! they did. Recording captures the external input each tick — the only
//! thing a deterministic simulation cannot derive for itself — alongside the
//! events worth showing on a timeline, and optionally the digest that tick
//! produced. Playing it back re-feeds exactly that.
//!
//! The file is JSON Lines: a [`Header`] on the first line, one [`Frame`] per
//! tick, and a [`Trailer`] when the session ends cleanly. Line-oriented so it
//! diffs, greps and streams; two machines' files are directly comparable with
//! `diff`. A file with no trailer ended in a crash, which is exactly the
//! session worth having recorded.
//!
//! Recording and playing both run inside the frame loop: [`Recording`] holds
//! the open file and [`ReplayPlayer`] the loaded session, and `App` writes
//! and feeds them. Nothing here starts either — [`begin`] and
//! [`Recorder::create`] are called by the CLI, by the editor through the
//! `replay` script module, or by a test.

use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

/// Bumped when a change makes an older file unplayable.
pub const FORMAT: u32 = 2;

/// How many log entries a frame scans for lines to record. The buffer holds
/// 500 and a frame that produced more than this has other problems.
const LOG_SCAN: usize = 200;

/// Events one tick may record before the rest are dropped. A runaway loop
/// must not eat memory faster than the file can absorb it.
const MAX_EVENTS_PER_TICK: usize = 2000;

/// What a subsystem contributes to a recorded tick, and takes back on replay.
///
/// Only state the simulation cannot derive belongs here: what the OS, the
/// network or the player handed in this tick.
pub type CaptureFn = Box<dyn Fn(&Engine) -> serde_json::Value>;
/// The inverse of a [`CaptureFn`], applied before the tick reads anything.
pub type RestoreFn = Box<dyn Fn(&Engine, &serde_json::Value)>;

/// Every subsystem that owns per-tick external input, in registration order.
#[derive(Default)]
pub struct ReplayRegistry(pub Vec<(String, CaptureFn, RestoreFn)>);

/// The same seam for state that is decided once, before the first tick, and
/// then read all session: input bindings, a locale, an audio mix.
///
/// Per-tick capture would be waste — the value does not change — but leaving
/// it out entirely means a replay derives from whatever the *replaying*
/// machine has, which is how a rebound key silently changes what a recording
/// reproduces. This is the RNG seed's treatment for everything else that is
/// loaded rather than simulated.
#[derive(Default)]
pub struct ReplaySetupRegistry(pub Vec<(String, CaptureFn, RestoreFn)>);

/// The frame the next tick will be fed, set by whoever drives a replay.
///
/// A resource rather than a system the driver adds, because the restore has
/// to happen before *every* plugin's `Stage::First` work — the net pump would
/// otherwise dispatch this tick's real traffic before the recording landed.
#[derive(Default)]
pub struct ReplayFeed(pub Option<serde_json::Map<String, serde_json::Value>>);

/// What this run is doing about recordings. Set once by whoever drives it,
/// before the project loads — a script's `init` can already open a socket.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayMode {
    #[default]
    Off,
    Recording,
    Playing,
}

/// Whether this run is playing a recording back.
///
/// Read it live rather than caching it per tick: a subsystem that cached the
/// answer would still be wrong for anything a script does before the first
/// tick, which is exactly where `init` opens its connections.
pub fn is_playing(eng: &Engine) -> bool {
    eng.try_resource::<ReplayMode>()
        .is_some_and(|mode| *mode.borrow() == ReplayMode::Playing)
}

/// Whether outbound work must not happen: a recording is playing, or the
/// rollback session is running a tick for the second time.
///
/// [`ExternalIo::start`] is the enforced form of this, and what a subsystem
/// should reach for. A listener asks directly, because what it hands back per
/// connection is a live channel rather than serializable data, which is the
/// one thing `ExternalIo` cannot wrap.
#[must_use]
pub fn suppressed(eng: &Engine) -> bool {
    is_playing(eng) || crate::rollback::is_resimulating(eng)
}

/// A subsystem's channel to the outside world, with recording built in.
///
/// Wraps the three things every I/O subsystem needs — the worker channel,
/// this tick's arrivals, and their serialization — and enforces the one they
/// keep forgetting: nothing may reach the outside while a recording is
/// playing back. The `Sender` workers report on is only reachable through
/// [`ExternalIo::start`], so a subsystem cannot spawn real work by accident;
/// there is no other way to get one.
pub struct ExternalIo<E> {
    events: Receiver<E>,
    report: Sender<E>,
    recorded: Vec<E>,
}

impl<E> Default for ExternalIo<E> {
    fn default() -> Self {
        let (report, events) = channel();
        Self {
            events,
            report,
            recorded: Vec::new(),
        }
    }
}

impl<E: Clone + Serialize + DeserializeOwned> ExternalIo<E> {
    /// Start outbound work — unless a recording is playing, in which case the
    /// file supplies the arrivals and the outside world is not touched.
    ///
    /// Returns whether `spawn` ran, for the caller that must skip bookkeeping
    /// of its own.
    pub fn start(&self, eng: &Engine, spawn: impl FnOnce(&Sender<E>)) -> bool {
        if suppressed(eng) {
            return false;
        }
        spawn(&self.report);
        true
    }

    /// This tick's arrivals, remembered for the recording on the way past.
    pub fn drain(&mut self) -> Vec<E> {
        self.recorded.clear();
        while let Ok(event) = self.events.try_recv() {
            self.recorded.push(event);
        }
        self.recorded.clone()
    }

    /// What [`drain`](ExternalIo::drain) last saw, as a recording writes it.
    pub fn capture(&self) -> serde_json::Value {
        serde_json::to_value(&self.recorded).unwrap_or(serde_json::Value::Null)
    }

    /// Push a recorded tick's arrivals back down the same channel the workers
    /// use, so the subsystem's own dispatch runs unchanged.
    pub fn restore(&self, value: &serde_json::Value) {
        match serde_json::from_value::<Vec<E>>(value.clone()) {
            Ok(events) => {
                for event in events {
                    let _ = self.report.send(event);
                }
            }
            Err(e) => tracing::error!(error = %e, "replaying external io"),
        }
    }
}

/// Where the engine's counters stood when a session started.
///
/// A recording made in a long-lived process — the editor, which plays a game
/// many times without restarting — starts at whatever tick, time and token
/// the editor had reached. Replay puts all three back, so a script sees the
/// numbers it saw, and an http reply keyed by its request id finds the
/// request that recorded it.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Origin {
    pub tick: u64,
    pub time: f64,
    pub tokens: u64,
}

impl Origin {
    /// The counters as they stand, as a starting point.
    ///
    /// The tick and time here are provisional: whether the frame this was
    /// read in is itself recorded depends on where in the frame the recording
    /// started, so [`Recorder`] settles both against the first frame it
    /// actually writes. The token counter is not provisional — it has to be
    /// the value in force when recording began, because a request made
    /// between then and the first frame is part of that frame.
    pub fn of(eng: &Engine) -> Self {
        Self {
            tick: eng.tick(),
            time: eng.time(),
            tokens: eng.tokens(),
        }
    }

    pub fn restore(&self, eng: &Engine) {
        eng.set_clock(self.tick, self.time);
        eng.set_tokens(self.tokens);
    }
}

/// Something worth showing on a session timeline.
///
/// Not a replay source: nothing restores an event, the subsystems re-emit
/// them when the recording plays, and a session view compares the two.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// `net.request`, `log`, `script.error`, `debug.pause`.
    pub kind: String,
    /// One line, as a timeline row shows it.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// This tick's events, refilled every frame.
///
/// Cleared at the end of every frame whether or not anything is recording, so
/// a run with no recorder does not accumulate.
#[derive(Default)]
pub struct EventLog(pub Vec<Event>);

/// Record one event against the current tick.
///
/// Free when nothing is recording, and dropped past
/// [`MAX_EVENTS_PER_TICK`], so a subsystem may call it unconditionally.
pub fn event(eng: &Engine, kind: &str, label: impl Into<String>, data: Option<serde_json::Value>) {
    let Some(log) = eng.try_resource::<EventLog>() else {
        return;
    };
    let mut log = log.borrow_mut();
    if log.0.len() >= MAX_EVENTS_PER_TICK {
        return;
    }
    log.0.push(Event {
        kind: kind.to_string(),
        label: label.into(),
        data,
    });
}

/// What a file needs before its first tick makes sense.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    /// The project the session ran, so replay needs only the file.
    pub project: String,
    /// The RNG stream's starting position.
    pub seed: u64,
    #[serde(default)]
    pub origin: Origin,
    /// A fingerprint of the game's scripts when the session started, from
    /// whoever started it. A replay whose sources no longer match still
    /// plays; it just cannot promise to reproduce what was seen.
    #[serde(default)]
    pub scripts: String,
    /// When the session started, for naming and listing it.
    #[serde(default)]
    pub started: String,
    /// Loaded state a plugin declared through `App::add_replay_setup`, keyed
    /// by its name. Restored before the first tick, so what a recording
    /// derives is what it derived when it was made.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub setup: serde_json::Map<String, serde_json::Value>,
}

/// One recorded tick.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Frame {
    pub tick: u64,
    /// The step this tick ran at, as bits: a replay that rounded the step
    /// would diverge for a reason that has nothing to do with the bug.
    pub dt: u32,
    /// Captured input, keyed by source name.
    pub sources: serde_json::Map<String, serde_json::Value>,
    /// The digest at the end of the tick, when the session was recording
    /// them. Off by default: it walks every node and hashes every component,
    /// which is a real cost to pay on every frame of every play session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

impl Frame {
    pub fn step(&self) -> f32 {
        f32::from_bits(self.dt)
    }
}

/// How a session ended, and the world it ended on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trailer {
    /// `stop`, `reload` or `quit`.
    pub reason: String,
    pub tick: u64,
    /// The world when the session was stopped. Not a frame boundary — a stop
    /// lands wherever in the frame the button was pressed — so this is a note
    /// for a reader, not something a replay can check itself against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<u64>,
}

/// The trailer's line shape, so a reader can tell it from a frame by its one
/// required key.
#[derive(Serialize, Deserialize)]
struct EndLine {
    end: Trailer,
}

/// Ask every registered source what it saw this tick.
pub fn capture(eng: &Engine) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    if let Some(sources) = eng.try_resource::<ReplayRegistry>() {
        for (name, capture, _) in &sources.borrow().0 {
            out.insert(name.clone(), capture(eng));
        }
    }
    out
}

/// Everything the setup registry declares, for a recording's header.
fn capture_setup(eng: &Engine) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    if let Some(registry) = eng.try_resource::<ReplaySetupRegistry>() {
        for (name, capture, _) in &registry.borrow().0 {
            out.insert(name.clone(), capture(eng));
        }
    }
    out
}

/// Put a recording's setup back before its first tick. A name the file does
/// not carry is left alone, so a recording made before a plugin declared its
/// setup still plays — with that plugin reading whatever this machine has,
/// which is what it did before this existed.
fn restore_setup(eng: &Engine, setup: &serde_json::Map<String, serde_json::Value>) {
    let Some(registry) = eng.try_resource::<ReplaySetupRegistry>() else {
        return;
    };
    for (name, _, restore) in &registry.borrow().0 {
        if let Some(value) = setup.get(name) {
            restore(eng, value);
        }
    }
}

/// Feed a recorded tick back in. A source missing from the file keeps
/// whatever it already had, so an older recording still plays.
pub fn restore(eng: &Engine, sources: &serde_json::Map<String, serde_json::Value>) {
    if let Some(registry) = eng.try_resource::<ReplayRegistry>() {
        for (name, _, restore) in &registry.borrow().0 {
            if let Some(value) = sources.get(name) {
                restore(eng, value);
            }
        }
    }
}

/// Appends frames as they happen, so a crashed session still leaves a file.
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent recording flags, not a state enum"
)]
pub struct Recorder {
    out: BufWriter<std::fs::File>,
    /// Held back until the first frame settles its origin, so the file's one
    /// header is written once and correct.
    header: Option<Header>,
    path: PathBuf,
    /// Whether every frame carries a digest, or only the trailer does.
    per_tick_digest: bool,
    frames: u64,
    /// The log buffer's timestamp of the last line already recorded.
    log_cursor: f64,
    finished: bool,
    /// Whether a script was stopped at the end of the last frame, so a pause
    /// is recorded once rather than on every frame it lasts.
    was_paused: bool,
    /// The tick the recorder was made on. A frame offered for that same tick
    /// is the one recording started part-way through, and half a frame is not
    /// something a replay can reproduce.
    born: u64,
    /// Taken by the app, which zeroes its fixed-step accumulator. A replay
    /// zeroes its own, so both take the same steps on the same frames.
    restart: bool,
}

impl Recorder {
    pub fn create(path: &Path, header: Header, per_tick_digest: bool, born: u64) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let file =
            std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
        Ok(Self {
            out: BufWriter::new(file),
            header: Some(header),
            path: path.to_path_buf(),
            per_tick_digest,
            frames: 0,
            log_cursor: crate::logbuf::recent(1).first().map_or(0.0, |e| e.time),
            finished: false,
            was_paused: false,
            born,
            restart: true,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Flushed per frame: a session that crashes is exactly the session
    /// worth having recorded.
    pub fn write(&mut self, frame: &Frame) -> Result<()> {
        if let Some(header) = self.header.take() {
            self.write_line(&header)?;
        }
        self.write_line(frame)?;
        self.frames += 1;
        Ok(())
    }

    fn write_line(&mut self, line: &impl Serialize) -> Result<()> {
        serde_json::to_writer(&mut self.out, line)?;
        self.out.write_all(b"\n")?;
        self.out.flush()?;
        Ok(())
    }

    /// Close the session, naming why it ended and the world it ended on.
    pub fn finish(mut self, trailer: &Trailer) -> Result<PathBuf> {
        self.finished = true;
        self.end(trailer)?;
        Ok(self.path.clone())
    }

    /// A session that recorded no frame still needs its header, or the file
    /// is not a recording at all.
    fn end(&mut self, trailer: &Trailer) -> Result<()> {
        if let Some(header) = self.header.take() {
            self.write_line(&header)?;
        }
        self.write_line(&EndLine {
            end: trailer.clone(),
        })
    }

    /// A debugger pause that arrived since the last frame, as one event.
    fn fresh_pause(&mut self, eng: &Engine) -> Option<Event> {
        let pause = eng.script_host().and_then(|host| host.paused());
        let was = std::mem::replace(&mut self.was_paused, pause.is_some());
        let pause = pause.filter(|_| !was)?;
        Some(Event {
            kind: "debug.pause".into(),
            label: format!(
                "{} paused at {}:{}",
                pause.reason.name(),
                pause.path,
                pause.line
            ),
            data: Some(serde_json::json!({
                "reason": pause.reason.name(),
                "path": pause.path,
                "line": pause.line,
            })),
        })
    }

    /// Log lines written since the last frame, as events.
    fn fresh_logs(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        for entry in crate::logbuf::recent(LOG_SCAN) {
            if entry.time <= self.log_cursor {
                continue;
            }
            self.log_cursor = entry.time;
            out.push(Event {
                kind: "log".into(),
                label: format!("[{}] {}", entry.tag, entry.message),
                data: Some(serde_json::json!({ "level": entry.level, "tag": entry.tag })),
            });
        }
        out
    }
}

/// A recorder dropped without [`Recorder::finish`] still closes its file:
/// the run ended some way that never got to say so, and a session with no
/// trailer would otherwise be indistinguishable from one that crashed.
impl Drop for Recorder {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let trailer = Trailer {
            reason: "quit".into(),
            tick: 0,
            digest: None,
        };
        if let Err(e) = self.end(&trailer) {
            tracing::error!(error = %e, "closing the recording");
        }
    }
}

/// The session being written, if any.
#[derive(Default)]
pub struct Recording(pub Option<Recorder>);

/// UTC, as `YYYY-MM-DD HH:MM:SS`: sorts chronologically as text, which is
/// what a session list wants, and reads as a time, which a file name wants.
///
/// Written here rather than taken from a date crate because this is the only
/// date the engine formats, and the civil-date conversion is shorter than the
/// dependency.
#[allow(
    clippy::disallowed_methods,
    reason = "names a recording file, not simulation"
)]
pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970 fits an i64 for any clock a machine can hold.
    #[allow(clippy::cast_possible_wrap, reason = "a day count is far inside i64")]
    let (year, month, day) = civil_from_days(days as i64);
    let (h, m, s) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}")
}

/// Days since the Unix epoch to a civil date, by Howard Hinnant's algorithm:
/// the year is shifted to start in March so a leap day lands last.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Start recording into `path`. Replaces whatever the engine was recording.
///
/// Call it before the code whose session it records runs: the header's origin
/// is read here, and a request made before it is taken would replay under a
/// different id.
pub fn start_recording(
    eng: &Engine,
    path: &Path,
    project: &str,
    scripts: &str,
    per_tick_digest: bool,
) -> Result<()> {
    let header = Header {
        format: FORMAT,
        project: project.to_string(),
        seed: crate::rng::with_rng(eng, |rng| rng.state()),
        origin: Origin::of(eng),
        scripts: scripts.to_string(),
        started: timestamp(),
        setup: capture_setup(eng),
    };
    let recorder = Recorder::create(path, header, per_tick_digest, eng.tick())?;
    *eng.resource::<ReplayMode>().borrow_mut() = ReplayMode::Recording;
    eng.resource::<Recording>().borrow_mut().0 = Some(recorder);
    Ok(())
}

/// Close the open recording, if there is one, and return the file it wrote.
pub fn stop_recording(eng: &Engine, reason: &str) -> Option<PathBuf> {
    let recorder = eng.resource::<Recording>().borrow_mut().0.take()?;
    let digest = Some(crate::digest::digest(eng).0);
    if *eng.resource::<ReplayMode>().borrow() == ReplayMode::Recording {
        *eng.resource::<ReplayMode>().borrow_mut() = ReplayMode::Off;
    }
    let trailer = Trailer {
        reason: reason.to_string(),
        tick: eng.tick(),
        digest,
    };
    match recorder.finish(&trailer) {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::error!(error = %e, "closing the recording");
            None
        }
    }
}

/// Write this tick to the open recording, then clear the tick's events.
///
/// Core runs this at the end of every frame, after deferred destruction, so a
/// digest describes the world the next tick starts from.
pub(crate) fn record_frame_system(eng: &Engine, dt: f32) {
    let recording = eng.resource::<Recording>();
    if let Some(recorder) = recording.borrow_mut().0.as_mut() {
        if eng.tick() == recorder.born {
            eng.resource::<EventLog>().borrow_mut().0.clear();
            return;
        }
        // Only the first frame written says where the session really starts,
        // and it says it from the clock: a `record` call inside a frame is
        // recorded from that frame, one between two frames from the next.
        if let Some(header) = recorder.header.as_mut() {
            header.origin.tick = eng.tick().saturating_sub(1);
            header.origin.time = eng.time() - f64::from(dt);
        }
        let mut events = std::mem::take(&mut eng.resource::<EventLog>().borrow_mut().0);
        events.extend(recorder.fresh_logs());
        events.extend(recorder.fresh_pause(eng));
        let frame = Frame {
            tick: eng.tick(),
            dt: dt.to_bits(),
            sources: capture(eng),
            digest: recorder
                .per_tick_digest
                .then(|| crate::digest::digest(eng).0),
            events,
        };
        if std::env::var("BALAUR_DUMP")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            == Some(eng.tick())
        {
            for e in crate::digest::entries(eng) {
                eprintln!("REC {} {}", e.label, e.digest);
            }
        }
        if let Err(e) = recorder.write(&frame) {
            tracing::error!(error = %e, "writing the recording");
        }
    }
    eng.resource::<EventLog>().borrow_mut().0.clear();
}

/// A whole recorded session, read back.
#[derive(Debug)]
pub struct Session {
    pub header: Header,
    pub frames: Vec<Frame>,
    /// Absent when the session crashed rather than stopped.
    pub trailer: Option<Trailer>,
}

impl Session {
    pub fn read(path: &Path) -> Result<Self> {
        let file =
            std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
        let mut lines = std::io::BufReader::new(file).lines();
        let first = lines
            .next()
            .transpose()?
            .ok_or_else(|| anyhow::anyhow!("{} is empty", path.display()))?;
        let header: Header = serde_json::from_str(&first)
            .with_context(|| format!("{} does not start with a replay header", path.display()))?;
        anyhow::ensure!(
            header.format == FORMAT,
            "{} is format {}, this build plays {FORMAT}",
            path.display(),
            header.format
        );
        let mut frames = Vec::new();
        let mut trailer = None;
        for (index, line) in lines.enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(end) = serde_json::from_str::<EndLine>(&line) {
                trailer = Some(end.end);
                continue;
            }
            frames.push(
                serde_json::from_str(&line)
                    .with_context(|| format!("{}: tick {}", path.display(), index + 1))?,
            );
        }
        Ok(Self {
            header,
            frames,
            trailer,
        })
    }

    /// The tick the session's first frame ran at, or the origin's when it has
    /// no frames.
    pub fn first_tick(&self) -> u64 {
        self.frames
            .first()
            .map_or(self.header.origin.tick, |f| f.tick)
    }

    pub fn last_tick(&self) -> u64 {
        self.frames.last().map_or(self.first_tick(), |f| f.tick)
    }

    /// Ticks at which one source's `key` held a non-empty list, with what it
    /// held: the timeline's input and arrival lanes, without core learning
    /// the shape of any plugin's snapshot.
    ///
    /// An empty `key` reads the source itself, for the ones that serialize as
    /// a list rather than a table.
    pub fn marks(&self, source: &str, key: &str) -> Vec<(u64, Vec<serde_json::Value>)> {
        self.frames
            .iter()
            .filter_map(|frame| {
                let value = frame.sources.get(source)?;
                let list = if key.is_empty() {
                    value.as_array()?
                } else {
                    value.get(key)?.as_array()?
                };
                (!list.is_empty()).then(|| (frame.tick, list.clone()))
            })
            .collect()
    }

    /// Every event between two ticks inclusive, each paired with its tick.
    pub fn events_between(&self, from: u64, to: u64) -> Vec<(u64, &Event)> {
        self.frames
            .iter()
            .filter(|f| f.tick >= from && f.tick <= to)
            .flat_map(|f| f.events.iter().map(move |e| (f.tick, e)))
            .collect()
    }
}

/// What a player is doing with the session it holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PlayState {
    /// No session loaded, or one that has been unloaded: the game runs live.
    #[default]
    Stopped,
    Playing,
    /// Between frames, with the simulation held still.
    Paused,
    /// Running frames as fast as the budget allows, to reach a tick.
    Seeking,
}

impl PlayState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Seeking => "seeking",
        }
    }
}

/// A tick whose replay did not reproduce what was recorded.
#[derive(Clone, Copy, Debug)]
pub struct Divergence {
    pub tick: u64,
    pub recorded: u64,
    pub replayed: u64,
}

/// How many frames a seek runs per call, so a long one does not stall the
/// window it is seeking in front of.
pub const SEEK_BUDGET: usize = 600;

/// The loaded session and where playback has reached.
///
/// `App::advance` drives it: nothing here ticks anything, because feeding a
/// frame and running it are one operation and only the app can do the second.
#[derive(Default)]
pub struct ReplayPlayer {
    pub session: Option<Session>,
    /// Index of the next frame to feed.
    pub cursor: usize,
    pub state: PlayState,
    /// While seeking, the tick to stop on.
    pub seek_to: u64,
    pub budget: usize,
    pub diverged: Option<Divergence>,
    /// Set by [`begin`] and taken by the app, which zeroes its fixed-step
    /// accumulator: a replay that started mid-accumulation would take its
    /// fixed steps on different frames than the recording did.
    pub restart: bool,
}

/// What one call to `App::advance` should do about the replay.
pub enum Step {
    /// Nothing loaded: run the measured frame as usual.
    Live,
    /// Paused: hold the simulation still and draw one frame.
    Hold,
    /// Feed this many recorded frames.
    Frames(usize),
}

impl ReplayPlayer {
    /// How many frames are left to feed.
    pub fn remaining(&self) -> usize {
        self.session
            .as_ref()
            .map_or(0, |s| s.frames.len().saturating_sub(self.cursor))
    }

    pub fn length(&self) -> usize {
        self.session.as_ref().map_or(0, |s| s.frames.len())
    }

    /// The tick playback has reached. Before the first frame is fed that is
    /// the tick *before* the session's first, since nothing has run yet.
    pub fn position(&self) -> u64 {
        let Some(session) = &self.session else {
            return 0;
        };
        match self.cursor.checked_sub(1) {
            Some(i) => session
                .frames
                .get(i)
                .map_or_else(|| session.last_tick(), |f| f.tick),
            None => session.first_tick().saturating_sub(1),
        }
    }

    /// Decided before anything ticks: a script may pause or seek from inside
    /// the frame, and that has to take effect on the next one.
    pub fn plan(&self) -> Step {
        match self.state {
            PlayState::Stopped => Step::Live,
            _ if self.remaining() == 0 => Step::Hold,
            PlayState::Paused => Step::Hold,
            PlayState::Playing => Step::Frames(1),
            PlayState::Seeking => Step::Frames(self.budget.min(self.remaining())),
        }
    }

    /// Aim playback at `tick`. Only forward: going back means rebuilding the
    /// world and seeking from the start, which is the caller's business
    /// because only it knows how the world was built.
    pub fn seek(&mut self, tick: u64) {
        if self.session.is_none() {
            return;
        }
        self.seek_to = tick;
        self.state = if self.position() >= tick {
            PlayState::Paused
        } else {
            PlayState::Seeking
        };
    }

    /// Whether a seek has arrived, called after each fed frame.
    fn seek_done(&self) -> bool {
        self.state == PlayState::Seeking && self.position() >= self.seek_to
    }
}

/// Put a session in front of the engine, ready to play.
///
/// Sets playing mode, restores the recorded origin and RNG seed, and parks
/// the cursor on the first frame. Call before the project loads, or before
/// the game's scripts attach: an `init` that opens a socket must be
/// suppressed, and one that takes a token must take the recorded one.
pub fn begin(eng: &Engine, session: Session) {
    session.header.origin.restore(eng);
    restore_setup(eng, &session.header.setup);
    let seed = session.header.seed;
    crate::rng::with_rng(eng, |rng| *rng = crate::rng::Pcg32::from_state(seed));
    *eng.resource::<ReplayMode>().borrow_mut() = ReplayMode::Playing;
    let player = eng.resource::<ReplayPlayer>();
    let mut player = player.borrow_mut();
    player.session = Some(session);
    player.cursor = 0;
    player.state = PlayState::Paused;
    player.seek_to = 0;
    player.budget = SEEK_BUDGET;
    player.diverged = None;
    player.restart = true;
    drop(player);
    // Held from here: `load` is called part-way through a frame, and the rest
    // of that frame would step the game before the first recorded tick was
    // ever fed.
    eng.set_replay_hold(true);
}

/// Whether the app should zero its fixed-step accumulator before the next
/// frame, because a recording is about to start at that boundary.
pub(crate) fn take_record_restart(eng: &Engine) -> bool {
    eng.try_resource::<Recording>().is_some_and(|r| {
        r.borrow_mut()
            .0
            .as_mut()
            .is_some_and(|rec| std::mem::take(&mut rec.restart))
    })
}

/// Start playing the loaded session, for a Rust driver.
pub fn play(eng: &Engine) {
    let player = eng.resource::<ReplayPlayer>();
    let mut player = player.borrow_mut();
    if player.remaining() > 0 {
        player.state = PlayState::Playing;
    }
}

/// Whether the player would feed another frame right now.
pub fn is_advancing(eng: &Engine) -> bool {
    eng.try_resource::<ReplayPlayer>().is_some_and(|p| {
        let p = p.borrow();
        matches!(p.state, PlayState::Playing | PlayState::Seeking) && p.remaining() > 0
    })
}

/// Whether a loaded session still has frames to feed.
pub fn is_running(eng: &Engine) -> bool {
    eng.try_resource::<ReplayPlayer>()
        .is_some_and(|p| p.borrow().remaining() > 0)
}

/// Drop the loaded session and let the engine run live again.
pub fn end(eng: &Engine) {
    let player = eng.resource::<ReplayPlayer>();
    let mut player = player.borrow_mut();
    player.session = None;
    player.cursor = 0;
    player.state = PlayState::Stopped;
    player.diverged = None;
    eng.set_replay_hold(false);
    eng.resource::<ReplayFeed>().borrow_mut().0 = None;
    if *eng.resource::<ReplayMode>().borrow() == ReplayMode::Playing {
        *eng.resource::<ReplayMode>().borrow_mut() = ReplayMode::Off;
    }
}

/// Put the next recorded frame in the feed and report the step it ran at.
///
/// Returns `None` when the session is spent. Split from running the tick
/// because the borrow on the player must be released before any script runs.
pub(crate) fn feed_next(eng: &Engine) -> Option<f32> {
    let player = eng.resource::<ReplayPlayer>();
    let (sources, dt) = {
        let mut player = player.borrow_mut();
        let session = player.session.as_ref()?;
        let frame = session.frames.get(player.cursor)?;
        let taken = (frame.sources.clone(), frame.step());
        player.cursor += 1;
        taken
    };
    eng.resource::<ReplayFeed>().borrow_mut().0 = Some(sources);
    Some(dt)
}

/// Compare the tick just replayed against what the recording says, and settle
/// the player's state now that a frame has landed.
pub(crate) fn after_frame(eng: &Engine) {
    let player = eng.resource::<ReplayPlayer>();
    let mut player = player.borrow_mut();
    let (expected, spent) = {
        let Some(session) = &player.session else {
            return;
        };
        // Only a frame's own digest is comparable. The trailer's is taken
        // wherever in its frame the session was stopped, which is not a frame
        // boundary and so is a record for a reader, not a check.
        let expected = player
            .cursor
            .checked_sub(1)
            .and_then(|i| session.frames.get(i))
            .and_then(|f| f.digest.map(|d| (f.tick, d)));
        (expected, player.cursor >= session.frames.len())
    };
    if let Some((tick, expected)) = expected {
        if std::env::var("BALAUR_DUMP")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            == Some(tick)
        {
            for e in crate::digest::entries(eng) {
                eprintln!("PLAY {} {}", e.label, e.digest);
            }
        }
        let live = crate::digest::digest(eng).0;
        if live != expected && player.diverged.is_none() {
            player.diverged = Some(Divergence {
                tick,
                recorded: expected,
                replayed: live,
            });
        }
    }
    if spent || player.seek_done() {
        player.state = PlayState::Paused;
    }
    // Nothing is fed while paused, and the feed is what makes the restore at
    // `Stage::First` overwrite live input.
    if player.state == PlayState::Paused {
        eng.resource::<ReplayFeed>().borrow_mut().0 = None;
    }
}
