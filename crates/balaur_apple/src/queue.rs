//! Arrivals nobody asked for: a player signing out, a subscription renewing
//! on another device.
//!
//! They cannot go straight down the channel. A recording answers a replay
//! from the file, and anything that reached the channel meanwhile would be
//! taken for recorded input — so unsolicited arrivals wait here until a pump
//! that is allowed to touch the outside world moves them across, and are
//! thrown away by one that is not. Every one of them carries request 0,
//! which no call is ever given: `Engine::next_token` starts at 1.

use std::sync::Mutex;
use std::sync::mpsc::Sender;

use balaur_platform::PlatformEvent;

use crate::AppleEvent;

/// Calls waiting on an answer the OS gives once for all of them, wherever it
/// lands: the push token the application delegate is handed, the sign-in
/// GameKit's handler hears. Each call's channel waits here until then.
#[cfg(any(target_vendor = "apple", test))]
pub(crate) struct Waiters(Mutex<Vec<(u64, Sender<PlatformEvent>)>>);

#[cfg(any(target_vendor = "apple", test))]
impl Waiters {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    /// `request` waits for the next answer.
    pub(crate) fn wait(&self, request: u64, report: &Sender<PlatformEvent>) {
        if let Ok(mut waiting) = self.0.lock() {
            // copy: the channel outlives this call, until the answer crosses it.
            waiting.push((request, report.clone()));
        }
    }

    /// Answer every call waiting, each with the event `answer` makes of its
    /// request; false when none was.
    pub(crate) fn answer(&self, answer: impl Fn(u64) -> PlatformEvent) -> bool {
        let waiting = self
            .0
            .lock()
            .map(|mut waiting| std::mem::take(&mut *waiting))
            .unwrap_or_default();
        for (request, report) in &waiting {
            let _ = report.send(answer(*request));
        }
        !waiting.is_empty()
    }
}

static STORE: Mutex<Vec<PlatformEvent>> = Mutex::new(Vec::new());
static APPLE: Mutex<Vec<AppleEvent>> = Mutex::new(Vec::new());
/// A notification tapped or shown in front: the OS's, drained by the device
/// seam whichever store is loaded.
#[cfg(target_vendor = "apple")]
static DEVICE: Mutex<Vec<PlatformEvent>> = Mutex::new(Vec::new());

#[cfg(target_vendor = "apple")]
pub(crate) fn push_store(event: PlatformEvent) {
    if let Ok(mut queue) = STORE.lock() {
        queue.push(event);
    }
}

#[cfg(target_vendor = "apple")]
pub(crate) fn push_apple(event: AppleEvent) {
    if let Ok(mut queue) = APPLE.lock() {
        queue.push(event);
    }
}

#[cfg(target_vendor = "apple")]
pub(crate) fn push_device(event: PlatformEvent) {
    if let Ok(mut queue) = DEVICE.lock() {
        queue.push(event);
    }
}

#[cfg(target_vendor = "apple")]
pub(crate) fn drain_device(report: &Sender<PlatformEvent>) {
    if let Ok(mut queue) = DEVICE.lock() {
        for event in queue.drain(..) {
            balaur_core::replay::report(report, event);
        }
    }
}

#[cfg(target_vendor = "apple")]
pub(crate) fn discard_device() {
    if let Ok(mut queue) = DEVICE.lock() {
        queue.clear();
    }
}

pub(crate) fn drain_store(report: &Sender<PlatformEvent>) {
    if let Ok(mut queue) = STORE.lock() {
        for event in queue.drain(..) {
            balaur_core::replay::report(report, event);
        }
    }
}

pub(crate) fn drain_apple(report: &Sender<AppleEvent>) {
    if let Ok(mut queue) = APPLE.lock() {
        for event in queue.drain(..) {
            balaur_core::replay::report(report, event);
        }
    }
}

/// What arrived while the engine was replaying or re-simulating is not this
/// run's to deliver.
pub(crate) fn discard_store() {
    if let Ok(mut queue) = STORE.lock() {
        queue.clear();
    }
}

pub(crate) fn discard_apple() {
    if let Ok(mut queue) = APPLE.lock() {
        queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use balaur_platform::PlatformEvent;

    use super::Waiters;

    // Two calls made before the OS answers both hear its one answer, and an
    // answer with nobody waiting reaches nobody.
    #[test]
    fn every_call_waiting_hears_the_one_answer_once() {
        let waiters = Waiters::new();
        let (report, heard) = std::sync::mpsc::channel();
        waiters.wait(3, &report);
        waiters.wait(4, &report);
        assert!(waiters.answer(|request| PlatformEvent::Cancelled { request }));
        let answered: Vec<u64> = heard
            .try_iter()
            .map(|event| match event {
                PlatformEvent::Cancelled { request } => request,
                _ => 0,
            })
            .collect();
        assert_eq!(answered, [3, 4]);
        assert!(!waiters.answer(|request| PlatformEvent::Cancelled { request }));
        assert!(heard.try_recv().is_err());
    }
}
