//! Input as a Balaur plugin.
//!
//! Backend-agnostic: window backends (and, later, replay files) feed events
//! into [`InputSnapshot`] each frame; scripts read it through the `input`
//! module. In headless runs nothing feeds it, and every query returns the
//! neutral answer, so simulation code does not need to care.
//!
//! Determinism note: input is part of the simulation's inputs. Recording the
//! per-frame `InputSnapshot` gives byte-exact replays; the snapshot itself
//! uses ordered collections so any future iteration/serialization is stable.

use anyhow::Result;
use balaur_core::collections::DetHashSet;
use balaur_core::Engine;
use balaur_core::{App, Plugin};
use balaur_script::{Bindings, BindingsExt};

const MOUSE_BUTTONS: usize = 8;

/// One frame of input, republished by whichever backend owns the OS events.
///
/// The backend calls [`InputSnapshot::begin_frame`] and then the `*_event`
/// feeders once per frame; everything else reads. Scripts must never write it
/// — there is no binding that does, and a written edge would be erased by the
/// next `begin_frame` anyway.
///
/// **Under a headless backend the entry exists but nothing ever feeds it**, so
/// it keeps its `Default` forever: no key is down, no edge ever fires, the
/// mouse sits at `(0, 0)` with zero delta and zero scroll. Every `input.*`
/// query therefore returns the neutral answer rather than failing, which is
/// what lets the same simulation code run in CI and in a window.
#[derive(Default)]
pub struct InputSnapshot {
    down: DetHashSet<String>,
    just_pressed: DetHashSet<String>,
    just_released: DetHashSet<String>,
    mouse_down: [bool; MOUSE_BUTTONS],
    mouse_just_pressed: [bool; MOUSE_BUTTONS],
    mouse_just_released: [bool; MOUSE_BUTTONS],
    mouse_pos: (f32, f32),
    mouse_delta: (f32, f32),
    scroll: (f32, f32),
}

impl InputSnapshot {
    /// Reset per-frame edges. Backends call this before feeding the frame's
    /// events.
    pub fn begin_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
        self.mouse_just_pressed = [false; MOUSE_BUTTONS];
        self.mouse_just_released = [false; MOUSE_BUTTONS];
        self.mouse_delta = (0.0, 0.0);
        self.scroll = (0.0, 0.0);
    }

    pub fn key_event(&mut self, key: &str, pressed: bool) {
        if pressed {
            if self.down.insert(key.to_string()) {
                self.just_pressed.insert(key.to_string());
            }
        } else if self.down.shift_remove(key) {
            self.just_released.insert(key.to_string());
        }
    }

    pub const fn mouse_button_event(&mut self, button: usize, pressed: bool) {
        if button >= MOUSE_BUTTONS {
            return;
        }
        if pressed && !self.mouse_down[button] {
            self.mouse_just_pressed[button] = true;
        }
        if !pressed && self.mouse_down[button] {
            self.mouse_just_released[button] = true;
        }
        self.mouse_down[button] = pressed;
    }

    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        self.mouse_delta.0 += x - self.mouse_pos.0;
        self.mouse_delta.1 += y - self.mouse_pos.1;
        self.mouse_pos = (x, y);
    }

    pub fn add_scroll(&mut self, dx: f32, dy: f32) {
        self.scroll.0 += dx;
        self.scroll.1 += dy;
    }

    /// True for the one frame a key went down.
    pub fn just_pressed(&self, key: &str) -> bool {
        self.just_pressed.contains(key)
    }

    /// True for the one frame a key came up.
    pub fn just_released(&self, key: &str) -> bool {
        self.just_released.contains(key)
    }

    pub fn is_mouse_down(&self, button: usize) -> bool {
        self.mouse_down.get(button).copied().unwrap_or(false)
    }

    pub fn mouse_just_pressed(&self, button: usize) -> bool {
        self.mouse_just_pressed
            .get(button)
            .copied()
            .unwrap_or(false)
    }

    pub fn mouse_just_released(&self, button: usize) -> bool {
        self.mouse_just_released
            .get(button)
            .copied()
            .unwrap_or(false)
    }

    /// Movement since the last frame, not an absolute position.
    pub const fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    pub fn is_down(&self, key: &str) -> bool {
        self.down.contains(key)
    }

    pub const fn mouse_pos(&self) -> (f32, f32) {
        self.mouse_pos
    }
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn name(&self) -> &'static str {
        "input"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        app.engine.insert_resource(InputSnapshot::default());

        let mut m = app.script_module("input")?;
        install_input_api(&mut m);
        Ok(())
    }
}

