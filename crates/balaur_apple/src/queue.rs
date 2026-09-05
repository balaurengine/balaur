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

static STORE: Mutex<Vec<PlatformEvent>> = Mutex::new(Vec::new());
static APPLE: Mutex<Vec<AppleEvent>> = Mutex::new(Vec::new());

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

pub(crate) fn drain_store(report: &Sender<PlatformEvent>) {
    if let Ok(mut queue) = STORE.lock() {
        for event in queue.drain(..) {
            let _ = report.send(event);
        }
    }
}

pub(crate) fn drain_apple(report: &Sender<AppleEvent>) {
    if let Ok(mut queue) = APPLE.lock() {
        for event in queue.drain(..) {
            let _ = report.send(event);
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
