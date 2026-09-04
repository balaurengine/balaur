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
use balaur_core::{App, Plugin, Stage};
use balaur_script::{Bindings, BindingsExt, Value};

pub mod actions;
pub mod gamepad;
pub mod haptics;

pub use actions::InputActions;
pub use gamepad::{GamepadState, Motion, PadTouch, PAD_AXIS_NAMES, PAD_BUTTON_NAMES};

const MOUSE_BUTTONS: usize = 8;

/// What a finger did this frame, as reported by the window backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TouchPhase {
    Start,
    Move,
    End,
    Cancel,
}

/// One frame of input, republished by whichever backend owns the OS events.
///
/// The backend calls [`InputSnapshot::begin_frame`] and then the `*_event`
/// feeders once per frame; everything else reads. The one script-side writer
/// is `input.feed_*`, the automation seam: it calls the same feeders, so a
/// fed frame records and replays like one the window fed.
///
/// **Under a headless backend the entry exists but nothing ever feeds it**, so
/// it keeps its `Default` forever: no key is down, no edge ever fires, the
/// mouse sits at `(0, 0)` with zero delta and zero scroll. Every `input.*`
/// query therefore returns the neutral answer rather than failing, which is
/// what lets the same simulation code run in CI and in a window.
/// Serialized as-is into a replay recording: the field names are the file
/// format, so renaming one is a format change and wants `replay::FORMAT`.
#[derive(Default, serde::Serialize, serde::Deserialize)]
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
    /// Active touches in the order they began, so iteration is stable and
    /// `touches()[0]` is the oldest finger still down.
    touches: Vec<(u64, f32, f32)>,
    touches_started: Vec<u64>,
    touches_ended: Vec<u64>,
    /// Files dropped onto the window this frame, in drop order.
    dropped_files: Vec<String>,
    /// Characters typed this frame, in order. Keys say which key went down;
    /// this says what it produced, which is what a text field needs and what
    /// a keyboard layout decides.
    ///
    /// Defaulted, so a session recorded before this field existed still
    /// restores: without it the whole snapshot fails to parse and the tick is
    /// fed nothing at all, which is a divergence with no message.
    #[serde(default)]
    typed: String,
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
        self.touches_started.clear();
        self.touches_ended.clear();
        self.dropped_files.clear();
        self.typed.clear();
    }

    /// One finger's report from the backend. `Start` and `Move` update the
    /// active set; `End` and `Cancel` remove from it — a cancelled touch ends
    /// without ever counting as a tap, which is a script-side distinction, so
    /// both land in `touches_ended`.
    pub fn touch_event(&mut self, id: u64, x: f32, y: f32, phase: TouchPhase) {
        match phase {
            TouchPhase::Start => {
                if !self.touches.iter().any(|(t, _, _)| *t == id) {
                    self.touches.push((id, x, y));
                    self.touches_started.push(id);
                }
            }
            TouchPhase::Move => {
                if let Some(touch) = self.touches.iter_mut().find(|(t, _, _)| *t == id) {
                    touch.1 = x;
                    touch.2 = y;
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                self.touches.retain(|(t, _, _)| *t != id);
                self.touches_ended.push(id);
            }
        }
    }

    pub fn file_drop_event(&mut self, path: String) {
        self.dropped_files.push(path);
    }

    /// One character the window backend received. Control characters are the
    /// backend's to filter: what reaches here is what was typed.
    pub fn char_event(&mut self, c: char) {
        self.typed.push(c);
    }

    /// What was typed this frame, in order, or empty.
    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// Active touches as `(id, x, y)`, oldest finger first.
    pub fn touches(&self) -> &[(u64, f32, f32)] {
        &self.touches
    }

    /// Touches that began this frame.
    pub fn touches_started(&self) -> &[u64] {
        &self.touches_started
    }

    /// Touches that ended (or were cancelled) this frame.
    pub fn touches_ended(&self) -> &[u64] {
        &self.touches_ended
    }

    /// Files dropped onto the window this frame, in drop order.
    pub fn dropped_files(&self) -> &[String] {
        &self.dropped_files
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

    /// Wheel movement since the last frame.
    pub const fn scroll_delta(&self) -> (f32, f32) {
        self.scroll
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
        app.engine.insert_resource(GamepadState::default());
        app.engine.insert_resource(InputActions::default());
        app.add_replay_source(
            "gamepad",
            |eng| gamepad::capture(&eng.resource::<GamepadState>().borrow()),
            |eng, value| gamepad::restore(&mut eng.resource::<GamepadState>().borrow_mut(), value),
        );
        app.add_replay_resource::<InputSnapshot>("input");
        // Bindings are loaded, not simulated: the recording carries them so a
        // player who rebinds a key does not change what a replay reproduces.
        actions::add_replay_setup(app);

        // Controllers are not window events, so they are polled inside the
        // tick rather than by the windowed backend: a headless run with a pad
        // plugged in sees it too. First, so scripts read this frame's state.
        app.add_system(Stage::First, |eng, _| {
            // Not while replaying: the recorded pads were restored moments
            // ago and polling the real hardware would overwrite them.
            if balaur_core::replay::is_playing(eng) {
                return;
            }
            eng.resource::<GamepadState>().borrow_mut().poll();
        });
        // After the poll and after a replay restored the recording's snapshot,
        // so an action is derived from exactly the input that was recorded.
        app.add_system(Stage::First, |eng, _| actions::tick(eng));

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
    m.module_doc(
        "One frame of input: the keyboard, mouse, touch screen and gamepads \
         as they stand now, plus the edges — what went down or came up this \
         frame. Nothing feeds it in a headless run, where every query answers \
         neutrally rather than failing.",
    );
    m.describe(&[
        ("is_down", &[], "", "Whether the `KEY_*` key is held down right now, however many frames it has been down."),
        ("just_pressed", &[], "", "Whether the `KEY_*` key went down this frame; true for that one frame only."),
        ("just_released", &[], "", "Whether the `KEY_*` key came up this frame; true for that one frame only."),
        ("mouse_position", &[], "", "The cursor's position in window pixels, with (0, 0) at the top-left corner."),
        ("mouse_delta", &[], "", "How far the cursor moved this frame, in pixels; movement, not a position."),
        ("scroll_delta", &[], "", "How far the wheel turned this frame, as an (x, y) pair; zero when it did not turn."),
        ("is_mouse_down", &[], "", "Whether the `MOUSE_*` button is held down right now, however many frames it has been down."),
        ("mouse_just_pressed", &[], "", "Whether the `MOUSE_*` button went down this frame; true for that one frame only."),
        ("mouse_just_released", &[], "", "Whether the `MOUSE_*` button came up this frame; true for that one frame only."),
    ]);
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
        let v = state.borrow().scroll_delta();
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
    install_feed_api(m);
    install_touch_api(m);
    install_gamepad_api(m);
    actions::install_actions(m);
}

/// `input.feed_*`: the window backend's feeders, for a script that stands in
/// for a person — a showcase, a test, an automation client. Fed edges last
/// until the next frame's `begin_frame`, exactly like an OS event's.
fn install_feed_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("feed_key", &[], "(key: string, down: bool)", "Press or release a `KEY_*` key as if the window had reported it; the edge lasts this frame, the state until the opposite feed."),
        ("feed_mouse", &[], "(x: float, y: float)", "Move the cursor to a window-pixel position as if the window had reported it; the delta accumulates for this frame."),
        ("feed_mouse_button", &[], "(button: int, down: bool)", "Press or release a `MOUSE_*` button as if the window had reported it."),
    ]);
    m.function("feed_key", |eng: &Engine, (key, down): (String, bool)| {
        check_key(&key);
        eng.resource::<InputSnapshot>()
            .borrow_mut()
            .key_event(&key, down);
        Ok(())
    });
    m.function("feed_mouse", |eng: &Engine, (x, y): (f32, f32)| {
        eng.resource::<InputSnapshot>()
            .borrow_mut()
            .set_mouse_pos(x, y);
        Ok(())
    });
    m.function(
        "feed_mouse_button",
        |eng: &Engine, (button, down): (usize, bool)| {
            eng.resource::<InputSnapshot>()
                .borrow_mut()
                .mouse_button_event(button, down);
            Ok(())
        },
    );
}

