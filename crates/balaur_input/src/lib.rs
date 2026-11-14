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
use balaur_core::Engine;
use balaur_core::Stage;
use balaur_core::collections::DetHashSet;
use balaur_script::{Module, Value};

pub mod actions;
pub mod gamepad;
pub mod gestures;
pub mod haptics;
mod keys;
mod pad_api;
mod rumble;
pub mod settings;
pub mod touch_controls;
mod vocabulary;
// A pad's motion and touchpad come from reading its HID reports, which a tab
// cannot do; `GamepadState` gates the field the same way.
#[cfg(not(target_family = "wasm"))]
mod sensors;
// gilrs, on a thread of its own; a tab reads no pads.
#[cfg(not(target_family = "wasm"))]
mod pad_thread;

use crate::vocabulary::words;
pub use actions::InputActions;
pub use gamepad::{
    GamepadState, Motion, PAD_AXIS_NAMES, PAD_BUTTON_NAMES, Pad, PadEvent, PadInfo, PadTouch,
    Power, PowerState,
};
pub use gestures::Gestures;
pub use keys::{EITHER_SIDE, FOCUS_KEYS, KEYS};
pub use settings::{InputConfig, PadSettings};
pub use touch_controls::{TouchButton, TouchStick};

const CENTER: &str = "center";
const COMPOSING: &str = "composing";
const DIRECTION: &str = "direction";
const SET_KEY_DOWN: &str = "set_key_down";
const SET_MOUSE_POSITION: &str = "set_mouse_position";
const SET_MOUSE_DOWN: &str = "set_mouse_down";
const SET_SCROLL_DELTA: &str = "set_scroll_delta";
const SET_TOUCH: &str = "set_touch";
const KEY_DOWN: &str = "key_down";
const KEY_JUST_PRESSED: &str = "key_just_pressed";
const KEY_JUST_RELEASED: &str = "key_just_released";
const KEYBOARD_HEIGHT: &str = "keyboard_height";
const LONG_PRESS: &str = "long_press";
const MOUSE_DELTA: &str = "mouse_delta";
const MOUSE_DOWN: &str = "mouse_down";
const MOUSE_JUST_PRESSED: &str = "mouse_just_pressed";
const MOUSE_JUST_RELEASED: &str = "mouse_just_released";
const MOUSE_POSITION: &str = "mouse_position";
const PAN: &str = "pan";
const PINCH: &str = "pinch";
const SCROLL_DELTA: &str = "scroll_delta";
const SWIPE: &str = "swipe";
const TOUCHES: &str = "touches";
const TOUCHES_JUST_ENDED: &str = "touches_just_ended";
const TOUCHES_JUST_STARTED: &str = "touches_just_started";
const TYPED: &str = "typed";

const MOUSE_BUTTONS: usize = 8;

/// The finger a mouse stands in for when `emulate_touch_from_mouse` is on.
/// Reserved, the way Godot reserves a device id for the same trick, so a real
/// finger can never collide with the emulated one.
pub const EMULATED_TOUCH_ID: u64 = u64::MAX;

/// Which conversions between pointer and finger are on. Not serialized: the
/// snapshot records what the conversion produced, so a replay restores both
/// sets of fields and converts nothing.
#[derive(Clone, Copy)]
struct Emulation {
    mouse_from_touch: bool,
    touch_from_mouse: bool,
    /// The finger the mouse is following, so the second finger down does not
    /// fight the first over one cursor.
    primary: Option<u64>,
}

impl Default for Emulation {
    fn default() -> Self {
        Self {
            mouse_from_touch: true,
            touch_from_mouse: false,
            primary: None,
        }
    }
}

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
/// is `input.set_*`, the automation seam: it calls the same feeders, so a
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
    /// Uncommitted input-method text, kept until the editor commits or lets go.
    #[serde(default)]
    composing: String,
    /// Whether a text field held the keyboard this frame, which keeps key
    /// bindings from firing on what is typed. Recorded, so a replay derives
    /// the same actions.
    #[serde(default)]
    keyboard_taken: bool,
    /// Whether ⌘ rather than Ctrl is the command key; `None` for what the
    /// build's platform says. A browser can only tell from its user agent,
    /// so its backend says. A property of the machine, not of a frame.
    #[serde(skip)]
    command_is_meta: Option<bool>,
    /// Pointer and finger conversion. Skipped rather than defaulted: it is a
    /// project setting, not a frame of input, and a recording made under one
    /// setting replays under the setting it was recorded with because the
    /// fields it produced are what got recorded.
    #[serde(skip)]
    emulate: Emulation,
    /// What `input.set_*` handed in this frame, delivered as the next
    /// frame's events: a script feeds during the tick, and the systems that
    /// dispatch a press run at the top of one.
    #[serde(skip)]
    fed: Vec<Fed>,
    /// A script has read one of [`FOCUS_KEYS`], which makes those keys the
    /// game's rather than the widgets'.
    #[serde(skip)]
    focus_key_read: bool,
    /// Whether a backend began this frame, so the tick does not begin it
    /// again and clear the events the backend fed.
    #[serde(skip)]
    begun: bool,
    /// Where the mouse is as far as the window knows. Defaulted, so a session
    /// recorded before this field existed still restores.
    #[serde(default)]
    mouse_place: MousePlace,
}

