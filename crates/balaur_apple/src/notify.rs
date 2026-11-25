//! Local notifications, `platform.notify` on Apple: asking for permission,
//! scheduling one, the tap that brings a player back, and one that arrives
//! while the game is in front.
//!
//! The notification centre's delegate is ours to set — nothing else in the
//! engine wants it — so taps need no proxying. It goes up before launch ends,
//! so the tap that launched the game is heard as `notification_opened`. What a tap carries is the
//! identifier the game scheduled it under and the `data` it put in, kept as
//! JSON text under one `userInfo` key. A push comes through the same
//! delegate: its trigger says so, and its `data` is its payload less `aps`.

use std::sync::mpsc::Sender;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNTimeIntervalNotificationTrigger, UNUserNotificationCenter,
};

use balaur_platform::PlatformEvent;

/// The `userInfo` key a local notification's `data` is kept under, as JSON.
const DATA: &str = "balaur_data";

/// Ask for permission to show notifications. The answer reaches the script
/// whether the player said yes or no; nothing here asks twice.
pub(crate) fn request_authorization(request: u64, report: &Sender<PlatformEvent>) {
    let report = report.clone();
    let answered = RcBlock::new(move |allowed: Bool, error: *mut NSError| {
        let allowed = allowed.as_bool();
        let event = match unsafe { error.as_ref() } {
            Some(error) if !allowed => PlatformEvent::Failed {
                request,
                message: error.localizedDescription().to_string(),
            },
            _ => PlatformEvent::Notifications { request, allowed },
        };
        let _ = report.send(event);
    });
    let options = UNAuthorizationOptions::Alert
        | UNAuthorizationOptions::Sound
        | UNAuthorizationOptions::Badge;
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(options, &answered);
}

/// Schedule one notification, `after` seconds from now, carrying `data`.
pub(crate) fn schedule(
    request: u64,
    id: &str,
    title: &str,
    body: &str,
    after: f64,
    data: &serde_json::Value,
    report: &Sender<PlatformEvent>,
) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    if !data.is_null()
        && let Some(dictionary) = AnyClass::get(c"NSDictionary")
    {
        let text = NSString::from_str(&data.to_string());
        let key = NSString::from_str(DATA);
        unsafe {
            let info: *mut AnyObject =
                msg_send![dictionary, dictionaryWithObject: &*text, forKey: &*key];
            let _: () = msg_send![&content, setUserInfo: info];
        }
    }
    // Zero seconds is not a trigger UserNotifications accepts, and a
    // notification with no trigger fires immediately — which is what asking
    // for zero means.
    let trigger = if after > 0.0 {
        Some(UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(after, false))
    } else {
        None
    };
    let scheduled = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(id),
        &content,
        trigger.as_deref().map(AsRef::as_ref),
    );
    let report = report.clone();
    let id = id.to_string();
    let done = RcBlock::new(move |error: *mut NSError| {
        let event = match unsafe { error.as_ref() } {
            Some(error) => PlatformEvent::Failed {
                request,
                message: error.localizedDescription().to_string(),
            },
            None => PlatformEvent::Scheduled {
                request,
                id: id.clone(),
            },
        };
        let _ = report.send(event);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&scheduled, Some(&done));
}

/// Drop a scheduled notification that has not fired.
pub(crate) fn cancel(id: &str) {
    let ids = NSArray::from_retained_slice(&[NSString::from_str(id)]);
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.removePendingNotificationRequestsWithIdentifiers(&ids);
    center.removeDeliveredNotificationsWithIdentifiers(&ids);
}

