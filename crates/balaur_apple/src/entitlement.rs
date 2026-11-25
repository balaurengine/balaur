//! What this process's signature claims, for the services only an
//! entitlement unlocks: push, and Sign in with Apple.
//!
//! A Mac reads its own signature, since a build signed against a Developer ID
//! profile lacks what that profile does not grant (Sign in with Apple), and
//! the export leaves it out rather than fail. iOS has no public call for
//! this, and needs none: its profiles grant what the App ID has, and the
//! export refuses one that does not.

/// Push's entitlement on a Mac.
pub(crate) const APS_MACOS: &str = "com.apple.developer.aps-environment";
/// Sign in with Apple's, on both.
pub(crate) const APPLESIGNIN: &str = "com.apple.developer.applesignin";

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
    use super::{APPLESIGNIN, granted};

    // A test binary is signed ad hoc, with no entitlement at all: a Mac build
    // without Sign in with Apple reads the same.
    #[test]
    fn an_unentitled_process_claims_no_sign_in_with_apple() {
        assert!(!granted(APPLESIGNIN));
    }
}
