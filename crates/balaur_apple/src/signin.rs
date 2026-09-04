//! Sign in with Apple: an identity token a server can check.
//!
//! Two things this needs that GameKit does not. A delegate object that
//! outlives the call, because the controller holds it weakly — it lives in
//! [`IN_FLIGHT`] until the next sign-in clears it, rather than being dropped
//! from inside its own callback. And a window to present over, which is the
//! engine's, reached through the shared application rather than through
//! AppKit and UIKit bindings; the presentation selector is declared here
//! informally for the same reason.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Sender;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{
    define_class, msg_send, AllocAnyThread, ClassType, DefinedClass, MainThreadMarker,
    MainThreadOnly,
};
use objc2_authentication_services::{
    ASAuthorization, ASAuthorizationAppleIDProvider, ASAuthorizationAppleIDProviderCredentialState,
    ASAuthorizationController, ASAuthorizationControllerDelegate, ASAuthorizationCredential,
    ASAuthorizationRequest, ASAuthorizationScopeEmail, ASAuthorizationScopeFullName,
};
use objc2_foundation::{NSArray, NSData, NSDataBase64EncodingOptions, NSError, NSString};

use crate::AppleEvent;

/// One request's delegate: the id it answers under, where to report, and the
/// flag that says its entry can be let go.
struct Ivars {
    request: u64,
    report: Sender<AppleEvent>,
    done: Rc<Cell<bool>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BalaurAppleSignIn"]
    #[ivars = Ivars]
    struct SignIn;

    unsafe impl NSObjectProtocol for SignIn {}

    unsafe impl ASAuthorizationControllerDelegate for SignIn {
        #[unsafe(method(authorizationController:didCompleteWithAuthorization:))]
        fn completed(&self, _controller: &ASAuthorizationController, authorization: &ASAuthorization) {
            let credential = unsafe { authorization.credential() };
            // By selector rather than by downcast: the credential arrives as
            // a protocol object, and every field wanted here is on the Apple
            // ID credential that answers it.
            let user: Retained<NSString> = unsafe { msg_send![&credential, user] };
            let token: Option<Retained<NSData>> = unsafe { msg_send![&credential, identityToken] };
            let code: Option<Retained<NSData>> = unsafe { msg_send![&credential, authorizationCode] };
            let email: Option<Retained<NSString>> = unsafe { msg_send![&credential, email] };
            self.answer(AppleEvent::SignedIn {
                request: self.ivars().request,
                user: user.to_string(),
                // Apple sends the name once, on the authorization that first
                // granted it, and never again: a game that wants it has to
                // save it that first time.
                name: full_name(&credential),
                // Base64 because both are bytes whose destination is a JSON
                // post to the server that verifies them.
                token: base64(token.as_deref()),
                code: base64(code.as_deref()),
                email: email.map(|email| email.to_string()).unwrap_or_default(),
            });
        }

        #[unsafe(method(authorizationController:didCompleteWithError:))]
        fn failed(&self, _controller: &ASAuthorizationController, error: &NSError) {
            self.answer(AppleEvent::Failed {
                request: self.ivars().request,
                message: error.localizedDescription().to_string(),
            });
        }
    }

    impl SignIn {
        /// `ASAuthorizationControllerPresentationContextProviding`, declared
        /// without the protocol: the typed binding returns the platform's own
        /// window type, and the framework only sends the selector.
        #[unsafe(method(presentationAnchorForAuthorizationController:))]
        fn anchor(&self, _controller: &ASAuthorizationController) -> *mut AnyObject {
            key_window()
        }
    }
);

impl SignIn {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }

    fn answer(&self, event: AppleEvent) {
        self.ivars().done.set(true);
        let _ = self.ivars().report.send(event);
    }
}

// What keeps the controller and its delegate alive while the sheet is up: the
// controller holds both weakly, and dropping a delegate from inside its own
// callback would free the object running it.
thread_local! {
    static IN_FLIGHT: RefCell<Vec<Held>> = const { RefCell::new(Vec::new()) };
}

struct Held {
    done: Rc<Cell<bool>>,
    _controller: Retained<ASAuthorizationController>,
    _delegate: Retained<SignIn>,
}

