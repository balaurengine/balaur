//! Input as a Balaur plugin.
//!
//! Backend-agnostic: window backends (and, later, replay files) feed events
//! into [`InputState`] each frame; scripts read it through the `input`
//! module. In headless runs nothing feeds it, and every query returns the
//! neutral answer, so simulation code does not need to care.
//!
//! Determinism note: input is part of the simulation's inputs. Recording the
//! per-frame `InputState` gives byte-exact replays; the state itself uses
//! ordered collections so any future iteration/serialization is stable.

use anyhow::Result;
use balaur_core::collections::DetHashSet;
use balaur_core::Engine;
use balaur_core::{App, Plugin};
use balaur_script::{Bindings, BindingsExt};

const MOUSE_BUTTONS: usize = 8;

#[derive(Default)]
pub struct InputState {
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

impl InputState {
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
        app.engine.insert_resource(InputState::default());

        let mut m = app.script_module("input")?;
        register(&mut m);
        Ok(())
    }
}

/// `input.*`. Declared against the neutral seam.
fn register(m: &mut dyn Bindings<Engine>) {
    m.function("is_down", |eng: &Engine, key: String| {
        let state = eng.resource::<InputState>();
        let v = state.borrow().down.contains(&key);
        Ok(v)
    });
    m.function("just_pressed", |eng: &Engine, key: String| {
        let state = eng.resource::<InputState>();
        let v = state.borrow().just_pressed.contains(&key);
        Ok(v)
    });
    m.function("just_released", |eng: &Engine, key: String| {
        let state = eng.resource::<InputState>();
        let v = state.borrow().just_released.contains(&key);
        Ok(v)
    });
    m.function("mouse_position", |eng: &Engine, ()| {
        let state = eng.resource::<InputState>();
        let v = state.borrow().mouse_pos;
        Ok(v)
    });
    m.function("mouse_delta", |eng: &Engine, ()| {
        let state = eng.resource::<InputState>();
        let v = state.borrow().mouse_delta;
        Ok(v)
    });
    m.function("scroll_delta", |eng: &Engine, ()| {
        let state = eng.resource::<InputState>();
        let v = state.borrow().scroll;
        Ok(v)
    });
    // Buttons are 1-based: 1 = left, 2 = right, 3 = middle. This follows Lua's
    // convention and is a script-visible contract, so a 0-based backend such as
    // Rune must either keep it or the scripts change. Decide before shipping a
    // second language, not after.
    m.function("is_mouse_down", |eng: &Engine, button: usize| {
        let state = eng.resource::<InputState>();
        let v = button >= 1 && *state.borrow().mouse_down.get(button - 1).unwrap_or(&false);
        Ok(v)
    });
    m.function("mouse_just_pressed", |eng: &Engine, button: usize| {
        let state = eng.resource::<InputState>();
        let v = button >= 1
            && *state
                .borrow()
                .mouse_just_pressed
                .get(button - 1)
                .unwrap_or(&false);
        Ok(v)
    });
}
