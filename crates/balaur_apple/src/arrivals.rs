//! What reaches a game through the application delegate and no other way:
//! the push token, a push's payload, and a link that opened the game.
//!
//! winit owns the real delegate, so this stands in front of it — answering
//! the selectors it knows and forwarding the rest, which is what
//! `forwardingTargetForSelector:` is for. It goes up as launch ends, in a
//! bundled game, so a link that launches the game reaches it: on iOS once
//! `UIApplicationDidFinishLaunchingNotification` is posted, on a Mac at
//! `NSApplicationWillFinishLaunchingNotification`, before the Apple event
//! carrying the URL is handled.
//!
//! The URL an iOS game was *launched* with also reaches the window layer,
//! which reads it from the launch options and hands it over once the engine
//! is up ([`deliver_launch_url`]); UIKit may hand the same one to
//! `application:openURL:options:` too, so the two are told apart here and a
//! game hears it once, as `url_opened` on `platform_event`.

use std::fmt::Write;
use std::ptr::NonNull;
use std::sync::Mutex;

use balaur_platform::PlatformEvent;
use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_foundation::{
    NSArray, NSData, NSError, NSNotification, NSNotificationCenter, NSString, NSURL,
};

/// The links heard before the launch URL was handed over, and the launch URL
/// once it was, each waiting for its twin from the other path.
#[derive(Default)]
struct Launch {
    /// Whether the window layer has handed its launch URL over, or said it
    /// had none.
    handed: bool,
    /// What the delegate heard before then.
    early: Vec<String>,
    /// The launch URL, until the delegate repeats it.
    launched: Option<String>,
}

static LAUNCH: Mutex<Launch> = Mutex::new(Launch {
    handed: false,
    early: Vec::new(),
    launched: None,
});

/// Hand over the URL the game was launched with, `None` for none, once the
/// engine is up. A link the delegate already delivered is not delivered again.
pub(crate) fn deliver_launch_url(url: Option<String>) {
    let deliver = LAUNCH.lock().map_or(true, |mut launch| {
        launch.handed = true;
        let early = std::mem::take(&mut launch.early);
        match &url {
            Some(url) if !early.contains(url) => {
                launch.launched = Some(url.clone());
                true
            }
            _ => false,
        }
    });
    if let (true, Some(url)) = (deliver, url) {
        crate::queue::push_device(PlatformEvent::UrlOpened { url });
    }
}

/// A link the delegate heard: delivered, unless it repeats the launch URL
/// the window layer already handed over.
fn opened(url: String) {
    let deliver = LAUNCH.lock().map_or(true, |mut launch| {
        if !launch.handed {
            launch.early.push(url.clone());
            return true;
        }
        if launch.launched.as_ref() == Some(&url) {
            launch.launched = None;
            return false;
        }
        true
    });
    if deliver {
        crate::queue::push_device(PlatformEvent::UrlOpened { url });
    }
}

/// Ask the OS for a push token. The token itself arrives at the delegate,
/// which is why this installs one; a push shown while the game is in front
/// arrives at the notification centre's, so that one goes up too.
pub(crate) fn request_push_token() -> bool {
    if !install() {
        return false;
    }
    crate::notify::watch_taps();
    let Some(app) = shared_application() else {
        return false;
    };
    // macOS and iOS spell it the same; a Mac that is not configured for push
    // answers with the failure selector rather than refusing here.
    unsafe {
        let _: () = msg_send![app, registerForRemoteNotifications];
    }
    true
}

/// Put the proxy up as launch ends, so a link that launches the game reaches
/// it. Called as the plugin declares, before the OS starts launching.
pub(crate) fn watch_launch() {
    let name = launch_notification();
    let installing = RcBlock::new(|_: NonNull<NSNotification>| {
        install();
    });
    let token = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(&name),
            None,
            None,
            &installing,
        )
    };
    OBSERVER.with_borrow_mut(|held| *held = Some(token));
}

/// The notification posted while the delegate is up and launch has not yet
/// handed a link over.
fn launch_notification() -> Retained<NSString> {
    NSString::from_str(if cfg!(target_os = "macos") {
        "NSApplicationWillFinishLaunchingNotification"
    } else {
        "UIApplicationDidFinishLaunchingNotification"
    })
}