/// Every key name a script may ask about.
///
/// This is the vocabulary, not a mirror of some backend's enum: whichever
/// windowing backend feeds events has to produce names from this list, and
/// `kiss3d_backend` has a test that says so. Keeping it here means a typo in
/// a script is caught even in a headless run, where no backend is attached.
pub const KEY_NAMES: &[&str] = &[
    "Key1",
    "Key2",
    "Key3",
    "Key4",
    "Key5",
    "Key6",
    "Key7",
    "Key8",
    "Key9",
    "Key0",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "Escape",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "F13",
    "F14",
    "F15",
    "F16",
    "F17",
    "F18",
    "F19",
    "F20",
    "F21",
    "F22",
    "F23",
    "F24",
    "Snapshot",
    "Scroll",
    "Pause",
    "Insert",
    "Home",
    "Delete",
    "End",
    "PageDown",
    "PageUp",
    "Left",
    "Up",
    "Right",
    "Down",
    "Back",
    "Return",
    "Space",
    "Compose",
    "Caret",
    "Numlock",
    "Numpad0",
    "Numpad1",
    "Numpad2",
    "Numpad3",
    "Numpad4",
    "Numpad5",
    "Numpad6",
    "Numpad7",
    "Numpad8",
    "Numpad9",
    "AbntC1",
    "AbntC2",
    "Add",
    "Apostrophe",
    "Apps",
    "At",
    "Ax",
    "Backslash",
    "Calculator",
    "Capital",
    "Colon",
    "Comma",
    "Convert",
    "Decimal",
    "Divide",
    "Equals",
    "Grave",
    "Kana",
    "Kanji",
    "LAlt",
    "LBracket",
    "LControl",
    "LShift",
    "LWin",
    "Mail",
    "MediaSelect",
    "MediaStop",
    "Minus",
    "Multiply",
    "Mute",
    "MyComputer",
    "NavigateForward",
    "NavigateBackward",
    "NextTrack",
    "NoConvert",
    "NumpadComma",
    "NumpadEnter",
    "NumpadEquals",
    "OEM102",
    "Period",
    "PlayPause",
    "Power",
    "PrevTrack",
    "RAlt",
    "RBracket",
    "RControl",
    "RShift",
    "RWin",
    "Semicolon",
    "Slash",
    "Sleep",
    "Stop",
    "Subtract",
    "Sysrq",
    "Tab",
    "Underline",
    "Unlabeled",
    "VolumeDown",
    "VolumeUp",
    "Wake",
    "WebBack",
    "WebFavorites",
    "WebForward",
    "WebHome",
    "WebRefresh",
    "WebSearch",
    "WebStop",
    "Yen",
    "Copy",
    "Paste",
    "Cut",
    "Unknown",
];

