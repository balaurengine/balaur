//! Gamepads: the snapshot scripts read, and the rules that make it the
//! engine's rather than a backend's.
//!
//! Same shape as the rest of input: a per-frame snapshot scripts read through
//! `input.*`, with neutral answers when there is no pad. A backend (gilrs, on
//! the thread in `pad_thread.rs`) or a script (`input.feed_gamepad`) reports
//! changes as [`PadEvent`]s, and this file turns them into the snapshot by the
//! contract in `docs/PLAN-input.md`: one deadzone, a threshold on pressure,
//! every edge kept, repeat counted in tick time.
//!
//! The tick takes the changes in `Stage::First`, not the windowed backend: a
//! controller is not a window event, and a headless run with a pad plugged in
//! sees it too. Motion and the touchpad come from `sensors.rs` over raw HID;
//! a second backend fills them through [`GamepadState::set_motion`] and
//! [`GamepadState::set_touchpad`].

use balaur_core::collections::DetHashSet;
use serde::Deserialize as _;

use crate::settings::PadSettings;

/// Buttons scripts can ask about, named by position as SDL3 names them, so
/// `south` is the same button on every pad. `c` and `z` are the third column
/// of a six-button pad. Queries validate against the list, and every backend
/// reports by index into it.
pub const PAD_BUTTON_NAMES: &[&str] = &[
    "south",
    "east",
    "north",
    "west",
    "left_shoulder",
    "left_trigger",
    "right_shoulder",
    "right_trigger",
    "back",
    "start",
    "guide",
    "left_stick",
    "right_stick",
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
    "c",
    "z",
];

/// Axes scripts can ask about: a stick -1..1 with up and right positive, a
/// trigger 0..1. The sticks come first; a trigger axis reads the pressure on
/// the button of the same name.
pub const PAD_AXIS_NAMES: &[&str] = &[
    "left_x",
    "left_y",
    "right_x",
    "right_y",
    "left_trigger",
    "right_trigger",
];

pub(crate) const BUTTON_COUNT: usize = PAD_BUTTON_NAMES.len();

/// The stick axes: the first four of [`PAD_AXIS_NAMES`], in x-y pairs.
pub(crate) const STICKS: usize = 4;

pub(crate) fn button_index(name: &str) -> Option<usize> {
    PAD_BUTTON_NAMES.iter().position(|known| *known == name)
}

pub(crate) fn axis_index(name: &str) -> Option<usize> {
    PAD_AXIS_NAMES.iter().position(|known| *known == name)
}

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

/// What powers a pad, as SDL3 names the states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerState {
    #[default]
    Unknown,
    OnBattery,
    NoBattery,
    Charging,
    Charged,
}

impl PowerState {
    pub const ALL: [Self; 5] = [
        Self::Unknown,
        Self::OnBattery,
        Self::NoBattery,
        Self::Charging,
        Self::Charged,
    ];

    #[must_use]
    pub const fn word(self) -> &'static str {
        use crate::vocabulary::words as w;
        match self {
            Self::Unknown => w::POWER_UNKNOWN,
            Self::OnBattery => w::POWER_ON_BATTERY,
            Self::NoBattery => w::POWER_NO_BATTERY,
            Self::Charging => w::POWER_CHARGING,
            Self::Charged => w::POWER_CHARGED,
        }
    }

    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.word() == word)
    }
}

/// A pad's power: its state, and its charge from 0 to 1, or -1 when not known.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Power {
    pub state: PowerState,
    pub level: f32,
}

impl Default for Power {
    fn default() -> Self {
        Self {
            state: PowerState::Unknown,
            level: -1.0,
        }
    }
}

/// Who a pad is, in the standards' terms rather than a backend's.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PadInfo {
    /// The mapping's name for the pad when one matched, else the OS's.
    pub name: String,
    pub os_name: String,
    /// SDL's GUID, 32 hex digits: what an SDL mapping is keyed by.
    pub guid: String,
    pub vendor: u16,
    pub product: u16,
    /// `sdl` when a mapping in SDL's format laid the pad out, `driver` when
    /// the OS's own layout did.
    pub mapping: String,
    /// Whether the pad has motors a rumble can drive.
    pub rumble: bool,
}

