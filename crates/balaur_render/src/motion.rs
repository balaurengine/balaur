//! Whether the player asked the system to reduce motion, as CSS's
//! `prefers-reduced-motion: reduce` reads it: macOS's and iOS's Reduce
//! Motion, Windows' and GNOME's animation switch (`mundy`, subscribed with
//! dark mode in `appearance.rs`), a page's media query. Android's Remove
//! animations is read by the platform crate, which holds the JNI.

use std::sync::atomic::{AtomicU8, Ordering};

const UNHEARD: u8 = 0;
const NO_PREFERENCE: u8 = 1;
const REDUCE: u8 = 2;

/// What the platform last announced, or [`UNHEARD`] before it has.
static HEARD: AtomicU8 = AtomicU8::new(UNHEARD);

/// The platform announced the setting: kept as the answer from now on, and
/// the device facts read again. Any thread.
#[cfg_attr(
    not(any(target_os = "linux", windows, target_family = "wasm")),
    allow(dead_code, reason = "only a subscription or a listener announces it")
)]
pub(crate) fn hear(reduce: bool) {
    HEARD.store(
        if reduce { REDUCE } else { NO_PREFERENCE },
        Ordering::Relaxed,
    );
    crate::device::moved();
}

/// The setting as of now: `None` where this crate cannot ask, so the fact
/// is left to whoever can.
pub(crate) fn reduced() -> Option<bool> {
    match HEARD.load(Ordering::Relaxed) {
        REDUCE => Some(true),
        NO_PREFERENCE => Some(false),
        _ => asked(),
    }
}

#[cfg(target_os = "macos")]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the shape of the systems that cannot always ask"
)]
fn asked() -> Option<bool> {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    Some(workspace.accessibilityDisplayShouldReduceMotion())
}

#[cfg(target_os = "ios")]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the shape of the systems that cannot always ask"
)]
fn asked() -> Option<bool> {
    #[link(name = "UIKit", kind = "framework")]
    unsafe extern "C" {
        fn UIAccessibilityIsReduceMotionEnabled() -> bool;
    }
    // Safety: a C function of no arguments that only reads a setting.
    Some(unsafe { UIAccessibilityIsReduceMotionEnabled() })
}

/// The page's answer, with a listener on the query so every change after is
/// heard rather than asked.
#[cfg(target_family = "wasm")]
fn asked() -> Option<bool> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    const REDUCE_QUERY: &str = "(prefers-reduced-motion: reduce)";
    const CHANGE: &str = "change";
    let list = web_sys::window().and_then(|w| w.match_media(REDUCE_QUERY).ok().flatten())?;
    let changed = {
        let list = list.clone();
        Closure::<dyn FnMut()>::new(move || {
            hear(list.matches());
            balaur_core::wake::wake();
        })
    };
    let _ = list.add_event_listener_with_callback(CHANGE, changed.as_ref().unchecked_ref());
    // Heard for the page's life: only the first ask gets here, the rest are heard.
    changed.forget();
    let reduce = list.matches();
    HEARD.store(
        if reduce { REDUCE } else { NO_PREFERENCE },
        Ordering::Relaxed,
    );
    Some(reduce)
}

/// The subscription `appearance.rs` opens for dark mode hears this too; its
/// first answer is waited for there.
#[cfg(any(target_os = "linux", windows))]
fn asked() -> Option<bool> {
    crate::appearance::watch();
    match HEARD.load(Ordering::Relaxed) {
        REDUCE => Some(true),
        _ => Some(false),
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_family = "wasm",
    target_os = "linux",
    windows
)))]
const fn asked() -> Option<bool> {
    None
}

/// What `mundy` first answered, kept unless an announcement came first.
#[cfg(any(target_os = "linux", windows))]
pub(crate) fn first(reduce: bool) {
    let first = if reduce { REDUCE } else { NO_PREFERENCE };
    let _ = HEARD.compare_exchange(UNHEARD, first, Ordering::Relaxed, Ordering::Relaxed);
}