/// The constant a key name is exposed as: `Space` becomes `KEY_SPACE`,
/// `PageDown` becomes `KEY_PAGE_DOWN`, `Key1` becomes `KEY_1`.
///
/// Derived rather than listed, so the names and the constants cannot drift.
fn const_name(key: &str) -> String {
    let core = match key.strip_prefix("Key") {
        Some(digit) if digit.len() == 1 && digit.starts_with(|c: char| c.is_ascii_digit()) => digit,
        _ => key,
    };
    let mut out = String::from("KEY_");
    let mut prev = '_';
    for c in core.chars() {
        if c.is_ascii_uppercase() && (prev.is_ascii_lowercase() || prev.is_ascii_digit()) {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
        prev = c;
    }
    out
}

/// True when `key` is a name this engine can ever report.
pub fn is_known_key(key: &str) -> bool {
    KEY_NAMES.contains(&key)
}

/// Warn once per unrecognised name.
///
/// A query is per frame, so warning every time would bury the log; and a
/// misspelled key is not fatal, it just never fires, which is exactly the
/// failure that is hard to spot without being told.
fn check_key(key: &str) {
    if is_known_key(key) {
        return;
    }
    thread_local! {
        static WARNED: std::cell::RefCell<std::collections::BTreeSet<String>> =
            const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    }
    let fresh = WARNED.with_borrow_mut(|w| w.insert(key.to_string()));
    if fresh {
        tracing::warn!(key, "unknown key name; it will never match");
    }
}

/// Mouse buttons, so scripts say `input.MOUSE_LEFT` rather than `0`.
///
/// The index is the one `InputSnapshot` stores, so a constant and a raw number
/// cannot disagree.
pub const MOUSE_BUTTON_CONSTANTS: &[(&str, i64)] =
    &[("MOUSE_LEFT", 0), ("MOUSE_RIGHT", 1), ("MOUSE_MIDDLE", 2)];

/// `input.*`. Declared against the neutral seam.
fn install_input_api(m: &mut dyn Bindings<Engine>) {
    for (name, index) in MOUSE_BUTTON_CONSTANTS {
        m.constant(name, balaur_script::Value::Int(*index));
    }
    // `input.KEY_SPACE` instead of "Space". In Rune a misspelled constant is a
    // compile error rather than a key that quietly never fires.
    for key in KEY_NAMES {
        m.constant(
            &const_name(key),
            balaur_script::Value::Str((*key).to_string()),
        );
    }
    m.function("is_down", |eng: &Engine, key: String| {
        check_key(&key);
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().is_down(&key);
        Ok(v)
    });
    m.function("just_pressed", |eng: &Engine, key: String| {
        check_key(&key);
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().just_pressed(&key);
        Ok(v)
    });
    m.function("just_released", |eng: &Engine, key: String| {
        check_key(&key);
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().just_released(&key);
        Ok(v)
    });
    m.function("mouse_position", |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_pos;
        Ok(v)
    });
    m.function("mouse_delta", |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_delta;
        Ok(v)
    });
    m.function("scroll_delta", |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().scroll;
        Ok(v)
    });
    // Buttons are 0-based: 0 = left, 1 = right, 2 = middle. Same indexing as
    // the engine uses internally, and the same in every language.
    m.function("is_mouse_down", |eng: &Engine, button: usize| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().is_mouse_down(button);
        Ok(v)
    });
    m.function("mouse_just_pressed", |eng: &Engine, button: usize| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_just_pressed(button);
        Ok(v)
    });
    m.function("mouse_just_released", |eng: &Engine, button: usize| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_just_released(button);
        Ok(v)
    });
}

#[cfg(test)]
mod tests {
    use super::{const_name, is_known_key, KEY_NAMES, MOUSE_BUTTON_CONSTANTS};

    #[test]
    fn constant_names_are_unique_and_well_formed() {
        let mut seen = std::collections::BTreeMap::new();
        for key in KEY_NAMES {
            let name = const_name(key);
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "{key} mangles to {name}, which is not SCREAMING_SNAKE_CASE"
            );
            if let Some(other) = seen.insert(name.clone(), *key) {
                panic!("{key} and {other} both mangle to {name}");
            }
        }
        for (name, _) in MOUSE_BUTTON_CONSTANTS {
            assert!(!seen.contains_key(*name), "{name} collides with a key");
        }
    }

    #[test]
    fn the_mangling_reads_the_way_a_script_author_would_guess() {
        assert_eq!(const_name("Space"), "KEY_SPACE");
        assert_eq!(const_name("PageDown"), "KEY_PAGE_DOWN");
        assert_eq!(const_name("Key1"), "KEY_1");
        assert_eq!(const_name("F12"), "KEY_F12");
        assert_eq!(const_name("Numpad0"), "KEY_NUMPAD0");
        assert_eq!(const_name("AbntC1"), "KEY_ABNT_C1");
    }

    #[test]
    fn every_constant_names_a_key_the_engine_knows() {
        for key in KEY_NAMES {
            assert!(is_known_key(key));
        }
        assert!(!is_known_key("Spcae"));
    }
}
