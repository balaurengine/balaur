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

/// A pad on its way into or out of a recording.
///
/// Separate from [`Pad`] for one reason: an axis name is a `&'static str`
/// borrowed from [`PAD_AXIS_NAMES`], which serializes but cannot be
/// deserialized. Coming back in, the name is looked up in that list again.
#[derive(serde::Serialize, serde::Deserialize)]
struct PadFrame {
    id: i64,
    name: String,
    down: Vec<String>,
    just_pressed: Vec<String>,
    just_released: Vec<String>,
    axes: Vec<(String, f32)>,
}

/// Every pad's state this tick, for a recording.
pub(crate) fn capture(state: &GamepadState) -> serde_json::Value {
    let pads: Vec<PadFrame> = state
        .pads
        .iter()
        .map(|pad| PadFrame {
            id: pad.id,
            name: pad.name.clone(),
            down: pad.down.iter().cloned().collect(),
            just_pressed: pad.just_pressed.iter().cloned().collect(),
            just_released: pad.just_released.iter().cloned().collect(),
            axes: pad
                .axes
                .iter()
                .map(|(name, value)| ((*name).to_string(), *value))
                .collect(),
        })
        .collect();
    serde_json::to_value(pads).unwrap_or(serde_json::Value::Null)
}

/// Replace the pads with a recorded tick's. An axis the build no longer
/// knows is dropped rather than guessed at.
pub(crate) fn restore(state: &mut GamepadState, value: &serde_json::Value) {
    let frames: Vec<PadFrame> = match serde_json::from_value(value.clone()) {
        Ok(frames) => frames,
        Err(e) => {
            tracing::error!(error = %e, "replaying gamepad input");
            return;
        }
    };
    state.pads = frames
        .into_iter()
        .map(|frame| Pad {
            id: frame.id,
            name: frame.name,
            down: frame.down.into_iter().collect(),
            just_pressed: frame.just_pressed.into_iter().collect(),
            just_released: frame.just_released.into_iter().collect(),
            axes: frame
                .axes
                .into_iter()
                .filter_map(|(name, value)| {
                    PAD_AXIS_NAMES
                        .iter()
                        .find(|known| **known == name)
                        .map(|known| (*known, value))
                })
                .collect(),
        })
        .collect();
}

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

/// The gilrs mapping behind [`PAD_BUTTON_NAMES`]; one entry per name, in the
/// same order, so the two lists cannot drift apart silently (the test below
/// says so).
#[cfg(not(target_family = "wasm"))]
const BUTTONS: &[(&str, gilrs::Button)] = &[
    ("South", gilrs::Button::South),
    ("East", gilrs::Button::East),
    ("North", gilrs::Button::North),
    ("West", gilrs::Button::West),
    ("LeftTrigger", gilrs::Button::LeftTrigger),
    ("LeftTrigger2", gilrs::Button::LeftTrigger2),
    ("RightTrigger", gilrs::Button::RightTrigger),
    ("RightTrigger2", gilrs::Button::RightTrigger2),
    ("Select", gilrs::Button::Select),
    ("Start", gilrs::Button::Start),
    ("Mode", gilrs::Button::Mode),
    ("LeftThumb", gilrs::Button::LeftThumb),
    ("RightThumb", gilrs::Button::RightThumb),
    ("DPadUp", gilrs::Button::DPadUp),
    ("DPadDown", gilrs::Button::DPadDown),
    ("DPadLeft", gilrs::Button::DPadLeft),
    ("DPadRight", gilrs::Button::DPadRight),
];

/// The gilrs mapping behind [`PAD_AXIS_NAMES`], same contract as [`BUTTONS`].
#[cfg(not(target_family = "wasm"))]
const AXES: &[(&str, gilrs::Axis)] = &[
    ("LeftStickX", gilrs::Axis::LeftStickX),
    ("LeftStickY", gilrs::Axis::LeftStickY),
    ("LeftZ", gilrs::Axis::LeftZ),
    ("RightStickX", gilrs::Axis::RightStickX),
    ("RightStickY", gilrs::Axis::RightStickY),
    ("RightZ", gilrs::Axis::RightZ),
    ("DPadX", gilrs::Axis::DPadX),
    ("DPadY", gilrs::Axis::DPadY),
];

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
        use gilrs::Gilrs;

        if self.runtime.is_none() {
            let gilrs = match Gilrs::new() {
                Ok(gilrs) => Some(gilrs),
                // The dummy backend (iOS, Android): expected, not news.
                Err(gilrs::Error::NotImplemented(_)) => None,
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

        let mut fresh: Vec<Pad> = Vec::new();
        for (id, gamepad) in gilrs.gamepads() {
            let id = i64::try_from(usize::from(id)).unwrap_or(i64::MAX);
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
                let value = gamepad
                    .axis_data(*axis)
                    .map_or(0.0, gilrs::ev::state::AxisData::value);
                pad.axes.push((name, value));
            }
            fresh.push(pad);
        }
        fresh.sort_by_key(|p| p.id);
        self.pads = fresh;
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::{AXES, BUTTONS, PAD_AXIS_NAMES, PAD_BUTTON_NAMES};

    /// The script-facing vocabulary and the gilrs mapping are two lists; this
    /// is what keeps them from drifting apart.
    #[test]
    fn the_pad_vocabulary_and_the_gilrs_mapping_agree() {
        let button_names: Vec<&str> = BUTTONS.iter().map(|(name, _)| *name).collect();
        assert_eq!(button_names, PAD_BUTTON_NAMES);
        let axis_names: Vec<&str> = AXES.iter().map(|(name, _)| *name).collect();
        assert_eq!(axis_names, PAD_AXIS_NAMES);
    }
}
