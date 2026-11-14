//! `input.gamepad_*` and `input.set_gamepad`: what a script reads of a pad,
//! and how a script stands in for one.
//!
//! Ids come from `input.gamepads()`; a query about a pad that is not connected
//! answers neutrally (false, 0.0, ""), the same convention as a headless
//! keyboard. A fed pad goes through the same path a backend's changes take,
//! so a test that feeds one checks the contract every backend is held to.

use balaur_core::Engine;
use balaur_script::{Module, Value};

use crate::gamepad::{
    GamepadState, PAD_AXIS_NAMES, PAD_BUTTON_NAMES, PadEvent, PadInfo, PadTouch, Power, PowerState,
    STICKS, axis_index, button_index,
};
use crate::haptics::{field, number, vec3};
use crate::vocabulary::{keys as k, words as w};

const SET_GAMEPAD: &str = "set_gamepad";
const GAMEPAD_ACCELERATION: &str = "gamepad_acceleration";
const GAMEPAD_AXIS: &str = "gamepad_axis";
const GAMEPAD_DOWN: &str = "gamepad_down";
const GAMEPAD_GYRO: &str = "gamepad_gyro";
const GAMEPAD_INFO: &str = "gamepad_info";
const GAMEPAD_JUST_PRESSED: &str = "gamepad_just_pressed";
const GAMEPAD_JUST_RELEASED: &str = "gamepad_just_released";
const GAMEPAD_NAME: &str = "gamepad_name";
const GAMEPAD_POWER: &str = "gamepad_power";
const GAMEPAD_PRESSURE: &str = "gamepad_pressure";
const GAMEPAD_REPEATED: &str = "gamepad_repeated";
const GAMEPAD_TOUCHES: &str = "gamepad_touches";
const GAMEPADS: &str = "gamepads";

pub(crate) fn install_gamepad_api(m: &mut Module<Engine>) {
    m.describe(&[
        (GAMEPADS, &[], "() -> [int]", "The ids of every connected pad: player slots from 0. A pad takes the lowest free slot last held by the same model, else the lowest free slot, and keeps it while connected."),
        (GAMEPAD_NAME, &[], "", "The pad's name: its mapping's when one matched, else the platform's. Empty when no pad has that id."),
        (GAMEPAD_DOWN, &[], "", "Whether the pad's `GAMEPAD_BUTTON_*` button is held down: pressed past `[input] gamepad_press`, and not yet back under `gamepad_release`."),
        (GAMEPAD_JUST_PRESSED, &[], "", "Whether the pad's `GAMEPAD_BUTTON_*` button went down this frame; a press and a release between two frames are both seen."),
        (GAMEPAD_JUST_RELEASED, &[], "", "Whether the pad's `GAMEPAD_BUTTON_*` button came up this frame; true for that one frame only."),
        (GAMEPAD_REPEATED, &[], "", "True the frame the pad's `GAMEPAD_BUTTON_*` button goes down, then every `[input] gamepad_repeat_interval_seconds` once it has been held for `gamepad_repeat_delay_seconds`: for stepping through a menu."),
        (GAMEPAD_PRESSURE, &[], "", "How far the pad's `GAMEPAD_BUTTON_*` button is pressed, 0 to 1, before any deadzone: a digital button reads 0 or 1, a trigger or a pressure-sensitive button anything between."),
        (GAMEPAD_AXIS, &[], "", "How far the pad's `GAMEPAD_AXIS_*` stick or trigger is pushed: a stick -1 to 1 with up and right positive, a trigger 0 to 1, after `[input] gamepad_deadzone`; zero at rest and for an absent pad."),
        (GAMEPAD_INFO, &[], "(id: int) -> GamepadInfo?", "Who the pad is; nil for an absent pad."),
        (GAMEPAD_POWER, &[], "(id: int) -> GamepadPower", "The pad's power. Read whenever the pad sends anything."),
        (GAMEPAD_GYRO, &[], "(id: int) -> Vec3", "How fast the pad is turning, in radians per second about each axis. Read from PlayStation pads on desktop; zero for a pad with no gyroscope."),
        (GAMEPAD_ACCELERATION, &[], "(id: int) -> Vec3", "The pad's acceleration in g, gravity included, so a pad at rest reads 1 on one axis. Read from PlayStation pads on desktop; zero for a pad with no accelerometer."),
        (GAMEPAD_TOUCHES, &[], "(id: int) -> [Touch]", "Every finger on the pad's touchpad, oldest first, with x and y running 0 to 1 across the surface. Read from PlayStation pads on desktop; empty for a pad with no touchpad."),
        (SET_GAMEPAD, &[], "(id: int, report: GamepadReport)", "Report a pad as if a backend had, from next frame. A pad fed anything connects; `connected: false` unplugs it."),
    ]);
    install_pad_records(m);
    for name in PAD_BUTTON_NAMES {
        m.constant(
            &const_name("GAMEPAD_BUTTON_", name),
            Value::Str((*name).to_string()),
        );
    }
    for name in PAD_AXIS_NAMES {
        m.constant(
            &const_name("GAMEPAD_AXIS_", name),
            Value::Str((*name).to_string()),
        );
    }
    for state in PowerState::ALL {
        let word = state.word();
        m.constant(
            &const_name("GAMEPAD_POWER_", word),
            Value::Str(word.to_string()),
        );
    }
    for word in [w::MAPPING_SDL, w::MAPPING_DRIVER] {
        m.constant(
            &const_name("GAMEPAD_MAPPING_", word),
            Value::Str(word.to_string()),
        );
    }
    install_pad_readers(m);
}