/// `input.touches*` and `input.dropped_files` — the per-frame lists the
/// window backend feeds.
fn install_touch_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("touches", &[], "", "Every finger on the screen as `{ id, x, y }`, oldest first, in the same pixels as `mouse_position`."),
        ("touches_started", &[], "", "The ids of the fingers that touched down this frame."),
        ("touches_ended", &[], "", "The ids of the fingers that lifted or were cancelled this frame."),
        ("dropped_files", &[], "", "The absolute paths of files dropped onto the window this frame, in drop order; desktop only."),
        ("typed", &[], "", "The characters typed this frame, in order: what a text field appends, where `just_pressed` says which key went down."),
    ]);
    // Active touches as `{ id, x, y }` maps, oldest finger first. Pixel
    // coordinates, same space as `mouse_position`.
    m.function("touches", |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let touches = state
            .borrow()
            .touches()
            .iter()
            .map(|(id, x, y)| {
                Value::Map(vec![
                    ("id".to_string(), Value::Int(id.cast_signed())),
                    ("x".to_string(), Value::Num(f64::from(*x))),
                    ("y".to_string(), Value::Num(f64::from(*y))),
                ])
            })
            .collect();
        Ok(Value::List(touches))
    });
    m.function("touches_started", |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let ids = state
            .borrow()
            .touches_started()
            .iter()
            .map(|id| Value::Int(id.cast_signed()))
            .collect();
        Ok(Value::List(ids))
    });
    m.function("touches_ended", |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let ids = state
            .borrow()
            .touches_ended()
            .iter()
            .map(|id| Value::Int(id.cast_signed()))
            .collect();
        Ok(Value::List(ids))
    });
    // What the keyboard produced this frame, which is not what it pressed: a
    // layout decides, and a text field wants the result.
    m.function("typed", |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let typed = state.borrow().typed().to_string();
        Ok(Value::Str(typed))
    });
    // Files dropped onto the window this frame, absolute paths in drop order.
    // Desktop only: browsers and phones have no window to drop onto.
    m.function("dropped_files", |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let files = state
            .borrow()
            .dropped_files()
            .iter()
            .map(|path| Value::Str(path.clone()))
            .collect();
        Ok(Value::List(files))
    });
}

