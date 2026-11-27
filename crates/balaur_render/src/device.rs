//! What the window and the display say, published to the engine once per
//! frame as recorded input: focus, dark mode, reduced motion, the safe area
//! and the display's refresh rate. Each platform answers what it can; the
//! rest keeps the default.
//!
//! Dark mode, the safe area, the keyboard and the text scale are asked of the
//! platform only once it says they moved ([`moved`]): a resize, a scale or
//! appearance change, the window coming back. What a platform never says is
//! asked every frame ([`unheard`]).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use balaur_core::App;

/// Counts the platform's word that a device fact may have moved.
static MOVED: AtomicU64 = AtomicU64::new(0);

/// A device fact may have moved: the next frame asks the platform again.
/// Callable from any thread, as a subscription's callback is.
pub(crate) fn moved() {
    MOVED.fetch_add(1, Ordering::Relaxed);
}

/// The window events that say a fact may have moved. A theme change carries
/// its answer, which is kept as the one the platform last gave.
pub(crate) fn hear(event: &kiss3d::event::WindowEvent) {
    use kiss3d::event::WindowEvent;
    match event {
        WindowEvent::DarkMode(dark) => crate::appearance::hear(*dark),
        WindowEvent::FramebufferSize(..)
        | WindowEvent::ScaleFactor(_)
        | WindowEvent::Focus(_)
        | WindowEvent::Iconify(_) => moved(),
        _ => {}
    }
}

/// What the platform said when last asked.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Reading {
    pub(crate) dark_mode: bool,
    /// `None` where this crate cannot ask the platform.
    pub(crate) reduced_motion: Option<bool>,
    pub(crate) safe_area: [f32; 4],
    pub(crate) keyboard_height: f32,
    pub(crate) text_scale: f32,
}

impl Reading {
    fn of(window: &kiss3d::window::Window) -> Self {
        #[cfg(target_family = "wasm")]
        web::listen();
        Self {
            dark_mode: crate::appearance::is_dark(),
            reduced_motion: crate::motion::reduced(),
            safe_area: safe_area(window),
            keyboard_height: keyboard_height(window),
            text_scale: window.text_scale().max(f32::EPSILON),
        }
    }
}

/// What this platform asks again every frame because nothing says when it
/// moves: Android's content rect, which winit drops `ContentRectChanged` for;
/// iOS's keyboard, whose notification kiss3d keeps to itself, and its safe
/// area and dark mode, which only a view's own overrides hear.
#[cfg_attr(
    not(mobile),
    allow(
        clippy::missing_const_for_fn,
        reason = "only a phone reads anything here"
    )
)]
fn unheard(window: &kiss3d::window::Window, reading: &mut Reading) {
    #[cfg(target_os = "android")]
    {
        reading.safe_area = safe_area(window);
        reading.keyboard_height = keyboard_height(window);
    }
    #[cfg(target_os = "ios")]
    {
        reading.dark_mode = crate::appearance::is_dark();
        reading.reduced_motion = crate::motion::reduced();
        reading.safe_area = safe_area(window);
        reading.keyboard_height = keyboard_height(window);
    }
    #[cfg(not(mobile))]
    let _ = (window, reading);
}

/// Frame intervals the refresh rate is measured over, where the system does
/// not say it.
const SAMPLES: usize = 90;

/// The longest frame a display's refresh could be: a frame slower than 20 Hz
/// is a loop asleep between inputs or one held up, not the display.
const LONGEST_REFRESH: f32 = 1.0 / 20.0;

/// The measurements one window keeps between frames.
#[derive(Default)]
pub(crate) struct Probe {
    intervals: VecDeque<f32>,
    /// The display's own rate, asked of the system again when the scale
    /// changes, as a move to another display does, or after `SAMPLES` frames:
    /// asking is a driver query on macOS and Windows.
    display_rate: Option<f32>,
    asked_at_scale: f64,
    frames_since_asked: usize,
    /// The platform's last answer, and the [`MOVED`] count it was asked at.
    heard: Option<(u64, Reading)>,
}

impl Probe {
    /// This frame's answer: asked again only once the platform says a fact
    /// moved, with what it never says asked every frame.
    pub(crate) fn read(&mut self, window: &kiss3d::window::Window) -> Reading {
        let mut reading = self.reading(MOVED.load(Ordering::Relaxed), || Reading::of(window));
        unheard(window, &mut reading);
        reading
    }

