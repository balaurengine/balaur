//! A page's motor: `navigator.vibrate`, where the browser has it.

use crate::device::Haptic;

/// Buzz for `milliseconds`; a browser without a motor ignores it.
pub(crate) fn vibrate(milliseconds: u32) {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().vibrate_with_duration(milliseconds);
    }
}

/// Whether the browser has `navigator.vibrate` at all: Safari has none.
pub(crate) fn can_vibrate() -> bool {
    web_sys::window().is_some_and(|window| {
        js_sys::Reflect::has(&window.navigator(), &"vibrate".into()).unwrap_or(false)
    })
}

/// A tap of feedback as one short buzz, the only shape a page has.
pub(crate) const fn haptic_millis(effect: Haptic) -> u32 {
    match effect {
        Haptic::Selection => 5,
        Haptic::Light => 10,
        Haptic::Medium => 20,
        Haptic::Success => 30,
        Haptic::Heavy => 40,
        Haptic::Warning => 60,
        Haptic::Error => 90,
    }
}
