//! Record a session and play it back: the determinism debugger.
//!
//! A digest says two runs diverged; a replay says *what they were fed* when
//! they did. Recording captures the external input each tick — the only
//! thing a deterministic simulation cannot derive for itself — alongside the
//! digest that tick produced. Playing it back re-feeds exactly that and
//! compares digests, so a divergence is a tick number rather than a bug
//! report that begins "sometimes".
//!
//! The file is JSON Lines: a [`Header`] on the first line, then one
//! [`Frame`] per tick. Line-oriented so it diffs, greps and streams; two
//! machines' files are directly comparable with `diff`.

use std::io::{BufRead, BufWriter, Write};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

/// Bumped when a change makes an older file unplayable.
pub const FORMAT: u32 = 1;

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
        if is_playing(eng) {
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

/// What a file needs before its first tick makes sense.
#[derive(Debug, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    /// The project the session ran, so replay needs only the file.
    pub project: String,
    /// The RNG stream's starting position.
    pub seed: u64,
}

/// One recorded tick.
#[derive(Debug, Serialize, Deserialize)]
pub struct Frame {
    pub tick: u64,
    /// The step this tick ran at, as bits: a replay that rounded the step
    /// would diverge for a reason that has nothing to do with the bug.
    pub dt: u32,
    /// Captured input, keyed by source name.
    pub sources: serde_json::Map<String, serde_json::Value>,
    /// The digest at the end of the tick.
    pub digest: u64,
}

impl Frame {
    pub fn step(&self) -> f32 {
        f32::from_bits(self.dt)
    }
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
pub struct Recorder {
    out: BufWriter<std::fs::File>,
}

impl Recorder {
    pub fn create(path: &Path, header: &Header) -> Result<Self> {
        let file =
            std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut out = BufWriter::new(file);
        serde_json::to_writer(&mut out, header)?;
        out.write_all(b"\n")?;
        Ok(Self { out })
    }

    /// Flushed per frame: a session that crashes is exactly the session
    /// worth having recorded.
    pub fn write(&mut self, frame: &Frame) -> Result<()> {
        serde_json::to_writer(&mut self.out, frame)?;
        self.out.write_all(b"\n")?;
        self.out.flush()?;
        Ok(())
    }
}

/// A whole recorded session, read back.
#[derive(Debug)]
pub struct Session {
    pub header: Header,
    pub frames: Vec<Frame>,
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
        for (index, line) in lines.enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            frames.push(
                serde_json::from_str(&line)
                    .with_context(|| format!("{}: tick {}", path.display(), index + 1))?,
            );
        }
        Ok(Self { header, frames })
    }
}
