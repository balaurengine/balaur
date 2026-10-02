//! OS events into the input snapshot: one pump per frame, and the key names
//! the input plugin knows.

use balaur_core::App;
use balaur_input::InputSnapshot;
use kiss3d::event::{Action, ImeEvent, TouchAction, WindowEvent};
use kiss3d::window::Window;

/// What arrived this frame, as the UI's pacing reads it: a pointer that only
/// moved is the one kind of event that may leave the shell as it was.
#[derive(Clone, Copy, Default)]
pub(crate) struct Seen {
    pub(crate) any: bool,
    /// Anything but cursor movement: a key, a button, a wheel, a touch, a drop.
    pub(crate) beyond_motion: bool,
}

/// Feed this frame's OS events into the input resource (if the input plugin
/// is installed). Answers what arrived; without the plugin nothing counts
/// them, so it answers as though everything did.
pub(crate) fn pump_input(app: &App, window: &Window) -> Seen {
    let Some(input) = app.engine.try_resource::<InputSnapshot>() else {
        return Seen {
            any: true,
            beyond_motion: true,
        };
    };
    let mut input = input.borrow_mut();
    input.begin_frame();
    let mut seen = Seen::default();
    let mut closing = false;
    for event in window.events().iter() {
        seen.any = true;
        seen.beyond_motion |= !matches!(event.value, WindowEvent::CursorPos(_, _, _));
        if matches!(
            event.value,
            WindowEvent::Key(_, Action::Press, _)
                | WindowEvent::MouseButton(_, Action::Press, _)
                | WindowEvent::Touch(_, _, _, TouchAction::Start, _)
        ) && app
            .engine
            .try_resource::<balaur_core::UserActivation>()
            .is_none()
        {
            app.engine.insert_resource(balaur_core::UserActivation);
        }
        match event.value {
            WindowEvent::Key(key, action, _) => {
                if let Some(code) = key_code(key) {
                    input.key_event(code, action == Action::Press);
                }
            }
            WindowEvent::MouseButton(button, action, _) => {
                let idx = button as usize;
                match action {
                    Action::Press => input.mouse_button_event(idx, true),
                    Action::Release => input.mouse_button_event(idx, false),
                }
            }
            WindowEvent::Char(c) | WindowEvent::CharModifiers(c, _) => {
                // Control characters are key presses that produced no text.
                if !c.is_control() {
                    input.char_event(c);
                }
            }
            WindowEvent::CursorPos(x, y, _) => input.set_mouse_pos(x as f32, y as f32),
            WindowEvent::Scroll(dx, dy, _) => input.add_scroll(dx as f32, dy as f32),
            WindowEvent::Touch(id, x, y, action, _) => {
                let phase = match action {
                    TouchAction::Start => balaur_input::TouchPhase::Start,
                    TouchAction::Move => balaur_input::TouchPhase::Move,
                    TouchAction::End => balaur_input::TouchPhase::End,
                    TouchAction::Cancel => balaur_input::TouchPhase::Cancel,
                };
                input.touch_event(id, x as f32, y as f32, phase);
            }
            WindowEvent::Focus(focused) => crate::device::set_focused(app, focused),
            WindowEvent::Iconify(hidden) => crate::device::set_suspended(app, hidden),
            WindowEvent::LowMemory => crate::device::warn_low_memory(app),
            WindowEvent::Close => closing = true,
            _ => {}
        }
    }
    // A preedit shows under the caret; a commit is typed like any character.
    for ime in window.ime_events() {
        match ime {
            ImeEvent::Preedit { text, .. } => input.set_composing(&text),
            ImeEvent::Commit(text) => {
                input.set_composing("");
                for c in text.chars().filter(|c| !c.is_control()) {
                    input.char_event(c);
                }
            }
            ImeEvent::Enabled | ImeEvent::Disabled => input.set_composing(""),
        }
    }
    // Dragging a file onto the window needs a desktop with a file manager;
    // kiss3d has no such event on mobile.
    #[cfg(not(mobile))]
    for path in window.dropped_files() {
        seen.any = true;
        seen.beyond_motion = true;
        input.file_drop_event(path.to_string_lossy().into_owned());
    }
    // A chance to save, not a veto, and outside the borrow above: a handler
    // reading input would re-enter that `RefCell`.
    drop(input);
    if closing {
        if let Some(host) = app.engine.script_host() {
            host.announce(balaur_core::hooks::ON_QUIT_REQUESTED, &[]);
        }
        app.engine.request_quit();
    }
    seen
}

