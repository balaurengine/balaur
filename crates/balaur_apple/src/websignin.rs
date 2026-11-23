//! A sign-in page in the system's own sheet: `ASWebAuthenticationSession`,
//! `platform.web_sign_in`.
//!
//! The sheet shows a provider's page over the game, with the browser's
//! cookies, so a player already signed in to the provider in Safari only
//! agrees. It shuts by itself when the page reaches the scheme the call
//! named, and answers that URL. Without one it stays up until the player
//! shuts it or the game calls `close_web_sign_in`, once it has learned some
//! other way (a server it polls) that the sign-in is done.
//!
//! One sheet at a time: opening another shuts the first.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Sender;

use balaur_platform::PlatformEvent;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_authentication_services::{
    ASWebAuthenticationPresentationContextProviding, ASWebAuthenticationSession,
    ASWebAuthenticationSessionErrorCode, ASWebAuthenticationSessionErrorDomain,
};
use objc2_foundation::{NSError, NSString, NSURL};

use crate::signin::key_window;

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BalaurWebSignInAnchor"]
    struct Anchor;

    unsafe impl NSObjectProtocol for Anchor {}

    // The protocol's one required method, inside its block: registering the
    // class checks it is there. It answers a bare object rather than the
    // protocol's type, which names the platform's own window class.
    unsafe impl ASWebAuthenticationPresentationContextProviding for Anchor {
        #[unsafe(method(presentationAnchorForWebAuthenticationSession:))]
        fn anchor(&self, _session: &ASWebAuthenticationSession) -> *mut AnyObject {
            key_window()
        }
    }
);

impl Anchor {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// The open sheet and what it needs alive: the session holds its
/// presentation provider weakly.
struct Open {
    done: Rc<Cell<bool>>,
    session: Retained<ASWebAuthenticationSession>,
    _anchor: Retained<Anchor>,
    _completion: RcBlock<dyn Fn(*mut NSURL, *mut NSError)>,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

/// Show `url` in the sheet, answering under `request` once it shuts.
pub(crate) fn open(request: u64, url: &str, scheme: Option<&str>, report: &Sender<PlatformEvent>) {
    close();
    let Some(mtm) = MainThreadMarker::new() else {
        failed(
            report,
            request,
            "a sign-in page has to be opened from the main thread",
        );
        return;
    };
    if key_window().is_null() {
        failed(
            report,
            request,
            "no window to present the sign-in page over",
        );
        return;
    }
    let Some(page) = NSURL::URLWithString(&NSString::from_str(url)) else {
        failed(report, request, "the sign-in page's address is not a URL");
        return;
    };
    let done = Rc::new(Cell::new(false));
    let finished = Rc::clone(&done);
    let answer = report.clone();
    let completion = RcBlock::new(move |ended: *mut NSURL, error: *mut NSError| {
        finished.set(true);
        let event = match (unsafe { ended.as_ref() }, unsafe { error.as_ref() }) {
            (Some(ended), _) => PlatformEvent::WebSignIn {
                request,
                url: ended
                    .absoluteString()
                    .map(|url| url.to_string())
                    .unwrap_or_default(),
            },
            (None, Some(error)) if !cancelled(error) => PlatformEvent::Failed {
                request,
                message: error.localizedDescription().to_string(),
            },
            (None, _) => PlatformEvent::Cancelled { request },
        };
        let _ = answer.send(event);
    });
    let scheme = scheme.map(NSString::from_str);
    // The scheme form, not the iOS 17.4 and macOS 14.4 callback object: the
    // game runs on systems older than those.
    #[allow(deprecated, reason = "the callback object needs iOS 17.4")]
    let session = unsafe {
        ASWebAuthenticationSession::initWithURL_callbackURLScheme_completionHandler(
            ASWebAuthenticationSession::alloc(),
            &page,
            scheme.as_deref(),
            RcBlock::as_ptr(&completion),
        )
    };
    let anchor = Anchor::new(mtm);
    unsafe {
        session.setPresentationContextProvider(Some(ProtocolObject::from_ref(&*anchor)));
    }
    if !unsafe { session.start() } {
        failed(report, request, "the sign-in page could not be shown");
        return;
    }
    OPEN.with_borrow_mut(|open| {
        *open = Some(Open {
            done,
            session,
            _anchor: anchor,
            _completion: completion,
        });
    });
}

/// Shut the open sheet, which answers `cancelled`. The session stays held
/// until the next one replaces it: its completion may still be on its way.
pub(crate) fn close() {
    let session = OPEN.with_borrow(|open| {
        open.as_ref()
            .filter(|open| !open.done.get())
            // copy: a retain, so the cancel runs outside the borrow its
            // completion may reach.
            .map(|open| open.session.clone())
    });
    if let Some(session) = session {
        unsafe { session.cancel() };
    }
}

/// Whether the sheet was shut rather than failing: by the player, or by
/// [`close`].
fn cancelled(error: &NSError) -> bool {
    *error.domain() == *unsafe { ASWebAuthenticationSessionErrorDomain }
        && error.code() == ASWebAuthenticationSessionErrorCode::CanceledLogin.0
}

fn failed(report: &Sender<PlatformEvent>, request: u64, message: &str) {
    let _ = report.send(PlatformEvent::Failed {
        request,
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use objc2::ClassType;

    // Registering a class checks it implements its protocols' required
    // methods; a sheet opened with one missing panics on the player's tap.
    #[test]
    fn the_anchor_class_registers_with_its_protocol_whole() {
        let class = super::Anchor::class();
        assert_eq!(class.name().to_str(), Ok("BalaurWebSignInAnchor"));
    }
}
