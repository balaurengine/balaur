//! Telling a loop that sleeps between frames that something arrived.
//!
//! `[window] low_processor` lets the windowed loop sleep until input or a
//! request. Work that finishes on another thread cannot be either, so what
//! hands its result back calls [`wake`]: an `ExternalIo` report, a log line,
//! a file the watcher saw change. The loop reads it with [`take`] and, while
//! it sleeps, is woken through the hook it installed with [`set_hook`].

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

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