/// The W3C `KeyboardEvent.code` for a key kiss3d reports, which is the name
/// `balaur_input` keeps. `None` for a key with no code, which no binding can
/// name.
fn key_code(key: kiss3d::event::Key) -> Option<&'static str> {
    typing_code(key)
        .or_else(|| control_code(key))
        .or_else(|| system_code(key))
}

/// The kiss3d key a `balaur_input` name stands for, for a setting kiss3d
/// reads itself, such as a camera's reset key.
pub(crate) fn key_named(name: &str) -> Option<kiss3d::event::Key> {
    EVERY_KEY
        .iter()
        .copied()
        .find(|key| key_code(*key) == Some(name))
}

/// Every key kiss3d declares, in its own order.
const EVERY_KEY: [kiss3d::event::Key; 162] = {
    use kiss3d::event::Key as K;
    [
        K::Key1,
        K::Key2,
        K::Key3,
        K::Key4,
        K::Key5,
        K::Key6,
        K::Key7,
        K::Key8,
        K::Key9,
        K::Key0,
        K::A,
        K::B,
        K::C,
        K::D,
        K::E,
        K::F,
        K::G,
        K::H,
        K::I,
        K::J,
        K::K,
        K::L,
        K::M,
        K::N,
        K::O,
        K::P,
        K::Q,
        K::R,
        K::S,
        K::T,
        K::U,
        K::V,
        K::W,
        K::X,
        K::Y,
        K::Z,
        K::Escape,
        K::F1,
        K::F2,
        K::F3,
        K::F4,
        K::F5,
        K::F6,
        K::F7,
        K::F8,
        K::F9,
        K::F10,
        K::F11,
        K::F12,
        K::F13,
        K::F14,
        K::F15,
        K::F16,
        K::F17,
        K::F18,
        K::F19,
        K::F20,
        K::F21,
        K::F22,
        K::F23,
        K::F24,
        K::Snapshot,
        K::Scroll,
        K::Pause,
        K::Insert,
        K::Home,
        K::Delete,
        K::End,
        K::PageDown,
        K::PageUp,
        K::Left,
        K::Up,
        K::Right,
        K::Down,
        K::Back,
        K::Return,
        K::Space,
        K::Compose,
        K::Caret,
        K::Numlock,
        K::Numpad0,
        K::Numpad1,
        K::Numpad2,
        K::Numpad3,
        K::Numpad4,
        K::Numpad5,
        K::Numpad6,
        K::Numpad7,
        K::Numpad8,
        K::Numpad9,
        K::AbntC1,
        K::AbntC2,
        K::Add,
        K::Apostrophe,
        K::Apps,
        K::At,
        K::Ax,
        K::Backslash,
        K::Calculator,
        K::Capital,
        K::Colon,
        K::Comma,
        K::Convert,
        K::Decimal,
        K::Divide,
        K::Equals,
        K::Grave,
        K::Kana,
        K::Kanji,
        K::LAlt,
        K::LBracket,
        K::LControl,
        K::LShift,
        K::LWin,
        K::Mail,
        K::MediaSelect,
        K::MediaStop,
        K::Minus,
        K::Multiply,
        K::Mute,
        K::MyComputer,
        K::NavigateForward,
        K::NavigateBackward,
        K::NextTrack,
        K::NoConvert,
        K::NumpadComma,
        K::NumpadEnter,
        K::NumpadEquals,
        K::OEM102,
        K::Period,
        K::PlayPause,
        K::Power,
        K::PrevTrack,
        K::RAlt,
        K::RBracket,
        K::RControl,
        K::RShift,
        K::RWin,
        K::Semicolon,
        K::Slash,
        K::Sleep,
        K::Stop,
        K::Subtract,
        K::Sysrq,
        K::Tab,
        K::Underline,
        K::Unlabeled,
        K::VolumeDown,
        K::VolumeUp,
        K::Wake,
        K::WebBack,
        K::WebFavorites,
        K::WebForward,
        K::WebHome,
        K::WebRefresh,
        K::WebSearch,
        K::WebStop,
        K::Yen,
        K::Copy,
        K::Paste,
        K::Cut,
        K::Unknown,
    ]
};

