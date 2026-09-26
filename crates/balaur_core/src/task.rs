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
/// On a desktop it takes a thread of its own and runs to completion there,
/// which is why it is `Send` here and not on the web. In a browser there is no
/// thread to take, so this is [`park`] and the pump advances it.
#[cfg(not(target_family = "wasm"))]
pub fn step(work: impl Stepped + Send + 'static) {
    let mut work = work;
    RUNNING.fetch_add(1, Ordering::Relaxed);
    std::thread::spawn(move || {
        while work.step() == Progress::More {}
        RUNNING.fetch_sub(1, Ordering::Relaxed);
    });
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
/// For work that touches no file: a parse or an encode. A desktop gives it a
/// thread and a tab built with shared memory a worker from the page's pool; a
/// tab without one runs it here, so the answer is waiting on return.
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
    std::thread::spawn(run);
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
