//! OS events into the input snapshot: one pump per frame, and the key names
//! the input plugin knows.

use balaur_core::App;
use balaur_input::InputSnapshot;
use kiss3d::event::{Action, TouchAction, WindowEvent};
use kiss3d::window::Window;

/// Feed this frame's OS events into the input resource (if the input plugin
/// is installed).
pub(crate) fn pump_input(app: &App, window: &Window) {
    let Some(input) = app.engine.try_resource::<InputSnapshot>() else {
        return;
    };
    let mut input = input.borrow_mut();
    input.begin_frame();
    for event in window.events().iter() {
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
                let name = key_name(key);
                match action {
                    Action::Press => input.key_event(&name, true),
                    Action::Release => input.key_event(&name, false),
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
            // A chance to save, not a veto: every script hears it, then the
            // app goes.
            WindowEvent::Close => {
                if let Some(host) = app.engine.script_host() {
                    host.call_all("on_quit_requested");
                }
                app.engine.request_quit();
            }
            _ => {}
        }
    }
    input.set_keyboard_height(keyboard_height());
    // Dragging a file onto the window needs a desktop with a file manager;
    // kiss3d has no such event on mobile.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    for path in window.dropped_files() {
        input.file_drop_event(path.to_string_lossy().into_owned());
    }
}

/// The name this backend reports for a key.
///
/// kiss3d's `Key` debug-prints as its variant name, which is where
/// `balaur_input::KEY_NAMES` came from. If kiss3d ever renames one, the name
/// stops matching what scripts ask for and the key silently goes dead, so say
/// so the first time it happens.
fn key_name(key: kiss3d::event::Key) -> String {
    let name = format!("{key:?}");
    if !balaur_input::is_known_key(&name) {
        use std::sync::atomic::{AtomicBool, Ordering};
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                key = name,
                "the window backend reported a key balaur_input does not know; \
                 scripts cannot match it"
            );
        }
    }
    name
}

/// What the on-screen keyboard covers: the part of the window the visual
/// viewport no longer reaches. Only a page can say; nothing else reports it.
#[cfg(all(target_family = "wasm", not(target_os = "emscripten")))]
fn keyboard_height() -> f32 {
    let Some(window) = web_sys::window() else {
        return 0.0;
    };
    let inner = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let Some(viewport) = window.visual_viewport() else {
        return 0.0;
    };
    let covered = inner - viewport.height() - viewport.offset_top();
    let ratio = window.device_pixel_ratio();
    (covered.max(0.0) * ratio) as f32
}

#[cfg(not(all(target_family = "wasm", not(target_os = "emscripten"))))]
fn keyboard_height() -> f32 {
    0.0
}

#[cfg(test)]
mod key_name_tests {
    use super::key_name;
    use kiss3d::event::Key;

    /// The vocabulary in `balaur_input` was copied from kiss3d's enum. Spot
    /// check that the copy still matches for the keys games actually use — a
    /// silent rename upstream would make them stop working.
    #[test]
    fn common_keys_are_names_scripts_can_ask_for() {
        for key in [
            Key::Space,
            Key::Return,
            Key::Escape,
            Key::Tab,
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::A,
            Key::Z,
            Key::Key0,
            Key::Key9,
            Key::LShift,
            Key::LControl,
            Key::F1,
        ] {
            let name = key_name(key);
            assert!(
                balaur_input::is_known_key(&name),
                "kiss3d reports {name:?}, which balaur_input does not know"
            );
        }
    }

    #[test]
    fn the_vocabulary_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for name in balaur_input::KEY_NAMES {
            assert!(seen.insert(*name), "{name} is listed twice");
        }
    }
}
