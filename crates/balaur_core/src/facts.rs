//! Facts about the machine and the moment that scripts may read: the OS,
//! whether this is a page or a phone, a stable id for the install, the
//! wall clock. Every one is external input, so each is recorded and answered
//! from the recording on replay.

use serde::{Deserialize, Serialize};

use crate::engine::Engine;

/// What never changes for one run: recorded once, in the recording's
/// header, so a replay on another machine answers as the original did.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlatformFacts {
    pub os: String,
    pub web: bool,
    pub mobile: bool,
    pub editor: bool,
    pub system_locale: Option<String>,
    pub device_id: String,
}

impl PlatformFacts {
    /// This build, on this machine.
    pub fn current(eng: &Engine) -> Self {
        let os = if cfg!(target_family = "wasm") {
            "web"
        } else {
            std::env::consts::OS
        };
        Self {
            os: os.to_string(),
            web: cfg!(target_family = "wasm"),
            mobile: cfg!(any(target_os = "ios", target_os = "android")),
            editor: eng.debug_scope().is_some(),
            system_locale: sys_locale::get_locale(),
            device_id: device_id(eng),
        }
    }
}

/// The facts once read, or restored from a recording's header. Read lazily,
/// since the device id lives under a directory named by the manifest.
#[derive(Default)]
pub struct Facts(pub Option<PlatformFacts>);

/// The facts, reading them on first use.
pub fn platform(eng: &Engine) -> PlatformFacts {
    let facts = eng.resource::<Facts>();
    if let Some(known) = &facts.borrow().0 {
        return known.clone();
    }
    let read = PlatformFacts::current(eng);
    facts.borrow_mut().0 = Some(read.clone());
    read
}

/// The wall clock as of the top of this tick, in seconds since the epoch.
/// Read once per frame and recorded, never mid-tick.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct WallClock {
    pub unix_time: f64,
}

/// One id per install, made once and kept in the user directory; what a
/// device login sends. Random from the clock and the process, never from
/// the engine's own stream, which must stay the game's.
#[allow(
    clippy::disallowed_methods,
    reason = "the wall clock seeds an id made once per install, outside any tick"
)]
fn device_id(eng: &Engine) -> String {
    let path = crate::engine_api::user_data_dir_of(eng).join("device_id");
    let fs = crate::files::backend(eng);
    if let Ok(bytes) = fs.read(&path) {
        let text = String::from_utf8_lossy(&bytes).trim().to_string();
        if !text.is_empty() {
            return text;
        }
    }
    let nanos = crate::time::SystemTime::now()
        .duration_since(crate::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut h = crate::digest::Hasher::new();
    h.write(&nanos.to_le_bytes());
    h.write_u64(u64::from(std::process::id()));
    h.write_u64(eng.tick());
    let id = format!("{}", h.finish());
    if let Some(parent) = path.parent() {
        let _ = fs.mkdir(parent);
    }
    if let Err(err) = fs.write(&path, id.as_bytes()) {
        tracing::warn!("the device id could not be kept: {err}");
    }
    id
}

#[allow(
    clippy::disallowed_methods,
    reason = "the one place the wall clock enters, read once per tick and recorded"
)]
pub(crate) fn read_clock_system(eng: &Engine, _: f32) {
    if crate::replay::is_playing(eng) {
        return;
    }
    let now = crate::time::SystemTime::now()
        .duration_since(crate::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    if let Some(clock) = eng.try_resource::<WallClock>() {
        clock.borrow_mut().unix_time = now;
    }
}

/// What the window and the display say this tick: whether the game is in
/// front, whether the system is in dark mode, the screen's safe area and its
/// refresh rate. Read once per frame by the backend and recorded, so a
/// replay sees what the original did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceFacts {
    pub focused: bool,
    pub dark_mode: bool,
    /// Left, top, right and bottom insets in pixels: what a notch or a home
    /// bar covers. Zero on a desktop.
    pub safe_area: [f32; 4],
    /// Frames per second the display refreshes at, as measured.
    pub refresh_rate: f32,
}

impl Default for DeviceFacts {
    fn default() -> Self {
        Self {
            focused: true,
            dark_mode: false,
            safe_area: [0.0; 4],
            refresh_rate: 60.0,
        }
    }
}

/// The device facts as of this tick and as of the last one, so a change is
/// announced once.
#[derive(Default)]
pub struct Device {
    pub now: DeviceFacts,
    pub was: DeviceFacts,
}

/// The device facts as of this tick.
pub fn device(eng: &Engine) -> DeviceFacts {
    eng.try_resource::<Device>()
        .map(|d| d.borrow().now.clone())
        .unwrap_or_default()
}

/// A backend's report. Ignored while a recording plays: the recording says
/// what the device said.
pub fn update_device(eng: &Engine, change: impl FnOnce(&mut DeviceFacts)) {
    if crate::replay::is_playing(eng) {
        return;
    }
    if let Some(device) = eng.try_resource::<Device>() {
        change(&mut device.borrow_mut().now);
    }
}

/// Tell every script what changed since the last tick: `on_focus_changed`
/// and `on_dark_mode`, each with the new state.
pub(crate) fn announce_device_system(eng: &Engine, _: f32) {
    let Some(device) = eng.try_resource::<Device>() else {
        return;
    };
    let (focus, dark) = {
        let mut device = device.borrow_mut();
        let Device { now, was } = &mut *device;
        let focus = (now.focused != was.focused).then_some(now.focused);
        let dark = (now.dark_mode != was.dark_mode).then_some(now.dark_mode);
        *was = now.clone();
        (focus, dark)
    };
    if focus.is_none() && dark.is_none() {
        return;
    }
    let Some(host) = eng.script_host() else {
        return;
    };
    if let Some(focused) = focus {
        host.call_all_with("on_focus_changed", &[balaur_script::Value::Bool(focused)]);
    }
    if let Some(dark) = dark {
        host.call_all_with("on_dark_mode", &[balaur_script::Value::Bool(dark)]);
    }
}