/// Where the mouse is as far as the window knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum MousePlace {
    /// No position yet: the first one places it and moves nothing, since it
    /// came from nowhere rather than from `(0, 0)`.
    #[default]
    Unplaced,
    /// A position adds its move from the last one to the delta.
    Placed,
    /// Left the window: nothing in it is under the mouse, and where it comes
    /// back in is a new place rather than a move from the edge it left by.
    Outside,
    /// A grab holds the cursor: a position moves nothing, and the delta is the
    /// raw motion `add_mouse_motion` brings, which keeps counting at the edge.
    Held,
}

/// One event a script fed, held until the next frame begins.
enum Fed {
    Key(String, bool),
    Mouse(f32, f32),
    Scroll(f32, f32),
    Button(usize, bool),
    Touch(u64, f32, f32, TouchPhase),
}

impl InputSnapshot {
    /// Reset per-frame edges, then deliver what scripts fed last frame.
    /// Backends call this before feeding the frame's events.
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
        self.begun = true;
        for fed in std::mem::take(&mut self.fed) {
            match fed {
                Fed::Key(key, down) => self.key_event(&key, down),
                Fed::Mouse(x, y) => self.set_mouse_pos(x, y),
                Fed::Scroll(x, y) => self.add_scroll(x, y),
                Fed::Button(button, down) => self.mouse_button_event(button, down),
                Fed::Touch(id, x, y, phase) => self.touch_event(id, x, y, phase),
            }
        }
    }

    /// Begin the frame unless a backend already did: a headless run has no
    /// window to pump, and its fed events still want a frame to land in.
    pub fn begin_frame_unless_begun(&mut self) {
        if !std::mem::take(&mut self.begun) {
            self.begin_frame();
            self.begun = false;
        }
    }

    /// Hold a script-fed event for the next frame.
    fn feed(&mut self, fed: Fed) {
        self.fed.push(fed);
    }

    /// One finger's report from the backend. `Start` and `Move` update the
    /// active set; `End` and `Cancel` remove from it: a cancelled touch ends
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

    /// Which conversions are on, from the project's `[input]` table.
    pub fn set_emulation(&mut self, mouse_from_touch: bool, touch_from_mouse: bool) {
        self.emulate.mouse_from_touch = mouse_from_touch;
        self.emulate.touch_from_mouse = touch_from_mouse;
    }

    /// Fill in whichever of the pointer and the finger the frame's events did
    /// not report, so every reader below sees both.
    ///
    /// Once per frame at the top of the tick rather than per event, for two
    /// reasons: the project's settings are read when the project loads, which
    /// is after the first events could arrive, and a conversion applied per
    /// event would run again for every event in a frame. The edges it writes
    /// last until the next `begin_frame`, exactly like an OS event's.
    ///
    /// A replay must not call this: the recording already holds what the
    /// conversion produced.
    pub fn convert_pointer_and_touch(&mut self) {
        if self.emulate.mouse_from_touch {
            self.mouse_from_touch();
        }
        if self.emulate.touch_from_mouse {
            self.touch_from_mouse();
        }
    }

    /// A finger is also a left click, which is what makes every widget kind
    /// written against a pointer work on a phone. Godot's default, and for
    /// Godot's reason.
    fn mouse_from_touch(&mut self) {
        // One cursor, so one finger drives it: the first one down keeps it
        // until it lifts, and a second finger is a touch and nothing else.
        if let Some(id) = self.emulate.primary
            && self.touches_ended.contains(&id)
        {
            self.emulate.primary = None;
            self.mouse_button_event(0, false);
        }
        if self.emulate.primary.is_none()
            && let Some(id) = self.touches_started.first().copied()
            && let Some((_, x, y)) = self.touches.iter().find(|(t, _, _)| *t == id).copied()
        {
            self.emulate.primary = Some(id);
            // Placed before the press, so a tap does not also report a drag
            // from wherever the cursor was last left.
            self.mouse_pos = (x, y);
            self.mouse_button_event(0, true);
        }
        if let Some(id) = self.emulate.primary
            && let Some((_, x, y)) = self.touches.iter().find(|(t, _, _)| *t == id).copied()
        {
            self.set_mouse_pos(x, y);
        }
    }

    /// The mouse is also a finger, so touch code runs on a desktop. A hover
    /// is not a touch: only a held button reports one.
    fn touch_from_mouse(&mut self) {
        // With both conversions on, the button may be a finger's already.
        // Converting it back would report that finger twice, once real and
        // once emulated, which is the loop the two settings invite.
        if self.emulate.primary.is_some() {
            return;
        }
        let (x, y) = self.mouse_pos;
        if self.mouse_just_pressed[0] {
            self.touch_event(EMULATED_TOUCH_ID, x, y, TouchPhase::Start);
        } else if self.mouse_just_released[0] {
            self.touch_event(EMULATED_TOUCH_ID, x, y, TouchPhase::End);
        } else if self.mouse_down[0] {
            self.touch_event(EMULATED_TOUCH_ID, x, y, TouchPhase::Move);
        }
    }

    pub fn file_drop_event(&mut self, path: String) {
        self.dropped_files.push(path);
    }
}

