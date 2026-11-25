//! What Apple lends a game beside its store, behind `balaur_platform`'s device
//! seam: local notifications (`notify.rs`), the iPhone's motor, the share
//! sheet and the App Store's review prompt.
//!
//! By message rather than through typed bindings, as `ui.rs` is: UIKit's
//! classes are iOS's alone and AppKit's macOS's, and one raw path reads both.

use std::path::Path;
use std::sync::mpsc::Sender;

use balaur_platform::{DeviceBackend, DeviceCall, Haptic, PlatformEvent};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject};
use objc2_foundation::{NSArray, NSString, NSURL};

use crate::signin::key_window;

/// Apple's OS services, in place of `balaur_platform`'s own.
pub(crate) struct AppleDevice;

impl DeviceBackend for AppleDevice {
    fn start(&mut self, request: u64, call: &DeviceCall, report: &Sender<PlatformEvent>) {
        // The notification centre throws for a process with no bundle id.
        let bundled = crate::gamekit::bundled();
        match call {
            DeviceCall::RequestNotifications if bundled => {
                crate::notify::watch_taps();
                crate::notify::request_authorization(request, report);
            }
            DeviceCall::Notify {
                id,
                title,
                body,
                after,
                data,
            } if bundled => {
                crate::notify::watch_taps();
                crate::notify::schedule(request, id, title, body, *after, data, report);
            }
            DeviceCall::Share { text, file, .. } => share(request, text, file.as_deref(), report),
            DeviceCall::RequestReview if bundled => review(request, report),
            DeviceCall::RegisterForPush if bundled => crate::push::register(request, report),
            other => {
                let _ = report.send(PlatformEvent::Unsupported {
                    request,
                    call: other.name().to_string(),
                });
            }
        }
    }

    fn cancel_notification(&mut self, id: &str) {
        if crate::gamekit::bundled() {
            crate::notify::cancel(id);
        }
    }

    fn vibrate(&mut self, _seconds: f64) {
        motor::vibrate();
    }

    fn haptic(&mut self, effect: Haptic) {
        motor::haptic(effect);
    }

    fn pump(&mut self, report: &Sender<PlatformEvent>) {
        crate::queue::drain_device(report);
    }

    fn discard(&mut self) {
        crate::queue::discard_device();
    }
}

/// Answer `platform.is_available` for what Apple reaches. Game Center,
/// iCloud, StoreKit and the notification centre know a game by its bundle id,
/// so a process with none (a test, a run from the shell) has none of them.
pub(crate) fn provide(eng: &balaur_core::Engine) -> anyhow::Result<()> {
    use balaur_platform::{Service, provide};
    let bundled = |_: &_| crate::gamekit::bundled();
    provide(eng, Service::GameCenter, |_| {
        crate::gamekit::bundled() && crate::entitlement::granted(crate::entitlement::GAME_CENTER)
    })?;
    provide(eng, Service::AppleSignIn, |_| {
        crate::gamekit::bundled() && crate::entitlement::granted(crate::entitlement::APPLESIGNIN)
    })?;
    provide(eng, Service::Notifications, bundled)?;
    provide(eng, Service::Push, |_| {
        crate::gamekit::bundled() && crate::push::entitled()
    })?;
    // The platform verbs reach iCloud and the sheet only while Apple's is the store.
    provide(eng, Service::Icloud, |eng| {
        crate::gamekit::bundled() && is_store(eng) && signed_in_to_icloud()
    })?;
    provide(eng, Service::WebSignIn, is_store)?;
    // A Mac sells only through the Mac App Store's build, the sandboxed one.
    provide(eng, Service::Purchases, |_| {
        crate::gamekit::bundled()
            && crate::entitlement::granted(crate::entitlement::APP_SANDBOX)
            && can_pay()
    })?;
    provide(eng, Service::Haptics, |_| has_motor())?;
    provide(eng, Service::Share, |_| {
        AnyClass::get(if cfg!(target_os = "macos") {
            c"NSSharingServicePicker"
        } else {
            c"UIActivityViewController"
        })
        .is_some()
    })?;
    provide(eng, Service::Review, |_| can_review())
}

