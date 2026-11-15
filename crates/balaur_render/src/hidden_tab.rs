//! A hidden tab gets no animation frame, and nothing stands in for one: the
//! windowed loop waits for the tab to be shown again (a `visibilitychange`).
//! A visible tab with nothing owed asks for no animation frame either: it
//! waits on a promise that a wake, input, the repaint due or turning hidden
//! settles.

use std::cell::RefCell;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

const VISIBILITYCHANGE: &str = "visibilitychange";

/// What kiss3d hears on the canvas and on the page, each also a wake. kiss3d
/// listened first, so the frame a wake starts finds the input queued.
const CANVAS_INPUT: [&str; 11] = [
    "pointermove",
    "pointerdown",
    "pointerup",
    "pointerenter",
    "pointerleave",
    "wheel",
    "contextmenu",
    "touchstart",
    "touchmove",
    "touchend",
    "touchcancel",
];
const PAGE_INPUT: [&str; 4] = ["keydown", "keyup", "focus", "blur"];

thread_local! {
    /// What settles the wait in [`until_owed`], while the loop is in it.
    static SETTLE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

/// End the loop's wait: what `balaur_core::wake::wake` calls in a tab.
/// Harmless while the loop is awake, which reads the wake at its next wait.
pub(crate) fn rouse() {
    let settle = SETTLE.with(|slot| slot.borrow_mut().take());
    if let Some(settle) = settle {
        let _ = settle.call0(&wasm_bindgen::JsValue::NULL);
    }
}

/// A tab has no loop to wake: a wake settles the promise the loop awaits,
/// and every input kiss3d hears wakes it, as a desktop's window system does.
/// Passive, so a touch still scrolls the page; kept for the page's life.
pub(crate) fn wake_on_input(canvas_id: &str) {
    use wasm_bindgen::closure::Closure;
    balaur_core::wake::set_hook(Some(Box::new(rouse)));
    let Some(window) = web_sys::window() else {
        return;
    };
    let canvas = window
        .document()
        .and_then(|document| document.get_element_by_id(canvas_id));
    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(true);
    let woken = Closure::<dyn FnMut()>::new(balaur_core::wake::wake);
    let listen = |target: &web_sys::EventTarget, names: &[&str]| {
        for name in names {
            let _ = target.add_event_listener_with_callback_and_add_event_listener_options(
                name,
                woken.as_ref().unchecked_ref(),
                &options,
            );
        }
    };
    if let Some(canvas) = &canvas {
        listen(canvas, &CANVAS_INPUT);
    }
    listen(&window, &PAGE_INPUT);
    woken.forget();
}

/// Tell the page the game's first frame is on the canvas: a `balaur-ready`
/// event on the window, which the page's loading screen waits for, so it
/// gives way to the game's own first picture and never to a bare canvas.
pub(crate) fn announce_ready() {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Ok(event) = web_sys::Event::new("balaur-ready") {
        let _ = window.dispatch_event(&event);
    }
}

pub(crate) fn is_hidden() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .is_some_and(|d| d.hidden())
}

/// Until the document is visible again: each `visibilitychange` is looked
/// at, since one may leave the tab hidden still.
pub(crate) async fn until_visible() {
    use wasm_bindgen::closure::Closure;
    while is_hidden() {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let mut on_change: Option<Closure<dyn FnMut()>> = None;
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            let changed = Closure::<dyn FnMut()>::new(move || {
                let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
            });
            let options = web_sys::AddEventListenerOptions::new();
            options.set_once(true);
            let _ = document.add_event_listener_with_callback_and_add_event_listener_options(
                VISIBILITYCHANGE,
                changed.as_ref().unchecked_ref(),
                &options,
            );
            on_change = Some(changed);
        });
        let _ = JsFuture::from(promise).await;
        drop(on_change);
    }
}

/// One wait of a `low_processor` tab whose next repaint is `due` from now:
/// true when a frame is owed already, else once the wait has ended.
pub(crate) async fn rest(
    window: &mut kiss3d::window::Window,
    due: Option<std::time::Duration>,
) -> bool {
    // Before `take`: a `wake::at` deadline already passed is a wake.
    let deadline = balaur_core::wake::soonest()
        .map(|at| at.saturating_duration_since(balaur_core::time::Instant::now()));
    if balaur_core::wake::take() || window.wait_events(None) || is_hidden() {
        return true;
    }
    let due = match (due, deadline) {
        (Some(due), Some(deadline)) => Some(due.min(deadline)),
        (due, deadline) => due.or(deadline),
    };
    until_owed(due).await;
    false
}

/// Until a frame is owed: a wake, which input and finished work call, `due`
/// passing, or the tab turning hidden, which leaves the loop to
/// [`until_visible`]. `None` waits for as long as it takes.
async fn until_owed(due: Option<std::time::Duration>) {
    use wasm_bindgen::closure::Closure;
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let mut on_due: Option<Closure<dyn FnMut()>> = None;
    let mut on_hidden: Option<Closure<dyn FnMut()>> = None;
    let mut timer: Option<i32> = None;
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        SETTLE.with(|slot| *slot.borrow_mut() = Some(resolve.clone()));
        if let Some(due) = due {
            let done = resolve.clone();
            let fired = Closure::<dyn FnMut()>::new(move || {
                let _ = done.call0(&wasm_bindgen::JsValue::NULL);
            });
            // Rounded up: a timer that fires early finds the repaint not yet due.
            let millis = i32::try_from(due.as_micros().div_ceil(1000)).unwrap_or(i32::MAX);
            timer = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    fired.as_ref().unchecked_ref(),
                    millis,
                )
                .ok();
            on_due = Some(fired);
        }
        let hidden = Closure::<dyn FnMut()>::new(move || {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_once(true);
        let _ = document.add_event_listener_with_callback_and_add_event_listener_options(
            VISIBILITYCHANGE,
            hidden.as_ref().unchecked_ref(),
            &options,
        );
        on_hidden = Some(hidden);
    });
    let _ = JsFuture::from(promise).await;
    // Whichever did not settle it is taken down with the closures.
    SETTLE.with(|slot| slot.borrow_mut().take());
    if let Some(id) = timer {
        window.clear_timeout_with_handle(id);
    }
    if let Some(hidden) = &on_hidden {
        let _ = document
            .remove_event_listener_with_callback(VISIBILITYCHANGE, hidden.as_ref().unchecked_ref());
    }
    drop(on_due);
    drop(on_hidden);
}
