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
pub struct InputConfig {
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

impl Default for InputConfig {
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
    if eng.resource::<InputConfig>().borrow().loaded {
        return;
    }
    install(eng, read(eng).config());
}

/// Take a project's `[input]` table from a script rather than the manifest:
/// the editor's engine reads the editor's own `project.toml`, so a game
/// played there would otherwise keep the editor's settings.
pub(crate) fn declare(eng: &Engine, table: toml::Value) -> anyhow::Result<()> {
    let table: InputTable = table.try_into()?;
    install(eng, table.config());
    Ok(())
}

/// Make `config` the one in force, and hand its emulation to the snapshot.
fn install(eng: &Engine, mut config: InputConfig) {
    config.loaded = true;
    eng.resource::<crate::InputSnapshot>()
        .borrow_mut()
        .set_emulation(config.emulate_mouse_from_touch, config.emulate_touch_from_mouse);
    *eng.resource::<InputConfig>().borrow_mut() = config;
}

/// What `[input]` may say. Every key optional: one left out keeps its default.
#[derive(serde::Deserialize, Default)]
struct InputTable {
    emulate_mouse_from_touch: Option<bool>,
    emulate_touch_from_mouse: Option<bool>,
    swipe_pixels: Option<f32>,
    long_press_seconds: Option<f32>,
    long_press_slop: Option<f32>,
}

impl InputTable {
    fn config(self) -> InputConfig {
        let out = InputConfig::default();
        InputConfig {
            emulate_mouse_from_touch: self
                .emulate_mouse_from_touch
                .unwrap_or(out.emulate_mouse_from_touch),
            emulate_touch_from_mouse: self
                .emulate_touch_from_mouse
                .unwrap_or(out.emulate_touch_from_mouse),
            swipe_pixels: self.swipe_pixels.unwrap_or(out.swipe_pixels).max(0.0),
            long_press_seconds: self
                .long_press_seconds
                .unwrap_or(out.long_press_seconds)
                .max(0.0),
            long_press_slop: self.long_press_slop.unwrap_or(out.long_press_slop).max(0.0),
            loaded: false,
        }
    }
}

/// The manifest's `[input]` table, or an empty one when it is absent or
/// malformed.
fn read(eng: &Engine) -> InputTable {
    #[derive(serde::Deserialize)]
    struct Manifest {
        #[serde(default)]
        input: InputTable,
    }
    let Some(source) = balaur_core::project::manifest_source(eng) else {
        return InputTable::default();
    };
    match toml::from_str::<Manifest>(&source) {
        Ok(manifest) => manifest.input,
        Err(err) => {
            tracing::warn!("project.toml [input]: {err}; touch settings are the defaults");
            InputTable::default()
        }
    }
}
