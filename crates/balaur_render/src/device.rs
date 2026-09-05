//! What the window and the display say, published to the engine once per
//! frame as recorded input: focus, dark mode, the safe area and the refresh
//! rate. Each platform answers what it can; the rest keeps the default.

use std::collections::VecDeque;

use balaur_core::App;

/// Frame intervals the refresh rate is measured over.
const SAMPLES: usize = 90;

/// The measurements one window keeps between frames.
#[derive(Default)]
pub(crate) struct Probe {
    intervals: VecDeque<f32>,
}

impl Probe {
    /// Fold this frame in and publish every fact the platform can answer.
    pub(crate) fn publish(&mut self, app: &App, window: &kiss3d::window::Window, dt: f32) {
        if dt > 0.0 {
            if self.intervals.len() == SAMPLES {
                self.intervals.pop_front();
            }
            self.intervals.push_back(dt);
        }
        let refresh_rate = self.refresh_rate();
        let dark_mode = dark_mode();
        let safe_area = safe_area(window);
        balaur_core::facts::update_device(&app.engine, |facts| {
            facts.dark_mode = dark_mode;
            facts.safe_area = safe_area;
            if let Some(rate) = refresh_rate {
                facts.refresh_rate = rate;
            }
        });
    }

    /// The display's rate from the median frame interval: vsync paces the
    /// loop, so the middle sample is the display, whatever the outliers.
    fn refresh_rate(&self) -> Option<f32> {
        if self.intervals.len() < SAMPLES / 3 {
            return None;
        }
        let mut sorted: Vec<f32> = self.intervals.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let median = sorted[sorted.len() / 2];
        if median <= 0.0 {
            return None;
        }
        Some((10.0 / median).round() / 10.0)
    }
}

/// The window came to the front or went behind.
pub(crate) fn set_focused(app: &App, focused: bool) {
    balaur_core::facts::update_device(&app.engine, |facts| facts.focused = focused);
}

/// Ask the platform to keep the screen on, or let it dim again.
pub(crate) fn keep_awake(on: bool) {
    #[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
    web::keep_awake(on);
    #[cfg(not(all(target_family = "wasm", not(target_os = "emscripten"))))]
    tracing::debug!(on, "keep awake: nothing to ask on this platform");
}

#[cfg(target_os = "macos")]
fn dark_mode() -> bool {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let name = app.effectiveAppearance().name();
    name.to_string().contains("Dark")
}

#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
fn dark_mode() -> bool {
    web::dark_mode()
}

#[cfg(not(any(
    target_os = "macos",
    all(target_family = "wasm", not(target_os = "emscripten"))
)))]
fn dark_mode() -> bool {
    false
}

/// The page reads its insets off the shell's CSS; a window asks kiss3d.
#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
fn safe_area(_window: &kiss3d::window::Window) -> [f32; 4] {
    web::safe_area()
}

#[cfg(not(all(target_family = "wasm", not(target_os = "emscripten"))))]
fn safe_area(window: &kiss3d::window::Window) -> [f32; 4] {
    window.safe_area()
}

#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
mod web {
    use wasm_bindgen::JsValue;

    pub(super) fn dark_mode() -> bool {
        web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
            .is_some_and(|list| list.matches())
    }

    /// The shell's `--balaur-safe-*` variables carry `env(safe-area-inset-*)`
    /// where a script can read them; a page without them answers zero.
    pub(super) fn safe_area() -> [f32; 4] {
        let Some(window) = web_sys::window() else {
            return [0.0; 4];
        };
        let Some(root) = window.document().and_then(|d| d.document_element()) else {
            return [0.0; 4];
        };
        let Ok(Some(style)) = window.get_computed_style(&root) else {
            return [0.0; 4];
        };
        let ratio = window.device_pixel_ratio() as f32;
        let read = |name: &str| {
            style
                .get_property_value(name)
                .ok()
                .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok())
                .unwrap_or(0.0)
                * ratio
        };
        [
            read("--balaur-safe-left"),
            read("--balaur-safe-top"),
            read("--balaur-safe-right"),
            read("--balaur-safe-bottom"),
        ]
    }

    /// `navigator.wakeLock.request("screen")`, reached by name so a browser
    /// without it is a no-op rather than a missing binding.
    pub(super) fn keep_awake(on: bool) {
        thread_local! {
            static LOCK: std::cell::RefCell<Option<JsValue>> = const { std::cell::RefCell::new(None) };
        }
        if !on {
            LOCK.with(|lock| {
                if let Some(sentinel) = lock.borrow_mut().take() {
                    if let Ok(release) = js_sys::Reflect::get(&sentinel, &"release".into()) {
                        if let Some(release) = release.dyn_ref::<js_sys::Function>() {
                            let _ = release.call0(&sentinel);
                        }
                    }
                }
            });
            return;
        }
        let Some(navigator) = web_sys::window().map(|w| w.navigator()) else {
            return;
        };
        let Ok(wake_lock) = js_sys::Reflect::get(&navigator, &"wakeLock".into()) else {
            return;
        };
        let Ok(request) = js_sys::Reflect::get(&wake_lock, &"request".into()) else {
            return;
        };
        let Some(request) = request.dyn_ref::<js_sys::Function>() else {
            return;
        };
        let Ok(promise) = request.call1(&wake_lock, &"screen".into()) else {
            return;
        };
        let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
            return;
        };
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(sentinel) = wasm_bindgen_futures::JsFuture::from(promise).await {
                LOCK.with(|lock| *lock.borrow_mut() = Some(sentinel));
            }
        });
    }

    use wasm_bindgen::JsCast;
}