thread_local! {
    /// Files a page handed the engine since the last frame read them.
    static HANDED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Files a page handed the engine, already written where the engine's `fs`
/// reads them, said as dropped on the next frame: a browser's drop, paste and
/// file picker, whose files have no path on the machine to drop.
pub fn hand_files(paths: impl IntoIterator<Item = String>) {
    HANDED.with(|handed| handed.borrow_mut().extend(paths));
}

/// The files handed since the last call, taken.
#[must_use]
pub fn take_handed() -> Vec<String> {
    HANDED.with(|handed| std::mem::take(&mut *handed.borrow_mut()))
}

impl InputSnapshot {
    /// One character the window backend received. Control characters are the
    /// backend's to filter: what reaches here is what was typed.
    pub fn char_event(&mut self, c: char) {
        self.typed.push(c);
    }

    pub fn set_composing(&mut self, text: &str) {
        self.composing.clear();
        self.composing.push_str(text);
    }

    pub fn composing(&self) -> &str {
        &self.composing
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// Active touches as `(id, x, y)`, oldest finger first.
    pub fn touches(&self) -> &[(u64, f32, f32)] {
        &self.touches
    }

    /// Touches that began this frame.
    pub fn touches_just_started(&self) -> &[u64] {
        &self.touches_started
    }

    /// Touches that ended (or were cancelled) this frame.
    pub fn touches_just_ended(&self) -> &[u64] {
        &self.touches_ended
    }

    /// Files dropped onto the window this frame, in drop order.
    pub fn dropped_files(&self) -> &[String] {
        &self.dropped_files
    }

    /// Whether a text field holds the keyboard, as the window's UI reports it
    /// each frame.
    pub fn set_keyboard_taken(&mut self, taken: bool) {
        self.keyboard_taken = taken;
    }

    pub const fn keyboard_taken(&self) -> bool {
        self.keyboard_taken
    }

    /// Whether a script has read one of [`FOCUS_KEYS`] at any point in this run.
    pub const fn focus_key_read(&self) -> bool {
        self.focus_key_read
    }

    /// A script heard one of [`FOCUS_KEYS`] through a hook.
    pub const fn note_focus_key_read(&mut self) {
        self.focus_key_read = true;
    }

    /// Say which key `cmd` is, where the build's platform cannot know it.
    pub fn set_command_is_meta(&mut self, meta: bool) {
        self.command_is_meta = Some(meta);
    }

    /// Whether `cmd` in a binding is ⌘ (Apple platforms) rather than Ctrl.
    pub fn command_is_meta(&self) -> bool {
        self.command_is_meta
            .unwrap_or(cfg!(target_vendor = "apple"))
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
        match self.mouse_place {
            MousePlace::Placed => {
                self.mouse_delta.0 += x - self.mouse_pos.0;
                self.mouse_delta.1 += y - self.mouse_pos.1;
            }
            MousePlace::Held => {}
            MousePlace::Unplaced | MousePlace::Outside => self.mouse_place = MousePlace::Placed,
        }
        self.mouse_pos = (x, y);
    }

    /// Whether a grab holds the cursor this frame. Released, the next position
    /// places the mouse afresh rather than counting the jump to it.
    pub fn hold_mouse(&mut self, held: bool) {
        if held {
            self.mouse_place = MousePlace::Held;
        } else if self.mouse_place == MousePlace::Held {
            self.mouse_place = MousePlace::Unplaced;
        }
    }

    /// The mouse left the window.
    pub fn mouse_exited(&mut self) {
        self.mouse_place = MousePlace::Outside;
    }

    /// Whether the mouse is over the window: false once it leaves, until it
    /// next moves inside.
    #[must_use]
    pub fn mouse_inside(&self) -> bool {
        self.mouse_place != MousePlace::Outside
    }

    /// How far a held mouse moved: its delta while a grab holds the cursor.
    pub fn add_mouse_motion(&mut self, dx: f32, dy: f32) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    pub fn add_scroll(&mut self, dx: f32, dy: f32) {
        self.scroll.0 += dx;
        self.scroll.1 += dy;
    }

    /// True for the one frame a key went down. `Shift`, `Control`, `Alt` and
    /// `Meta` answer for either side.
    pub fn key_just_pressed(&self, key: &str) -> bool {
        keys::any_code(key, |code| self.just_pressed.contains(code))
    }

    /// True for the one frame a key came up.
    pub fn key_just_released(&self, key: &str) -> bool {
        keys::any_code(key, |code| self.just_released.contains(code))
    }

    pub fn mouse_down(&self, button: usize) -> bool {
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

    /// Whether any mouse button went down this frame.
    pub fn any_mouse_just_pressed(&self) -> bool {
        self.mouse_just_pressed.contains(&true)
    }

    /// Whether any mouse button is held.
    pub fn any_mouse_down(&self) -> bool {
        self.mouse_down.contains(&true)
    }

    /// Whether any mouse button came up this frame.
    pub fn any_mouse_just_released(&self) -> bool {
        self.mouse_just_released.contains(&true)
    }

    /// Movement since the last frame, not an absolute position.
    pub const fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    /// Wheel movement since the last frame.
    pub const fn scroll_delta(&self) -> (f32, f32) {
        self.scroll
    }

    /// Whether a key is held, however long it has been.
    pub fn key_down(&self, key: &str) -> bool {
        keys::any_code(key, |code| self.down.contains(code))
    }

    pub const fn mouse_pos(&self) -> (f32, f32) {
        self.mouse_pos
    }
}

/// The pads there last frame, so a pad arriving or leaving is told once.
#[derive(Default)]
struct SeenPads(Vec<i64>);

/// Tell every script which pads were plugged in or went away since the last
/// frame, from the snapshot a replay restores as readily as a poll fills.
fn announce_pads_system(eng: &Engine, _: f32) {
    let now: Vec<i64> = eng
        .resource::<GamepadState>()
        .borrow()
        .pads()
        .iter()
        .map(|pad| pad.id)
        .collect();
    let seen = eng.resource::<SeenPads>();
    let was = std::mem::replace(&mut seen.borrow_mut().0, now.clone());
    for id in now.iter().filter(|id| !was.contains(id)) {
        let pad = balaur_script::Value::Int(*id);
        balaur_core::facts::notice(eng, balaur_core::hooks::ON_GAMEPAD_CONNECTED, pad);
    }
    for id in was.iter().filter(|id| !now.contains(id)) {
        let pad = balaur_script::Value::Int(*id);
        balaur_core::facts::notice(eng, balaur_core::hooks::ON_GAMEPAD_DISCONNECTED, pad);
    }
}

#[derive(Default)]
pub struct InputPlugin;

impl balaur_plugin::Plugin for InputPlugin {
    fn name(&self) -> &'static str {
        words::general::INPUT
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(InputSnapshot::default());
        reg.insert_resource(GamepadState::default());
        reg.insert_resource(InputActions::default());
        reg.insert_resource(InputConfig::default());
        settings::declare_settings(reg.engine());
        reg.insert_resource(Gestures::default());
        touch_controls::register_touch_button(reg);
        touch_controls::register_touch_stick(reg);
        reg.add_replay_source(
            words::bindings::GAMEPAD,
            |eng| gamepad::capture(&eng.resource::<GamepadState>().borrow()),
            |eng, value| gamepad::restore(&mut eng.resource::<GamepadState>().borrow_mut(), value),
        );
        reg.add_replay_resource::<InputSnapshot>(words::general::INPUT);
        // Bindings are loaded, not simulated: the recording carries them so a
        // player who rebinds a key does not change what a replay reproduces.
        actions::add_replay_setup(reg);

        // A windowed backend begins the frame when it pumps the window; a
        // headless run begins it here, so fed events land and edges clear.
        reg.add_system(Stage::First, |eng, _| {
            eng.resource::<InputSnapshot>()
                .borrow_mut()
                .begin_frame_unless_begun();
        });
        // Controllers are not window events, so they are polled inside the
        // tick rather than by the windowed backend: a headless run with a pad
        // plugged in sees it too. First, so scripts read this frame's state.
        reg.add_system(Stage::First, |eng, dt| {
            // Not while replaying, and not on a rollback's second run of a
            // tick: the recorded pads were restored moments ago.
            if balaur_core::replay::suppressed(eng) {
                return;
            }
            // The settings shape the readings, and the first poll opens the
            // pads with the project's mappings.
            settings::ensure_loaded(eng);
            let pads = eng.resource::<InputConfig>().borrow().pads.clone();
            eng.resource::<GamepadState>().borrow_mut().poll(dt, &pads);
        });
        reg.add_system(Stage::First, haptics::rumble_system);
        reg.insert_resource(SeenPads::default());
        reg.add_system(Stage::First, announce_pads_system);
        // Emulation, gestures and controls, in that order, after the restore
        // and before the actions they feed.
        reg.add_system(Stage::First, |eng, dt| {
            settings::ensure_loaded(eng);
            // Before every reader below, and never on playback: the recording
            // holds what the conversion produced the first time.
            if !balaur_core::replay::is_playing(eng) {
                eng.resource::<InputSnapshot>()
                    .borrow_mut()
                    .convert_pointer_and_touch();
            }
            // Derived state that walks forward, so re-running one tick would
            // count its time twice. A replay still runs it: the fingers it
            // reads were restored a moment ago.
            if !balaur_core::rollback::is_resimulating(eng) {
                gestures::tick(eng, dt);
            }
            touch_controls::tick(eng);
        });
        reg.add_system(Stage::First, |eng, _| actions::tick(eng));

        let mut m = reg.script_module(words::general::INPUT)?;
        install_input_api(&mut m);
        Ok(())
    }
}

/// Every key code a backend can report, in the order `input.KEY_*` lists
/// them. A headless run has no backend, so a typo in a script is caught here.
pub fn known_keys() -> impl Iterator<Item = &'static str> {
    keys::KEYS.iter().map(|(_, code)| *code)
}

/// True when `key` is a code a backend can report, or a modifier on either
/// side.
pub fn is_known_key(key: &str) -> bool {
    keys::KEYS.iter().any(|(_, code)| *code == key)
        || keys::EITHER_SIDE.iter().any(|(_, name, _)| *name == key)
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
    if balaur_core::logbuf::first_time("input key", key) {
        tracing::warn!(key, "unknown key name; it will never match");
    }
}

/// Mouse buttons, so scripts say `input.MOUSE_BUTTON_LEFT` rather than `0`.
///
/// The index is the one `InputSnapshot` stores, so a constant and a raw number
/// cannot disagree.
/// The snapshot, noting a read of a key widget focus also moves on.
fn keys_for_script(eng: &Engine, key: &str) -> std::rc::Rc<std::cell::RefCell<InputSnapshot>> {
    let state = eng.resource::<InputSnapshot>();
    if FOCUS_KEYS.contains(&key) {
        state.borrow_mut().note_focus_key_read();
    }
    state
}

pub const MOUSE_BUTTON_CONSTANTS: &[(&str, i64)] = &[
    ("MOUSE_BUTTON_LEFT", 0),
    ("MOUSE_BUTTON_RIGHT", 1),
    ("MOUSE_BUTTON_MIDDLE", 2),
    ("MOUSE_BUTTON_BACK", 3),
    ("MOUSE_BUTTON_FORWARD", 4),
];

/// `input.*`. Declared against the neutral seam.
fn install_input_api(m: &mut Module<Engine>) {
    m.module_doc(
        "One frame of input: keyboard, mouse, touch screen and gamepads, plus what went down or came up this frame. A headless run answers every query neutrally.",
    );
    m.describe(&[
        (KEY_DOWN, &[], "", "Whether the `KEY_*` key is held down right now, however many frames it has been down. `KEY_SHIFT`, `KEY_CONTROL`, `KEY_ALT` and `KEY_META` answer for either side."),
        (KEY_JUST_PRESSED, &[], "", "Whether the `KEY_*` key went down this frame; true for that one frame only."),
        (KEY_JUST_RELEASED, &[], "", "Whether the `KEY_*` key came up this frame; true for that one frame only."),
        (MOUSE_POSITION, &[], "", "The cursor's position in window pixels, with (0, 0) at the top-left corner. A script hears it move with `events::listen(this.node, \"pointer_moved\")`, as `on_pointer_moved([x, y])`, and a button go down or up anywhere, the UI included, as `pointer_pressed` and `pointer_released` with `{ button, x, y }`; the wheel turn as `pointer_scrolled` with `[x, y]`, and two fingers as `pointer_gestured` with `{ scale, x, y, pan_x, pan_y }`."),
        (MOUSE_DELTA, &[], "", "How far the cursor moved this frame, in pixels; movement, not a position."),
        (SCROLL_DELTA, &[], "", "How far the wheel turned this frame, as an (x, y) pair; zero when it did not turn."),
        (MOUSE_DOWN, &[], "", "Whether the `MOUSE_BUTTON_*` button is held down right now, however many frames it has been down."),
        (MOUSE_JUST_PRESSED, &[], "", "Whether the `MOUSE_BUTTON_*` button went down this frame; true for that one frame only."),
        (MOUSE_JUST_RELEASED, &[], "", "Whether the `MOUSE_BUTTON_*` button came up this frame; true for that one frame only."),
    ]);
    for (name, index) in MOUSE_BUTTON_CONSTANTS {
        m.constant(name, balaur_script::Value::Int(*index));
    }
    // `input::KEY_SPACE` instead of "Space". A misspelled constant is a
    // compile error rather than a key that quietly never fires.
    for (name, code) in keys::KEYS {
        m.constant(name, balaur_script::Value::Str((*code).to_string()));
    }
    for (name, key, _) in keys::EITHER_SIDE {
        m.constant(name, balaur_script::Value::Str((*key).to_string()));
    }
    m.function(KEY_DOWN, |eng: &Engine, key: String| {
        check_key(&key);
        let v = keys_for_script(eng, &key).borrow().key_down(&key);
        Ok(v)
    });
    m.function(KEY_JUST_PRESSED, |eng: &Engine, key: String| {
        check_key(&key);
        let v = keys_for_script(eng, &key).borrow().key_just_pressed(&key);
        Ok(v)
    });
    m.function(KEY_JUST_RELEASED, |eng: &Engine, key: String| {
        check_key(&key);
        let v = keys_for_script(eng, &key).borrow().key_just_released(&key);
        Ok(v)
    });
    m.function(MOUSE_POSITION, |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_pos;
        Ok(v)
    });
    m.function(MOUSE_DELTA, |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_delta;
        Ok(v)
    });
    m.function(SCROLL_DELTA, |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().scroll_delta();
        Ok(v)
    });
    // Buttons are 0-based: left, right, middle, back, forward. Same indexing
    // as the engine uses internally.
    m.function(MOUSE_DOWN, |eng: &Engine, button: usize| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_down(button);
        Ok(v)
    });
    m.function(MOUSE_JUST_PRESSED, |eng: &Engine, button: usize| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_just_pressed(button);
        Ok(v)
    });
    m.function(MOUSE_JUST_RELEASED, |eng: &Engine, button: usize| {
        let state = eng.resource::<InputSnapshot>();
        let v = state.borrow().mouse_just_released(button);
        Ok(v)
    });
    install_feed_api(m);
    install_touch_api(m);
    install_gesture_api(m);
    pad_api::install_gamepad_api(m);
    actions::install_actions(m);
}