fn install_pad_readers(m: &mut Module<Engine>) {
    m.function(GAMEPADS, |eng: &Engine, ()| {
        let state = eng.resource::<GamepadState>();
        let ids = state
            .borrow()
            .pads()
            .iter()
            .map(|pad| Value::Int(pad.id))
            .collect();
        Ok(Value::List(ids))
    });
    m.function(GAMEPAD_NAME, |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let name = state
            .borrow()
            .pad(id)
            .map_or_else(String::new, |pad| pad.name().to_string());
        Ok(name)
    });
    install_button_readers(m);
    m.function(GAMEPAD_AXIS, |eng: &Engine, (id, axis): (i64, String)| {
        check_pad_axis(&axis);
        let state = eng.resource::<GamepadState>();
        let v = state.borrow().pad(id).map_or(0.0, |p| p.axis(&axis));
        Ok(v)
    });
    m.function(GAMEPAD_INFO, |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let state = state.borrow();
        let Some(pad) = state.pad(id) else {
            return Ok(Value::Nil);
        };
        let info = pad.info();
        Ok(Value::Map(vec![
            (k::OS_NAME.to_string(), Value::Str(info.os_name.clone())),
            (k::GUID.to_string(), Value::Str(info.guid.clone())),
            (k::VENDOR.to_string(), Value::Int(i64::from(info.vendor))),
            (k::PRODUCT.to_string(), Value::Int(i64::from(info.product))),
            (k::MAPPING.to_string(), Value::Str(info.mapping.clone())),
        ]))
    });
    m.function(GAMEPAD_POWER, |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let power = state
            .borrow()
            .pad(id)
            .map_or_else(Power::default, crate::Pad::power);
        Ok(Value::Map(vec![
            (
                k::STATE.to_string(),
                Value::Str(power.state.word().to_string()),
            ),
            (k::LEVEL.to_string(), Value::Num(f64::from(power.level))),
        ]))
    });
    install_sensor_readers(m);
    m.function(SET_GAMEPAD, |eng: &Engine, (id, opts): (i64, Value)| {
        feed(&mut eng.resource::<GamepadState>().borrow_mut(), id, &opts)
    });
    crate::haptics::install_haptics_api(m);
}

/// What a script reads of a pad, and what it feeds one.
fn install_pad_records(m: &mut Module<Engine>) {
    m.records(&[
        (
            "GamepadInfo",
            "{ os_name: string, guid: string, vendor: int, product: int, mapping: string }",
            "Who a pad is, with `guid` SDL's 32 hex digits and `mapping` either `sdl` or `driver`.",
        ),
        (
            "GamepadPower",
            "{ state: string, level: float }",
            "A pad's power: a `GAMEPAD_POWER_*` state and a charge from 0 to 1, or -1 when not known.",
        ),
        (
            "PadButtons",
            "{ south: float?, east: float?, north: float?, west: float?, left_shoulder: float?, left_trigger: float?, right_shoulder: float?, right_trigger: float?, back: float?, start: float?, guide: float?, left_stick: float?, right_stick: float?, dpad_up: float?, dpad_down: float?, dpad_left: float?, dpad_right: float?, c: float?, z: float? }",
            "How far a fed pad's buttons are pressed, 0 to 1; one left out stays as it was.",
        ),
        (
            "PadAxes",
            "{ left_x: float?, left_y: float?, right_x: float?, right_y: float?, left_trigger: float?, right_trigger: float? }",
            "Where a fed pad's sticks and triggers are; one left out stays as it was.",
        ),
        (
            "GamepadReport",
            "{ connected: bool?, name: string?, os_name: string?, guid: string?, vendor: int?, product: int?, mapping: string?, rumble: bool?, buttons: PadButtons?, axes: PadAxes?, power: GamepadPower?, gyro: Vec3?, acceleration: Vec3?, touches: [Touch]? }",
            "What a script reports of a pad: its identity, its buttons and sticks, its power, its motion and its touchpad.",
        ),
    ]);
}