fn is_store(eng: &balaur_core::Engine) -> bool {
    balaur_platform::backend_name(eng) == crate::APPLE
}

/// An iCloud account on the device: `ubiquityIdentityToken` is nil without one.
fn signed_in_to_icloud() -> bool {
    let Some(class) = AnyClass::get(c"NSFileManager") else {
        return false;
    };
    unsafe {
        let files: *mut AnyObject = objc2::msg_send![class, defaultManager];
        !files.is_null() && {
            let token: *mut AnyObject = objc2::msg_send![files, ubiquityIdentityToken];
            !token.is_null()
        }
    }
}

/// Whether this account may buy: parental controls can say no.
fn can_pay() -> bool {
    AnyClass::get(c"SKPaymentQueue").is_some_and(|class| unsafe {
        let allowed: Bool = objc2::msg_send![class, canMakePayments];
        allowed.as_bool()
    })
}

/// Whether this device has a motor to tap: an iPhone. An iPad and a Mac
/// have none a game can reach.
pub(crate) fn has_motor() -> bool {
    motor::is_phone()
}

/// Whether the App Store's review prompt is here to ask.
pub(crate) fn can_review() -> bool {
    crate::gamekit::bundled() && AnyClass::get(c"SKStoreReviewController").is_some()
}

/// A Mac: no motor a game can reach.
#[cfg(not(target_os = "ios"))]
mod motor {
    use balaur_platform::Haptic;

    pub(super) const fn vibrate() {}

    pub(super) const fn is_phone() -> bool {
        false
    }

    pub(super) const fn haptic(_effect: Haptic) {}
}

#[cfg(target_os = "ios")]
mod motor {
    use balaur_platform::Haptic;
    use objc2::msg_send;
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};

    /// `kSystemSoundID_Vibrate`, the iPhone's one system buzz.
    const VIBRATE: u32 = 4095;
    /// `UIUserInterfaceIdiomPhone`.
    const PHONE: isize = 0;

    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        fn AudioServicesPlaySystemSound(sound: u32);
    }

    pub(super) fn vibrate() {
        // Safety: a system sound id, which the call only reads.
        unsafe { AudioServicesPlaySystemSound(VIBRATE) };
    }

    pub(super) fn is_phone() -> bool {
        let Some(class) = AnyClass::get(c"UIDevice") else {
            return false;
        };
        unsafe {
            let device: *mut AnyObject = msg_send![class, currentDevice];
            !device.is_null() && {
                let idiom: isize = msg_send![device, userInterfaceIdiom];
                idiom == PHONE
            }
        }
    }

    /// One of UIKit's feedback generators, made and fired at once.
    pub(super) fn haptic(effect: Haptic) {
        let (name, style) = match effect {
            Haptic::Light => (c"UIImpactFeedbackGenerator", 0),
            Haptic::Medium => (c"UIImpactFeedbackGenerator", 1),
            Haptic::Heavy => (c"UIImpactFeedbackGenerator", 2),
            Haptic::Success => (c"UINotificationFeedbackGenerator", 0),
            Haptic::Warning => (c"UINotificationFeedbackGenerator", 1),
            Haptic::Error => (c"UINotificationFeedbackGenerator", 2),
            Haptic::Selection => (c"UISelectionFeedbackGenerator", 0),
        };
        let Some(class) = AnyClass::get(name) else {
            return;
        };
        unsafe {
            match effect {
                Haptic::Light | Haptic::Medium | Haptic::Heavy => {
                    let allocated: Allocated<AnyObject> = msg_send![class, alloc];
                    let generator: Option<Retained<AnyObject>> =
                        msg_send![allocated, initWithStyle: style];
                    if let Some(generator) = generator {
                        let _: () = msg_send![&generator, impactOccurred];
                    }
                }
                Haptic::Success | Haptic::Warning | Haptic::Error => {
                    let generator: Option<Retained<AnyObject>> = msg_send![class, new];
                    if let Some(generator) = generator {
                        let _: () = msg_send![&generator, notificationOccurred: style];
                    }
                }
                Haptic::Selection => {
                    let generator: Option<Retained<AnyObject>> = msg_send![class, new];
                    if let Some(generator) = generator {
                        let _: () = msg_send![&generator, selectionChanged];
                    }
                }
            }
        }
    }
}

