//! Push on Apple, `platform.register_for_push` over APNs: the device token,
//! and a silent push reaching the app.
//!
//! The token reaches the application delegate (`arrivals.rs`), never the
//! call, so a call waits here for the next one the OS hands over. One that
//! arrives with no call waiting, and differs from the last, is the OS
//! replacing it: `push_token_changed`. A push with an alert reaches the game
//! through the notification centre's delegate instead (`notify.rs`).

use std::sync::Mutex;
use std::sync::mpsc::Sender;

use balaur_platform::{PlatformEvent, PushToken};

/// What `register_for_push` says when the bundle was signed without the
/// entitlement, whose name APNs' own error carries.
pub(crate) const MISSING: &str = "push needs [apple] capabilities = [\"push-notifications\"]: \
     this build was signed without the aps-environment entitlement it writes";

/// The entitlement APNs' refusal names when the bundle lacks it.
const ENTITLEMENT: &str = "aps-environment";

/// The `register_for_push` calls the OS has not answered, each with the
/// channel its answer crosses.
static WAITING: Mutex<Vec<(u64, Sender<PlatformEvent>)>> = Mutex::new(Vec::new());

/// The token the OS handed over last.
static LAST: Mutex<Option<String>> = Mutex::new(None);

/// Ask the OS for the device token, answering `request` when it arrives.
pub(crate) fn register(request: u64, report: &Sender<PlatformEvent>) {
    if !entitled() {
        let _ = report.send(PlatformEvent::PushFailed {
            request,
            message: MISSING.into(),
        });
        return;
    }
    if let Ok(mut waiting) = WAITING.lock() {
        // copy: the channel outlives this call, until the token crosses it.
        waiting.push((request, report.clone()));
    }
    if !crate::arrivals::request_push_token() {
        failed("the application delegate is not up yet: ask again on a later tick");
    }
}

/// The OS handed a token over, as hex.
pub(crate) fn token(hex: String) {
    let changed = LAST.lock().map_or(true, |mut last| {
        last.replace(hex.clone()).as_deref() != Some(hex.as_str())
    });
    let waiting = WAITING
        .lock()
        .map(|mut waiting| std::mem::take(&mut *waiting))
        .unwrap_or_default();
    if waiting.is_empty() {
        if changed {
            crate::queue::push_device(PlatformEvent::PushTokenChanged {
                token: PushToken::apns(hex),
            });
        }
        return;
    }
    for (request, report) in waiting {
        let _ = report.send(PlatformEvent::PushRegistered {
            request,
            token: PushToken::apns(hex.clone()),
        });
    }
}

/// The OS would not register: every call waiting hears why, the missing
/// entitlement named as the capability that writes it.
pub(crate) fn failed(message: &str) {
    let message = if message.contains(ENTITLEMENT) {
        MISSING.to_string()
    } else {
        message.to_string()
    };
    let waiting = WAITING
        .lock()
        .map(|mut waiting| std::mem::take(&mut *waiting))
        .unwrap_or_default();
    if waiting.is_empty() {
        tracing::warn!("push: the OS refused to register: {message}");
    }
    for (request, report) in waiting {
        let _ = report.send(PlatformEvent::PushFailed {
            request,
            message: message.clone(),
        });
    }
}

/// A push that reached the app itself. One with an alert is the notification
/// centre's to report, shown in front or tapped; a silent one arrives here
/// alone, with nothing to show.
pub(crate) fn received(payload: serde_json::Value) {
    let alert = payload
        .get(APS)
        .and_then(|aps| aps.get(ALERT))
        .is_some_and(|alert| !alert.is_null());
    if alert {
        return;
    }
    crate::queue::push_device(PlatformEvent::PushReceived {
        id: String::new(),
        title: String::new(),
        body: String::new(),
        data: data_of(payload),
    });
}

/// A push's own data: its payload less `aps`, which is APNs' and the alert's.
pub(crate) fn data_of(mut payload: serde_json::Value) -> serde_json::Value {
    if let Some(fields) = payload.as_object_mut() {
        fields.remove(APS);
    }
    payload
}

const APS: &str = "aps";
const ALERT: &str = "alert";

/// Whether this bundle was signed with the push entitlement. A Mac reads its
/// own signature; iOS has no public call for that, so APNs' refusal, which
/// names the entitlement, says it instead.
#[cfg(target_os = "macos")]
pub(crate) fn entitled() -> bool {
    use std::ffi::c_void;

    use objc2_foundation::NSString;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecTaskCreateFromSelf(allocator: *const c_void) -> *mut c_void;
        fn SecTaskCopyValueForEntitlement(
            task: *mut c_void,
            entitlement: *const c_void,
            error: *mut *mut c_void,
        ) -> *mut c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(object: *const c_void);
    }

    let name = NSString::from_str("com.apple.developer.aps-environment");
    // Safety: a task for this process, released below; the name is an
    // NSString, toll-free bridged to the CFString the call reads.
    unsafe {
        let task = SecTaskCreateFromSelf(std::ptr::null());
        if task.is_null() {
            return false;
        }
        let value = SecTaskCopyValueForEntitlement(
            task,
            std::ptr::from_ref(&*name).cast(),
            std::ptr::null_mut(),
        );
        CFRelease(task);
        if value.is_null() {
            return false;
        }
        CFRelease(value);
        true
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) const fn entitled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::data_of;

    #[test]
    fn a_pushs_data_is_its_payload_less_apns_own_key() {
        let payload = serde_json::json!({ "aps": { "alert": { "title": "Chest" } }, "chest": 3 });
        assert_eq!(data_of(payload), serde_json::json!({ "chest": 3 }));
    }
}
