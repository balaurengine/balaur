//! In-engine log capture: a bounded ring buffer that records every `tracing`
//! event so tools (the editor's Output dock, in-game consoles) can display
//! them, and so tests can assert that something was reported.
//!
//! Events keep their structured fields. A test asserts on `fields`, not on the
//! wording of `message`.
//!
//! The buffer is process-global because the subscriber is process-global.

use crate::time::Instant;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Mutex;

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;

const CAPACITY: usize = 500;

#[derive(Clone, Debug)]
pub struct LogEntry {
    /// Its place in everything captured, from 1: a reader resumes after one.
    pub seq: u64,
    /// Seconds since the subscriber was installed.
    pub time: f64,
    /// "info", "warn", "error", "debug", "trace".
    pub level: String,
    /// Last segment of the event target, used as the tag column.
    pub tag: String,
    pub message: String,
    /// Structured fields other than `message`, in declaration order.
    pub fields: Vec<(String, String)>,
}

impl LogEntry {
    /// The value of a field, or `None` if the event did not carry it.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

struct Buffer {
    start: Instant,
    entries: VecDeque<LogEntry>,
    /// Every entry ever captured, the ring's evictions included.
    total: u64,
}

static BUFFER: Mutex<Option<Buffer>> = Mutex::new(None);

/// Lock the buffer, recovering from poisoning.
///
/// A panic while holding this lock must not make every later event panic too:
/// losing the buffer's contents is acceptable, losing the process is not.
fn lock_buffer() -> std::sync::MutexGuard<'static, Option<Buffer>> {
    BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
struct FieldVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            self.fields
                .push((field.name().to_string(), format!("{value:?}")));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

/// Records every event into the ring buffer.
struct CaptureLayer;

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let meta = event.metadata();
        let tag = meta
            .target()
            .rsplit("::")
            .next()
            .unwrap_or("log")
            .to_string();

        let mut guard = lock_buffer();
        if let Some(buffer) = guard.as_mut() {
            if buffer.entries.len() == CAPACITY {
                buffer.entries.pop_front();
            }
            let time = buffer.start.elapsed().as_secs_f64();
            buffer.total += 1;
            let entry = LogEntry {
                seq: buffer.total,
                time,
                level: meta.level().as_str().to_lowercase(),
                tag,
                message: visitor.message,
                fields: visitor.fields,
            };
            file::append(&entry);
            buffer.entries.push_back(entry);
        }
    }
}

/// Where gilrs times its force-feedback loop, held to errors.
const QUIET_RUMBLE: &str = "gilrs::ff::server=error";

/// Start capturing: stderr output plus the ring buffer, and a bridge so `log`
/// records from dependencies land in the same place.
///
/// Idempotent — a second call is a no-op, which keeps tests from fighting.
#[allow(clippy::disallowed_methods, reason = "log timestamps, not simulation")]
pub fn capture(max_level: LevelFilter) {
    open_buffer();
    let _ = tracing_log::LogTracer::init();
    let filter = EnvFilter::builder()
        .with_default_directive(max_level.into())
        .from_env_lossy()
        // gilrs times its rumble thread and warns whenever the machine is busy:
        // a note about load, not about the game, and a warning fails a test run.
        .add_directive(QUIET_RUMBLE.parse().expect("a fixed directive"));
    #[cfg(not(target_arch = "wasm32"))]
    let fmt = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    // A browser has no stderr and no clock for the timestamp column —
    // `SystemTime::now()` is where a wasm build used to die — so lines go to
    // the console, untimed and unstyled.
    #[cfg(target_arch = "wasm32")]
    let fmt = tracing_subscriber::fmt::layer()
        .without_time()
        .with_ansi(false)
        .with_writer(console::Console);
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt)
        .with(CaptureLayer)
        .try_init();
}

/// The browser console as a `tracing` writer: the fmt layer asks for a
/// writer per event and writes the whole line to it, so each one is a
/// `console.log` call when it is dropped.
#[cfg(target_arch = "wasm32")]
mod console {
    use std::io::{self, Write};

    pub(super) struct Console;

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Console {
        type Writer = Line;

        fn make_writer(&'a self) -> Line {
            Line(Vec::new())
        }
    }

    pub(super) struct Line(Vec<u8>);

