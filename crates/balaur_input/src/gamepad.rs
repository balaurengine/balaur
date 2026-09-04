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
//!
//! Buttons and axes are what gilrs reads. Motion and the touchpad it does not
//! read at all, so those come from [`crate::sensors`], which opens the same
//! pad over raw HID and decodes the report gilrs discards — PlayStation pads
//! today, since they are the ones carrying a gyroscope and a touchpad. Both
//! land in the same snapshot and the same recording, and a second backend
//! (Steam Input, `docs/PLAN-steam.md`) fills them the same way through
//! [`GamepadState::set_motion`] and [`GamepadState::set_touchpad`].

use balaur_core::collections::DetHashSet;
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, Value};

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

/// A pad's motion sensors, in the units a script integrates directly: gyro as
/// radians per second about each axis, acceleration in g with gravity in it.
#[derive(Clone, Copy, Default, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Motion {
    pub gyro: [f32; 3],
    pub acceleration: [f32; 3],
}

/// One finger on a pad's touchpad. `x` and `y` run 0..1 across the surface, so
/// a script never needs to know the pad's own resolution.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct PadTouch {
    pub id: i64,
    pub x: f32,
    pub y: f32,
}

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
    /// Defaulted for the same reason `InputSnapshot::typed` is: without it an
    /// older recording fails to parse and the tick is fed nothing at all.
    #[serde(default)]
    motion: Motion,
    #[serde(default)]
    touches: Vec<PadTouch>,
    #[serde(default)]
    rumble: bool,
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
            motion: pad.motion,
            touches: pad.touches.clone(),
            rumble: pad.rumble,
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
            motion: frame.motion,
            touches: frame.touches,
            rumble: frame.rumble,
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
    motion: Motion,
    touches: Vec<PadTouch>,
    rumble: bool,
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

    /// Zero on every axis unless a backend wrote this frame's reading.
    pub const fn motion(&self) -> Motion {
        self.motion
    }

    /// Fingers on the pad's touchpad, in the order they landed.
    pub fn touches(&self) -> &[PadTouch] {
        &self.touches
    }

    /// Whether the pad has motors. Recorded, so a script that branches on it
    /// takes the same branch on a machine whose pad has none.
    pub const fn can_rumble(&self) -> bool {
        self.rumble
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
    #[cfg(not(target_family = "wasm"))]
    haptics: crate::haptics::Rumble,
    #[cfg(not(target_family = "wasm"))]
    sensors: crate::sensors::Sensors,
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

    /// This frame's motion reading for a pad the poll already listed. A
    /// backend calls it after the poll, every frame it has one: like the rest
    /// of the snapshot, motion is republished rather than remembered.
    pub fn set_motion(&mut self, id: i64, motion: Motion) {
        if let Some(pad) = self.pads.iter_mut().find(|p| p.id == id) {
            pad.motion = motion;
        }
    }

    /// This frame's touchpad fingers, same contract as [`Self::set_motion`].
    pub fn set_touchpad(&mut self, id: i64, touches: Vec<PadTouch>) {
        if let Some(pad) = self.pads.iter_mut().find(|p| p.id == id) {
            pad.touches = touches;
        }
    }

    /// Rumble the pad's two motors at 0..1 for `seconds`. False when the pad
    /// is gone, has no motors, or the build has no force feedback at all.
    pub fn rumble(&mut self, id: i64, strong: f32, weak: f32, seconds: f32) -> bool {
        #[cfg(not(target_family = "wasm"))]
        {
            let Some(gilrs) = self.runtime.as_mut().and_then(|r| r.gilrs.as_mut()) else {
                return false;
            };
            self.haptics.play(gilrs, id, strong, weak, seconds)
        }
        #[cfg(target_family = "wasm")]
        {
            let _ = (id, strong, weak, seconds);
            false
        }
    }

    /// Silence the pad now rather than at the end of its rumble.
    pub fn stop_rumble(&mut self, id: i64) {
        #[cfg(not(target_family = "wasm"))]
        self.haptics.stop(id);
        #[cfg(target_family = "wasm")]
        let _ = id;
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

        let mut fresh: Vec<(Pad, (u16, u16))> = Vec::new();
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
                motion: Motion::default(),
                touches: Vec::new(),
                rumble: gamepad.is_ff_supported(),
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
            let ids = (
                gamepad.vendor_id().unwrap_or_default(),
                gamepad.product_id().unwrap_or_default(),
            );
            fresh.push((pad, ids));
        }
        fresh.sort_by_key(|(pad, _)| pad.id);
        let ids: Vec<(u16, u16)> = fresh.iter().map(|(_, ids)| *ids).collect();
        self.pads = fresh.into_iter().map(|(pad, _)| pad).collect();

        let connected: Vec<i64> = self.pads.iter().map(|p| p.id).collect();
        self.haptics.retain_pads(&connected);
        self.read_sensors(&ids);
    }

    /// Fill in what gilrs cannot read. Two identical pads are told apart by
    /// order, the same order the snapshot lists them in.
    #[cfg(not(target_family = "wasm"))]
    fn read_sensors(&mut self, ids: &[(u16, u16)]) {
        self.sensors.poll(ids);
        for (i, pad) in self.pads.iter_mut().enumerate() {
            let (vendor, product) = ids[i];
            let nth = ids[..i].iter().filter(|pair| **pair == ids[i]).count();
            if let Some(reading) = self.sensors.reading(vendor, product, nth) {
                pad.motion = reading.motion;
                pad.touches.clone_from(&reading.touches);
            }
        }
    }
}

