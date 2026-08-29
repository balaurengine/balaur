//! In-engine log capture: a bounded ring buffer that tees every `log` record
//! so tools (the editor's Output dock, in-game consoles) can display them.
//!
//! The buffer is process-global because the `log` facade is process-global;
//! `install` replaces `env_logger` in binaries that want capture.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

const CAPACITY: usize = 500;

#[derive(Clone)]
pub struct LogEntry {
    /// Seconds since logger installation.
    pub time: f64,
    /// "info", "warn", "error", "debug", "trace".
    pub level: String,
    /// Shortened module path (last segment), used as the tag column.
    pub tag: String,
    pub message: String,
}

struct Buffer {
    start: Instant,
    entries: VecDeque<LogEntry>,
}

static BUFFER: Mutex<Option<Buffer>> = Mutex::new(None);

struct TeeLogger {
    filter: log::LevelFilter,
}

/// Lock the buffer, recovering from poisoning.
///
/// A panic while holding this lock must not make every later log call panic
/// too: losing the buffer's contents is acceptable, losing the process is not.
fn lock_buffer() -> std::sync::MutexGuard<'static, Option<Buffer>> {
    BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl log::Log for TeeLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.filter
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let tag = record
            .target()
            .rsplit("::")
            .next()
            .unwrap_or("log")
            .to_string();
        eprintln!(
            "[{} {}] {}",
            record.level().as_str(),
            record.target(),
            record.args()
        );
        let mut guard = lock_buffer();
        if let Some(buffer) = guard.as_mut() {
            if buffer.entries.len() == CAPACITY {
                buffer.entries.pop_front();
            }
            let time = buffer.start.elapsed().as_secs_f64();
            buffer.entries.push_back(LogEntry {
                time,
                level: record.level().as_str().to_lowercase(),
                tag,
                message: record.args().to_string(),
            });
        }
    }

    fn flush(&self) {}
}

/// Install the capturing logger. `max_level`: e.g. `log::LevelFilter::Info`.
pub fn install(max_level: log::LevelFilter) {
    *lock_buffer() = Some(Buffer {
        start: Instant::now(),
        entries: VecDeque::new(),
    });
    let _ = log::set_boxed_logger(Box::new(TeeLogger { filter: max_level }));
    log::set_max_level(max_level);
}

/// The most recent `n` entries, oldest first.
pub fn recent(n: usize) -> Vec<LogEntry> {
    let guard = lock_buffer();
    match guard.as_ref() {
        Some(buffer) => {
            let skip = buffer.entries.len().saturating_sub(n);
            buffer.entries.iter().skip(skip).cloned().collect()
        }
        None => Vec::new(),
    }
}

pub fn clear() {
    if let Some(buffer) = lock_buffer().as_mut() {
        buffer.entries.clear();
    }
}
