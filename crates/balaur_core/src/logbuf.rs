//! In-engine log capture: a bounded ring buffer that records every `tracing`
//! event so tools (the editor's Output dock, in-game consoles) can display
//! them, and so tests can assert that something was reported.
//!
//! Events keep their structured fields. A test asserts on `fields`, not on the
//! wording of `message`.
//!
//! The buffer is process-global because the subscriber is process-global.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Mutex;
use crate::time::Instant;

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::EnvFilter;

const CAPACITY: usize = 500;

#[derive(Clone, Debug)]
pub struct LogEntry {
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
            buffer.entries.push_back(LogEntry {
                time,
                level: meta.level().as_str().to_lowercase(),
                tag,
                message: visitor.message,
                fields: visitor.fields,
            });
        }
    }
}

/// Start capturing: stderr output plus the ring buffer, and a bridge so `log`
/// records from dependencies land in the same place.
///
/// Idempotent — a second call is a no-op, which keeps tests from fighting.
#[allow(clippy::disallowed_methods, reason = "log timestamps, not simulation")]
pub fn capture(max_level: LevelFilter) {
    *lock_buffer() = Some(Buffer {
        start: Instant::now(),
        entries: VecDeque::new(),
    });
    let _ = tracing_log::LogTracer::init();
    let filter = EnvFilter::builder()
        .with_default_directive(max_level.into())
        .from_env_lossy();
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(CaptureLayer)
        .try_init();
}

/// Capture only, without stderr output. For tests.
#[allow(clippy::disallowed_methods, reason = "log timestamps, not simulation")]
pub fn capture_for_test() {
    *lock_buffer() = Some(Buffer {
        start: Instant::now(),
        entries: VecDeque::new(),
    });
    let _ = tracing_subscriber::registry().with(CaptureLayer).try_init();
}

/// The most recent `n` entries, oldest first.
pub fn recent(n: usize) -> Vec<LogEntry> {
    let guard = lock_buffer();
    guard.as_ref().map_or_else(Vec::new, |buffer| {
        let skip = buffer.entries.len().saturating_sub(n);
        buffer.entries.iter().skip(skip).cloned().collect()
    })
}

pub fn clear() {
    if let Some(buffer) = lock_buffer().as_mut() {
        buffer.entries.clear();
    }
}
