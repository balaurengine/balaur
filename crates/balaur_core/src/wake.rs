//! Telling a sleeping loop that something arrived, in either direction.
//!
//! `[window] low_processor` lets the windowed loop sleep until input or a
//! request. Work that finishes on another thread cannot be either, so what
//! hands its result back calls [`wake`]: an `ExternalIo` report, a log line,
//! a file the watcher saw change. The loop reads it with [`take`] and, while
//! it sleeps, is woken through the hook it installed with [`set_hook`].
//!
//! The other way round, an I/O thread sleeps in a `mio::Poll` until its
//! socket is ready or the engine queues work for it; [`Commands`] is that
//! queue, and sending on it wakes the thread.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

static WOKEN: AtomicBool = AtomicBool::new(false);

type Hook = Box<dyn Fn() + Send>;

static HOOK: Mutex<Option<Hook>> = Mutex::new(None);

/// Say that something arrived which the next frame should see. Callable from
/// any thread, and cheap when nothing sleeps.
pub fn wake() {
    if WOKEN.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(hook) = HOOK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        hook();
    }
}

/// Wake the sleeping loop at `when`, for work due then that nothing else
/// would wake it for: a heartbeat, a reconnect back-off. A browser tab keeps
/// its frames coming, so there it does nothing.
pub fn at(when: crate::time::Instant) {
    #[cfg(not(target_family = "wasm"))]
    {
        static TIMER: std::sync::OnceLock<Timer> = std::sync::OnceLock::new();
        TIMER.get_or_init(|| Timer::start(wake)).schedule(when);
    }
    #[cfg(target_family = "wasm")]
    let _ = when;
}

/// A thread asleep until the earliest deadline it holds, which then calls
/// `due`. One for the process, so any number of deadlines costs one thread.
#[cfg(not(target_family = "wasm"))]
struct Timer {
    shared: std::sync::Arc<(
        Mutex<std::collections::BTreeSet<std::time::Instant>>,
        std::sync::Condvar,
    )>,
}

#[cfg(not(target_family = "wasm"))]
impl Timer {
    #[allow(
        clippy::disallowed_methods,
        reason = "a deadline to wake the loop at; never a simulation input"
    )]
    fn start(due: impl Fn() + Send + 'static) -> Self {
        let shared = std::sync::Arc::new((
            Mutex::new(std::collections::BTreeSet::<std::time::Instant>::new()),
            std::sync::Condvar::new(),
        ));
        let theirs = std::sync::Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("balaur-wake-timer".into())
            .spawn(move || {
                let (deadlines, changed) = &*theirs;
                let mut held = deadlines
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                loop {
                    let now = std::time::Instant::now();
                    let passed = held.first().is_some_and(|first| *first <= now);
                    if passed {
                        held.retain(|deadline| *deadline > now);
                        drop(held);
                        due();
                        held = deadlines
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        continue;
                    }
                    held = match held.first() {
                        Some(first) => {
                            let left = first.saturating_duration_since(now);
                            changed
                                .wait_timeout(held, left)
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .0
                        }
                        None => changed
                            .wait(held)
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                    };
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "no timer thread: deadlines will wait for other wakes");
        }
        Self { shared }
    }

    fn schedule(&self, when: std::time::Instant) {
        let (deadlines, changed) = &*self.shared;
        let earliest = {
            let mut held = deadlines
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let earliest = held.first().is_none_or(|first| when < *first);
            held.insert(when);
            earliest
        };
        if earliest {
            changed.notify_one();
        }
    }
}

/// Whether anything called [`wake`] since the last call, clearing it.
pub fn take() -> bool {
    WOKEN.swap(false, Ordering::AcqRel)
}

/// What [`wake`] calls to end the sleeping loop's wait; `None` removes it.
pub fn set_hook(hook: Option<Hook>) {
    *HOOK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = hook;
}

/// The engine's end of a thread's work queue. A send also fires the thread's
/// waker, so the thread sleeps until then instead of checking the queue.
pub struct Commands<T> {
    sender: Sender<T>,
    #[cfg(not(target_family = "wasm"))]
    waker: Option<std::sync::Arc<mio::Waker>>,
}

