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

#[cfg(all(test, not(target_family = "wasm")))]
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
    fn a_send_to_a_finished_worker_says_so() {
        let (commands, worker) = worker::<u32>().unwrap();
        drop(worker);
        assert!(!commands.send(1));
    }
}