/// Letters, digits, punctuation and the space bar.
fn typing_code(key: kiss3d::event::Key) -> Option<&'static str> {
    use kiss3d::event::Key;
    Some(match key {
        Key::Key0 => "Digit0",
        Key::Key1 => "Digit1",
        Key::Key2 => "Digit2",
        Key::Key3 => "Digit3",
        Key::Key4 => "Digit4",
        Key::Key5 => "Digit5",
        Key::Key6 => "Digit6",
        Key::Key7 => "Digit7",
        Key::Key8 => "Digit8",
        Key::Key9 => "Digit9",
        Key::A => "KeyA",
        Key::B => "KeyB",
        Key::C => "KeyC",
        Key::D => "KeyD",
        Key::E => "KeyE",
        Key::F => "KeyF",
        Key::G => "KeyG",
        Key::H => "KeyH",
        Key::I => "KeyI",
        Key::J => "KeyJ",
        Key::K => "KeyK",
        Key::L => "KeyL",
        Key::M => "KeyM",
        Key::N => "KeyN",
        Key::O => "KeyO",
        Key::P => "KeyP",
        Key::Q => "KeyQ",
        Key::R => "KeyR",
        Key::S => "KeyS",
        Key::T => "KeyT",
        Key::U => "KeyU",
        Key::V => "KeyV",
        Key::W => "KeyW",
        Key::X => "KeyX",
        Key::Y => "KeyY",
        Key::Z => "KeyZ",
        Key::Space => "Space",
        Key::AbntC1 => "IntlRo",
        Key::Apostrophe => "Quote",
        Key::Backslash => "Backslash",
        Key::Comma => "Comma",
        Key::Equals => "Equal",
        Key::Grave => "Backquote",
        Key::LBracket => "BracketLeft",
        Key::Minus => "Minus",
        Key::OEM102 => "IntlBackslash",
        Key::Period => "Period",
        Key::RBracket => "BracketRight",
        Key::Semicolon => "Semicolon",
        Key::Slash => "Slash",
        Key::Yen => "IntlYen",
        _ => return None,
    })
}

/// Function, navigation and editing keys, and the numeric keypad.
fn control_code(key: kiss3d::event::Key) -> Option<&'static str> {
    use kiss3d::event::Key;
    Some(match key {
        Key::Escape => "Escape",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::F13 => "F13",
        Key::F14 => "F14",
        Key::F15 => "F15",
        Key::F16 => "F16",
        Key::F17 => "F17",
        Key::F18 => "F18",
        Key::F19 => "F19",
        Key::F20 => "F20",
        Key::F21 => "F21",
        Key::F22 => "F22",
        Key::F23 => "F23",
        Key::F24 => "F24",
        Key::Snapshot => "PrintScreen",
        Key::Scroll => "ScrollLock",
        Key::Pause => "Pause",
        Key::Insert => "Insert",
        Key::Home => "Home",
        Key::Delete => "Delete",
        Key::End => "End",
        Key::PageDown => "PageDown",
        Key::PageUp => "PageUp",
        Key::Left => "ArrowLeft",
        Key::Up => "ArrowUp",
        Key::Right => "ArrowRight",
        Key::Down => "ArrowDown",
        Key::Back => "Backspace",
        Key::Return => "Enter",
        Key::Numlock => "NumLock",
        Key::Numpad0 => "Numpad0",
        Key::Numpad1 => "Numpad1",
        Key::Numpad2 => "Numpad2",
        Key::Numpad3 => "Numpad3",
        Key::Numpad4 => "Numpad4",
        Key::Numpad5 => "Numpad5",
        Key::Numpad6 => "Numpad6",
        Key::Numpad7 => "Numpad7",
        Key::Numpad8 => "Numpad8",
        Key::Numpad9 => "Numpad9",
        Key::Add => "NumpadAdd",
        Key::Apps => "ContextMenu",
        Key::Capital => "CapsLock",
        Key::Decimal => "NumpadDecimal",
        Key::Divide => "NumpadDivide",
        Key::Multiply => "NumpadMultiply",
        Key::NumpadComma => "NumpadComma",
        Key::NumpadEnter => "NumpadEnter",
        Key::NumpadEquals => "NumpadEqual",
        Key::Subtract => "NumpadSubtract",
        Key::Tab => "Tab",
        _ => return None,
    })
}

