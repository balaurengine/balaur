//! A hidden tab gets no animation frame, and nothing stands in for one: the
//! windowed loop waits for the tab to be shown again (a `visibilitychange`),
//! and a visible tab with nothing owed waits for its next frame. No timer
//! anywhere (a 100 ms step loop and a 16 ms check stood here until
//! 2026-10-20).

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

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
                "visibilitychange",
                changed.as_ref().unchecked_ref(),
                &options,
            );
            on_change = Some(changed);
        });
        let _ = JsFuture::from(promise).await;
        drop(on_change);
    }
}

/// The next animation frame, or the tab turning hidden, whichever comes
/// first: what a visible tab with nothing owed waits on. The frame is the
/// browser's own clock, and a tab that hides leaves the loop to
/// [`until_visible`].
pub(crate) async fn next_frame_or_hidden() {
    use wasm_bindgen::closure::Closure;
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let mut on_frame: Option<Closure<dyn FnMut()>> = None;
    let mut on_hidden: Option<Closure<dyn FnMut()>> = None;
    let mut frame: Option<i32> = None;
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let done = resolve.clone();
        let fired = Closure::<dyn FnMut()>::new(move || {
            let _ = done.call0(&wasm_bindgen::JsValue::NULL);
        });
        frame = window
            .request_animation_frame(fired.as_ref().unchecked_ref())
            .ok();
        on_frame = Some(fired);
        let hidden = Closure::<dyn FnMut()>::new(move || {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_once(true);
        let _ = document.add_event_listener_with_callback_and_add_event_listener_options(
            "visibilitychange",
            hidden.as_ref().unchecked_ref(),
            &options,
        );
        on_hidden = Some(hidden);
    });
    let _ = JsFuture::from(promise).await;
    // Whichever did not fire is taken down with the closures.
    if let Some(id) = frame {
        let _ = window.cancel_animation_frame(id);
    }
    if let Some(hidden) = &on_hidden {
        let _ = document.remove_event_listener_with_callback(
            "visibilitychange",
            hidden.as_ref().unchecked_ref(),
        );
    }
    drop(on_frame);
    drop(on_hidden);
}
