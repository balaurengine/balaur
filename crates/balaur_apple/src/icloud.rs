//! Cloud saves through the iCloud key-value store.
//!
//! The store is the right size for what `save` writes — a small file, synced
//! between the player's own devices — and the entitlement is one key. A
//! record database is not what a save game is; docs/PLAN-apple.md says why
//! CloudKit is not planned.

use std::sync::mpsc::Sender;

use balaur_platform::PlatformEvent;
use objc2_foundation::{NSString, NSUbiquitousKeyValueStore};

pub(crate) fn read(request: u64, key: &str, report: &Sender<PlatformEvent>) {
    let store = NSUbiquitousKeyValueStore::defaultStore();
    let value = store.stringForKey(&NSString::from_str(key));
    let _ = report.send(PlatformEvent::Read {
        request,
        key: key.to_string(),
        value: value.map(|value| value.to_string()),
    });
}

pub(crate) fn write(request: u64, key: &str, value: &str, report: &Sender<PlatformEvent>) {
    let store = NSUbiquitousKeyValueStore::defaultStore();
    store.setString_forKey(Some(&NSString::from_str(value)), &NSString::from_str(key));
    // `synchronize` only schedules the upload; it fails when the app has no
    // iCloud entitlement, which is the mistake worth reporting.
    let event = if store.synchronize() {
        PlatformEvent::Done {
            request,
            call: "cloud_write".into(),
        }
    } else {
        PlatformEvent::Failed {
            request,
            message: "the iCloud key-value store refused the write: check the \
                      icloud-kv capability and the entitlement it writes"
                .into(),
        }
    };
    let _ = report.send(event);
}
