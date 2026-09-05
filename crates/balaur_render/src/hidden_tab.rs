//! A hidden tab gets no animation frame, so the windowed loop steps the
//! simulation on a timer there and draws nothing; the browser still runs
//! timers, throttled, in a tab it no longer paints.

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// How often a hidden tab is stepped: ten times a second keeps a 60 Hz tick
/// within its substep budget and a heartbeat well inside its window.
const MILLISECONDS: i32 = 100;

pub(crate) fn is_hidden() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .is_some_and(|d| d.hidden())
}

/// One hidden-tab interval, as a `setTimeout` future.
pub(crate) async fn sleep() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            resolve.unchecked_ref(),
            MILLISECONDS,
        );
    });
    let _ = JsFuture::from(promise).await;
}