/// Put the proxy in front of whatever delegate the window layer set, once.
fn install() -> bool {
    if INSTALLED.get() {
        return true;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(app) = shared_application() else {
        return false;
    };
    let original: *mut AnyObject = unsafe { msg_send![app, delegate] };
    let Some(original) = (unsafe { original.as_ref() }) else {
        // No delegate yet means the window layer has not finished starting;
        // the caller can ask again on a later tick.
        return false;
    };
    let proxy = Proxy::new(mtm, Original(original.retain()));
    unsafe {
        let _: () = msg_send![app, setDelegate: &*proxy];
    }
    PROXY.with_borrow_mut(|held| *held = Some(proxy));
    INSTALLED.set(true);
    true
}

fn shared_application() -> Option<&'static AnyObject> {
    let name = if cfg!(target_os = "macos") {
        c"NSApplication"
    } else {
        c"UIApplication"
    };
    let class = objc2::runtime::AnyClass::get(name)?;
    unsafe {
        let app: *mut AnyObject = msg_send![class, sharedApplication];
        app.as_ref()
    }
}

/// The delegate this one stands in front of.
struct Original(Retained<AnyObject>);

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BalaurAppDelegateProxy"]
    #[ivars = Original]
    struct Proxy;

    unsafe impl NSObjectProtocol for Proxy {}

    impl Proxy {
        /// Everything this class does not implement is the other delegate's.
        #[unsafe(method(forwardingTargetForSelector:))]
        fn forwarding(&self, _selector: Sel) -> *mut AnyObject {
            Retained::as_ptr(&self.ivars().0).cast_mut()
        }

        /// And the window layer must still look like it answers what it
        /// answered before the proxy went up.
        #[unsafe(method(respondsToSelector:))]
        fn responds(&self, selector: Sel) -> bool {
            let ours: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
            ours || unsafe { msg_send![&self.ivars().0, respondsToSelector: selector] }
        }

        #[unsafe(method(application:didRegisterForRemoteNotificationsWithDeviceToken:))]
        fn token(&self, app: *mut AnyObject, token: *mut NSData) {
            crate::push::token(hex(token));
            self.forward_token(app, token);
        }

        #[unsafe(method(application:didFailToRegisterForRemoteNotificationsWithError:))]
        fn token_failed(&self, app: *mut AnyObject, error: *mut NSError) {
            let message = unsafe { error.as_ref() }.map_or_else(
                || "the OS refused to register for push".to_string(),
                |error| error.localizedDescription().to_string(),
            );
            crate::push::failed(&message);
            self.forward_token_failure(app, error);
        }

        /// iOS: a push that reached the app itself, a silent one included.
        /// Answered here alone: the system waits on exactly one call of `done`.
        #[unsafe(method(application:didReceiveRemoteNotification:fetchCompletionHandler:))]
        fn remote(
            &self,
            _app: *mut AnyObject,
            info: *mut AnyObject,
            done: &DynBlock<dyn Fn(usize)>,
        ) {
            crate::push::received(json_of(info));
            // UIBackgroundFetchResultNewData.
            done.call((0,));
        }

        /// macOS: the same arrival, with nothing to answer.
        #[unsafe(method(application:didReceiveRemoteNotification:))]
        fn remote_on_mac(&self, _app: *mut AnyObject, info: *mut AnyObject) {
            crate::push::received(json_of(info));
        }

        /// iOS: a URL the game was asked to open. True says it was handled,
        /// and the other delegate's answer is kept when it has one.
        #[unsafe(method(application:openURL:options:))]
        fn open_url(&self, app: *mut AnyObject, url: *mut NSURL, options: *mut AnyObject) -> bool {
            opened(text_of(url));
            self.forward_url(app, url, options)
        }

        /// macOS: the same arrival, plural and returning nothing.
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, app: *mut AnyObject, urls: *mut NSArray<NSURL>) {
            if let Some(list) = unsafe { urls.as_ref() } {
                for at in 0..list.count() {
                    let url = list.objectAtIndex(at);
                    opened(text_of(Retained::as_ptr(&url).cast_mut()));
                }
            }
            self.forward_urls(app, urls);
        }

        /// Both platforms: a universal link, on one of `[apple]
        /// associated_domains`, as an activity carrying the page's URL.
        #[unsafe(method(application:continueUserActivity:restorationHandler:))]
        fn continue_activity(
            &self,
            app: *mut AnyObject,
            activity: *mut AnyObject,
            restoration: *mut AnyObject,
        ) -> bool {
            let page: *mut NSURL = unsafe { activity.as_ref() }
                .map_or(std::ptr::null_mut(), |activity| unsafe { msg_send![activity, webpageURL] });
            if !page.is_null() {
                opened(text_of(page));
            }
            self.forward_activity(app, activity, restoration)
        }
    }
);

