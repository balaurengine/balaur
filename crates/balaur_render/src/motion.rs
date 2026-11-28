//! Whether the player asked the system to reduce motion, as CSS's
//! `prefers-reduced-motion: reduce` reads it: macOS's and iOS's Reduce
//! Motion, Windows' and GNOME's animation switch (`mundy`, subscribed with
//! dark mode in `appearance.rs`), a page's media query. Android's Remove
//! animations is read by the platform crate, which holds the JNI.

/// Whether the system asks for reduced motion, as the platform last announced it.
static REDUCE: crate::appearance::Heard = crate::appearance::Heard::new();

/// The platform announced the setting: kept as the answer from now on, and
/// the device facts read again. Any thread.
#[cfg_attr(
    not(any(target_os = "linux", windows, target_family = "wasm")),
    allow(dead_code, reason = "only a subscription or a listener announces it")
)]
pub(crate) fn hear(reduce: bool) {
    REDUCE.hear(reduce);
    crate::device::moved();
}

/// What `mundy` first answered, kept unless an announcement came first.
#[cfg(any(target_os = "linux", windows))]
pub(crate) fn first(reduce: bool) {
    REDUCE.first(reduce);
}

/// The setting as of now: `None` where this crate cannot ask, so the fact
/// is left to whoever can.
pub(crate) fn reduced() -> Option<bool> {
    REDUCE.get().or_else(asked)
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
    let reduce = crate::appearance::listen_media("(prefers-reduced-motion: reduce)", hear)?;
    REDUCE.hear(reduce);
    Some(reduce)
}

/// The subscription `appearance.rs` opens for dark mode hears this too; its
/// first answer is waited for there.
#[cfg(any(target_os = "linux", windows))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the shape of the systems that cannot always ask"
)]
fn asked() -> Option<bool> {
    crate::appearance::watch();
    Some(REDUCE.get() == Some(true))
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