/// One change a backend or a script reports, by pad id. Buttons and axes are
/// indices into [`PAD_BUTTON_NAMES`] and the stick part of [`PAD_AXIS_NAMES`].
#[derive(Clone, Debug, PartialEq)]
pub enum PadEvent {
    Connected(i64, Box<PadInfo>),
    Disconnected(i64),
    /// The pad's identity changed while it stayed connected.
    Info(i64, Box<PadInfo>),
    /// A button's pressure, 0 to 1.
    Button(i64, usize, f32),
    /// A stick axis, -1 to 1.
    Axis(i64, usize, f32),
    Power(i64, Power),
}

impl PadEvent {
    #[must_use]
    pub const fn pad(&self) -> i64 {
        match self {
            Self::Connected(pad, _)
            | Self::Disconnected(pad)
            | Self::Info(pad, _)
            | Self::Button(pad, _, _)
            | Self::Axis(pad, _, _)
            | Self::Power(pad, _) => *pad,
        }
    }

    /// The same change, for another pad id.
    #[must_use]
    pub fn for_pad(self, pad: i64) -> Self {
        match self {
            Self::Connected(_, info) => Self::Connected(pad, info),
            Self::Disconnected(_) => Self::Disconnected(pad),
            Self::Info(_, info) => Self::Info(pad, info),
            Self::Button(_, button, value) => Self::Button(pad, button, value),
            Self::Axis(_, axis, value) => Self::Axis(pad, axis, value),
            Self::Power(_, power) => Self::Power(pad, power),
        }
    }
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
    #[serde(default)]
    repeated: Vec<String>,
    #[serde(default)]
    pressure: Vec<(String, f32)>,
    #[serde(default)]
    power: Power,
    #[serde(default)]
    info: PadInfo,
}

/// Every pad's state this tick, for a recording.
pub(crate) fn capture(state: &GamepadState) -> serde_json::Value {
    let names = |set: &DetHashSet<String>| set.iter().cloned().collect();
    let pairs = |list: &[(&str, f32)]| {
        list.iter()
            .map(|(name, value)| ((*name).to_string(), *value))
            .collect()
    };
    let pads: Vec<PadFrame> = state
        .pads
        .iter()
        .map(|pad| PadFrame {
            id: pad.id,
            name: pad.info.name.clone(),
            down: names(&pad.down),
            just_pressed: names(&pad.just_pressed),
            just_released: names(&pad.just_released),
            axes: pairs(&pad.axes),
            motion: pad.motion,
            touches: pad.touches.clone(),
            rumble: pad.info.rumble,
            repeated: names(&pad.repeated),
            pressure: pairs(&pad.pressure),
            power: pad.power,
            info: pad.info.clone(),
        })
        .collect();
    serde_json::to_value(pads).unwrap_or(serde_json::Value::Null)
}

/// Replace the pads with a recorded tick's. A name the build no longer knows
/// is dropped rather than guessed at.
pub(crate) fn restore(state: &mut GamepadState, value: &serde_json::Value) {
    let frames: Vec<PadFrame> = match Vec::<PadFrame>::deserialize(value) {
        Ok(frames) => frames,
        Err(e) => {
            tracing::error!(error = %e, "replaying gamepad input");
            return;
        }
    };
    let known = |list: Vec<(String, f32)>, names: &'static [&'static str]| {
        list.into_iter()
            .filter_map(|(name, value)| {
                names
                    .iter()
                    .find(|known| **known == name)
                    .map(|known| (*known, value))
            })
            .collect()
    };
    state.pads = frames
        .into_iter()
        .map(|frame| {
            let mut info = frame.info;
            info.name = frame.name;
            info.rumble = frame.rumble;
            Pad {
                id: frame.id,
                info,
                down: frame.down.into_iter().collect(),
                just_pressed: frame.just_pressed.into_iter().collect(),
                just_released: frame.just_released.into_iter().collect(),
                repeated: frame.repeated.into_iter().collect(),
                pressure: known(frame.pressure, PAD_BUTTON_NAMES),
                axes: known(frame.axes, PAD_AXIS_NAMES),
                motion: frame.motion,
                touches: frame.touches,
                power: frame.power,
            }
        })
        .collect();
}

