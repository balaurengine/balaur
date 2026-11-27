//! The OS services this crate reaches itself, where no OS plugin took the
//! seam: Android's through the Java shims (`android/`), a page's motor and
//! file input, a desktop's file dialog, and `unsupported` everywhere else.

use std::sync::mpsc::Sender;

use anyhow::Result;
use balaur_core::Engine;

use crate::PlatformEvent;
use crate::device::{DeviceBackend, DeviceCall, Haptic};

/// This crate's own [`DeviceBackend`], until an OS plugin replaces it.
pub(crate) struct Native;

impl DeviceBackend for Native {
    fn start(&mut self, request: u64, call: &DeviceCall, report: &Sender<PlatformEvent>) {
        os::start(request, call, report);
    }

    fn cancel_notification(&mut self, id: &str) {
        os::cancel(id);
    }

    fn vibrate(&mut self, seconds: f64) {
        os::vibrate(millis(seconds));
    }

    fn haptic(&mut self, effect: Haptic) {
        os::haptic(effect);
    }

    fn pump(&mut self, report: &Sender<PlatformEvent>) {
        os::pump(report);
    }

    fn discard(&mut self) {
        os::discard();
    }
}

#[cfg(target_os = "android")]
mod os {
    pub(super) use crate::android::device::{haptic, vibrate};
    pub(super) use crate::android::notifications::cancel;
    pub(super) use crate::android::{discard, pump, start};

    /// Android's notifications, motor, share sheet and Custom Tab, and Play's
    /// review prompt and Firebase's push where the game's dex carries them.
    pub(super) fn provide(eng: &super::Engine) -> super::Result<()> {
        for service in crate::android::device::SERVICES {
            crate::provide(eng, service, move |_| {
                crate::android::device::available(service)
            })?;
        }
        crate::provide(eng, crate::Service::Review, |_| {
            crate::android::capability_available(crate::android::REVIEW)
        })?;
        crate::provide(eng, crate::Service::Push, |_| {
            crate::android::capability_available(crate::android::PUSH)
        })
    }
}

#[cfg(target_family = "wasm")]
mod os {
    use std::sync::mpsc::Sender;

    use crate::PlatformEvent;
    use crate::device::Haptic;

    pub(super) use crate::web::vibrate;

    /// A picture through the page's file input; everything else is not a page's.
    pub(super) fn start(
        request: u64,
        call: &crate::device::DeviceCall,
        report: &Sender<PlatformEvent>,
    ) {
        match call {
            crate::device::DeviceCall::PickPhoto => crate::web::pick_photo(request, report),
            other => super::unsupported(request, other, report),
        }
    }

    pub(super) const fn cancel(_id: &str) {}

    pub(super) fn haptic(effect: Haptic) {
        vibrate(crate::web::haptic_millis(effect));
    }

    pub(super) const fn pump(_report: &Sender<PlatformEvent>) {}

    pub(super) const fn discard() {}

    /// A page's motor, where the browser has one, and its file input.
    pub(super) fn provide(eng: &super::Engine) -> super::Result<()> {
        crate::provide(eng, crate::Service::Haptics, |_| crate::web::can_vibrate())?;
        crate::provide(eng, crate::Service::PhotoPicker, |_| true)
    }
}

/// A desktop: no motor, no sheet, nothing scheduled; a picture through the
/// system's file dialog.
#[cfg(not(any(target_os = "android", target_family = "wasm")))]
mod os {
    use std::sync::mpsc::Sender;

    use crate::PlatformEvent;
    use crate::device::{DeviceCall, Haptic};

    pub(super) fn start(request: u64, call: &DeviceCall, report: &Sender<PlatformEvent>) {
        match call {
            #[cfg(not(target_os = "ios"))]
            DeviceCall::PickPhoto => crate::photos::pick_from_files(request, report),
            other => super::unsupported(request, other, report),
        }
    }

    pub(super) const fn cancel(_id: &str) {}

    pub(super) fn vibrate(milliseconds: u32) {
        tracing::debug!(milliseconds, "vibrate: no motor on this platform");
    }

    pub(super) const fn haptic(_effect: Haptic) {}

    pub(super) const fn pump(_report: &Sender<PlatformEvent>) {}

    pub(super) const fn discard() {}

    /// The file dialog, on a desktop.
    pub(super) fn provide(eng: &super::Engine) -> super::Result<()> {
        crate::provide(eng, crate::Service::PhotoPicker, |_| {
            cfg!(not(target_os = "ios"))
        })
    }
}

/// What this crate reaches itself for `call`, for an OS's plugin to fall back
/// on for a call it does not take: a desktop's file dialog, a page's input.
pub fn start_natively(request: u64, call: &DeviceCall, report: &Sender<PlatformEvent>) {
    os::start(request, call, report);
}

/// A call nothing here can make.
pub(crate) fn unsupported(request: u64, call: &DeviceCall, report: &Sender<PlatformEvent>) {
    let _ = report.send(PlatformEvent::Unsupported {
        request,
        call: call.name().to_string(),
    });
}

/// Answer `is_available` for what this crate reaches itself.
pub(crate) fn provide(eng: &Engine) -> Result<()> {
    os::provide(eng)
}

/// Whole milliseconds, which is what a motor takes; a negative or NaN
/// duration is none.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to u32's range first"
)]
fn millis(seconds: f64) -> u32 {
    let ms = (seconds * 1000.0).round();
    if ms.is_nan() || ms <= 0.0 {
        0
    } else if ms >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        ms as u32
    }
}