/// The four per-button readers, which differ only in what they ask the pad.
fn install_button_readers(m: &mut Module<Engine>) {
    type Reader = fn(&crate::Pad, &str) -> bool;
    let readers: [(&str, Reader); 4] = [
        (GAMEPAD_DOWN, crate::Pad::is_down),
        (GAMEPAD_JUST_PRESSED, crate::Pad::just_pressed),
        (GAMEPAD_JUST_RELEASED, crate::Pad::just_released),
        (GAMEPAD_REPEATED, crate::Pad::is_repeated),
    ];
    for (name, read) in readers {
        m.function(name, move |eng: &Engine, (id, button): (i64, String)| {
            check_pad_button(&button);
            let state = eng.resource::<GamepadState>();
            let v = state.borrow().pad(id).is_some_and(|p| read(p, &button));
            Ok(v)
        });
    }
    m.function(
        GAMEPAD_PRESSURE,
        |eng: &Engine, (id, button): (i64, String)| {
            check_pad_button(&button);
            let state = eng.resource::<GamepadState>();
            let v = state.borrow().pad(id).map_or(0.0, |p| p.pressure(&button));
            Ok(v)
        },
    );
}

/// Motion and the touchpad, which `sensors.rs` or a feed writes after the poll.
fn install_sensor_readers(m: &mut Module<Engine>) {
    m.function(GAMEPAD_GYRO, |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let v = state.borrow().pad(id).map_or([0.0; 3], |p| p.motion().gyro);
        Ok(Value::Vec3(v))
    });
    m.function(GAMEPAD_ACCELERATION, |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let v = state
            .borrow()
            .pad(id)
            .map_or([0.0; 3], |p| p.motion().acceleration);
        Ok(Value::Vec3(v))
    });
    // Shaped like `input.touches`, so a pad's touchpad reads like a screen.
    m.function(GAMEPAD_TOUCHES, |eng: &Engine, id: i64| {
        let state = eng.resource::<GamepadState>();
        let touches = state.borrow().pad(id).map_or_else(Vec::new, |pad| {
            pad.touches()
                .iter()
                .map(|touch| {
                    Value::Map(vec![
                        (k::ID.to_string(), Value::Int(touch.id)),
                        (k::POSITION.to_string(), Value::Vec2([touch.x, touch.y])),
                    ])
                })
                .collect()
        });
        Ok(Value::List(touches))
    });
}

/// `input.set_gamepad`: the options table as the changes a backend would
/// have reported.
fn feed(state: &mut GamepadState, id: i64, opts: &Value) -> anyhow::Result<()> {
    let opts = Some(opts);
    if let Some(Value::Bool(false)) = field(opts, k::CONNECTED) {
        state.feed([PadEvent::Disconnected(id)]);
        return Ok(());
    }
    let mut info = state.info_after_feeds(id);
    let identity = merge_info(&mut info, opts)?;
    let reconnect = matches!(field(opts, k::CONNECTED), Some(Value::Bool(true)));
    let mut events = Vec::new();
    if reconnect || !state.connected_after_feeds(id) {
        events.push(PadEvent::Connected(id, Box::new(info)));
    } else if identity {
        events.push(PadEvent::Info(id, Box::new(info)));
    }
    if let Some(Value::Map(buttons)) = field(opts, k::BUTTONS) {
        for (name, value) in buttons {
            check_pad_button(name);
            if let (Some(button), Some(value)) = (button_index(name), pressure_of(value)) {
                events.push(PadEvent::Button(id, button, value));
            }
        }
    }
    if let Some(Value::Map(axes)) = field(opts, k::AXES) {
        for (name, value) in axes {
            check_pad_axis(name);
            let Some(value) = pressure_of(value) else {
                continue;
            };
            match axis_index(name) {
                Some(axis) if axis < STICKS => events.push(PadEvent::Axis(id, axis, value)),
                // A trigger axis is the pressure on its button.
                Some(_) => {
                    events.extend(button_index(name).map(|b| PadEvent::Button(id, b, value)));
                }
                None => {}
            }
        }
    }
    if let Some(power) = field(opts, k::POWER) {
        events.push(PadEvent::Power(id, power_of(power)?));
    }
    state.feed(events);
    let gyro = field(opts, k::GYRO).and_then(vec3);
    let acceleration = field(opts, k::ACCELERATION).and_then(vec3);
    let touches = field(opts, k::TOUCHES).map(touches_of);
    if gyro.is_some() || acceleration.is_some() || touches.is_some() {
        state.feed_sensors(id, gyro, acceleration, touches);
    }
    Ok(())
}