    impl Write for Line {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Drop for Line {
        fn drop(&mut self) {
            if !self.0.is_empty() {
                let line = String::from_utf8_lossy(&self.0);
                web_sys::console::log_1(&line.trim_end().into());
            }
        }
    }
}

/// Capture only, without stderr output. For tests.
pub fn capture_for_test() {
    open_buffer();
    let _ = tracing_subscriber::registry().with(CaptureLayer).try_init();
}

/// The buffer, opened if it is not open already.
///
/// Emphatically not a reset: the buffer is one per process, and the tests of
/// one binary run in parallel threads that each ask for capture. Emptying it
/// here would let one test wipe the lines another was still counting, which
/// is a failure that only shows up on a machine scheduling them differently.
/// A caller that wants a fresh start calls [`clear`], and they all do.
#[allow(clippy::disallowed_methods, reason = "log timestamps, not simulation")]
fn open_buffer() {
    let mut guard = lock_buffer();
    if guard.is_none() {
        *guard = Some(Buffer {
            start: Instant::now(),
            entries: VecDeque::new(),
            total: 0,
        });
    }
}

/// The most recent `n` entries, oldest first.
pub fn recent(n: usize) -> Vec<LogEntry> {
    let guard = lock_buffer();
    guard.as_ref().map_or_else(Vec::new, |buffer| {
        let skip = buffer.entries.len().saturating_sub(n);
        buffer.entries.iter().skip(skip).cloned().collect()
    })
}

/// What was captured after `cursor`, oldest first, the cursor to pass next,
/// and how many the ring dropped before they could be read.
///
/// `cursor` is a `seq`; 0 reads everything the ring still holds.
pub fn since(cursor: u64) -> (Vec<LogEntry>, u64, u64) {
    let guard = lock_buffer();
    let Some(buffer) = guard.as_ref() else {
        return (Vec::new(), cursor, 0);
    };
    let entries: Vec<LogEntry> = buffer
        .entries
        .iter()
        .filter(|entry| entry.seq > cursor)
        .cloned()
        .collect();
    let first = entries.first().map_or(buffer.total + 1, |entry| entry.seq);
    let missed = first.saturating_sub(cursor + 1);
    (entries, buffer.total, missed)
}

/// How many entries have been captured since `capture`, evicted ones
/// included: a reader compares two values to learn whether anything is new.
pub fn total() -> u64 {
    lock_buffer().as_ref().map_or(0, |buffer| buffer.total)
}

pub fn clear() {
    if let Some(buffer) = lock_buffer().as_mut() {
        buffer.entries.clear();
    }
}

/// Whether this thread is reporting `key` under `site` for the first time. A
/// problem hit every frame is logged once, not sixty times a second.
pub fn first_time(site: &'static str, key: &str) -> bool {
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    thread_local! {
        static SAID: RefCell<HashMap<&'static str, HashSet<String>>> = RefCell::new(HashMap::new());
    }
    SAID.with_borrow_mut(|said| {
        let keys = said.entry(site).or_default();
        !keys.contains(key) && keys.insert(key.to_owned())
    })
}

pub use file::{open as open_file, path as file_path};

/// The same stream teed to a file, so the run that crashed leaves its lines
/// behind. Native only: a browser has nowhere to write one.
mod file {
    #[cfg(not(target_family = "wasm"))]
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::LogEntry;

    struct Sink {
        path: PathBuf,
        #[cfg(not(target_family = "wasm"))]
        file: std::fs::File, // os files: native only
    }

    static SINK: Mutex<Option<Sink>> = Mutex::new(None);

    fn lock() -> std::sync::MutexGuard<'static, Option<Sink>> {
        SINK.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Where the file is, once one is open.
    pub fn path() -> Option<PathBuf> {
        lock().as_ref().map(|sink| sink.path.clone())
    }

    /// Start writing `<dir>/<name>.log`, keeping the last `keep` runs as
    /// `<name>.1.log` and so on, and write a panic there before it unwinds.
    ///
    /// # Errors
    /// When the directory or the file cannot be made.
    #[cfg(not(target_family = "wasm"))]
    pub fn open(dir: &Path, name: &str, keep: usize) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(dir)?; // os files: native only
        let path = dir.join(format!("{name}.log"));
        rotate(dir, name, keep);
        let mut file = std::fs::File::create(&path)?; // os files: native only
        // What was logged before the file opened, `init` included, goes first.
        for entry in &super::since(0).0 {
            let _ = writeln!(file, "{}", line_of(entry));
        }
        let had = lock().replace(Sink {
            path: path.clone(),
            file,
        });
        if had.is_none() {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if let Some(sink) = lock().as_mut() {
                    let _ = writeln!(sink.file, "panic {info}");
                    let _ = sink.file.flush();
                }
                previous(info);
            }));
        }
        Ok(path)
    }

    #[cfg(target_family = "wasm")]
    pub fn open(_dir: &Path, _name: &str, _keep: usize) -> anyhow::Result<PathBuf> {
        anyhow::bail!("a browser build keeps no log file")
    }

    /// `<name>.log` becomes `<name>.1.log`, and so on, the oldest dropped.
    #[cfg(not(target_family = "wasm"))]
    fn rotate(dir: &Path, name: &str, keep: usize) {
        let at = |n: usize| {
            if n == 0 {
                dir.join(format!("{name}.log"))
            } else {
                dir.join(format!("{name}.{n}.log"))
            }
        };
        if keep == 0 {
            return;
        }
        let _ = std::fs::remove_file(at(keep)); // os files: native only
        for n in (0..keep).rev() {
            let _ = std::fs::rename(at(n), at(n + 1)); // os files: native only
        }
    }

    /// One line per entry: elapsed seconds, level, tag, message, fields.
    #[cfg(not(target_family = "wasm"))]
    fn line_of(entry: &LogEntry) -> String {
        let mut line = format!(
            "{:10.3} {:5} {}: {}",
            entry.time, entry.level, entry.tag, entry.message
        );
        for (name, value) in &entry.fields {
            let _ = std::fmt::Write::write_fmt(&mut line, format_args!(" {name}={value}"));
        }
        line
    }

    pub(super) fn append(entry: &LogEntry) {
        #[cfg(not(target_family = "wasm"))]
        if let Some(sink) = lock().as_mut() {
            let _ = writeln!(sink.file, "{}", line_of(entry));
            if entry.level == "error" {
                let _ = sink.file.flush();
            }
        }
        #[cfg(target_family = "wasm")]
        let _ = entry;
    }
}
