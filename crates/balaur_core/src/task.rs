//! Work that leaves the tick, and how it gets off one on each machine.
//!
//! [`crate::replay::ExternalIo`] is how such work reports *back* into a tick,
//! recorded so a replay sees the same arrivals. This is the other half: how it
//! leaves. Every subsystem that has both writes the leaving out itself, once
//! per target — `balaur_http` in three, `balaur_gamend` and the export verb in
//! two each — and this is that, once.
//!
//! The shape here is for work that can be cut into slices, which is what file
//! work is. A desktop has threads and a browser tab has none, so a tab that
//! ran an import to completion in one call would not paint until it finished;
//! a slice per frame keeps the page alive and is also where a progress event
//! belongs. A caller waiting on a socket or a fetch is not this shape and
//! keeps its own future.
//!
//! **What a job may hold.** On a desktop it moves to a thread, so it is `Send`
//! there and not on the web. The file backend is an `Rc` and never `Send`, so
//! a job reads it where it runs rather than carrying one:
//! [`crate::files::default_backend`] answers per thread, and a fresh thread
//! gets the disk.

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::Engine;

/// How far a slice of stepped work got.
#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    /// There is more to do, so step it again.
    More,
    /// Nothing is left; the job is dropped.
    Done,
}

/// Work that runs in slices rather than in one call.
///
/// A slice is whatever the work can finish without holding anything up — one
/// file, for an import. What it costs is the implementation's to know, so the
/// size is its choice.
pub trait Stepped {
    /// Do the next slice.
    fn step(&mut self) -> Progress;
}

/// Jobs started and not yet finished, across every thread.
///
/// One counter for both targets, so `running()` answers the same question
/// whether the work is on a thread or parked for the pump.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Where [`step`] parks a job on a target with no threads, and where
    /// [`advance_parked_system`] takes them from.
    static PARKED: RefCell<Vec<Box<dyn Stepped>>> = const { RefCell::new(Vec::new()) };
}

/// Advance `work` from the tick: one slice per frame, on the thread that
/// asked, on every target.
///
/// What a browser gets from [`step`], and what a desktop caller may want
/// anyway: a job under the tick can be cancelled with a flag rather than a
/// signal, and its reports arrive in frame order.
pub fn park(work: impl Stepped + 'static) {
    RUNNING.fetch_add(1, Ordering::Relaxed);
    PARKED.with_borrow_mut(|parked| parked.push(Box::new(work)));
}

/// Run `work` off the tick, a slice at a time.
///
/// On a desktop it runs to completion on the stepped-work pool, a thread per
/// core, which is why it is `Send` here and not on the web; more jobs than
/// cores wait their turn. In a browser there is no thread to take, so this
/// is [`park`] and the pump advances it.
#[cfg(not(target_family = "wasm"))]
pub fn step(work: impl Stepped + Send + 'static) {
    static STEPPED: std::sync::OnceLock<Pool> = std::sync::OnceLock::new();
    let mut work = work;
    RUNNING.fetch_add(1, Ordering::Relaxed);
    STEPPED.get_or_init(|| Pool::new("balaur-step")).submit(
        move || {
            while work.step() == Progress::More {}
            RUNNING.fetch_sub(1, Ordering::Relaxed);
        },
        cores(),
    );
}

/// How many threads a pool of CPU work takes: one a core.
#[cfg(not(target_family = "wasm"))]
fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

/// Run `work` off the tick, a slice at a time. See the native twin.
///
/// A tab parks it even where it has workers: the tab's filesystem lives on
/// this thread, and a job on a worker would find none of it. [`compute`] is
/// how such a job hands a worker the part with no files in it.
#[cfg(target_family = "wasm")]
pub fn step(work: impl Stepped + 'static) {
    park(work);
}

/// Run `work` where it cannot hold up a frame, answering on the channel.
///
/// For work that touches no file: a parse or an encode. A desktop gives it the
/// compute pool, a thread per core, apart from stepped work so a job waiting
/// on an answer never holds the thread that would compute it. A tab built
/// with shared memory gives it a worker from the page's pool; a tab without
/// one runs it here, so the answer is waiting on return.
pub fn compute<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> std::sync::mpsc::Receiver<T> {
    // One answer, read by the caller's next slice; a dropped caller drops it.
    let (answer, answered) = std::sync::mpsc::channel();
    let run = move || {
        let _ = answer.send(work());
        crate::wake::wake();
    };
    #[cfg(not(target_family = "wasm"))]
    {
        static COMPUTE: std::sync::OnceLock<Pool> = std::sync::OnceLock::new();
        COMPUTE
            .get_or_init(|| Pool::new("balaur-compute"))
            .submit(run, cores());
    }
    #[cfg(all(target_family = "wasm", target_feature = "atomics"))]
    rayon::spawn(run);
    #[cfg(all(target_family = "wasm", not(target_feature = "atomics")))]
    run();
    answered
}

