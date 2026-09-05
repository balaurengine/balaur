//! Game Center's own screens: the sign-in sheet GameKit hands over, the
//! dashboard, and the access point.
//!
//! By message rather than through the typed bindings, because
//! `objc2-game-kit` has `GKGameCenterViewController` on macOS alone — one raw
//! path for both platforms beats a typed one for half of them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Sender;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};

use crate::AppleEvent;
use crate::signin::key_window;

/// Put a view controller Game Center handed over on screen.
///
/// iOS presents it from the window's root controller; macOS goes through
/// `GKDialogController`, which is what a GameKit view controller expects
/// there. False means there was nowhere to put it.
pub(crate) fn present(view_controller: *mut AnyObject) -> bool {
    if view_controller.is_null() {
        return false;
    }
    if cfg!(target_os = "macos") {
        let Some(class) = AnyClass::get(c"GKDialogController") else {
            return false;
        };
        unsafe {
            let dialog: *mut AnyObject = msg_send![class, sharedDialogController];
            if dialog.is_null() {
                return false;
            }
            let window = key_window();
            if !window.is_null() {
                let _: () = msg_send![dialog, setParentWindow: window];
            }
            msg_send![dialog, presentViewController: view_controller]
        }
    } else {
        let window = key_window();
        if window.is_null() {
            return false;
        }
        unsafe {
            let root: *mut AnyObject = msg_send![window, rootViewController];
            if root.is_null() {
                return false;
            }
            let _: () = msg_send![
                root,
                presentViewController: view_controller,
                animated: true,
                completion: Option::<&block2::DynBlock<dyn Fn()>>::None,
            ];
        }
        true
    }
}

fn dismiss(view_controller: *mut AnyObject) {
    if cfg!(target_os = "macos") {
        if let Some(class) = AnyClass::get(c"GKDialogController") {
            unsafe {
                let dialog: *mut AnyObject = msg_send![class, sharedDialogController];
                if !dialog.is_null() {
                    let _: () = msg_send![dialog, dismiss: view_controller];
                }
            }
        }
    } else {
        unsafe {
            let _: () = msg_send![
                view_controller,
                dismissViewControllerAnimated: true,
                completion: Option::<&block2::DynBlock<dyn Fn()>>::None,
            ];
        }
    }
}

/// Show Game Center's dashboard, opening on `state`.
///
/// The answer is the player closing it again, which is the only thing a game
/// can act on.
pub(crate) fn show_dashboard(request: u64, state: isize, report: &Sender<AppleEvent>) {
    let fail = |message: &str| {
        let _ = report.send(AppleEvent::Failed {
            request,
            message: message.to_string(),
        });
    };
    let Some(mtm) = MainThreadMarker::new() else {
        fail("Game Center's dashboard has to be asked for from the main thread");
        return;
    };
    let Some(class) = AnyClass::get(c"GKGameCenterViewController") else {
        fail("no Game Center on this system");
        return;
    };
    let controller: Option<Retained<AnyObject>> = unsafe {
        let allocated: *mut AnyObject = msg_send![class, alloc];
        let built: *mut AnyObject = msg_send![allocated, initWithState: state];
        Retained::from_raw(built)
    };
    let Some(controller) = controller else {
        fail("Game Center refused to build its dashboard");
        return;
    };
    let done = Rc::new(Cell::new(false));
    let delegate = Dismisser::new(
        mtm,
        Dismissal {
            request,
            report: report.clone(),
            done: Rc::clone(&done),
            controller: controller.clone(),
        },
    );
    unsafe {
        let _: () = msg_send![&controller, setGameCenterDelegate: &*delegate];
    }
    if !present(Retained::as_ptr(&controller).cast_mut()) {
        fail("no window to present Game Center's dashboard over");
        return;
    }
    HELD.with_borrow_mut(|held| {
        held.retain(|entry| !entry.done.get());
        held.push(Held {
            done,
            _delegate: delegate,
        });
    });
}

/// Show or hide Game Center's access point — the small badge that opens the
/// dashboard. False means this system has no Game Center.
pub(crate) fn access_point(active: bool, location: isize) -> bool {
    let Some(class) = AnyClass::get(c"GKAccessPoint") else {
        return false;
    };
    unsafe {
        let access_point: *mut AnyObject = msg_send![class, shared];
        if access_point.is_null() {
            return false;
        }
        let _: () = msg_send![access_point, setLocation: location];
        if cfg!(target_os = "macos") {
            let window = key_window();
            if !window.is_null() {
                let _: () = msg_send![access_point, setParentWindow: window];
            }
        }
        let _: () = msg_send![access_point, setActive: active];
    }
    true
}

/// What one open dashboard needs to answer with, and to dismiss.
struct Dismissal {
    request: u64,
    report: Sender<AppleEvent>,
    done: Rc<Cell<bool>>,
    controller: Retained<AnyObject>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BalaurGameCenterDismisser"]
    #[ivars = Dismissal]
    struct Dismisser;

    unsafe impl NSObjectProtocol for Dismisser {}

    impl Dismisser {
        /// `GKGameCenterControllerDelegate`, declared informally: the typed
        /// protocol is macOS-only in the bindings, and the framework only
        /// sends the selector.
        #[unsafe(method(gameCenterViewControllerDidFinish:))]
        fn finished(&self, _controller: *mut AnyObject) {
            dismiss(Retained::as_ptr(&self.ivars().controller).cast_mut());
            self.ivars().done.set(true);
            let _ = self.ivars().report.send(AppleEvent::DashboardClosed {
                request: self.ivars().request,
            });
        }
    }
);

impl Dismisser {
    fn new(mtm: MainThreadMarker, ivars: Dismissal) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

// The dashboard's delegate is held weakly by the view controller, so it lives
// here until the next dashboard clears it — dropping it from inside its own
// callback would free the object running it.
thread_local! {
    static HELD: RefCell<Vec<Held>> = const { RefCell::new(Vec::new()) };
}

struct Held {
    done: Rc<Cell<bool>>,
    _delegate: Retained<Dismisser>,
}
