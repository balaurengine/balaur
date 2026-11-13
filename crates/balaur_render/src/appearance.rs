//! Whether the system is in dark mode, asked of each platform its own way.
//! Every answer is cheap enough to read each frame; Linux's and Windows' come
//! from a `mundy` subscription, and the frame reads what it last heard.

/// True where the system says dark; false where it says light or nothing.
#[cfg(target_os = "macos")]
pub(crate) fn is_dark() -> bool {
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

#[cfg(target_family = "wasm")]
pub(crate) fn is_dark() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .is_some_and(|list| list.matches())
}

#[cfg(target_os = "ios")]
pub(crate) fn is_dark() -> bool {
    use objc2_foundation::MainThreadMarker;
    use objc2_ui_kit::{UIScreen, UITraitEnvironment, UIUserInterfaceStyle};
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    #[allow(
        deprecated,
        reason = "the window scene's screen needs a scene this code does not hold"
    )]
    let screen = UIScreen::mainScreen(mtm);
    // SAFETY: read on the main thread, which `mtm` proves this is.
    let style = unsafe { screen.traitCollection().userInterfaceStyle() };
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

#[cfg(target_os = "android")]
pub(crate) fn is_dark() -> bool {
    use kiss3d::winit::platform::android::activity::ndk::configuration::UiModeNight;
    ANDROID_APP
        .get()
        .is_some_and(|app| app.config().ui_mode_night() == UiModeNight::Yes)
}

/// The desktop's own setting as `mundy` reads it: the XDG portal's
/// `color-scheme` on Linux, the Settings app's choice on Windows. Subscribed
/// once, so a change reaches the next frame without anything asking again.
#[cfg(any(target_os = "linux", windows))]
pub(crate) fn is_dark() -> bool {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    static DARK: AtomicBool = AtomicBool::new(false);
    static WATCH: OnceLock<mundy::Subscription> = OnceLock::new();
    let dark = |prefs: &mundy::Preferences| prefs.color_scheme == mundy::ColorScheme::Dark;
    WATCH.get_or_init(|| {
        // The first answer waited for, bounded, so a dark desktop's first
        // frame is not drawn light; the subscription hears every change after.
        let first = mundy::Preferences::once_blocking(
            mundy::Interest::ColorScheme,
            std::time::Duration::from_millis(250),
        );
        DARK.store(first.as_ref().is_some_and(dark), Ordering::Relaxed);
        mundy::Preferences::subscribe(mundy::Interest::ColorScheme, move |prefs| {
            DARK.store(dark(&prefs), Ordering::Relaxed);
            // A loop asleep between inputs draws the new look only once woken.
            balaur_core::wake::wake();
        })
    });
    DARK.load(Ordering::Relaxed)
}

#[cfg(not(any(
    target_os = "macos",
    target_family = "wasm",
    windows,
    target_os = "ios",
    target_os = "android",
    target_os = "linux"
)))]
pub(crate) const fn is_dark() -> bool {
    false
}