impl<T> Commands<T> {
    /// Queue `command` and wake the thread. False once the thread is gone.
    pub fn send(&self, command: T) -> bool {
        let sent = self.sender.send(command).is_ok();
        #[cfg(not(target_family = "wasm"))]
        if sent && let Some(waker) = &self.waker {
            let _ = waker.wake();
        }
        sent
    }
}

impl<T> Clone for Commands<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            #[cfg(not(target_family = "wasm"))]
            waker: self.waker.clone(),
        }
    }
}

/// A queue with no thread behind it, for a backend its tick drains: the
/// browser, whose sockets call back on the page's own events.
#[must_use]
pub fn queue<T>() -> (Commands<T>, Receiver<T>) {
    let (sender, commands) = channel();
    (
        Commands {
            sender,
            #[cfg(not(target_family = "wasm"))]
            waker: None,
        },
        commands,
    )
}

/// The token a [`Worker`]'s poll reports when its queue woke it; every
/// other token is the thread's own to hand out.
#[cfg(not(target_family = "wasm"))]
pub const QUEUED: mio::Token = mio::Token(usize::MAX);

/// A thread's end of its queue, and the poll it sleeps in until the queue
/// wakes it ([`QUEUED`]) or a source it registered is ready.
#[cfg(not(target_family = "wasm"))]
pub struct Worker<T> {
    pub poll: mio::Poll,
    pub commands: Receiver<T>,
}

/// A queue whose sends wake the thread that holds the [`Worker`].
///
/// # Errors
/// When the OS will not make a poll or a waker: out of file descriptors.
#[cfg(not(target_family = "wasm"))]
pub fn worker<T>() -> std::io::Result<(Commands<T>, Worker<T>)> {
    let poll = mio::Poll::new()?;
    let waker = std::sync::Arc::new(mio::Waker::new(poll.registry(), QUEUED)?);
    let (sender, commands) = channel();
    Ok((
        Commands {
            sender,
            waker: Some(waker),
        },
        Worker { poll, commands },
    ))
}

// The native half only: a worker and a timer are threads.
#[cfg(test)]
#[cfg(not(target_family = "wasm"))]
mod tests {
    use super::*;

    #[test]
    fn a_send_wakes_a_worker_sleeping_with_no_timeout() {
        let (commands, mut worker) = worker::<u32>().unwrap();
        let sleeper = std::thread::spawn(move || {
            let mut events = mio::Events::with_capacity(4);
            worker.poll.poll(&mut events, None).unwrap();
            let woke = events.iter().any(|event| event.token() == QUEUED);
            (woke, worker.commands.try_recv().ok())
        });
        assert!(commands.send(7));
        assert_eq!(sleeper.join().unwrap(), (true, Some(7)));
    }

    #[test]
    #[allow(clippy::disallowed_methods, reason = "a test's deadlines")]
    fn a_timer_fires_its_deadlines_earliest_first() {
        let (fired, heard) = channel();
        let timer = Timer::start(move || {
            let _ = fired.send(std::time::Instant::now());
        });
        let start = std::time::Instant::now();
        let late = start + std::time::Duration::from_millis(80);
        let soon = start + std::time::Duration::from_millis(20);
        timer.schedule(late);
        timer.schedule(soon);
        let wait = std::time::Duration::from_secs(5);
        let first = heard.recv_timeout(wait).unwrap();
        assert!(first >= soon, "nothing fired before the earlier deadline");
        // A thread held up past both deadlines calls once for the two, as
        // one wake of the loop serves both; otherwise the later one follows.
        if first < late {
            let second = heard.recv_timeout(wait).unwrap();
            assert!(second >= late, "then the later one, at its time");
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods, reason = "a test's deadlines")]
    fn a_deadline_already_past_fires_at_once() {
        let (fired, heard) = channel();
        let timer = Timer::start(move || {
            let _ = fired.send(());
        });
        let past = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(1))
            .unwrap();
        timer.schedule(past);
        assert!(
            heard
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok()
        );
    }

    #[test]
    fn a_send_to_a_finished_worker_says_so() {
        let (commands, worker) = worker::<u32>().unwrap();
        drop(worker);
        assert!(!commands.send(1));
    }
}
