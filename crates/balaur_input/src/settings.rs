//! The `[input]` table of `project.toml`: what a project says about touch.
//!
//! Read once when the manifest loads, the way `[input.actions]` is, and kept
//! in a resource rather than re-parsed per frame. Every value has a default
//! that suits a game with no opinion, so a project without the table behaves
//! the way most games want.

use balaur_core::Engine;

/// Below this an emulated or recognised gesture is noise rather than intent.
/// Design pixels, and generous: a finger resting on glass wanders.
const SWIPE_PIXELS: f32 = 48.0;
const LONG_PRESS_SECONDS: f32 = 0.5;
const LONG_PRESS_SLOP: f32 = 24.0;

/// What a project asked for, or what it gets by not asking.
pub struct InputSettings {
    /// A finger also moves the mouse, so every widget kind written against a
    /// pointer works on a phone. Godot's default, and for Godot's reason.
    pub emulate_mouse_from_touch: bool,
    /// The mouse also reports a finger, so touch code runs on a desktop.
    /// Off by default: a game that reads both would see one press twice.
    pub emulate_touch_from_mouse: bool,
    /// How far a finger travels before a lift counts as a swipe, in design
    /// pixels.
    pub swipe_pixels: f32,
    /// How long a finger holds before it counts as a long press.
    pub long_press_seconds: f32,
    /// How far a held finger may wander and still be holding, in design
    /// pixels.
    pub long_press_slop: f32,
    loaded: bool,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            emulate_mouse_from_touch: true,
            emulate_touch_from_mouse: false,
            swipe_pixels: SWIPE_PIXELS,
            long_press_seconds: LONG_PRESS_SECONDS,
            long_press_slop: LONG_PRESS_SLOP,
            loaded: false,
        }
    }
}

/// Fill the resource from the manifest the first time this runs.
///
/// Lazy for the reason the action table is: the manifest is read when the
/// project loads, which is after every plugin has been built.
pub(crate) fn ensure_loaded(eng: &Engine) {
    let settings = eng.resource::<InputSettings>();
    if settings.borrow().loaded {
        return;
    }
    let parsed = read(eng);
    let mut settings = settings.borrow_mut();
    *settings = parsed;
    settings.loaded = true;
    let snapshot = eng.resource::<crate::InputSnapshot>();
    snapshot.borrow_mut().set_emulation(
        settings.emulate_mouse_from_touch,
        settings.emulate_touch_from_mouse,
    );
}

/// The `[input]` table, or the defaults when it is absent or malformed.
fn read(eng: &Engine) -> InputSettings {
    #[derive(serde::Deserialize, Default)]
    struct InputTable {
        emulate_mouse_from_touch: Option<bool>,
        emulate_touch_from_mouse: Option<bool>,
        swipe_pixels: Option<f32>,
        long_press_seconds: Option<f32>,
        long_press_slop: Option<f32>,
    }
    #[derive(serde::Deserialize)]
    struct Manifest {
        #[serde(default)]
        input: InputTable,
    }
    let out = InputSettings::default();
    let Some(source) = balaur_core::project::manifest_source(eng) else {
        return out;
    };
    let table = match toml::from_str::<Manifest>(&source) {
        Ok(manifest) => manifest.input,
        Err(err) => {
            tracing::warn!("project.toml [input]: {err}; touch settings are the defaults");
            return out;
        }
    };
    InputSettings {
        emulate_mouse_from_touch: table
            .emulate_mouse_from_touch
            .unwrap_or(out.emulate_mouse_from_touch),
        emulate_touch_from_mouse: table
            .emulate_touch_from_mouse
            .unwrap_or(out.emulate_touch_from_mouse),
        swipe_pixels: table.swipe_pixels.unwrap_or(out.swipe_pixels).max(0.0),
        long_press_seconds: table
            .long_press_seconds
            .unwrap_or(out.long_press_seconds)
            .max(0.0),
        long_press_slop: table.long_press_slop.unwrap_or(out.long_press_slop).max(0.0),
        loaded: false,
    }
}
