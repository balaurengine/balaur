//! What this process's signature claims, for the services only an
//! entitlement unlocks: push, Sign in with Apple, Game Center, and the App
//! Store's purchases.
//!
//! A Mac reads its own signature, since a build signed against a Developer ID
//! profile lacks what that profile does not grant (Sign in with Apple, Game
//! Center), and the export leaves it out rather than fail. StoreKit sells
//! only to a Mac App Store build, which the export alone sandboxes, so the
//! sandbox is what says a Mac build is the store's. iOS has no public call for
//! this, and needs none: its profiles grant what the App ID has, and the
//! export refuses one that does not.

/// Push's entitlement on a Mac.
pub(crate) const APS_MACOS: &str = "com.apple.developer.aps-environment";
/// Sign in with Apple's, on both.
pub(crate) const APPLESIGNIN: &str = "com.apple.developer.applesignin";
/// Game Center's, on both.
pub(crate) const GAME_CENTER: &str = "com.apple.developer.game-center";
/// The Mac App Store build's sandbox, which no Developer ID build claims.
pub(crate) const APP_SANDBOX: &str = "com.apple.security.app-sandbox";

/// The entitlement a Mac build must claim for `call`'s service to answer
/// it: Game Center's for its verbs, the store's sandbox for a purchase.
pub(crate) const fn needed_for(call: &balaur_platform::Call) -> Option<&'static str> {
    use balaur_platform::Call;
    match call {
        Call::SignIn
        | Call::Unlock { .. }
        | Call::Progress { .. }
        | Call::SubmitScore { .. }
        | Call::Scores { .. } => Some(GAME_CENTER),
        Call::Products { .. }
        | Call::Purchase { .. }
        | Call::FinishPurchase { .. }
        | Call::RestorePurchases
        | Call::Entitlements => Some(APP_SANDBOX),
        _ => None,
    }
}

/// Whether this build's signature lets `call`'s service answer it.
pub(crate) fn serves(call: &balaur_platform::Call) -> bool {
    needed_for(call).is_none_or(granted)
}

/// Whether the signature claims `entitlement`.
#[cfg(target_os = "macos")]
pub(crate) fn granted(entitlement: &str) -> bool {
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

    let name = NSString::from_str(entitlement);
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

/// Whether the signature claims `entitlement`: on iOS, taken as so.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn granted(_entitlement: &str) -> bool {
    true
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use balaur_platform::Call;

    use super::{APP_SANDBOX, APPLESIGNIN, GAME_CENTER, granted, needed_for, serves};

    // A test binary is signed ad hoc, with no entitlement at all: a Mac build
    // without Sign in with Apple reads the same.
    #[test]
    fn an_unentitled_process_claims_no_sign_in_with_apple() {
        assert!(!granted(APPLESIGNIN));
    }

    // A Developer ID Mac build, unsandboxed and without Game Center's
    // entitlement, answers neither the store's verbs nor Game Center's; the
    // rest go where they always did.
    #[test]
    fn a_mac_without_the_stores_entitlements_answers_neither() {
        assert_eq!(needed_for(&Call::SignIn), Some(GAME_CENTER));
        assert_eq!(needed_for(&Call::RestorePurchases), Some(APP_SANDBOX));
        assert!(!serves(&Call::RestorePurchases));
        assert!(!serves(&Call::SignIn));
        assert!(serves(&Call::Identity));
    }
}