impl Proxy {
    fn new(mtm: MainThreadMarker, original: Original) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(original);
        unsafe { msg_send![super(this), init] }
    }

    fn original(&self) -> &AnyObject {
        &self.ivars().0
    }

    fn forwards(&self, selector: Sel) -> bool {
        unsafe { msg_send![self.original(), respondsToSelector: selector] }
    }

    fn forward_token(&self, app: *mut AnyObject, token: *mut NSData) {
        let selector = objc2::sel!(application:didRegisterForRemoteNotificationsWithDeviceToken:);
        if self.forwards(selector) {
            unsafe {
                let _: () = msg_send![
                    self.original(),
                    application: app,
                    didRegisterForRemoteNotificationsWithDeviceToken: token,
                ];
            }
        }
    }

    fn forward_token_failure(&self, app: *mut AnyObject, error: *mut NSError) {
        let selector = objc2::sel!(application:didFailToRegisterForRemoteNotificationsWithError:);
        if self.forwards(selector) {
            unsafe {
                let _: () = msg_send![
                    self.original(),
                    application: app,
                    didFailToRegisterForRemoteNotificationsWithError: error,
                ];
            }
        }
    }

    fn forward_url(&self, app: *mut AnyObject, url: *mut NSURL, options: *mut AnyObject) -> bool {
        let selector = objc2::sel!(application:openURL:options:);
        if !self.forwards(selector) {
            return true;
        }
        unsafe {
            msg_send![
                self.original(),
                application: app,
                openURL: url,
                options: options,
            ]
        }
    }

    fn forward_activity(
        &self,
        app: *mut AnyObject,
        activity: *mut AnyObject,
        restoration: *mut AnyObject,
    ) -> bool {
        let selector = objc2::sel!(application:continueUserActivity:restorationHandler:);
        if !self.forwards(selector) {
            return true;
        }
        unsafe {
            msg_send![
                self.original(),
                application: app,
                continueUserActivity: activity,
                restorationHandler: restoration,
            ]
        }
    }

    fn forward_urls(&self, app: *mut AnyObject, urls: *mut NSArray<NSURL>) {
        let selector = objc2::sel!(application:openURLs:);
        if self.forwards(selector) {
            unsafe {
                let _: () = msg_send![self.original(), application: app, openURLs: urls];
            }
        }
    }
}

/// A Foundation dictionary as the JSON a script reads: what a push or a
/// notification carries in `userInfo`. Null for anything JSON cannot say.
pub(crate) fn json_of(object: *mut AnyObject) -> serde_json::Value {
    let (Some(object), Some(json)) = (
        unsafe { object.as_ref() },
        AnyClass::get(c"NSJSONSerialization"),
    ) else {
        return serde_json::Value::Null;
    };
    unsafe {
        let valid: bool = msg_send![json, isValidJSONObject: object];
        if !valid {
            return serde_json::Value::Null;
        }
        let none: *mut *mut NSError = std::ptr::null_mut();
        let data: *mut NSData =
            msg_send![json, dataWithJSONObject: object, options: 0usize, error: none];
        data.as_ref()
            .and_then(|data| serde_json::from_slice(&data.to_vec()).ok())
            .unwrap_or(serde_json::Value::Null)
    }
}

/// A device token, as the hex a push server expects.
fn hex(token: *mut NSData) -> String {
    let Some(token) = (unsafe { token.as_ref() }) else {
        return String::new();
    };
    token.to_vec().iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn text_of(url: *mut NSURL) -> String {
    unsafe { url.as_ref() }
        .and_then(NSURL::absoluteString)
        .map(|url| url.to_string())
        .unwrap_or_default()
}

thread_local! {
    static INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PROXY: std::cell::RefCell<Option<Retained<Proxy>>> =
        const { std::cell::RefCell::new(None) };
    /// The launch observer, kept so it can be named.
    static OBSERVER: std::cell::RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use balaur_platform::PlatformEvent;

    use super::{deliver_launch_url, opened};

    fn heard() -> Vec<String> {
        let (report, heard) = std::sync::mpsc::channel();
        crate::queue::drain_device(&report);
        heard
            .try_iter()
            .filter_map(|event| match event {
                PlatformEvent::UrlOpened { url } => Some(url),
                _ => None,
            })
            .collect()
    }

    // The link a game is launched with reaches it by the launch options and,
    // on iOS, by the delegate too: it is heard once, whichever comes first,
    // and the same link opened again later is heard again.
    #[test]
    fn the_launch_link_is_heard_once_whichever_way_comes_first() {
        opened("game://first".into());
        deliver_launch_url(Some("game://first".into()));
        assert_eq!(heard(), ["game://first"]);
        opened("game://first".into());
        assert_eq!(heard(), ["game://first"], "a second open is the player's");

        super::LAUNCH.lock().unwrap().handed = false;
        deliver_launch_url(Some("game://later".into()));
        opened("game://later".into());
        opened("game://later".into());
        assert_eq!(heard(), ["game://later", "game://later"]);
    }
}