/// `input.gamepad_gyro` and `input.gamepad_acceleration`.
pub(crate) fn install_motion_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("gamepad_gyro", &[], "", "How fast the pad is turning, in radians per second about each axis. Read from PlayStation pads on desktop; zero for a pad with no gyroscope."),
        ("gamepad_acceleration", &[], "", "The pad's acceleration in g, gravity included, so a pad at rest reads 1 on one axis. Read from PlayStation pads on desktop; zero for a pad with no accelerometer."),
    ]);
    m.function("gamepad_gyro", |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let v = state.borrow().pad(id).map_or([0.0; 3], |p| p.motion().gyro);
        Ok(Value::Vec3(v))
    });
    m.function("gamepad_acceleration", |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let v = state
            .borrow()
            .pad(id)
            .map_or([0.0; 3], |p| p.motion().acceleration);
        Ok(Value::Vec3(v))
    });
}

/// `input.gamepad_touches`, shaped like `input.touches` so a script reads a
/// pad's touchpad the way it reads a screen.
pub(crate) fn install_touchpad_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "gamepad_touches",
        &[],
        "",
        "Every finger on the pad's touchpad as `{ id, x, y }`, oldest first, with x and y running 0 to 1 across the surface. Read from PlayStation pads on desktop; empty for a pad with no touchpad.",
    )]);
    m.function("gamepad_touches", |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let touches = state.borrow().pad(id).map_or_else(Vec::new, |pad| {
            pad.touches()
                .iter()
                .map(|touch| {
                    Value::Map(vec![
                        ("id".to_string(), Value::Int(touch.id)),
                        ("x".to_string(), Value::Num(f64::from(touch.x))),
                        ("y".to_string(), Value::Num(f64::from(touch.y))),
                    ])
                })
                .collect()
        });
        Ok(Value::List(touches))
    });
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::{
        capture, restore, GamepadState, Motion, Pad, PadTouch, AXES, BUTTONS, PAD_AXIS_NAMES,
        PAD_BUTTON_NAMES,
    };
    use balaur_core::collections::DetHashSet;

    fn close(got: [f32; 3], want: [f32; 3]) -> bool {
        got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-6)
    }

    fn state_with_pad(id: i64) -> GamepadState {
        let mut state = GamepadState::default();
        state.pads.push(Pad {
            id,
            name: "Test Pad".to_string(),
            down: DetHashSet::default(),
            just_pressed: DetHashSet::default(),
            just_released: DetHashSet::default(),
            axes: Vec::new(),
            motion: Motion::default(),
            touches: Vec::new(),
            rumble: true,
        });
        state
    }

    /// The script-facing vocabulary and the gilrs mapping are two lists; this
    /// is what keeps them from drifting apart.
    #[test]
    fn the_pad_vocabulary_and_the_gilrs_mapping_agree() {
        let button_names: Vec<&str> = BUTTONS.iter().map(|(name, _)| *name).collect();
        assert_eq!(button_names, PAD_BUTTON_NAMES);
        let axis_names: Vec<&str> = AXES.iter().map(|(name, _)| *name).collect();
        assert_eq!(axis_names, PAD_AXIS_NAMES);
    }

    /// Motion is written after the poll, so it has to survive into the same
    /// frame's recording and come back out of it unchanged.
    #[test]
    fn motion_and_the_touchpad_round_trip_through_a_recording() {
        let mut state = state_with_pad(0);
        state.set_motion(
            0,
            Motion {
                gyro: [0.5, -1.5, 0.25],
                acceleration: [0.0, 1.0, 0.0],
            },
        );
        state.set_touchpad(
            0,
            vec![PadTouch {
                id: 7,
                x: 0.25,
                y: 0.75,
            }],
        );

        let recorded = capture(&state);
        let mut replayed = GamepadState::default();
        restore(&mut replayed, &recorded);

        let pad = replayed.pad(0).expect("the pad came back");
        assert!(close(pad.motion().gyro, [0.5, -1.5, 0.25]));
        assert!(close(pad.motion().acceleration, [0.0, 1.0, 0.0]));
        assert_eq!(
            pad.touches(),
            [PadTouch {
                id: 7,
                x: 0.25,
                y: 0.75
            }]
        );
        assert!(
            pad.can_rumble(),
            "whether the pad has motors is recorded too"
        );
    }

    /// The fields are `#[serde(default)]` for this: without it the whole
    /// snapshot fails to parse and the tick replays with no pads at all.
    #[test]
    fn a_recording_made_before_motion_existed_still_replays() {
        let older = serde_json::json!([{
            "id": 3,
            "name": "Older Pad",
            "down": ["South"],
            "just_pressed": [],
            "just_released": [],
            "axes": [["LeftStickX", 0.5]],
        }]);
        let mut state = GamepadState::default();
        restore(&mut state, &older);

        let pad = state.pad(3).expect("the pad still restored");
        assert!(pad.is_down("South"));
        assert!((pad.axis("LeftStickX") - 0.5).abs() < 1e-6);
        assert_eq!(pad.motion(), Motion::default());
        assert!(pad.touches().is_empty());
        assert!(!pad.can_rumble());
    }

    /// The neutral-answer rule: a pad that is not there reads zero rather
    /// than failing, so the same script runs headless.
    #[test]
    fn an_absent_pad_reads_neutral_and_cannot_be_written() {
        let mut state = state_with_pad(0);
        state.set_motion(
            1,
            Motion {
                gyro: [9.0; 3],
                acceleration: [9.0; 3],
            },
        );
        state.set_touchpad(
            1,
            vec![PadTouch {
                id: 0,
                x: 1.0,
                y: 1.0,
            }],
        );

        assert!(state.pad(1).is_none(), "writing did not invent a pad");
        assert_eq!(state.pad(0).unwrap().motion(), Motion::default());
        assert!(!state.rumble(1, 1.0, 1.0, 0.1), "no pad, no rumble");
        state.stop_rumble(1);
    }
}