/// Fold the identity keys a feed gave into `info`; true when it gave any.
fn merge_info(info: &mut PadInfo, opts: Option<&Value>) -> anyhow::Result<bool> {
    let text = |key| match field(opts, key) {
        Some(Value::Str(text)) => Some(text.clone()),
        _ => None,
    };
    let id16 = |key| {
        field(opts, key).and_then(|value| match value {
            Value::Int(n) => u16::try_from(*n).ok(),
            _ => None,
        })
    };
    let mut any = false;
    for (slot, key) in [
        (&mut info.name, k::NAME),
        (&mut info.os_name, k::OS_NAME),
        (&mut info.guid, k::GUID),
    ] {
        if let Some(text) = text(key) {
            *slot = text;
            any = true;
        }
    }
    if let Some(mapping) = text(k::MAPPING) {
        if ![w::MAPPING_SDL, w::MAPPING_DRIVER].contains(&mapping.as_str()) {
            anyhow::bail!(
                "'{mapping}' is not a mapping: {} or {}",
                w::MAPPING_SDL,
                w::MAPPING_DRIVER
            );
        }
        info.mapping = mapping;
        any = true;
    }
    for (slot, key) in [
        (&mut info.vendor, k::VENDOR),
        (&mut info.product, k::PRODUCT),
    ] {
        if let Some(id) = id16(key) {
            *slot = id;
            any = true;
        }
    }
    if let Some(Value::Bool(rumble)) = field(opts, k::RUMBLE) {
        info.rumble = *rumble;
        any = true;
    }
    Ok(any)
}

/// A button's or an axis's value, whichever way the language spelled the number.
fn pressure_of(value: &Value) -> Option<f32> {
    match value {
        Value::Num(n) => Some(*n as f32),
        Value::Int(n) => Some(*n as f32),
        _ => None,
    }
}

fn power_of(value: &Value) -> anyhow::Result<Power> {
    let opts = Some(value);
    let state = match field(opts, k::STATE) {
        None => PowerState::Unknown,
        Some(Value::Str(word)) => PowerState::from_word(word).ok_or_else(|| {
            anyhow::anyhow!(
                "'{word}' is not a power state: {}",
                w::POWER_STATES.join(", ")
            )
        })?,
        Some(other) => anyhow::bail!("a power state is a word, not {other:?}"),
    };
    Ok(Power {
        state,
        level: number(opts, k::LEVEL).unwrap_or(-1.0),
    })
}

fn touches_of(value: &Value) -> Vec<PadTouch> {
    let Value::List(list) = value else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|touch| {
            let touch = Some(touch);
            let id = match field(touch, k::ID) {
                Some(Value::Int(id)) => *id,
                _ => return None,
            };
            let Some(Value::Vec2([x, y])) = field(touch, k::POSITION) else {
                return None;
            };
            Some(PadTouch { id, x: *x, y: *y })
        })
        .collect()
}

/// `GAMEPAD_BUTTON_SOUTH` from `south`.
fn const_name(prefix: &str, name: &str) -> String {
    format!("{prefix}{}", name.to_ascii_uppercase())
}

/// Warn once per unrecognised pad button, mirroring `check_key`.
pub(crate) fn check_pad_button(button: &str) {
    crate::warn_unknown_once("gamepad button", button, PAD_BUTTON_NAMES);
}

/// Warn once per unrecognised pad axis, mirroring `check_key`.
pub(crate) fn check_pad_axis(axis: &str) {
    crate::warn_unknown_once("gamepad axis", axis, PAD_AXIS_NAMES);
}