/// `input.pinch`, `pan`, `swipe` and `long_press`: what the fingers are
/// doing, past where they are. Each reads neutral with nothing happening, so
/// a desktop and a headless run answer the same as a still screen.
fn install_gesture_api(m: &mut Module<Engine>) {
    m.describe(&[
        (PINCH, &[], "-> Pinch", "Two fingers moving apart or together as `{ scale, center }`: `scale` above 1 is apart, measured against last frame, and `center` is the point between them. Empty with fewer than two fingers down."),
        (PAN, &[], "() -> Vec2", "Two or more fingers moving together: the average movement since last frame, in the same pixels as `mouse_position`. Zero with fewer than two down."),
        (SWIPE, &[], "() -> Swipe?", "A finger that travelled far enough before it lifted. Reported on the frame it lifted and never again; nil otherwise."),
        (LONG_PRESS, &[], "() -> Vec2?", "Where a finger has been held still past `long_press_seconds`. Reported once per finger, on the frame the hold passes; nil otherwise."),
    ]);
    m.records(&[
        (
            "Pinch",
            "{ scale: float?, center: Vec2? }",
            "Two fingers moving apart or together since last frame, and the point between them; every field is nil with fewer than two fingers down.",
        ),
        (
            "Swipe",
            "{ direction: Vec2, speed: float }",
            "A finger that travelled before it lifted: a unit direction and pixels per second.",
        ),
    ]);
    m.function(PINCH, |eng: &Engine, ()| {
        let Some(pinch) = eng.resource::<Gestures>().borrow().pinch() else {
            return Ok(Value::Map(Vec::new()));
        };
        Ok(Value::Map(vec![
            ("scale".to_string(), Value::Num(f64::from(pinch.scale))),
            (
                CENTER.to_string(),
                Value::Vec2([pinch.center.0, pinch.center.1]),
            ),
        ]))
    });
    m.function(PAN, |eng: &Engine, ()| {
        let (x, y) = eng.resource::<Gestures>().borrow().pan();
        Ok(Value::Vec2([x, y]))
    });
    m.function(SWIPE, |eng: &Engine, ()| {
        let Some(swipe) = eng.resource::<Gestures>().borrow().swipe() else {
            return Ok(Value::Nil);
        };
        Ok(Value::Map(vec![
            (
                DIRECTION.to_string(),
                Value::Vec2([swipe.direction.0, swipe.direction.1]),
            ),
            ("speed".to_string(), Value::Num(f64::from(swipe.speed))),
        ]))
    });
    m.function(LONG_PRESS, |eng: &Engine, ()| {
        let held = eng.resource::<Gestures>().borrow().long_press();
        Ok(held.map_or(Value::Nil, |(x, y)| Value::Vec2([x, y])))
    });
}