/// One connected controller's state for the current frame.
pub struct Pad {
    pub id: i64,
    info: PadInfo,
    down: DetHashSet<String>,
    just_pressed: DetHashSet<String>,
    just_released: DetHashSet<String>,
    repeated: DetHashSet<String>,
    /// Buttons with any pressure on them; the rest read 0.
    pressure: Vec<(&'static str, f32)>,
    axes: Vec<(&'static str, f32)>,
    motion: Motion,
    touches: Vec<PadTouch>,
    power: Power,
}

impl Pad {
    pub fn name(&self) -> &str {
        &self.info.name
    }

    pub const fn info(&self) -> &PadInfo {
        &self.info
    }

    pub const fn power(&self) -> Power {
        self.power
    }

    pub fn is_down(&self, button: &str) -> bool {
        self.down.contains(button)
    }

    pub fn just_pressed(&self, button: &str) -> bool {
        self.just_pressed.contains(button)
    }

    pub fn just_released(&self, button: &str) -> bool {
        self.just_released.contains(button)
    }

    /// True on the frame a button goes down, then once per repeat interval
    /// while it stays down past the repeat delay.
    pub fn is_repeated(&self, button: &str) -> bool {
        self.repeated.contains(button)
    }

    /// How far a button is pressed, 0 to 1, before any deadzone.
    pub fn pressure(&self, button: &str) -> f32 {
        lookup(&self.pressure, button)
    }

    pub fn axis(&self, axis: &str) -> f32 {
        lookup(&self.axes, axis)
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
        self.info.rumble
    }
}

fn lookup(list: &[(&str, f32)], name: &str) -> f32 {
    list.iter()
        .find(|(known, _)| *known == name)
        .map_or(0.0, |(_, v)| *v)
}

/// A connected pad as its changes have left it, before the frame shapes it.
#[derive(Clone)]
struct Live {
    id: i64,
    info: PadInfo,
    pressure: [f32; BUTTON_COUNT],
    down: [bool; BUTTON_COUNT],
    /// Seconds each button has been down, for repeat.
    held: [f32; BUTTON_COUNT],
    sticks: [f32; STICKS],
    power: Power,
}

impl Live {
    fn new(id: i64, info: PadInfo) -> Self {
        Self {
            id,
            info,
            pressure: [0.0; BUTTON_COUNT],
            down: [false; BUTTON_COUNT],
            held: [0.0; BUTTON_COUNT],
            sticks: [0.0; STICKS],
            power: Power::default(),
        }
    }
}

/// This frame's edges, by pad and button index.
#[derive(Default)]
struct Edges {
    pressed: Vec<(i64, usize)>,
    released: Vec<(i64, usize)>,
}

/// Every connected pad, rebuilt once per frame by [`GamepadState::poll`].
///
/// Pads are ordered by id, so iteration (and therefore anything a script
/// derives from `input.gamepads()`) is stable across frames.
#[derive(Default)]
pub struct GamepadState {
    pads: Vec<Pad>,
    live: Vec<Live>,
    /// What scripts fed this frame, delivered when the next one begins.
    fed: Vec<PadEvent>,
    /// Motion and fingers a script fed, which a pad keeps until fed again.
    fed_sensors: Vec<(i64, Motion, Vec<PadTouch>)>,
    pub(crate) rumble: crate::rumble::Rumbles,
    #[cfg(not(target_family = "wasm"))]
    source: Option<Source>,
    #[cfg(not(target_family = "wasm"))]
    sensors: crate::sensors::Sensors,
}

/// The gamepad thread's end: its queue of changes for this state, and the hub
/// that holds the motors.
#[cfg(not(target_family = "wasm"))]
struct Source {
    hub: std::sync::Arc<crate::pad_thread::Hub>,
    queue: std::sync::Arc<crate::pad_thread::Queue>,
}

impl GamepadState {
    pub fn pads(&self) -> &[Pad] {
        &self.pads
    }

    pub fn pad(&self, id: i64) -> Option<&Pad> {
        self.pads.iter().find(|p| p.id == id)
    }

    /// A state reading from `hub` instead of the process's gilrs thread.
    #[cfg(all(test, not(target_family = "wasm")))]
    pub(crate) fn reading_from(hub: std::sync::Arc<crate::pad_thread::Hub>) -> Self {
        let queue = hub.subscribe();
        Self {
            source: Some(Source { hub, queue }),
            ..Self::default()
        }
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

    /// Changes a script reports as if a backend had, delivered when the next
    /// frame begins.
    pub fn feed(&mut self, events: impl IntoIterator<Item = PadEvent>) {
        self.fed.extend(events);
    }

    /// Motion and fingers for a fed pad, each kept until fed again.
    pub fn feed_sensors(
        &mut self,
        id: i64,
        gyro: Option<[f32; 3]>,
        acceleration: Option<[f32; 3]>,
        touches: Option<Vec<PadTouch>>,
    ) {
        let i = if let Some(i) = self.fed_sensors.iter().position(|(pad, _, _)| *pad == id) {
            i
        } else {
            self.fed_sensors.push((id, Motion::default(), Vec::new()));
            self.fed_sensors.len() - 1
        };
        let entry = &mut self.fed_sensors[i];
        if let Some(gyro) = gyro {
            entry.1.gyro = gyro;
        }
        if let Some(acceleration) = acceleration {
            entry.1.acceleration = acceleration;
        }
        if let Some(touches) = touches {
            entry.2 = touches;
        }
    }

    /// Whether a pad is connected once this frame's feeds land.
    pub fn connected_after_feeds(&self, id: i64) -> bool {
        let mut connected = self.live.iter().any(|live| live.id == id);
        for event in self.fed.iter().filter(|event| event.pad() == id) {
            match event {
                PadEvent::Connected(..) => connected = true,
                PadEvent::Disconnected(_) => connected = false,
                _ => {}
            }
        }
        connected
    }

    /// The identity a pad has now, or will once this frame's feeds land.
    pub fn info_after_feeds(&self, id: i64) -> PadInfo {
        let fed = self.fed.iter().rev().find_map(|event| match event {
            PadEvent::Connected(pad, info) | PadEvent::Info(pad, info) if *pad == id => {
                Some((**info).clone())
            }
            _ => None,
        });
        fed.or_else(|| {
            self.live
                .iter()
                .find(|live| live.id == id)
                .map(|live| live.info.clone())
        })
        .unwrap_or_default()
    }

    /// Take every change since the last frame and shape this frame's
    /// snapshot from them. Edges come from the changes, not from comparing
    /// two frames, so a press and a release between frames are both seen.
    pub fn poll(&mut self, dt: f32, settings: &PadSettings) {
        let mut events = Vec::new();
        #[cfg(not(target_family = "wasm"))]
        {
            let source = self
                .source
                .get_or_insert_with(|| Source::open(&settings.mappings));
            // Mid-push, the changes wait for the next frame rather than the tick.
            if let Some(taken) = source.queue.try_take() {
                events = taken;
            }
        }
        events.append(&mut self.fed);
        self.apply(events, dt, settings);
        #[cfg(not(target_family = "wasm"))]
        self.read_sensors();
        for (id, motion, touches) in &self.fed_sensors {
            if let Some(pad) = self.pads.iter_mut().find(|p| p.id == *id) {
                pad.motion = *motion;
                pad.touches.clone_from(touches);
            }
        }
        let connected: Vec<i64> = self.pads.iter().map(|pad| pad.id).collect();
        self.fed_sensors.retain(|(id, _, _)| connected.contains(id));
    }

    fn apply(&mut self, events: Vec<PadEvent>, dt: f32, settings: &PadSettings) {
        for live in &mut self.live {
            for (held, down) in live.held.iter_mut().zip(live.down) {
                if down {
                    *held += dt;
                }
            }
        }
        let mut edges = Edges::default();
        for event in events {
            self.apply_one(event, settings, &mut edges);
        }
        self.pads = self
            .live
            .iter()
            .map(|live| shape(live, &edges, dt, settings))
            .collect();
    }

    fn apply_one(&mut self, event: PadEvent, settings: &PadSettings, edges: &mut Edges) {
        let id = event.pad();
        if let PadEvent::Connected(_, info) = event {
            self.live.retain(|live| live.id != id);
            let at = self.live.partition_point(|live| live.id < id);
            self.live.insert(at, Live::new(id, *info));
            return;
        }
        if let PadEvent::Disconnected(_) = event {
            self.live.retain(|live| live.id != id);
            return;
        }
        let Some(live) = self.live.iter_mut().find(|live| live.id == id) else {
            return;
        };
        match event {
            PadEvent::Info(_, info) => live.info = *info,
            PadEvent::Button(_, button, value) if button < BUTTON_COUNT => {
                let value = clamp_unit(value, 0.0);
                live.pressure[button] = value;
                if !live.down[button] && value >= settings.press {
                    live.down[button] = true;
                    live.held[button] = 0.0;
                    edges.pressed.push((id, button));
                } else if live.down[button] && value < settings.release {
                    live.down[button] = false;
                    edges.released.push((id, button));
                }
            }
            PadEvent::Axis(_, axis, value) if axis < STICKS => {
                live.sticks[axis] = clamp_unit(value, -1.0);
            }
            PadEvent::Power(_, power) => live.power = power,
            _ => {}
        }
    }

    /// Fill in what gilrs cannot read. Two identical pads are told apart by
    /// order, the same order the snapshot lists them in.
    #[cfg(not(target_family = "wasm"))]
    fn read_sensors(&mut self) {
        let ids: Vec<(u16, u16)> = self
            .pads
            .iter()
            .map(|pad| (pad.info.vendor, pad.info.product))
            .collect();
        self.sensors.poll(&ids);
        for (i, pad) in self.pads.iter_mut().enumerate() {
            let (vendor, product) = ids[i];
            let nth = ids[..i].iter().filter(|pair| **pair == ids[i]).count();
            if let Some(reading) = self.sensors.reading(vendor, product, nth) {
                pad.motion = reading.motion;
                pad.touches.clone_from(&reading.touches);
            }
        }
    }

    /// Hold a pad's motors through whichever backend has them. A pad no
    /// backend holds, a fed one or one only in a recording, stays still.
    #[cfg_attr(
        target_family = "wasm",
        allow(clippy::unused_self, reason = "a tab has no motors to hold")
    )]
    pub(crate) fn hold_motors(&self, id: i64, strong: f32, weak: f32, seconds: f32) {
        #[cfg(not(target_family = "wasm"))]
        if let Some(source) = &self.source {
            source.hub.hold(id, strong, weak, seconds);
        }
        #[cfg(target_family = "wasm")]
        let _ = (id, strong, weak, seconds);
    }
}

#[cfg(not(target_family = "wasm"))]
impl Source {
    fn open(mappings: &[String]) -> Self {
        let hub = crate::pad_thread::gilrs(mappings);
        let queue = hub.subscribe();
        Self { hub, queue }
    }
}

/// 0..1 (or -1..1), with NaN read as rest rather than full.
fn clamp_unit(value: f32, low: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(low, 1.0)
    }
}