/// The App Store's prompt to rate the game, which it may or may not show and
/// says nothing about either way: `done` once asked.
fn review(request: u64, report: &Sender<PlatformEvent>) {
    let Some(class) = AnyClass::get(c"SKStoreReviewController") else {
        let _ = report.send(PlatformEvent::Unsupported {
            request,
            call: DeviceCall::RequestReview.name().to_string(),
        });
        return;
    };
    unsafe {
        if cfg!(target_os = "ios") {
            let window = key_window();
            let scene: *mut AnyObject = if window.is_null() {
                std::ptr::null_mut()
            } else {
                objc2::msg_send![window, windowScene]
            };
            if scene.is_null() {
                let _ = report.send(PlatformEvent::Failed {
                    request,
                    message: "no window scene to ask for a review in".to_string(),
                });
                return;
            }
            let _: () = objc2::msg_send![class, requestReviewInScene: scene];
        } else {
            let _: () = objc2::msg_send![class, requestReview];
        }
    }
    let _ = report.send(PlatformEvent::Done {
        request,
        call: DeviceCall::RequestReview.name().to_string(),
    });
}

/// What the sheet hands on: the text, then the file.
fn items(text: &str, file: Option<&Path>) -> Retained<NSArray<NSObject>> {
    let mut items: Vec<Retained<NSObject>> = Vec::new();
    if !text.is_empty() {
        items.push(Retained::into_super(NSString::from_str(text)));
    }
    if let Some(file) = file {
        let path = NSString::from_str(&file.to_string_lossy());
        items.push(Retained::into_super(NSURL::fileURLWithPath(&path)));
    }
    NSArray::from_retained_slice(&items)
}

fn failed(request: u64, message: &str, report: &Sender<PlatformEvent>) {
    let _ = report.send(PlatformEvent::Failed {
        request,
        message: message.to_string(),
    });
}

/// iOS's `UIActivityViewController`, from the window's root controller, and
/// on an iPad as a popover over its view.
#[cfg(not(target_os = "macos"))]
fn share(request: u64, text: &str, file: Option<&Path>, report: &Sender<PlatformEvent>) {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2_foundation::NSError;
    let Some(class) = AnyClass::get(c"UIActivityViewController") else {
        failed(request, "no share sheet on this system", report);
        return;
    };
    let window = key_window();
    let items = items(text, file);
    let sheet: Option<Retained<AnyObject>> = unsafe {
        let allocated: *mut AnyObject = msg_send![class, alloc];
        let built: *mut AnyObject = msg_send![
            allocated,
            initWithActivityItems: &*items,
            applicationActivities: std::ptr::null::<AnyObject>(),
        ];
        Retained::from_raw(built)
    };
    let (false, Some(sheet)) = (window.is_null(), sheet) else {
        failed(request, "no window to show the share sheet over", report);
        return;
    };
    let answer = report.clone();
    let finished = RcBlock::new(
        move |activity: *mut NSString, completed: Bool, _: *mut AnyObject, error: *mut NSError| {
            let event = match unsafe { error.as_ref() } {
                Some(error) => PlatformEvent::Failed {
                    request,
                    message: error.localizedDescription().to_string(),
                },
                None if completed.as_bool() => PlatformEvent::Shared {
                    request,
                    target: unsafe { activity.as_ref() }
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                },
                None => PlatformEvent::Cancelled { request },
            };
            let _ = answer.send(event);
        },
    );
    unsafe {
        let _: () = msg_send![&sheet, setCompletionWithItemsHandler: &*finished];
        let popover: *mut AnyObject = msg_send![&sheet, popoverPresentationController];
        let root: *mut AnyObject = msg_send![window, rootViewController];
        if !popover.is_null() && !root.is_null() {
            let view: *mut AnyObject = msg_send![root, view];
            let _: () = msg_send![popover, setSourceView: view];
        }
    }
    if !crate::ui::present(Retained::as_ptr(&sheet).cast_mut()) {
        failed(request, "no window to show the share sheet over", report);
    }
}