pub(crate) fn sign_in(request: u64, report: &Sender<AppleEvent>) {
    IN_FLIGHT.with_borrow_mut(|held| held.retain(|entry| !entry.done.get()));
    let Some(mtm) = MainThreadMarker::new() else {
        let _ = report.send(AppleEvent::Failed {
            request,
            message: "Sign in with Apple has to be asked for from the main thread".into(),
        });
        return;
    };
    if key_window().is_null() {
        let _ = report.send(AppleEvent::Failed {
            request,
            message: "no window to present the Sign in with Apple sheet over".into(),
        });
        return;
    }
    let provider = unsafe { ASAuthorizationAppleIDProvider::new() };
    let apple_id = unsafe { provider.createRequest() };
    let scopes = NSArray::from_slice(&[unsafe { ASAuthorizationScopeFullName }, unsafe {
        ASAuthorizationScopeEmail
    }]);
    unsafe { apple_id.setRequestedScopes(Some(&scopes)) };
    let requests: Retained<NSArray<ASAuthorizationRequest>> =
        NSArray::from_slice(&[apple_id.as_super().as_super()]);
    let controller = unsafe {
        ASAuthorizationController::initWithAuthorizationRequests(
            ASAuthorizationController::alloc(),
            &requests,
        )
    };
    let done = Rc::new(Cell::new(false));
    let delegate = SignIn::new(
        mtm,
        Ivars {
            request,
            report: report.clone(),
            done: Rc::clone(&done),
        },
    );
    unsafe {
        controller.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let _: () = msg_send![&controller, setPresentationContextProvider: &*delegate];
        controller.performRequests();
    }
    IN_FLIGHT.with_borrow_mut(|held| {
        held.push(Held {
            done,
            _controller: controller,
            _delegate: delegate,
        });
    });
}

/// Ask Apple whether the account behind `user` is still good — signed out on
/// this device, revoked, or never seen. The id is the game's to keep; the
/// engine does not store accounts.
pub(crate) fn credential_state(request: u64, user: &str, report: &Sender<AppleEvent>) {
    let provider = unsafe { ASAuthorizationAppleIDProvider::new() };
    let user = NSString::from_str(user);
    let report = report.clone();
    let completion = RcBlock::new(
        move |state: ASAuthorizationAppleIDProviderCredentialState, error: *mut NSError| {
            // The state is answered even when an error comes with it — a
            // credential Apple has never seen is `not_found` and an error
            // both, and the state is the half a game can act on.
            let named = match state {
                ASAuthorizationAppleIDProviderCredentialState::Revoked => Some("revoked"),
                ASAuthorizationAppleIDProviderCredentialState::Authorized => Some("authorized"),
                ASAuthorizationAppleIDProviderCredentialState::NotFound => Some("not_found"),
                ASAuthorizationAppleIDProviderCredentialState::Transferred => Some("transferred"),
                _ => None,
            };
            let event = match (named, unsafe { error.as_ref() }) {
                (Some(state), _) => AppleEvent::CredentialState {
                    request,
                    state: state.to_string(),
                },
                (None, Some(error)) => AppleEvent::Failed {
                    request,
                    message: error.localizedDescription().to_string(),
                },
                (None, None) => AppleEvent::Failed {
                    request,
                    message: "Apple answered with a credential state this build does not know"
                        .into(),
                },
            };
            let _ = report.send(event);
        },
    );
    unsafe { provider.getCredentialStateForUserID_completion(&user, &completion) };
}

/// The name Apple hands over, as one string. Empty when this authorization
/// carried none, which is every one after the first.
fn full_name(credential: &ProtocolObject<dyn ASAuthorizationCredential>) -> String {
    let components: *mut AnyObject = unsafe { msg_send![credential, fullName] };
    let Some(components) = (unsafe { components.as_ref() }) else {
        return String::new();
    };
    let given: Option<Retained<NSString>> = unsafe { msg_send![components, givenName] };
    let family: Option<Retained<NSString>> = unsafe { msg_send![components, familyName] };
    let parts: Vec<String> = [given, family]
        .into_iter()
        .flatten()
        .map(|part| part.to_string())
        .filter(|part| !part.is_empty())
        .collect();
    parts.join(" ")
}

/// The window a sheet is presented over, from whichever shared application
/// this platform has. Null when the game has no window yet.
pub(crate) fn key_window() -> *mut AnyObject {
    let class = if cfg!(target_os = "macos") {
        objc2::runtime::AnyClass::get(c"NSApplication")
    } else {
        objc2::runtime::AnyClass::get(c"UIApplication")
    };
    let Some(class) = class else {
        return std::ptr::null_mut();
    };
    unsafe {
        let app: *mut AnyObject = msg_send![class, sharedApplication];
        if app.is_null() {
            return std::ptr::null_mut();
        }
        msg_send![app, keyWindow]
    }
}

fn base64(data: Option<&NSData>) -> String {
    data.map_or_else(String::new, |data| {
        data.base64EncodedStringWithOptions(NSDataBase64EncodingOptions(0))
            .to_string()
    })
}