/// A `Vec2` argument's two floats, or an error naming what came instead.
fn vec2_of(what: &str, value: &Value) -> anyhow::Result<(f32, f32)> {
    match value {
        Value::Vec2([x, y]) => Ok((*x, *y)),
        other => anyhow::bail!("{what} is a Vec2, not a {}", other.type_name()),
    }
}

/// `input.set_*`: the window backend's feeders, for a script that stands in
/// for a person: a showcase, a test, an automation client. A fed event is
/// the next frame's, delivered when it begins, so the systems that dispatch
/// a press at the top of a tick see it exactly like an OS event.
fn install_feed_api(m: &mut Module<Engine>) {
    m.describe(&[
        (SET_KEY_DOWN, &[], "(key: string, down: bool)", "Press or release a `KEY_*` key as if the window had reported it: next frame's edge, and the state until the opposite call."),
        (SET_MOUSE_POSITION, &[], "(position: Vec2)", "Move the cursor to a window-pixel position as if the window had reported it, from next frame; the delta accumulates for that frame."),
        (SET_SCROLL_DELTA, &[], "(delta: Vec2)", "Turn the wheel as if the window had reported it, from next frame; adds to that frame's `scroll_delta`."),
        (SET_MOUSE_DOWN, &[], "(button: int, down: bool)", "Press or release a `MOUSE_BUTTON_*` button as if the window had reported it, from next frame."),
        (SET_TOUCH, &[], "(id: int, position: Vec2, phase: string)", "Put a finger on the screen as if the window had reported it: `phase` is `start`, `move`, `end` or `cancel`, and the position is in the same pixels as `mouse_position`."),
    ]);
    m.function(SET_KEY_DOWN, |eng: &Engine, (key, down): (String, bool)| {
        check_key(&key);
        eng.resource::<InputSnapshot>()
            .borrow_mut()
            .feed(Fed::Key(key, down));
        Ok(())
    });
    m.function(SET_MOUSE_POSITION, |eng: &Engine, (position,): (Value,)| {
        let (x, y) = vec2_of("a mouse position", &position)?;
        eng.resource::<InputSnapshot>()
            .borrow_mut()
            .feed(Fed::Mouse(x, y));
        Ok(())
    });
    m.function(SET_SCROLL_DELTA, |eng: &Engine, (delta,): (Value,)| {
        let (x, y) = vec2_of("a scroll delta", &delta)?;
        eng.resource::<InputSnapshot>()
            .borrow_mut()
            .feed(Fed::Scroll(x, y));
        Ok(())
    });
    m.function(
        SET_MOUSE_DOWN,
        |eng: &Engine, (button, down): (usize, bool)| {
            eng.resource::<InputSnapshot>()
                .borrow_mut()
                .feed(Fed::Button(button, down));
            Ok(())
        },
    );
    m.function(
        SET_TOUCH,
        |eng: &Engine, (id, position, phase): (i64, Value, String)| {
            let (x, y) = vec2_of("a touch position", &position)?;
            let phase = match phase.as_str() {
                words::phases::START => TouchPhase::Start,
                words::phases::MOVE => TouchPhase::Move,
                words::phases::END => TouchPhase::End,
                words::phases::CANCEL => TouchPhase::Cancel,
                other => {
                    anyhow::bail!("'{other}' is not a touch phase: start, move, end or cancel")
                }
            };
            eng.resource::<InputSnapshot>()
                .borrow_mut()
                .feed(Fed::Touch(id.cast_unsigned(), x, y, phase));
            Ok(())
        },
    );
}

