//! Whether the system is in dark mode, asked of each platform its own way.
//! Every answer is cheap enough to read each frame; Linux's is a D-Bus round
//! trip, so a thread asks there and the frame reads what it last heard.

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

/// `AppsUseLightTheme` is what the Settings app's "Choose your mode" writes
/// for applications, as opposed to the taskbar's own `SystemUsesLightTheme`.
#[cfg(windows)]
pub(crate) fn is_dark() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let value: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
    let mut light: u32 = 1;
    let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(4);
    // SAFETY: both names are NUL-terminated UTF-16, and the out pointer and
    // its size describe one `u32`, which is what `RRF_RT_REG_DWORD` writes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut light).cast(),
            &raw mut size,
        )
    };
    status == 0 && light == 0
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

#[cfg(target_os = "linux")]
pub(crate) fn is_dark() -> bool {
    portal::is_dark()
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

/// The XDG desktop portal's `org.freedesktop.appearance` `color-scheme`, over
/// the system's own `libdbus`, loaded when first asked as SDL and Godot do:
/// a desktop without it, or without a portal, answers light. A thread reads it
/// once, then sleeps on the bus until the portal says a setting changed.
#[cfg(target_os = "linux")]
mod portal {
    use std::ffi::{c_char, c_int, c_void};
    use std::sync::Once;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The portal's settings interface, and the signal it sends on a change.
    const SETTINGS: &std::ffi::CStr = c"org.freedesktop.portal.Settings";
    const CHANGED: &std::ffi::CStr = c"SettingChanged";
    /// Only the appearance namespace's changes reach the thread.
    const MATCH: &std::ffi::CStr = c"type='signal',interface='org.freedesktop.portal.Settings',member='SettingChanged',arg0='org.freedesktop.appearance'";
    /// `dbus_connection_read_write`'s wait for "until something arrives".
    const FOREVER: c_int = -1;
    /// `1` is "prefer dark", `2` "prefer light", `0` no preference.
    const PREFER_DARK: u32 = 1;
    const TIMEOUT_MS: c_int = 500;
    const BUS_SESSION: c_int = 0;
    const TYPE_STRING: c_int = b's' as c_int;
    const TYPE_UINT32: c_int = b'u' as c_int;
    const TYPE_VARIANT: c_int = b'v' as c_int;

    static DARK: AtomicBool = AtomicBool::new(false);
    static START: Once = Once::new();

    pub(super) fn is_dark() -> bool {
        START.call_once(|| {
            let spawned = std::thread::Builder::new()
                .name("balaur-appearance".into())
                .spawn(watch);
            if let Err(err) = spawned {
                tracing::debug!(%err, "dark mode: no thread to ask the portal from");
            }
        });
        DARK.load(Ordering::Relaxed)
    }

    fn watch() {
        let Some(bus) = Bus::open() else {
            return;
        };
        let Some(scheme) = bus.color_scheme() else {
            return;
        };
        DARK.store(scheme == PREFER_DARK, Ordering::Relaxed);
        if !bus.listen() {
            tracing::debug!(
                "dark mode: the portal's changes cannot be heard; keeping the first answer"
            );
            return;
        }
        while bus.changed() {
            let Some(scheme) = bus.color_scheme() else {
                return;
            };
            let dark = scheme == PREFER_DARK;
            // A sleeping loop repaints in the new mode.
            if DARK.swap(dark, Ordering::Relaxed) != dark {
                balaur_core::wake::wake();
            }
        }
    }

    /// `DBusError` as `dbus-errors.h` lays it out: two strings, a bit field
    /// and a pointer libdbus keeps for itself.
    #[repr(C)]
    struct Error {
        _name: *const c_char,
        _message: *const c_char,
        _bits: u32,
        _padding: *mut c_void,
    }

    /// `DBusMessageIter` is a stack struct of a size the header fixes; this is
    /// more room than any libdbus has asked for, aligned as its pointers are.
    #[repr(C)]
    struct Iter {
        _room: [usize; 16],
    }

    impl Iter {
        const fn new() -> Self {
            Self { _room: [0; 16] }
        }
    }

    type ErrorInit = unsafe extern "C" fn(*mut Error);
    type ErrorFree = unsafe extern "C" fn(*mut Error);
    type BusGet = unsafe extern "C" fn(c_int, *mut Error) -> *mut c_void;
    type ExitOnDisconnect = unsafe extern "C" fn(*mut c_void, u32);
    type NewCall = unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> *mut c_void;
    type InitAppend = unsafe extern "C" fn(*mut c_void, *mut Iter);
    type AppendBasic = unsafe extern "C" fn(*mut Iter, c_int, *const c_void) -> u32;
    type SendBlock =
        unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, *mut Error) -> *mut c_void;
    type IterInit = unsafe extern "C" fn(*mut c_void, *mut Iter) -> u32;
    type ArgType = unsafe extern "C" fn(*mut Iter) -> c_int;
    type Recurse = unsafe extern "C" fn(*mut Iter, *mut Iter);
    type GetBasic = unsafe extern "C" fn(*mut Iter, *mut c_void);
    type Unref = unsafe extern "C" fn(*mut c_void);
    type AddMatch = unsafe extern "C" fn(*mut c_void, *const c_char, *mut Error);
    type ReadWrite = unsafe extern "C" fn(*mut c_void, c_int) -> u32;
    type PopMessage = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
    type IsSignal = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> u32;
    type ErrorIsSet = unsafe extern "C" fn(*const Error) -> u32;

    struct Bus {
        _lib: libloading::Library,
        connection: *mut c_void,
        error_init: ErrorInit,
        error_free: ErrorFree,
        new_call: NewCall,
        init_append: InitAppend,
        append_basic: AppendBasic,
        send_block: SendBlock,
        iter_init: IterInit,
        arg_type: ArgType,
        recurse: Recurse,
        get_basic: GetBasic,
        unref: Unref,
        add_match: AddMatch,
        read_write: ReadWrite,
        pop_message: PopMessage,
        is_signal: IsSignal,
        error_is_set: ErrorIsSet,
    }

    impl Bus {
        fn open() -> Option<Self> {
            // SAFETY: libdbus's initialisers run no code a caller could race;
            // each symbol is cast to the signature `dbus.h` declares for it.
            unsafe {
                let lib = libloading::Library::new("libdbus-1.so.3").ok()?;
                let bus_get: BusGet = *lib.get(b"dbus_bus_get_private\0").ok()?;
                let exit_on_disconnect: ExitOnDisconnect =
                    *lib.get(b"dbus_connection_set_exit_on_disconnect\0").ok()?;
                let bus = Self {
                    connection: std::ptr::null_mut(),
                    error_init: *lib.get(b"dbus_error_init\0").ok()?,
                    error_free: *lib.get(b"dbus_error_free\0").ok()?,
                    new_call: *lib.get(b"dbus_message_new_method_call\0").ok()?,
                    init_append: *lib.get(b"dbus_message_iter_init_append\0").ok()?,
                    append_basic: *lib.get(b"dbus_message_iter_append_basic\0").ok()?,
                    send_block: *lib
                        .get(b"dbus_connection_send_with_reply_and_block\0")
                        .ok()?,
                    iter_init: *lib.get(b"dbus_message_iter_init\0").ok()?,
                    arg_type: *lib.get(b"dbus_message_iter_get_arg_type\0").ok()?,
                    recurse: *lib.get(b"dbus_message_iter_recurse\0").ok()?,
                    get_basic: *lib.get(b"dbus_message_iter_get_basic\0").ok()?,
                    unref: *lib.get(b"dbus_message_unref\0").ok()?,
                    add_match: *lib.get(b"dbus_bus_add_match\0").ok()?,
                    read_write: *lib.get(b"dbus_connection_read_write\0").ok()?,
                    pop_message: *lib.get(b"dbus_connection_pop_message\0").ok()?,
                    is_signal: *lib.get(b"dbus_message_is_signal\0").ok()?,
                    error_is_set: *lib.get(b"dbus_error_is_set\0").ok()?,
                    _lib: lib,
                };
                let mut error = bus.error();
                let connection = bus_get(BUS_SESSION, &raw mut error);
                (bus.error_free)(&raw mut error);
                if connection.is_null() {
                    tracing::debug!("dark mode: no session bus");
                    return None;
                }
                // A connection libdbus opens exits the process when the bus
                // goes away, unless told otherwise.
                exit_on_disconnect(connection, 0);
                Some(Self { connection, ..bus })
            }
        }

        fn error(&self) -> Error {
            let mut error = Error {
                _name: std::ptr::null(),
                _message: std::ptr::null(),
                _bits: 0,
                _padding: std::ptr::null_mut(),
            };
            // SAFETY: `error` is a `DBusError` this function owns.
            unsafe { (self.error_init)(&raw mut error) };
            error
        }

        /// Ask the bus for the portal's appearance changes. False when it
        /// refused the match.
        fn listen(&self) -> bool {
            let mut error = self.error();
            // SAFETY: the connection is live, the rule outlives the call, and
            // `error` is a `DBusError` this function owns.
            unsafe {
                (self.add_match)(self.connection, MATCH.as_ptr(), &raw mut error);
                let refused = (self.error_is_set)(&raw const error) != 0;
                (self.error_free)(&raw mut error);
                !refused
            }
        }

        /// Sleep until the portal says an appearance setting changed. False
        /// when the bus went away.
        fn changed(&self) -> bool {
            // SAFETY: the connection is live; each message popped is libdbus's
            // until it is unreferenced here.
            unsafe {
                loop {
                    if (self.read_write)(self.connection, FOREVER) == 0 {
                        return false;
                    }
                    let mut heard = false;
                    loop {
                        let message = (self.pop_message)(self.connection);
                        if message.is_null() {
                            break;
                        }
                        heard |=
                            (self.is_signal)(message, SETTINGS.as_ptr(), CHANGED.as_ptr()) != 0;
                        (self.unref)(message);
                    }
                    if heard {
                        return true;
                    }
                }
            }
        }

        /// `Settings.ReadOne("org.freedesktop.appearance", "color-scheme")`,
        /// or `None` when the portal does not answer it.
        fn color_scheme(&self) -> Option<u32> {
            // SAFETY: every pointer handed to libdbus is either one it made
            // and has not freed, or a NUL-terminated string that outlives the
            // call; the reply is unreferenced once read.
            unsafe {
                let call = (self.new_call)(
                    c"org.freedesktop.portal.Desktop".as_ptr(),
                    c"/org/freedesktop/portal/desktop".as_ptr(),
                    SETTINGS.as_ptr(),
                    c"ReadOne".as_ptr(),
                );
                if call.is_null() {
                    return None;
                }
                let mut args = Iter::new();
                (self.init_append)(call, &raw mut args);
                for text in [c"org.freedesktop.appearance", c"color-scheme"] {
                    let pointer = text.as_ptr();
                    (self.append_basic)(&raw mut args, TYPE_STRING, (&raw const pointer).cast());
                }
                let mut error = self.error();
                let reply = (self.send_block)(self.connection, call, TIMEOUT_MS, &raw mut error);
                (self.unref)(call);
                (self.error_free)(&raw mut error);
                if reply.is_null() {
                    return None;
                }
                let scheme = self.read_uint(reply);
                (self.unref)(reply);
                scheme
            }
        }

        /// The reply's first argument, through however many variants wrap
        /// it, as a `u32`.
        unsafe fn read_uint(&self, reply: *mut c_void) -> Option<u32> {
            // SAFETY: `reply` is a live message; the iterators live on this
            // stack frame for as long as libdbus reads through them.
            unsafe {
                let mut at = Iter::new();
                if (self.iter_init)(reply, &raw mut at) == 0 {
                    return None;
                }
                loop {
                    match (self.arg_type)(&raw mut at) {
                        TYPE_VARIANT => {
                            let mut inner = Iter::new();
                            (self.recurse)(&raw mut at, &raw mut inner);
                            at = inner;
                        }
                        TYPE_UINT32 => {
                            let mut value: u32 = 0;
                            (self.get_basic)(&raw mut at, (&raw mut value).cast());
                            return Some(value);
                        }
                        _ => return None,
                    }
                }
            }
        }
    }
}
