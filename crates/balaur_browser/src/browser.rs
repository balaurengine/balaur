//! The page, through web-sys: facts read at load, `message` and
//! `visibilitychange` listeners that report on the plugin's channel, and a
//! `postMessage` to the parent frame.

use std::sync::mpsc::Sender;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::MessageEvent;

use crate::{PageFacts, WebEvent};

pub(crate) const SUPPORTED: bool = true;

pub(crate) fn facts() -> PageFacts {
    let Some(window) = web_sys::window() else {
        return PageFacts::default();
    };
    let navigator = window.navigator();
    PageFacts {
        user_agent: navigator.user_agent().ok(),
        location: window.location().href().ok(),
        hardware_concurrency: Some(navigator.hardware_concurrency() as u32),
    }
}

pub(crate) fn visible() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .is_none_or(|d| !d.hidden())
}

/// Register the listeners once. The closures are leaked on purpose: they
/// live as long as the page, and nothing ever needs to unregister them.
pub(crate) fn listen(report: Sender<WebEvent>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let on_message = {
        let report = report.clone();
        Closure::wrap(Box::new(move |event: MessageEvent| {
            let Some(payload) = json_of(&event.data()) else {
                return;
            };
            let _ = report.send(WebEvent::Message { payload });
        }) as Box<dyn FnMut(MessageEvent)>)
    };
    let _ = window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref());
    on_message.forget();

    if let Some(document) = window.document() {
        let on_visibility = Closure::wrap(Box::new(move |_: JsValue| {
            let visible = web_sys::window()
                .and_then(|w| w.document())
                .is_none_or(|d| !d.hidden());
            let _ = report.send(WebEvent::Visibility { visible });
        }) as Box<dyn FnMut(JsValue)>);
        let _ = document.add_event_listener_with_callback(
            "visibilitychange",
            on_visibility.as_ref().unchecked_ref(),
        );
        on_visibility.forget();
    }
}

/// To the embedding page, whatever its origin: a game inside another site's
/// frame has no other address for it.
pub(crate) fn post_message(payload: &serde_json::Value) {
    let Some(parent) = web_sys::window().and_then(|w| w.parent().ok().flatten()) else {
        return;
    };
    let Ok(value) = js_sys::JSON::parse(&payload.to_string()) else {
        return;
    };
    let _ = parent.post_message(&value, "*");
}

/// A JS value as JSON, through the page's own serializer; a value it cannot
/// serialize is dropped rather than half-read.
fn json_of(value: &JsValue) -> Option<serde_json::Value> {
    let text = js_sys::JSON::stringify(value).ok()?;
    let text = text.as_string()?;
    serde_json::from_str(&text).ok()
}