/// `input.touches*`: the per-frame lists the window backend feeds.
fn install_touch_api(m: &mut Module<Engine>) {
    m.describe(&[
        (TOUCHES, &[], "() -> [Touch]", "Every finger on the screen, oldest first, in the same pixels as `mouse_position`."),
        (TOUCHES_JUST_STARTED, &[], "() -> [int]", "The ids of the fingers that touched down this frame."),
        (TOUCHES_JUST_ENDED, &[], "() -> [int]", "The ids of the fingers that lifted or were cancelled this frame."),
        (TYPED, &[], "() -> string", "The characters typed this frame, in order: what a text field appends, where `key_just_pressed` says which key went down."),
        (KEYBOARD_HEIGHT, &[], "() -> float", "How much of the window the on-screen keyboard covers, in pixels from the bottom: what a form moves up by. Zero with no keyboard up, and always zero on a desktop."),
        (COMPOSING, &[], "() -> string", "The text an input method is still composing, for a field to show under its caret; empty once it commits into `typed`, and always empty without an input method."),
    ]);
    m.records(&[(
        "Touch",
        "{ id: int, position: Vec2 }",
        "One finger: its id and where it is.",
    )]);
    m.function(COMPOSING, |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let composing = state.borrow().composing().to_string();
        Ok(Value::Str(composing))
    });
    // Active touches as `{ id, position }` maps, oldest finger first. Pixel
    // coordinates, same space as `mouse_position`.
    m.function(TOUCHES, |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let touches = state
            .borrow()
            .touches()
            .iter()
            .map(|(id, x, y)| {
                Value::Map(vec![
                    ("id".to_string(), Value::Int(id.cast_signed())),
                    (
                        crate::vocabulary::keys::POSITION.to_string(),
                        Value::Vec2([*x, *y]),
                    ),
                ])
            })
            .collect();
        Ok(Value::List(touches))
    });
    m.function(TOUCHES_JUST_STARTED, |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let ids = state
            .borrow()
            .touches_just_started()
            .iter()
            .map(|id| Value::Int(id.cast_signed()))
            .collect();
        Ok(Value::List(ids))
    });
    m.function(TOUCHES_JUST_ENDED, |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let ids = state
            .borrow()
            .touches_just_ended()
            .iter()
            .map(|id| Value::Int(id.cast_signed()))
            .collect();
        Ok(Value::List(ids))
    });
    // What the keyboard produced this frame, which is not what it pressed: a
    // layout decides, and a text field wants the result.
    m.function(TYPED, |eng: &Engine, ()| {
        let state = eng.resource::<InputSnapshot>();
        let typed = state.borrow().typed().to_string();
        Ok(Value::Str(typed))
    });
    // A display fact rather than a frame of input, so it comes off the
    // device facts; the verb keeps its place here because a form asking for
    // it is asking about the keyboard.
    m.function(KEYBOARD_HEIGHT, |eng: &Engine, ()| {
        let height = balaur_core::facts::device(eng).keyboard_height;
        Ok(Value::Num(f64::from(height)))
    });
}

