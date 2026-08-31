//! Gamepads, polled through gilrs.
//!
//! Same shape as the rest of input: a per-frame snapshot scripts read through
//! `input.*`, with neutral answers when there is no pad — an unplugged
//! controller must not change what a script can call. gilrs covers desktop
//! and nothing else today, so on wasm and mobile the snapshot simply stays
//! empty, the same way a headless run keeps the keyboard empty.
//!
//! Polling runs inside the tick (`Stage::First`), not in the windowed
//! backend: controllers are not window events, and this way a headless run
//! on a desk with a pad plugged in sees it too.

use balaur_core::collections::DetHashSet;

/// Buttons scripts can ask about, in gilrs's naming. The list is the
/// vocabulary (same contract as `KEY_NAMES`): queries validate against it,
/// and the poller only ever produces names from it.
pub const PAD_BUTTON_NAMES: &[&str] = &[
    "South",
    "East",
    "North",
    "West",
    "LeftTrigger",
    "LeftTrigger2",
    "RightTrigger",
    "RightTrigger2",
    "Select",
    "Start",
    "Mode",
    "LeftThumb",
    "RightThumb",
    "DPadUp",
    "DPadDown",
    "DPadLeft",
    "DPadRight",
];

/// Axes scripts can ask about, in gilrs's naming. Values are -1..1.
pub const PAD_AXIS_NAMES: &[&str] = &[
    "LeftStickX",
    "LeftStickY",
    "LeftZ",
    "RightStickX",
    "RightStickY",
    "RightZ",
    "DPadX",
    "DPadY",
];

/// One connected controller's state for the current frame.
pub struct Pad {
    pub id: i64,
    pub name: String,
    down: DetHashSet<String>,
    just_pressed: DetHashSet<String>,
    just_released: DetHashSet<String>,
    axes: Vec<(&'static str, f32)>,
}

impl Pad {
    pub fn is_down(&self, button: &str) -> bool {
        self.down.contains(button)
    }

    pub fn just_pressed(&self, button: &str) -> bool {
        self.just_pressed.contains(button)
    }

    pub fn just_released(&self, button: &str) -> bool {
        self.just_released.contains(button)
    }

    pub fn axis(&self, axis: &str) -> f32 {
        self.axes
            .iter()
            .find(|(name, _)| *name == axis)
            .map_or(0.0, |(_, v)| *v)
    }
}

/// Every connected pad, rebuilt once per frame by [`GamepadState::poll`].
///
/// Pads are ordered by gilrs id, so iteration (and therefore anything a
/// script derives from `input.gamepads()`) is stable across frames.
#[derive(Default)]
pub struct GamepadState {
    pads: Vec<Pad>,
    #[cfg(not(target_family = "wasm"))]
    runtime: Option<Runtime>,
}

/// `None` inside means the platform backend failed to open (no udev in a bare
/// container); the failure is remembered so it warns once, not every frame.
#[cfg(not(target_family = "wasm"))]
struct Runtime {
    gilrs: Option<gilrs::Gilrs>,
}

impl GamepadState {
    pub fn pads(&self) -> &[Pad] {
        &self.pads
    }

    pub fn pad(&self, id: i64) -> Option<&Pad> {
        self.pads.iter().find(|p| p.id == id)
    }

    /// Refresh the snapshot from the platform. Edges (`just_*`) come from
    /// diffing against the previous frame, so a button that was down last
    /// frame and is down now reports held, not pressed.
    pub fn poll(&mut self) {
        #[cfg(not(target_family = "wasm"))]
        self.poll_gilrs();
    }

    /// gilrs cannot open its platform backend everywhere (no udev in a bare
    /// container, no backend at all on mobile). Same rule as audio: warn once
    /// and answer neutrally, so the same game runs regardless.
    #[cfg(not(target_family = "wasm"))]
    fn poll_gilrs(&mut self) {
        use gilrs::{Axis, Button, Gilrs};

        if self.runtime.is_none() {
            let gilrs = match Gilrs::new() {
                Ok(gilrs) => Some(gilrs),
                Err(err) => {
                    tracing::warn!("gamepads disabled: {err}");
                    None
                }
            };
            self.runtime = Some(Runtime { gilrs });
        }
        let Some(gilrs) = self.runtime.as_mut().and_then(|r| r.gilrs.as_mut()) else {
            return;
        };

        // gilrs updates its cached state while events are drained.
        while gilrs.next_event().is_some() {}

        const BUTTONS: &[(&str, Button)] = &[
            ("South", Button::South),
            ("East", Button::East),
            ("North", Button::North),
            ("West", Button::West),
            ("LeftTrigger", Button::LeftTrigger),
            ("LeftTrigger2", Button::LeftTrigger2),
            ("RightTrigger", Button::RightTrigger),
            ("RightTrigger2", Button::RightTrigger2),
            ("Select", Button::Select),
            ("Start", Button::Start),
            ("Mode", Button::Mode),
            ("LeftThumb", Button::LeftThumb),
            ("RightThumb", Button::RightThumb),
            ("DPadUp", Button::DPadUp),
            ("DPadDown", Button::DPadDown),
            ("DPadLeft", Button::DPadLeft),
            ("DPadRight", Button::DPadRight),
        ];
        const AXES: &[(&str, Axis)] = &[
            ("LeftStickX", Axis::LeftStickX),
            ("LeftStickY", Axis::LeftStickY),
            ("LeftZ", Axis::LeftZ),
            ("RightStickX", Axis::RightStickX),
            ("RightStickY", Axis::RightStickY),
            ("RightZ", Axis::RightZ),
            ("DPadX", Axis::DPadX),
            ("DPadY", Axis::DPadY),
        ];

        let mut fresh: Vec<Pad> = Vec::new();
        for (id, gamepad) in gilrs.gamepads() {
            let id = usize::from(id) as i64;
            let previous = self.pads.iter().find(|p| p.id == id);
            let mut pad = Pad {
                id,
                name: gamepad.name().to_string(),
                down: DetHashSet::default(),
                just_pressed: DetHashSet::default(),
                just_released: DetHashSet::default(),
                axes: Vec::with_capacity(AXES.len()),
            };
            for (name, button) in BUTTONS {
                let down = gamepad.is_pressed(*button);
                let was_down = previous.is_some_and(|p| p.down.contains(*name));
                if down {
                    pad.down.insert((*name).to_string());
                    if !was_down {
                        pad.just_pressed.insert((*name).to_string());
                    }
                } else if was_down {
                    pad.just_released.insert((*name).to_string());
                }
            }
            for (name, axis) in AXES {
                let value = gamepad.axis_data(*axis).map_or(0.0, |a| a.value());
                pad.axes.push((name, value));
            }
            fresh.push(pad);
        }
        fresh.sort_by_key(|p| p.id);
        self.pads = fresh;
    }
}
