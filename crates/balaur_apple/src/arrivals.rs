//! The two things that reach a game through the application delegate and no
//! other way: the push token, and a URL the game was opened with.
//!
//! winit owns the real delegate, so this stands in front of it — answering
//! the selectors it knows and forwarding the rest, which is what
//! `forwardingTargetForSelector:` is for. It goes up only when a game asks
//! for one of the two, because a delegate nobody needed is a lifecycle bug
//! waiting for a device to find it.
//!
//! A URL the game was *launched* with does not come this way: it reaches the
//! delegate before the engine has booted. Only URLs opened while the game is
//! running arrive here.

use std::fmt::Write;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_foundation::{NSArray, NSData, NSError, NSURL};

use crate::AppleEvent;

/// Ask the OS for a push token. The token itself arrives at the delegate,
/// which is why this installs one.
pub(crate) fn register_for_push() -> bool {
    if !install() {
        return false;
    }
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

/// Hear about URLs the game is asked to open while it runs.
pub(crate) fn watch_urls() -> bool {
    install()
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
            crate::queue::push_apple(AppleEvent::PushToken { token: hex(token) });
            self.forward_token(app, token);
        }

        #[unsafe(method(application:didFailToRegisterForRemoteNotificationsWithError:))]
        fn token_failed(&self, app: *mut AnyObject, error: *mut NSError) {
            let message = unsafe { error.as_ref() }.map_or_else(
                || "the OS refused to register for push".to_string(),
                |error| error.localizedDescription().to_string(),
            );
            crate::queue::push_apple(AppleEvent::PushFailed { message });
            self.forward_token_failure(app, error);
        }

        /// iOS: a URL the game was asked to open. True says it was handled,
        /// and the other delegate's answer is kept when it has one.
        #[unsafe(method(application:openURL:options:))]
        fn open_url(&self, app: *mut AnyObject, url: *mut NSURL, options: *mut AnyObject) -> bool {
            crate::queue::push_apple(AppleEvent::Url { url: text_of(url) });
            self.forward_url(app, url, options)
        }

        /// macOS: the same arrival, plural and returning nothing.
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, app: *mut AnyObject, urls: *mut NSArray<NSURL>) {
            if let Some(list) = unsafe { urls.as_ref() } {
                for at in 0..list.count() {
                    let url = list.objectAtIndex(at);
                    crate::queue::push_apple(AppleEvent::Url {
                        url: text_of(Retained::as_ptr(&url).cast_mut()),
                    });
                }
            }
            self.forward_urls(app, urls);
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

    fn forward_urls(&self, app: *mut AnyObject, urls: *mut NSArray<NSURL>) {
        let selector = objc2::sel!(application:openURLs:);
        if self.forwards(selector) {
            unsafe {
                let _: () = msg_send![self.original(), application: app, openURLs: urls];
            }
        }
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
}
