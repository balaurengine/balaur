//! Cloud saves through the iCloud key-value store.
//!
//! The store is the right size for what `save` writes — a small file, synced
//! between the player's own devices — and the entitlement is one key. A
//! record database is not what a save game is; docs/PLAN-apple.md says why
//! CloudKit is not planned.

use std::ptr::NonNull;
use std::sync::mpsc::Sender;

use balaur_platform::PlatformEvent;
use block2::RcBlock;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2_foundation::{
    NSArray, NSNotification, NSNotificationCenter, NSString, NSUbiquitousKeyValueStore,
    NSUbiquitousKeyValueStoreChangeReasonKey, NSUbiquitousKeyValueStoreChangedKeysKey,
    NSUbiquitousKeyValueStoreDidChangeExternallyNotification,
};

use crate::AppleEvent;

/// Why the store changed, in the order Apple numbers the reasons.
const REASONS: [&str; 4] = ["server", "initial_sync", "quota", "account"];

/// Hear about another device changing the store, once. Asking the store to
/// synchronise is what starts the changes flowing.
pub(crate) fn watch_changes() -> bool {
    if OBSERVER.with_borrow(Option::is_some) {
        return true;
    }
    let changed = RcBlock::new(move |note: NonNull<NSNotification>| {
        let info: *mut AnyObject = unsafe { msg_send![note.as_ptr(), userInfo] };
        let Some(info) = (unsafe { info.as_ref() }) else {
            return;
        };
        let reason: *mut AnyObject =
            unsafe { msg_send![info, objectForKey: NSUbiquitousKeyValueStoreChangeReasonKey] };
        let code: isize = unsafe { reason.as_ref() }
            .map_or(-1, |reason| unsafe { msg_send![reason, integerValue] });
        let keys: *mut NSArray<NSString> =
            unsafe { msg_send![info, objectForKey: NSUbiquitousKeyValueStoreChangedKeysKey] };
        let keys = unsafe { keys.as_ref() }.map_or_else(Vec::new, |keys| {
            (0..keys.count())
                .map(|at| keys.objectAtIndex(at).to_string())
                .collect()
        });
        let reason = usize::try_from(code)
            .ok()
            .and_then(|at| REASONS.get(at))
            .map_or_else(|| code.to_string(), |word| (*word).to_string());
        crate::queue::push_apple(AppleEvent::CloudChanged { reason, keys });
    });
    let store = NSUbiquitousKeyValueStore::defaultStore();
    let token = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSUbiquitousKeyValueStoreDidChangeExternallyNotification),
            Some(&store),
            None,
            &changed,
        )
    };
    store.synchronize();
    OBSERVER.with_borrow_mut(|held| *held = Some(token));
    true
}

thread_local! {
    static OBSERVER: std::cell::RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> =
        const { std::cell::RefCell::new(None) };
}

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
