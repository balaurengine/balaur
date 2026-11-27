//! A page's motor, `navigator.vibrate` where the browser has it, and its
//! file input for a picture.

use crate::device::Haptic;

/// Buzz for `milliseconds`; a browser without a motor ignores it.
pub(crate) fn vibrate(milliseconds: u32) {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().vibrate_with_duration(milliseconds);
    }
}

/// Whether the browser has `navigator.vibrate` at all: Safari has none.
pub(crate) fn can_vibrate() -> bool {
    web_sys::window().is_some_and(|window| {
        js_sys::Reflect::has(&window.navigator(), &"vibrate".into()).unwrap_or(false)
    })
}

/// A tap of feedback as one short buzz, the only shape a page has.
pub(crate) const fn haptic_millis(effect: Haptic) -> u32 {
    match effect {
        Haptic::Selection => 5,
        Haptic::Light => 10,
        Haptic::Medium => 20,
        Haptic::Success => 30,
        Haptic::Heavy => 40,
        Haptic::Warning => 60,
        Haptic::Error => 90,
    }
}

/// One picture through a file input the page never shows, as `<input
/// type=file accept=image/*>` does: `picked` with the file's bytes, or
/// `cancelled` when the browser says the chooser shut with none. A browser
/// opens it only shortly after the player touched the page.
pub(crate) fn pick_photo(request: u64, report: &std::sync::mpsc::Sender<crate::PlatformEvent>) {
    use std::cell::Cell;
    use std::rc::Rc;

    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    use crate::PlatformEvent;

    let input = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.create_element("input").ok())
        .and_then(|element| element.dyn_into::<web_sys::HtmlInputElement>().ok());
    let Some(input) = input else {
        let _ = report.send(PlatformEvent::Failed {
            request,
            message: "no page to pick a picture on".into(),
        });
        return;
    };
    input.set_type("file");
    input.set_accept("image/*");
    let answered = Rc::new(Cell::new(false));
    let changed = {
        let (input, report, answered) = (input.clone(), report.clone(), Rc::clone(&answered));
        Closure::<dyn FnMut()>::new(move || {
            if answered.replace(true) {
                return;
            }
            let Some(file) = input.files().and_then(|files| files.get(0)) else {
                let _ = report.send(PlatformEvent::Cancelled { request });
                return;
            };
            let report = report.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let event = match wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                    Ok(buffer) => PlatformEvent::Picked {
                        request,
                        bytes: js_sys::Uint8Array::new(&buffer).to_vec(),
                        media_type: file.type_(),
                    },
                    Err(_) => PlatformEvent::Failed {
                        request,
                        message: "the browser would not read the picture".into(),
                    },
                };
                balaur_core::replay::report(&report, event);
                balaur_core::wake::wake();
            });
        })
    };
    let shut = {
        let (report, answered) = (report.clone(), Rc::clone(&answered));
        Closure::<dyn FnMut()>::new(move || {
            if !answered.replace(true) {
                let _ = report.send(PlatformEvent::Cancelled { request });
                balaur_core::wake::wake();
            }
        })
    };
    let _ = input.add_event_listener_with_callback("change", changed.as_ref().unchecked_ref());
    let _ = input.add_event_listener_with_callback("cancel", shut.as_ref().unchecked_ref());
    // Each fires once, for an input that lives as long as the chooser.
    changed.forget();
    shut.forget();
    input.click();
}