/// Modifiers, media and browser keys, and the IME keys.
fn system_code(key: kiss3d::event::Key) -> Option<&'static str> {
    use kiss3d::event::Key;
    Some(match key {
        Key::Convert => "Convert",
        Key::Kana => "KanaMode",
        Key::LAlt => "AltLeft",
        Key::LControl => "ControlLeft",
        Key::LShift => "ShiftLeft",
        Key::LWin => "MetaLeft",
        Key::Mail => "LaunchMail",
        Key::MediaSelect => "MediaSelect",
        Key::MediaStop => "MediaStop",
        Key::Mute => "AudioVolumeMute",
        Key::NavigateForward => "BrowserForward",
        Key::NavigateBackward => "BrowserBack",
        Key::NextTrack => "MediaTrackNext",
        Key::NoConvert => "NonConvert",
        Key::PlayPause => "MediaPlayPause",
        Key::Power => "Power",
        Key::PrevTrack => "MediaTrackPrevious",
        Key::RAlt => "AltRight",
        Key::RControl => "ControlRight",
        Key::RShift => "ShiftRight",
        Key::RWin => "MetaRight",
        Key::Sleep => "Sleep",
        Key::VolumeDown => "AudioVolumeDown",
        Key::VolumeUp => "AudioVolumeUp",
        Key::Wake => "WakeUp",
        Key::WebHome => "BrowserHome",
        Key::WebRefresh => "BrowserRefresh",
        Key::WebSearch => "BrowserSearch",
        Key::Copy => "Copy",
        Key::Paste => "Paste",
        Key::Cut => "Cut",
        _ => return None,
    })
}

#[cfg(test)]
mod key_code_tests {
    use super::key_code;
    use kiss3d::event::Key;

    #[test]
    fn every_code_the_backend_reports_is_a_key_scripts_can_name() {
        for key in [
            Key::Space,
            Key::Return,
            Key::A,
            Key::Key1,
            Key::LShift,
            Key::RWin,
            Key::Capital,
            Key::Apps,
            Key::Decimal,
            Key::OEM102,
        ] {
            let code = key_code(key).expect("a key games use has a code");
            assert!(
                balaur_input::is_known_key(code),
                "{code} is not in balaur_input's table"
            );
        }
        assert_eq!(key_code(Key::A), Some("KeyA"));
        assert_eq!(key_code(Key::Return), Some("Enter"));
        assert_eq!(key_code(Key::Unknown), None);
    }

    #[test]
    fn every_key_with_a_code_is_found_by_its_name() {
        for key in super::EVERY_KEY {
            if let Some(code) = key_code(key) {
                assert!(
                    balaur_input::is_known_key(code),
                    "{code} is not in balaur_input's table"
                );
                assert_eq!(super::key_named(code), Some(key), "{code}");
            }
        }
        assert_eq!(super::key_named("Enter"), Some(Key::Return));
        assert_eq!(super::key_named("NotAKey"), None);
    }
}