    /// The answer kept from `moved`, or a fresh one from `ask` when the
    /// platform has said something moved since.
    fn reading(&mut self, moved: u64, ask: impl FnOnce() -> Reading) -> Reading {
        if let Some((at, reading)) = self.heard
            && at == moved
        {
            return reading;
        }
        let reading = ask();
        self.heard = Some((moved, reading));
        reading
    }

    /// Fold this frame in and publish every fact the platform can answer.
    pub(crate) fn publish(
        &mut self,
        app: &App,
        window: &kiss3d::window::Window,
        dt: f32,
        reading: &Reading,
    ) {
        if dt > 0.0 && dt <= LONGEST_REFRESH {
            if self.intervals.len() == SAMPLES {
                self.intervals.pop_front();
            }
            self.intervals.push_back(dt);
        }
        let scale = window.scale_factor();
        if (scale - self.asked_at_scale).abs() > f64::EPSILON || self.frames_since_asked >= SAMPLES
        {
            self.display_rate = window.refresh_rate();
            self.asked_at_scale = scale;
            self.frames_since_asked = 0;
        }
        self.frames_since_asked += 1;
        let refresh_rate = self.display_rate.or_else(|| self.refresh_rate());
        let Reading {
            dark_mode,
            reduced_motion,
            safe_area,
            keyboard_height: keyboard,
            text_scale,
        } = *reading;
        let screen_size = [window.width() as f32, window.height() as f32];
        // Physical pixels per design pixel: the zoom times the display's own
        // scale, since the scale a script sets is only half of it.
        let ui_scale = window.egui_pixels_per_point().max(f32::EPSILON);
        let game_area = game_area(app, ui_scale);
        balaur_core::facts::update_device(&app.engine, |facts| {
            facts.dark_mode = dark_mode;
            if let Some(reduced) = reduced_motion {
                facts.reduced_motion = reduced;
            }
            facts.safe_area = safe_area;
            facts.screen_size = screen_size;
            facts.ui_scale = ui_scale;
            facts.text_scale = text_scale;
            facts.keyboard_height = keyboard;
            facts.game_area = game_area;
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

/// The screen the game's widgets draw on in physical pixels, so touch controls
/// sit where they do: its viewport in the editor, nowhere while that viewport
/// has no size, and the window everywhere else.
fn game_area(app: &App, scale: f32) -> Option<[f32; 4]> {
    // Measured in design pixels; this fact is physical.
    balaur_ui::content_screen(&app.engine).map(|rect| rect.map(|v| v * scale))
}

/// Hand the scale a script asked for to egui, as its zoom factor, before
/// anything reads what a design pixel became. egui lays the whole pass out in
/// it, its own widgets included, which is why nothing multiplies a size by
/// hand any more.
pub(crate) fn apply_ui_zoom(app: &App, window: &mut kiss3d::window::Window, text_scale: f32) {
    let want = balaur_ui::scale(&app.engine);
    // The reader's own text size multiplies it, so a game that never touches
    // the scale still grows for someone who asked their phone for large type.
    let asked = if balaur_ui::takes_system_text_size(&app.engine) {
        text_scale
    } else {
        1.0
    };
    window.set_ui_zoom(want * asked);
}

/// The window came to the front or went behind.
pub(crate) fn set_focused(app: &App, focused: bool) {
    balaur_core::facts::update_device(&app.engine, |facts| facts.focused = focused);
}

/// The system warned it is short of memory.
pub(crate) fn warn_low_memory(app: &App) {
    balaur_core::facts::update_device(&app.engine, |facts| {
        facts.memory_warnings = facts.memory_warnings.saturating_add(1);
    });
}

/// A window out of sight or an app in the background, as the backend says.
pub(crate) fn set_suspended(app: &App, suspended: bool) {
    balaur_core::facts::update_device(&app.engine, |facts| facts.suspended = suspended);
}

/// Ask the platform to keep the screen on, or let it dim again.
pub(crate) fn keep_awake(on: bool) {
    #[cfg(target_family = "wasm")]
    web::keep_awake(on);
    #[cfg(not(target_family = "wasm"))]
    tracing::debug!(on, "keep awake: nothing to ask on this platform");
}

/// What the on-screen keyboard covers: the part of the window the visual
/// viewport no longer reaches. A window asks kiss3d, which asks UIKit or the
/// Android activity and answers zero on a desktop.
#[cfg(target_family = "wasm")]
fn keyboard_height(_window: &kiss3d::window::Window) -> f32 {
    let Some(window) = web_sys::window() else {
        return 0.0;
    };
    let inner = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let Some(viewport) = window.visual_viewport() else {
        return 0.0;
    };
    let covered = inner - viewport.height() - viewport.offset_top();
    let ratio = window.device_pixel_ratio();
    (covered.max(0.0) * ratio) as f32
}

#[cfg(not(target_family = "wasm"))]
fn keyboard_height(window: &kiss3d::window::Window) -> f32 {
    window.keyboard_height()
}

/// The page reads its insets off the shell's CSS; a window asks kiss3d.
#[cfg(target_family = "wasm")]
fn safe_area(_window: &kiss3d::window::Window) -> [f32; 4] {
    web::safe_area()
}

#[cfg(not(target_family = "wasm"))]
fn safe_area(window: &kiss3d::window::Window) -> [f32; 4] {
    window.safe_area()
}

#[cfg(target_family = "wasm")]
mod web {
    use wasm_bindgen::JsValue;

    const RESIZE: &str = "resize";
    const SCROLL: &str = "scroll";
    /// The game's canvas, the one the web shell lays out.
    const CANVAS: &str = "canvas";

    /// Hear the page move what [`super::Reading`] holds, once for the page:
    /// the window resizing turns it and moves the safe area, and the visual
    /// viewport shrinking or scrolling is the keyboard coming up.
    pub(super) fn listen() {
        use wasm_bindgen::closure::Closure;
        thread_local! {
            static LISTENING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        }
        if LISTENING.with(|listening| listening.replace(true)) {
            return;
        }
        let Some(window) = web_sys::window() else {
            return;
        };
        let moved = Closure::<dyn FnMut()>::new(|| {
            super::moved();
            balaur_core::wake::wake();
        });
        let _ = window.add_event_listener_with_callback(RESIZE, moved.as_ref().unchecked_ref());
        if let Some(viewport) = window.visual_viewport() {
            for name in [RESIZE, SCROLL] {
                let _ =
                    viewport.add_event_listener_with_callback(name, moved.as_ref().unchecked_ref());
            }
        }
        // A page resizing the game's box moves no window, so the canvas is
        // watched too: a resting loop would otherwise wait for the pointer.
        if let Some(canvas) = window
            .document()
            .and_then(|document| document.query_selector(CANVAS).ok().flatten())
            && let Ok(observer) = web_sys::ResizeObserver::new(moved.as_ref().unchecked_ref())
        {
            observer.observe(&canvas);
            std::mem::forget(observer);
        }
        // Heard for the page's life, as kiss3d's own listeners are.
        moved.forget();
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
                if let Some(sentinel) = lock.borrow_mut().take()
                    && let Ok(release) = js_sys::Reflect::get(&sentinel, &"release".into())
                    && let Some(release) = release.dyn_ref::<js_sys::Function>()
                {
                    let _ = release.call0(&sentinel);
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
        let Ok(promise) = request.call1(&wake_lock, &crate::vocabulary::words::SCREEN.into())
        else {
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

#[cfg(test)]
mod tests {
    use super::{Probe, Reading};

    #[test]
    fn the_platform_is_asked_again_only_once_it_says_a_fact_moved() {
        let mut probe = Probe::default();
        let asked = std::cell::Cell::new(0);
        let ask = |dark_mode| {
            asked.set(asked.get() + 1);
            Reading {
                dark_mode,
                ..Reading::default()
            }
        };
        for _ in 0..3 {
            assert!(!probe.reading(4, || ask(false)).dark_mode);
        }
        assert_eq!(asked.get(), 1, "three frames with nothing moved asked once");
        assert!(probe.reading(5, || ask(true)).dark_mode, "a move is asked");
        assert!(probe.reading(5, || ask(false)).dark_mode, "and kept");
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn a_resize_says_the_facts_moved() {
        use kiss3d::event::WindowEvent;
        let before = super::MOVED.load(std::sync::atomic::Ordering::Relaxed);
        super::hear(&WindowEvent::CursorPos(
            1.0,
            2.0,
            kiss3d::event::Modifiers::empty(),
        ));
        super::hear(&WindowEvent::FramebufferSize(640, 480));
        let after = super::MOVED.load(std::sync::atomic::Ordering::Relaxed);
        assert!(after > before);
    }

    #[test]
    fn an_appearance_the_platform_announces_is_the_answer_until_the_next() {
        use kiss3d::event::WindowEvent;
        let before = super::MOVED.load(std::sync::atomic::Ordering::Relaxed);
        super::hear(&WindowEvent::DarkMode(true));
        assert!(crate::appearance::is_dark());
        super::hear(&WindowEvent::DarkMode(false));
        assert!(!crate::appearance::is_dark());
        let after = super::MOVED.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            after >= before + 2,
            "each announcement asks the facts again"
        );
    }
}