pub(crate) fn warn_unknown_once(what: &'static str, name: &str, known: &[&str]) {
    if known.contains(&name) {
        return;
    }
    if balaur_core::logbuf::first_time(what, name) {
        tracing::warn!(what, name, "unknown name; it will never match");
    }
}

#[cfg(test)]
mod tests {
    use super::{EITHER_SIDE, Fed, InputSnapshot, KEYS, MOUSE_BUTTON_CONSTANTS, is_known_key};

    /// What a page hands over is taken once, in the order it was handed.
    #[test]
    fn handed_files_are_taken_once_in_order() {
        super::hand_files(["/handed/1/a.ttf".to_string()]);
        super::hand_files(["/handed/2/b.png".to_string()]);
        assert_eq!(
            super::take_handed(),
            ["/handed/1/a.ttf".to_string(), "/handed/2/b.png".to_string()]
        );
        assert!(super::take_handed().is_empty());
    }

    #[test]
    fn a_fed_press_is_next_frame_s_edge_and_stays_down_after_it() {
        let mut input = InputSnapshot::default();
        input.feed(Fed::Button(0, true));
        assert!(!input.mouse_just_pressed(0), "not this frame's");
        input.begin_frame();
        assert!(input.mouse_just_pressed(0));
        input.begin_frame();
        assert!(!input.mouse_just_pressed(0), "an edge is one frame's");
        assert!(input.mouse_down(0), "the state holds until the release");
    }

