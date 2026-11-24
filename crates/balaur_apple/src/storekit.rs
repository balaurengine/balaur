//! In-app purchases behind `platform.products` and the rest, over the Swift
//! shim in `swift/`.
//!
//! StoreKit 2 is Swift-only, so this half of the crate is not objc2 but a C
//! ABI of two arguments: a request id out, a JSON object back, already in the
//! shape `platform::Product` and `platform::Transaction` describe.
//!
//! A purchase is not finished when it lands. `platform.finish_purchase` is a
//! call of its own, because the thing that decides a purchase counts is the
//! server that verified its `token` — and a transaction nobody finishes comes
//! back on every launch, which is StoreKit's way of not losing one.

use std::sync::mpsc::Sender;

use balaur_platform::{Call, PlatformEvent};

#[cfg(swift_shim)]
use std::ffi::{CStr, CString, c_char};

#[cfg(swift_shim)]
unsafe extern "C" {
    fn balaur_storekit_products(request: u64, ids: *const c_char);
    fn balaur_storekit_purchase(request: u64, product: *const c_char);
    fn balaur_storekit_entitlements(request: u64);
    fn balaur_storekit_restore(request: u64);
    fn balaur_storekit_finish(request: u64, transaction: *const c_char);
    fn balaur_storekit_listen();
}

/// Where the Swift side reports, from whichever task StoreKit answered on.
///
/// Not the channel itself: an answer that arrived while a recording was
/// playing would be taken for recorded input, so it waits in the queue until
/// a pump that may reach the outside world moves it across.
#[cfg(swift_shim)]
#[unsafe(no_mangle)]
pub(crate) extern "C" fn balaur_storekit_report(request: u64, json: *const c_char) {
    if json.is_null() {
        return;
    }
    // SAFETY: the shim passes a NUL-terminated UTF-8 string that outlives
    // this call, and nothing here keeps the pointer.
    let text = unsafe { CStr::from_ptr(json) };
    let payload = match serde_json::from_slice::<serde_json::Value>(text.to_bytes()) {
        Ok(payload) => payload,
        Err(err) => {
            tracing::error!("StoreKit answered something that is not JSON: {err}");
            return;
        }
    };
    crate::queue::push_store(PlatformEvent::Answer {
        request,
        answer: payload,
    });
}

#[cfg(swift_shim)]
pub(crate) fn call(request: u64, call: &Call, report: &Sender<PlatformEvent>) {
    // Transactions land without being asked for — another device, a renewal,
    // a parent approving a request — so the listener goes up on the first
    // call of any kind.
    unsafe { balaur_storekit_listen() };
    let fail = |message: &str| {
        let _ = report.send(PlatformEvent::Failed {
            request,
            message: message.to_string(),
        });
    };
    match call {
        Call::Products { ids } => {
            let Ok(ids) = serde_json::to_string(ids).map(CString::new) else {
                fail("those product ids do not encode");
                return;
            };
            let Ok(ids) = ids else {
                fail("a product id has a NUL in it");
                return;
            };
            unsafe { balaur_storekit_products(request, ids.as_ptr()) };
        }
        Call::Purchase { product } => match CString::new(product.as_str()) {
            Ok(product) => unsafe { balaur_storekit_purchase(request, product.as_ptr()) },
            Err(_) => fail("that product id has a NUL in it"),
        },
        Call::Entitlements => unsafe { balaur_storekit_entitlements(request) },
        Call::RestorePurchases => unsafe { balaur_storekit_restore(request) },
        // StoreKit finishes a consumable as it finishes anything else.
        Call::FinishPurchase { transaction, .. } => match CString::new(transaction.as_str()) {
            Ok(id) => unsafe { balaur_storekit_finish(request, id.as_ptr()) },
            Err(_) => fail("that transaction id has a NUL in it"),
        },
        _ => {
            let _ = report.send(PlatformEvent::Unsupported {
                request,
                call: call.name().to_string(),
            });
        }
    }
}

/// A build with no Swift toolchain behind it. Everything else in the crate
/// works; purchases say what is missing rather than failing to link.
#[cfg(not(swift_shim))]
pub(crate) fn call(request: u64, call: &Call, report: &Sender<PlatformEvent>) {
    let _ = report.send(PlatformEvent::Failed {
        request,
        message: format!(
            "{}: this build has no StoreKit — it was compiled with no Swift toolchain",
            call.name()
        ),
    });
}