/// Hear about taps, and show a notification that arrives while the game is
/// in front rather than swallowing it. Installed as the plugin declares, on
/// the main thread before the OS finishes launching the game: a tap that
/// launched it is delivered to the delegate set by then, and to no other.
/// The notification calls install it too, for a plugin declared elsewhere.
pub(crate) fn watch_taps() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if TAPS.replace(true) {
        return;
    }
    let delegate = Taps::new(mtm);
    unsafe {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let _: () = msg_send![&center, setDelegate: &*delegate];
    }
    DELEGATE.with_borrow_mut(|held| *held = Some(delegate));
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BalaurNotificationDelegate"]
    struct Taps;

    unsafe impl NSObjectProtocol for Taps {}

    impl Taps {
        /// `UNUserNotificationCenterDelegate`, declared informally: the
        /// framework only sends these two selectors.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn tapped(
            &self,
            _center: *mut AnyObject,
            response: *mut AnyObject,
            completion: &DynBlock<dyn Fn()>,
        ) {
            let notification = unsafe { response.as_ref() }
                .map_or(std::ptr::null_mut(), |response| unsafe { msg_send![response, notification] });
            let said = Said::of(notification);
            crate::queue::push_device(PlatformEvent::NotificationOpened {
                id: said.id,
                data: said.data,
            });
            completion.call(());
        }

        /// A notification that arrives while the game is in front is shown
        /// rather than dropped, and reported: 1 << 2 is `banner`, 1 << 1 `sound`.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn presenting(
            &self,
            _center: *mut AnyObject,
            notification: *mut AnyObject,
            completion: &DynBlock<dyn Fn(usize)>,
        ) {
            let said = Said::of(notification);
            crate::queue::push_device(if said.push {
                PlatformEvent::PushReceived {
                    id: said.id,
                    title: said.title,
                    body: said.body,
                    data: said.data,
                }
            } else {
                PlatformEvent::NotificationReceived {
                    id: said.id,
                    title: said.title,
                    body: said.body,
                    data: said.data,
                }
            });
            completion.call(((1 << 2) | (1 << 1),));
        }
    }
);

impl Taps {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

/// What a notification says: the identifier it was scheduled under, its
/// title and body, the `data` a game put in it or a push's payload, and
/// whether a push brought it.
struct Said {
    id: String,
    title: String,
    body: String,
    data: serde_json::Value,
    push: bool,
}

impl Said {
    fn of(notification: *mut AnyObject) -> Self {
        let text =
            |value: Option<Retained<NSString>>| value.map(|v| v.to_string()).unwrap_or_default();
        let empty = Self {
            id: String::new(),
            title: String::new(),
            body: String::new(),
            data: serde_json::Value::Null,
            push: false,
        };
        let Some(notification) = (unsafe { notification.as_ref() }) else {
            return empty;
        };
        let request: *mut AnyObject = unsafe { msg_send![notification, request] };
        let Some(request) = (unsafe { request.as_ref() }) else {
            return empty;
        };
        let content: *mut AnyObject = unsafe { msg_send![request, content] };
        let Some(content) = (unsafe { content.as_ref() }) else {
            return empty;
        };
        let info: *mut AnyObject = unsafe { msg_send![content, userInfo] };
        let trigger: *mut AnyObject = unsafe { msg_send![request, trigger] };
        let push = AnyClass::get(c"UNPushNotificationTrigger").is_some_and(|class| {
            unsafe { trigger.as_ref() }
                .is_some_and(|trigger| unsafe { msg_send![trigger, isKindOfClass: class] })
        });
        let info = crate::arrivals::json_of(info);
        Self {
            id: text(unsafe { msg_send![request, identifier] }),
            title: text(unsafe { msg_send![content, title] }),
            body: text(unsafe { msg_send![content, body] }),
            data: if push {
                crate::push::data_of(info)
            } else {
                data_of(&info)
            },
            push,
        }
    }
}

/// A local notification's `data`, from the JSON text under [`DATA`].
fn data_of(info: &serde_json::Value) -> serde_json::Value {
    info.get(DATA)
        .and_then(serde_json::Value::as_str)
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(serde_json::Value::Null)
}

thread_local! {
    static TAPS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DELEGATE: std::cell::RefCell<Option<Retained<Taps>>> =
        const { std::cell::RefCell::new(None) };
}