    #[test]
    fn a_frame_a_backend_began_is_not_begun_again_by_the_tick() {
        let mut input = InputSnapshot::default();
        input.begin_frame();
        input.mouse_button_event(0, true);
        input.begin_frame_unless_begun();
        assert!(input.mouse_just_pressed(0), "the window's press survives");
        input.begin_frame_unless_begun();
        assert!(
            !input.mouse_just_pressed(0),
            "a headless tick begins its own"
        );
    }

    #[test]
    fn composed_text_outlives_the_frame_and_commits_into_typed() {
        let mut input = InputSnapshot::default();
        input.set_composing("にほ");
        input.begin_frame();
        assert_eq!(
            input.composing(),
            "にほ",
            "a preedit stays until the editor decides"
        );
        assert_eq!(input.typed(), "");
        input.set_composing("");
        for c in "日本".chars() {
            input.char_event(c);
        }
        assert_eq!(input.composing(), "");
        assert_eq!(input.typed(), "日本");
        input.begin_frame();
        assert_eq!(input.typed(), "", "typed text is one frame's");
    }

    #[test]
    fn constant_names_are_unique_and_well_formed() {
        let mut names = std::collections::BTreeSet::new();
        let mut codes = std::collections::BTreeSet::new();
        let every = KEYS
            .iter()
            .copied()
            .chain(EITHER_SIDE.iter().map(|(name, key, _)| (*name, *key)));
        for (name, code) in every {
            assert!(
                name.strip_prefix("KEY_").is_some_and(|rest| rest
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')),
                "{name} is not KEY_ and SCREAMING_SNAKE_CASE"
            );
            assert!(names.insert(name), "{name} is declared twice");
            assert!(codes.insert(code), "{code} has two names");
        }
        for (name, _) in MOUSE_BUTTON_CONSTANTS {
            assert!(!names.contains(*name), "{name} collides with a key");
        }
    }

    #[test]
    fn a_key_holds_its_w3c_code() {
        let code = |name: &str| KEYS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c);
        assert_eq!(code("KEY_A"), Some("KeyA"));
        assert_eq!(code("KEY_1"), Some("Digit1"));
        assert_eq!(code("KEY_LEFT"), Some("ArrowLeft"));
        assert_eq!(code("KEY_LEFT_SHIFT"), Some("ShiftLeft"));
        assert_eq!(code("KEY_ENTER"), Some("Enter"));
        assert_eq!(code("KEY_NUMPAD_PERIOD"), Some("NumpadDecimal"));
    }

    #[test]
    fn a_modifier_named_without_a_side_answers_for_either() {
        let mut input = InputSnapshot::default();
        input.begin_frame();
        input.key_event("ShiftRight", true);
        assert!(input.key_down("Shift"));
        assert!(input.key_just_pressed("Shift"));
        assert!(!input.key_down("ShiftLeft"), "the side still says which");
        assert!(is_known_key("Shift") && is_known_key("ShiftRight"));
        assert!(!is_known_key("Spcae"));
    }
}