/// Advance every parked job by one slice, dropping the ones that finished.
///
/// The app registers this at `Stage::First`. Nothing is ever parked on a
/// desktop, where it costs one empty borrow.
pub fn advance_parked_system(_: &Engine, _: f32) {
    let taken = PARKED.with_borrow_mut(std::mem::take);
    if taken.is_empty() {
        return;
    }
    let mut left = Vec::new();
    for mut job in taken {
        if job.step() == Progress::More {
            left.push(job);
        } else {
            RUNNING.fetch_sub(1, Ordering::Relaxed);
        }
    }
    // A slice may have started a job of its own, which is parked behind the
    // ones that were already running rather than losing its place.
    PARKED.with_borrow_mut(|parked| {
        let started = std::mem::take(parked);
        *parked = left;
        parked.extend(started);
    });
}

/// How many jobs are started and not yet finished.
#[must_use]
pub fn running() -> usize {
    RUNNING.load(Ordering::Relaxed)
}

/// Threads taking work off one queue, first in first out.
///
/// Up to `limit` jobs run at once, the limit a submit names, and the rest
/// wait their turn. A worker sleeps while the queue is empty. Dropping the
/// pool drops what still waits, and each worker ends once its current job
/// does.
#[cfg(not(target_family = "wasm"))]
pub struct Pool {
    shared: std::sync::Arc<Shared>,
    name: String,
}

#[cfg(not(target_family = "wasm"))]
type Job = Box<dyn FnOnce() + Send>;

#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
struct Queue {
    waiting: std::collections::VecDeque<Job>,
    workers: usize,
    idle: usize,
    closed: bool,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
struct Shared {
    queue: std::sync::Mutex<Queue>,
    /// Signalled when a job is queued, and when the pool closes.
    queued: std::sync::Condvar,
}

#[cfg(not(target_family = "wasm"))]
impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(not(target_family = "wasm"))]
impl Pool {
    /// An empty pool; its threads, named `name` and a number, start as work
    /// arrives.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            shared: std::sync::Arc::default(),
            name: name.to_string(),
        }
    }

    /// Queue `job`. A worker takes it at once while fewer than `limit` are
    /// busy; otherwise it waits behind the jobs queued before it.
    pub fn submit(&self, job: impl FnOnce() + Send + 'static, limit: usize) {
        let spawn = {
            let mut queue = self.shared.lock();
            queue.waiting.push_back(Box::new(job));
            let spawn = queue.idle == 0 && queue.workers < limit.max(1);
            if spawn {
                queue.workers += 1;
            }
            spawn.then_some(queue.workers)
        };
        if let Some(number) = spawn {
            let shared = std::sync::Arc::clone(&self.shared);
            let started = std::thread::Builder::new()
                .name(format!("{}-{number}", self.name))
                .spawn(move || work(&shared));
            if let Err(err) = started {
                self.shared.lock().workers -= 1;
                tracing::warn!(%err, pool = %self.name, "no thread for the pool");
            }
        }
        self.shared.queued.notify_one();
    }

    /// Jobs queued and not yet taken by a worker.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.shared.lock().waiting.len()
    }
}

#[cfg(not(target_family = "wasm"))]
impl Drop for Pool {
    fn drop(&mut self) {
        let mut queue = self.shared.lock();
        queue.closed = true;
        queue.waiting.clear();
        drop(queue);
        self.shared.queued.notify_all();
    }
}

/// One worker: take the next job, run it, and sleep when there is none.
#[cfg(not(target_family = "wasm"))]
fn work(shared: &Shared) {
    let mut queue = shared.lock();
    loop {
        if queue.closed {
            queue.workers -= 1;
            return;
        }
        if let Some(job) = queue.waiting.pop_front() {
            drop(queue);
            // A job that panics is its own failure: the worker lives on, so
            // the pool keeps every thread it counts.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                tracing::error!("a pooled job panicked");
            }
            queue = shared.lock();
            continue;
        }
        queue.idle += 1;
        queue = shared
            .queued
            .wait(queue)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.idle -= 1;
    }
}