/// macOS's `NSSharingServicePicker`, under the key window's content view.
/// It answers once the player picks a service, or shuts it with none.
#[cfg(target_os = "macos")]
fn share(request: u64, text: &str, file: Option<&Path>, report: &Sender<PlatformEvent>) {
    use objc2::{MainThreadMarker, msg_send};
    let (Some(class), Some(mtm)) = (
        AnyClass::get(c"NSSharingServicePicker"),
        MainThreadMarker::new(),
    ) else {
        failed(request, "no share sheet on this system", report);
        return;
    };
    let window = key_window();
    let view: *mut AnyObject = if window.is_null() {
        std::ptr::null_mut()
    } else {
        unsafe { msg_send![window, contentView] }
    };
    if view.is_null() {
        failed(request, "no window to show the share sheet over", report);
        return;
    }
    let items = items(text, file);
    let picker: Option<Retained<AnyObject>> = unsafe {
        let allocated: *mut AnyObject = msg_send![class, alloc];
        let built: *mut AnyObject = msg_send![allocated, initWithItems: &*items];
        Retained::from_raw(built)
    };
    let Some(picker) = picker else {
        failed(request, "the share sheet would not open", report);
        return;
    };
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let delegate = picker::Picked::new(mtm, request, report.clone(), std::rc::Rc::clone(&done));
    unsafe {
        let _: () = msg_send![&picker, setDelegate: &*delegate];
        let bounds: objc2_foundation::NSRect = msg_send![view, bounds];
        // NSRectEdgeMinY: the sheet opens below the rectangle's bottom edge.
        let _: () =
            msg_send![&picker, showRelativeToRect: bounds, ofView: view, preferredEdge: 1usize];
    }
    PICKERS.with_borrow_mut(|held| {
        held.retain(|(_, _, answered)| !answered.get());
        held.push((picker, delegate, done));
    });
}

/// An open picker, its delegate, and whether it has answered.
#[cfg(target_os = "macos")]
type Open = (
    Retained<AnyObject>,
    Retained<picker::Picked>,
    std::rc::Rc<std::cell::Cell<bool>>,
);

// The picker holds its delegate weakly, so both live here until the next
// share clears the ones that answered: dropping one from inside its own
// callback would free the object running it.
#[cfg(target_os = "macos")]
thread_local! {
    static PICKERS: std::cell::RefCell<Vec<Open>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(target_os = "macos")]
mod picker {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::mpsc::Sender;

    use balaur_platform::PlatformEvent;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_foundation::NSString;

    /// What one open picker answers with.
    pub(super) struct Asked {
        request: u64,
        report: Sender<PlatformEvent>,
        done: Rc<Cell<bool>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this class
        // does not implement Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "BalaurSharePicked"]
        #[ivars = Asked]
        pub(super) struct Picked;

        unsafe impl NSObjectProtocol for Picked {}

        impl Picked {
            /// `NSSharingServicePickerDelegate`, declared informally: the
            /// framework only sends the selector. No service is the picker
            /// shut with nothing picked.
            #[unsafe(method(sharingServicePicker:didChooseSharingService:))]
            fn chose(&self, _picker: *mut AnyObject, service: *mut AnyObject) {
                let request = self.ivars().request;
                let event = match unsafe { service.as_ref() } {
                    Some(service) => {
                        let title: Option<Retained<NSString>> = unsafe { msg_send![service, title] };
                        PlatformEvent::Shared {
                            request,
                            target: title.map(|title| title.to_string()).unwrap_or_default(),
                        }
                    }
                    None => PlatformEvent::Cancelled { request },
                };
                self.ivars().done.set(true);
                let _ = self.ivars().report.send(event);
            }
        }
    );

    impl Picked {
        pub(super) fn new(
            mtm: MainThreadMarker,
            request: u64,
            report: Sender<PlatformEvent>,
            done: Rc<Cell<bool>>,
        ) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(Asked {
                request,
                report,
                done,
            });
            unsafe { msg_send![super(this), init] }
        }
    }
}