/// `input.gamepad_*`. Ids come from `input.gamepads()`; a query about a pad
/// that is not connected answers neutrally (false, 0.0, ""), the same
/// convention as a headless keyboard.
fn install_gamepad_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("gamepads", &[], "", "The ids of every connected pad, ordered so the list is stable from frame to frame."),
        ("gamepad_name", &[], "", "The pad's name as the platform reports it, empty when no pad has that id."),
        ("gamepad_down", &[], "", "Whether the pad's `PAD_*` button is held down right now, however many frames it has been down."),
        ("gamepad_just_pressed", &[], "", "Whether the pad's `PAD_*` button went down this frame; true for that one frame only."),
        ("gamepad_just_released", &[], "", "Whether the pad's `PAD_*` button came up this frame; true for that one frame only."),
        ("gamepad_axis", &[], "", "How far the pad's `AXIS_*` stick or trigger is pushed, -1 to 1; zero at rest and for an absent pad."),
    ]);
    for name in PAD_BUTTON_NAMES {
        m.constant(
            &pad_const_name("PAD_", name),
            Value::Str((*name).to_string()),
        );
    }
    for name in PAD_AXIS_NAMES {
        m.constant(
            &pad_const_name("AXIS_", name),
            Value::Str((*name).to_string()),
        );
    }
    m.function("gamepads", |eng: &Engine, ()| {
        let state = eng.resource::<GamepadState>();
        let ids = state
            .borrow()
            .pads()
            .iter()
            .map(|pad| Value::Int(pad.id))
            .collect();
        Ok(Value::List(ids))
    });
    m.function("gamepad_name", |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let name = state
            .borrow()
            .pad(id)
            .map_or_else(String::new, |pad| pad.name.clone());
        Ok(name)
    });
    m.function(
        "gamepad_down",
        |eng: &Engine, (id, button): (i64, String)| {
            check_pad_button(&button);
            let state = eng.resource::<GamepadState>();
            let v = state.borrow().pad(id).is_some_and(|p| p.is_down(&button));
            Ok(v)
        },
    );
    m.function(
        "gamepad_just_pressed",
        |eng: &Engine, (id, button): (i64, String)| {
            check_pad_button(&button);
            let state = eng.resource::<GamepadState>();
            let v = state
                .borrow()
                .pad(id)
                .is_some_and(|p| p.just_pressed(&button));
            Ok(v)
        },
    );
    m.function(
        "gamepad_just_released",
        |eng: &Engine, (id, button): (i64, String)| {
            check_pad_button(&button);
            let state = eng.resource::<GamepadState>();
            let v = state
                .borrow()
                .pad(id)
                .is_some_and(|p| p.just_released(&button));
            Ok(v)
        },
    );
    // -1..1; sticks idle at 0. An absent pad or axis reads 0.
    m.function("gamepad_axis", |eng: &Engine, (id, axis): (i64, String)| {
        check_pad_axis(&axis);
        let state = eng.resource::<GamepadState>();
        let v = state.borrow().pad(id).map_or(0.0, |p| p.axis(&axis));
        Ok(v)
    });
    gamepad::install_motion_api(m);
    gamepad::install_touchpad_api(m);
    haptics::install_haptics_api(m);
}

/// `PAD_SOUTH` from `South`, `AXIS_LEFT_STICK_X` from `LeftStickX` — the same
/// camel-splitting the key constants use, with `DPad` kept as one word so
/// scripts read `PAD_DPAD_UP` rather than `PAD_D_PAD_UP`.
fn pad_const_name(prefix: &str, name: &str) -> String {
    let name = name.replace("DPad", "Dpad");
    let mut out = String::from(prefix);
    let mut prev = '_';
    for c in name.chars() {
        if c.is_ascii_uppercase() && (prev.is_ascii_lowercase() || prev.is_ascii_digit()) {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
        prev = c;
    }
    out
}

/// Warn once per unrecognised pad button, mirroring `check_key`.
fn check_pad_button(button: &str) {
    warn_unknown_once("gamepad button", button, PAD_BUTTON_NAMES);
}

/// Warn once per unrecognised pad axis, mirroring `check_key`.
fn check_pad_axis(axis: &str) {
    warn_unknown_once("gamepad axis", axis, PAD_AXIS_NAMES);
}

fn warn_unknown_once(what: &'static str, name: &str, known: &[&str]) {
    if known.contains(&name) {
        return;
    }
    thread_local! {
        static WARNED: std::cell::RefCell<std::collections::BTreeSet<String>> =
            const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    }
    let fresh = WARNED.with_borrow_mut(|w| w.insert(format!("{what}:{name}")));
    if fresh {
        tracing::warn!(what, name, "unknown name; it will never match");
    }
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