/// One live pad as this frame's snapshot shows it.
fn shape(live: &Live, edges: &Edges, dt: f32, settings: &PadSettings) -> Pad {
    let named = |list: &[(i64, usize)]| -> DetHashSet<String> {
        list.iter()
            .filter(|(pad, _)| *pad == live.id)
            .map(|(_, button)| PAD_BUTTON_NAMES[*button].to_string())
            .collect()
    };
    let just_pressed = named(&edges.pressed);
    let mut repeated = DetHashSet::default();
    for (i, name) in PAD_BUTTON_NAMES.iter().enumerate() {
        let pressed_now = just_pressed.contains(*name);
        let held = live.held[i];
        let repeats = live.down[i]
            && !pressed_now
            && repeats_between(
                held - dt,
                held,
                settings.repeat_delay,
                settings.repeat_interval,
            );
        if pressed_now || repeats {
            repeated.insert((*name).to_string());
        }
    }
    let dz = settings.deadzone;
    let (lx, ly) = radial(live.sticks[0], live.sticks[1], dz);
    let (rx, ry) = radial(live.sticks[2], live.sticks[3], dz);
    let mut axes: Vec<(&'static str, f32)> = PAD_AXIS_NAMES[..STICKS]
        .iter()
        .copied()
        .zip([lx, ly, rx, ry])
        .collect();
    for name in &PAD_AXIS_NAMES[STICKS..] {
        let pressure = button_index(name).map_or(0.0, |i| live.pressure[i]);
        axes.push((name, along(pressure, dz)));
    }
    Pad {
        id: live.id,
        info: live.info.clone(),
        down: PAD_BUTTON_NAMES
            .iter()
            .zip(live.down)
            .filter(|(_, down)| *down)
            .map(|(name, _)| (*name).to_string())
            .collect(),
        just_pressed,
        just_released: named(&edges.released),
        repeated,
        pressure: PAD_BUTTON_NAMES
            .iter()
            .copied()
            .zip(live.pressure)
            .filter(|(_, value)| *value > 0.0)
            .collect(),
        axes,
        motion: Motion::default(),
        touches: Vec::new(),
        power: live.power,
    }
}

/// Whether a repeat falls due between two lengths of hold: the first at
/// `delay`, then one every `interval`.
fn repeats_between(before: f32, after: f32, delay: f32, interval: f32) -> bool {
    let fired = |held: f32| {
        if held < delay {
            -1.0
        } else {
            ((held - delay) / interval.max(f32::EPSILON)).floor()
        }
    };
    fired(after) > fired(before)
}

/// A stick's deadzone, applied to its distance from centre so a diagonal is
/// not squared off, and rescaled so the first live reading is near zero.
fn radial(x: f32, y: f32, deadzone: f32) -> (f32, f32) {
    let distance = libm::hypotf(x, y);
    if distance <= deadzone || distance <= 0.0 {
        return (0.0, 0.0);
    }
    let scaled = ((distance - deadzone) / (1.0 - deadzone)).min(1.0);
    (x / distance * scaled, y / distance * scaled)
}

/// A trigger's deadzone, rescaled the same way.
fn along(value: f32, deadzone: f32) -> f32 {
    if value <= deadzone {
        0.0
    } else {
        ((value - deadzone) / (1.0 - deadzone)).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GamepadState, Motion, PadEvent, PadInfo, PadTouch, Power, PowerState, along, button_index,
        capture, radial, repeats_between, restore,
    };
    use crate::settings::PadSettings;

    const DT: f32 = 1.0 / 60.0;

    fn close(got: f32, want: f32) -> bool {
        (got - want).abs() < 1e-5
    }

    fn frame(state: &mut GamepadState, events: Vec<PadEvent>) {
        state.apply(events, DT, &PadSettings::default());
    }

    fn connected(id: i64) -> GamepadState {
        let mut state = GamepadState::default();
        let info = PadInfo {
            name: "Test Pad".into(),
            rumble: true,
            ..PadInfo::default()
        };
        frame(&mut state, vec![PadEvent::Connected(id, Box::new(info))]);
        state
    }

    fn south() -> usize {
        button_index("south").unwrap()
    }

    #[test]
    fn a_press_and_release_between_two_frames_are_both_edges() {
        let mut state = connected(0);
        frame(
            &mut state,
            vec![
                PadEvent::Button(0, south(), 1.0),
                PadEvent::Button(0, south(), 0.0),
            ],
        );
        let pad = state.pad(0).unwrap();
        assert!(pad.just_pressed("south") && pad.just_released("south"));
        assert!(!pad.is_down("south"));
    }

    #[test]
    fn a_trigger_goes_down_past_press_and_up_below_release() {
        let settings = PadSettings::default();
        let trigger = button_index("left_trigger").unwrap();
        let between = f32::midpoint(settings.press, settings.release);
        let mut state = connected(0);
        frame(&mut state, vec![PadEvent::Button(0, trigger, between)]);
        assert!(!state.pad(0).unwrap().is_down("left_trigger"));
        frame(
            &mut state,
            vec![PadEvent::Button(0, trigger, settings.press)],
        );
        assert!(state.pad(0).unwrap().just_pressed("left_trigger"));
        frame(&mut state, vec![PadEvent::Button(0, trigger, between)]);
        assert!(
            state.pad(0).unwrap().is_down("left_trigger"),
            "held above release"
        );
        frame(
            &mut state,
            vec![PadEvent::Button(0, trigger, settings.release / 2.0)],
        );
        let pad = state.pad(0).unwrap();
        assert!(pad.just_released("left_trigger"));
        assert!(close(pad.pressure("left_trigger"), settings.release / 2.0));
    }

    #[test]
    fn a_stick_inside_the_deadzone_reads_zero_and_outside_is_rescaled() {
        assert_eq!(radial(0.1, 0.1, 0.2), (0.0, 0.0));
        let (x, y) = radial(0.6, 0.0, 0.2);
        assert!(close(x, 0.5) && close(y, 0.0));
        let (x, y) = radial(1.0, 1.0, 0.2);
        assert!(
            close(libm::hypotf(x, y), 1.0),
            "a full diagonal is full, not more"
        );
        assert!(close(along(0.6, 0.2), 0.5));
        assert!(close(along(0.1, 0.2), 0.0));
    }

    #[test]
    fn a_held_button_repeats_after_the_delay_at_the_interval() {
        assert!(!repeats_between(0.3, 0.4, 0.5, 0.1));
        assert!(repeats_between(0.45, 0.5, 0.5, 0.1));
        assert!(!repeats_between(0.51, 0.55, 0.5, 0.1));
        assert!(repeats_between(0.58, 0.61, 0.5, 0.1));
    }

    #[test]
    fn a_press_counts_as_a_repeat_and_holding_does_not_until_the_delay() {
        let mut state = connected(0);
        frame(&mut state, vec![PadEvent::Button(0, south(), 1.0)]);
        assert!(state.pad(0).unwrap().is_repeated("south"));
        frame(&mut state, Vec::new());
        assert!(!state.pad(0).unwrap().is_repeated("south"));
    }

    #[test]
    fn identity_power_and_pressure_round_trip_through_a_recording() {
        let mut state = connected(0);
        let power = Power {
            state: PowerState::OnBattery,
            level: 0.5,
        };
        frame(
            &mut state,
            vec![PadEvent::Button(0, south(), 1.0), PadEvent::Power(0, power)],
        );
        let motion = Motion {
            gyro: [0.5, -1.5, 0.25],
            acceleration: [0.0, 1.0, 0.0],
        };
        state.set_motion(0, motion);
        let touch = PadTouch {
            id: 7,
            x: 0.25,
            y: 0.75,
        };
        state.set_touchpad(0, vec![touch]);

        let mut replayed = GamepadState::default();
        restore(&mut replayed, &capture(&state));
        let pad = replayed.pad(0).expect("the pad came back");
        assert_eq!(pad.name(), "Test Pad");
        assert!(pad.can_rumble() && pad.is_repeated("south"));
        assert!(close(pad.pressure("south"), 1.0));
        assert_eq!(pad.power(), power);
        assert_eq!(pad.motion(), motion);
        assert_eq!(pad.touches(), [touch]);
    }

    /// The fields are `#[serde(default)]` for this: without it the whole
    /// snapshot fails to parse and the tick replays with no pads at all.
    #[test]
    fn a_recording_made_before_motion_existed_still_replays() {
        let older = serde_json::json!([{
            "id": 3,
            "name": "Older Pad",
            "down": ["south"],
            "just_pressed": [],
            "just_released": [],
            "axes": [["left_x", 0.5]],
        }]);
        let mut state = GamepadState::default();
        restore(&mut state, &older);

        let pad = state.pad(3).expect("the pad still restored");
        assert!(pad.is_down("south") && pad.name() == "Older Pad");
        assert!(close(pad.axis("left_x"), 0.5));
        assert_eq!(pad.motion(), Motion::default());
        assert_eq!(pad.power(), Power::default());
        assert!(!pad.can_rumble());
    }

    /// The neutral-answer rule: a pad that is not there reads zero rather
    /// than failing, so the same script runs headless.
    #[test]
    fn an_absent_pad_reads_neutral_and_cannot_be_written() {
        let mut state = connected(0);
        let spin = Motion {
            gyro: [9.0; 3],
            acceleration: [9.0; 3],
        };
        state.set_motion(1, spin);
        frame(&mut state, vec![PadEvent::Button(1, south(), 1.0)]);
        assert!(state.pad(1).is_none(), "writing did not invent a pad");
        assert_eq!(state.pad(0).unwrap().motion(), Motion::default());
    }
}
