//! Whether the system is in dark mode, asked of each platform its own way,
//! and kept once the platform announces it: winit's theme event on a desktop
//! window, Linux's and Windows' `mundy` subscription, a page's `matchMedia`
//! listener. An iPhone is asked every frame and an Android phone when its
//! configuration changes, since neither announces the answer.

use std::sync::atomic::{AtomicU8, Ordering};

const UNHEARD: u8 = 0;
const LIGHT: u8 = 1;
const DARK: u8 = 2;

/// What the platform last announced, or [`UNHEARD`] before it has.
static HEARD: AtomicU8 = AtomicU8::new(UNHEARD);

/// The platform announced its appearance: kept as the answer from now on,
/// and the device facts read again. Any thread.
pub(crate) fn hear(dark: bool) {
    HEARD.store(if dark { DARK } else { LIGHT }, Ordering::Relaxed);
    crate::device::moved();
}

/// True where the system says dark; false where it says light or nothing.
pub(crate) fn is_dark() -> bool {
    match HEARD.load(Ordering::Relaxed) {
        DARK => true,
        LIGHT => false,
        _ => asked(),
    }
}

#[cfg(target_os = "macos")]
fn asked() -> bool {
    // The system setting first: an offscreen run has no application to ask,
    // and one that has not opened a window yet answers with the default
    // light appearance whatever the desktop is set to.
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{MainThreadMarker, NSString, NSUserDefaults};
    let defaults = NSUserDefaults::standardUserDefaults();
    let style = defaults.stringForKey(&NSString::from_str("AppleInterfaceStyle"));
    if let Some(style) = style {
        return style.to_string().contains("Dark");
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let name = app.effectiveAppearance().name();
    name.to_string().contains("Dark")
}

/// The page's answer, with a listener on the query so every change after is
/// heard rather than asked.
#[cfg(target_family = "wasm")]
fn asked() -> bool {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    const DARK_QUERY: &str = "(prefers-color-scheme: dark)";
    const CHANGE: &str = "change";
    let Some(list) = web_sys::window().and_then(|w| w.match_media(DARK_QUERY).ok().flatten())
    else {
        return false;
    };
    let changed = {
        let list = list.clone();
        Closure::<dyn FnMut()>::new(move || {
            hear(list.matches());
            balaur_core::wake::wake();
        })
    };
    let _ = list.add_event_listener_with_callback(CHANGE, changed.as_ref().unchecked_ref());
    // Heard for the page's life: only the first ask gets here, the rest are heard.
    changed.forget();
    let dark = list.matches();
    HEARD.store(if dark { DARK } else { LIGHT }, Ordering::Relaxed);
    dark
}

#[cfg(target_os = "ios")]
fn asked() -> bool {
    use objc2_foundation::MainThreadMarker;
    use objc2_ui_kit::{UIApplication, UIUserInterfaceStyle, UIWindowScene};
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    // None before the first scene connects; asked again next frame.
    let Some(scene) = UIApplication::sharedApplication(mtm)
        .connectedScenes()
        .iter()
        .find_map(|scene| scene.downcast::<UIWindowScene>().ok())
    else {
        return false;
    };
    // SAFETY: read on the main thread, which `mtm` proves this is.
    let style = unsafe { scene.traitCollection().userInterfaceStyle() };
    style == UIUserInterfaceStyle::Dark
}

#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::OnceLock<kiss3d::winit::platform::android::activity::AndroidApp> =
    std::sync::OnceLock::new();

/// Keep the activity handle `android_main` was given, which the configuration
/// is read off. kiss3d keeps its own copy for the window.
#[cfg(target_os = "android")]
pub fn keep_android_app(app: &kiss3d::winit::platform::android::activity::AndroidApp) {
    let _ = ANDROID_APP.set(app.clone());
}

/// The configuration android-activity keeps; a change to it arrives as
/// winit's scale event, which asks again.
#[cfg(target_os = "android")]
fn asked() -> bool {
    use kiss3d::winit::platform::android::activity::ndk::configuration::UiModeNight;
    ANDROID_APP
        .get()
        .is_some_and(|app| app.config().ui_mode_night() == UiModeNight::Yes)
}

/// The desktop's own setting as `mundy` reads it: the XDG portal's
/// `color-scheme` on Linux, the Settings app's choice on Windows. Subscribed
/// once, so every change after is heard rather than asked.
#[cfg(any(target_os = "linux", windows))]
fn asked() -> bool {
    use std::sync::OnceLock;
    static WATCH: OnceLock<mundy::Subscription> = OnceLock::new();
    let dark = |prefs: &mundy::Preferences| prefs.color_scheme == mundy::ColorScheme::Dark;
    WATCH.get_or_init(|| {
        // The first answer waited for, bounded, so a dark desktop's first
        // frame is not drawn light; the subscription hears every change after.
        let first = mundy::Preferences::once_blocking(
            mundy::Interest::ColorScheme,
            std::time::Duration::from_millis(250),
        );
        let first = if first.as_ref().is_some_and(dark) {
            DARK
        } else {
            LIGHT
        };
        let _ = HEARD.compare_exchange(UNHEARD, first, Ordering::Relaxed, Ordering::Relaxed);
        mundy::Preferences::subscribe(mundy::Interest::ColorScheme, move |prefs| {
            hear(dark(&prefs));
            // A loop asleep between inputs draws the new look only once woken.
            balaur_core::wake::wake();
        })
    });
    HEARD.load(Ordering::Relaxed) == DARK
}

#[cfg(not(any(
    target_os = "macos",
    target_family = "wasm",
    windows,
    target_os = "ios",
    target_os = "android",
    target_os = "linux"
)))]
const fn asked() -> bool {
    false
}
