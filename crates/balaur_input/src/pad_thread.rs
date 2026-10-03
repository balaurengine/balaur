//! The thread gamepads are read on, so a controller service that stops
//! answering cannot stop the game.
//!
//! Windows Gaming Input has been seen to block in `FromGameController` after
//! the machine wakes from sleep with a pad plugged in, until the pad is pulled
//! out (gilrs issue 196). gilrs makes that call on whichever thread opens it or
//! asks it for events, so both happen here and never on the tick: a backend
//! that hangs leaves the pads where they were, and every frame still runs.
//!
//! The thread sleeps in the backend until a pad changes, then hands the
//! changes to the [`Hub`]. The hub gives each pad its player slot, drops
//! changes too small to be anything but noise, and copies the rest into every
//! engine's [`Queue`]; a tick takes its queue when it begins and never waits
//! for it. The hub also holds each pad's motors, which the thread builds when
//! the pad connects: driving them only queues messages, so a rumble from the
//! tick never reaches into gilrs.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError, Weak};

use crate::gamepad::{BUTTON_COUNT, PadEvent, PadInfo, Power, STICKS};
use crate::haptics::Motors;

/// An axis or pressure change smaller than this is sensor noise: forwarding
/// it would wake the loop over a resting stick.
const NOISE: f32 = 0.01;

/// What a backend saw between two waits, oldest first, by its own pad keys,
/// with the motors of the pads that connected.
#[derive(Default)]
pub(crate) struct Batch {
    pub(crate) events: Vec<PadEvent>,
    pub(crate) motors: Vec<(i64, Box<dyn Motors>)>,
}

/// What the thread reads pads from: gilrs in a build, a stand-in in a test.
pub(crate) trait Backend {
    /// Sleep until a pad changes. Empty when nothing did after all: now and
    /// then an event the backend dropped, every time a backend that stopped.
    fn wait(&mut self) -> Batch;
}

/// One engine's end: the changes since its tick last took them.
#[derive(Default)]
pub(crate) struct Queue {
    events: Mutex<Vec<PadEvent>>,
}

impl Queue {
    /// The changes so far, or `None` while the thread is mid-push: the tick
    /// takes them next frame rather than wait.
    pub(crate) fn try_take(&self) -> Option<Vec<PadEvent>> {
        match self.events.try_lock() {
            Ok(mut events) => Some(std::mem::take(&mut *events)),
            Err(TryLockError::Poisoned(poisoned)) => {
                Some(std::mem::take(&mut *poisoned.into_inner()))
            }
            Err(TryLockError::WouldBlock) => None,
        }
    }

    fn push(&self, events: &[PadEvent]) {
        lock(&self.events).extend_from_slice(events);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Between the thread and every engine reading pads. Its lock is held to
/// file a batch or drive a motor, never while the thread is in the backend.
#[derive(Default)]
pub(crate) struct Hub {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    slots: Vec<Slot>,
    pads: Vec<Current>,
    queues: Vec<Weak<Queue>>,
    motors: Vec<(i64, Box<dyn Motors>)>,
}

/// A player slot: the backend key holding it, and the model that last did.
struct Slot {
    key: Option<i64>,
    guid: String,
}

/// A connected pad as last forwarded, by slot.
struct Current {
    slot: i64,
    info: PadInfo,
    pressure: [f32; BUTTON_COUNT],
    sticks: [f32; STICKS],
    power: Power,
}

impl Hub {
    /// A new engine's queue, opening with every pad as it is now.
    pub(crate) fn subscribe(&self) -> Arc<Queue> {
        let queue = Arc::new(Queue::default());
        let mut inner = lock(&self.inner);
        let mut now = Vec::new();
        for pad in &inner.pads {
            now.push(PadEvent::Connected(pad.slot, Box::new(pad.info.clone())));
            for (button, value) in pad.pressure.iter().enumerate() {
                if *value > 0.0 {
                    now.push(PadEvent::Button(pad.slot, button, *value));
                }
            }
            for (axis, value) in pad.sticks.iter().enumerate() {
                if value.abs() > 0.0 {
                    now.push(PadEvent::Axis(pad.slot, axis, *value));
                }
            }
            if pad.power != Power::default() {
                now.push(PadEvent::Power(pad.slot, pad.power));
            }
        }
        queue.push(&now);
        inner.queues.push(Arc::downgrade(&queue));
        queue
    }

    /// Hold a pad's motors, as [`Motors::hold`]. A pad with none stays still.
    pub(crate) fn hold(&self, pad: i64, strong: f32, weak: f32, seconds: f32) {
        let mut inner = lock(&self.inner);
        if let Some((_, motors)) = inner.motors.iter_mut().find(|(slot, _)| *slot == pad) {
            motors.hold(strong, weak, seconds);
        }
    }

    /// Unplug every pad, for a backend that has stopped: a button it last
    /// saw held must not stay held for good.
    fn release_all(&self) -> bool {
        let keys: Vec<i64> = lock(&self.inner)
            .slots
            .iter()
            .filter_map(|slot| slot.key)
            .collect();
        let events = keys.into_iter().map(PadEvent::Disconnected).collect();
        self.publish(Batch {
            events,
            motors: Vec::new(),
        })
    }

    /// File a batch. True when any change reached an engine.
    fn publish(&self, batch: Batch) -> bool {
        let mut inner = lock(&self.inner);
        let mut out = Vec::new();
        for event in batch.events {
            inner.file(event, &mut out);
        }
        for (key, motors) in batch.motors {
            if let Some(slot) = inner.slot_of(key) {
                inner.motors.retain(|(held, _)| *held != slot);
                inner.motors.push((slot, motors));
            }
        }
        if out.is_empty() {
            return false;
        }
        inner.queues.retain(|queue| queue.strong_count() > 0);
        for queue in inner.queues.iter().filter_map(Weak::upgrade) {
            queue.push(&out);
        }
        true
    }
}

impl Inner {
    fn slot_of(&self, key: i64) -> Option<i64> {
        let i = self.slots.iter().position(|slot| slot.key == Some(key))?;
        i64::try_from(i).ok()
    }

    /// The lowest free slot last held by the same model, else the lowest
    /// free slot, so a pad pulled out and put back is the same player.
    fn assign(&mut self, key: i64, guid: &str) -> i64 {
        if let Some(slot) = self.slot_of(key) {
            return slot;
        }
        let free = |slot: &Slot| slot.key.is_none();
        let i = self
            .slots
            .iter()
            .position(|slot| free(slot) && slot.guid == guid)
            .or_else(|| self.slots.iter().position(free))
            .unwrap_or_else(|| {
                self.slots.push(Slot {
                    key: None,
                    guid: String::new(),
                });
                self.slots.len() - 1
            });
        self.slots[i] = Slot {
            key: Some(key),
            guid: guid.to_string(),
        };
        i64::try_from(i).unwrap_or(i64::MAX)
    }

    /// One change by backend key, forwarded by slot unless it is noise.
    fn file(&mut self, event: PadEvent, out: &mut Vec<PadEvent>) {
        let key = event.pad();
        if let PadEvent::Connected(_, info) = &event {
            let slot = self.assign(key, &info.guid);
            self.pads.retain(|pad| pad.slot != slot);
            self.pads.push(Current {
                slot,
                info: (**info).clone(),
                pressure: [0.0; BUTTON_COUNT],
                sticks: [0.0; STICKS],
                power: Power::default(),
            });
            out.push(event.for_pad(slot));
            return;
        }
        let Some(slot) = self.slot_of(key) else {
            return;
        };
        if let PadEvent::Disconnected(_) = event {
            for held in &mut self.slots {
                if held.key == Some(key) {
                    held.key = None;
                }
            }
            self.pads.retain(|pad| pad.slot != slot);
            self.motors.retain(|(held, _)| *held != slot);
            out.push(event.for_pad(slot));
            return;
        }
        let Some(pad) = self.pads.iter_mut().find(|pad| pad.slot == slot) else {
            return;
        };
        let forward = match &event {
            PadEvent::Info(_, info) => {
                pad.info = (**info).clone();
                true
            }
            PadEvent::Button(_, button, value) => pad
                .pressure
                .get_mut(*button)
                .is_some_and(|last| moved(last, *value)),
            PadEvent::Axis(_, axis, value) => pad
                .sticks
                .get_mut(*axis)
                .is_some_and(|last| moved(last, *value)),
            PadEvent::Power(_, power) => {
                let changed = pad.power != *power;
                pad.power = *power;
                changed
            }
            PadEvent::Connected(..) | PadEvent::Disconnected(_) => false,
        };
        if forward {
            out.push(event.for_pad(slot));
        }
    }
}

/// Whether a reading moved enough to forward, remembering it when it did. A
/// reading that lands on rest or full always goes, so a stick let go reads 0.
fn moved(last: &mut f32, value: f32) -> bool {
    let lands = value.abs() < f32::EPSILON || (1.0 - value.abs()).abs() < f32::EPSILON;
    let step = (value - *last).abs();
    if step >= NOISE || (lands && step > 0.0) {
        *last = value;
        true
    } else {
        false
    }
}

/// Waits in a row that come back with nothing before the backend counts as
/// stopped. A stopped one answers at once and forever, which would spin.
const STOPPED_AFTER: u32 = 64;

/// Open a backend on a thread of its own and hand back the hub it fills.
/// `open` runs on that thread too: opening is where a hang was first seen.
pub(crate) fn start<B: Backend>(open: impl FnOnce() -> Option<B> + Send + 'static) -> Arc<Hub> {
    let hub = Arc::new(Hub::default());
    let theirs = Arc::clone(&hub);
    let spawned = std::thread::Builder::new()
        .name("balaur-gamepads".into())
        .spawn(move || {
            if let Some(backend) = open() {
                run(backend, &theirs);
            }
        });
    if let Err(err) = spawned {
        tracing::warn!("gamepads disabled: {err}");
    }
    hub
}

/// The process's pads, read through gilrs. One thread for every engine in the
/// process, since a pad on the desk is the same pad to each of them, opened
/// with the mappings of the first engine to ask.
pub(crate) fn gilrs(mappings: &[String]) -> Arc<Hub> {
    static OPENED: OnceLock<(Arc<Hub>, Vec<String>)> = OnceLock::new();
    let (hub, opened_with) = OPENED.get_or_init(|| {
        let theirs = mappings.to_vec();
        let hub = start(move || gilrs_backend::GilrsBackend::open(&theirs));
        (hub, mappings.to_vec())
    });
    if opened_with != mappings && balaur_core::logbuf::first_time("gamepad mappings", "late") {
        tracing::info!(
            "gamepad_mappings load when the process first reads pads; these apply from this project's own run"
        );
    }
    Arc::clone(hub)
}

fn run(mut backend: impl Backend, hub: &Hub) {
    let mut empty = 0;
    while empty < STOPPED_AFTER {
        let batch = backend.wait();
        if batch.events.is_empty() && batch.motors.is_empty() {
            empty += 1;
            continue;
        }
        empty = 0;
        if hub.publish(batch) {
            balaur_core::wake::wake();
        }
    }
    tracing::warn!("gamepads stopped: their backend no longer answers");
    if hub.release_all() {
        balaur_core::wake::wake();
    }
}

mod gilrs_backend {
    use gilrs::{Axis, Button, Event, EventType, GamepadId, Gilrs, GilrsBuilder};

    use super::{Backend, Batch};
    use crate::gamepad::{PAD_BUTTON_NAMES, PadEvent, PadInfo, Power, PowerState, STICKS};
    use crate::haptics::GilrsMotors;
    use crate::vocabulary::words as w;

    /// The gilrs button behind each of [`PAD_BUTTON_NAMES`], in the same
    /// order, so the two lists cannot drift apart silently (the test below
    /// says so).
    pub(super) const BUTTONS: &[(&str, Button)] = &[
        ("south", Button::South),
        ("east", Button::East),
        ("north", Button::North),
        ("west", Button::West),
        ("left_shoulder", Button::LeftTrigger),
        ("left_trigger", Button::LeftTrigger2),
        ("right_shoulder", Button::RightTrigger),
        ("right_trigger", Button::RightTrigger2),
        ("back", Button::Select),
        ("start", Button::Start),
        ("guide", Button::Mode),
        ("left_stick", Button::LeftThumb),
        ("right_stick", Button::RightThumb),
        ("dpad_up", Button::DPadUp),
        ("dpad_down", Button::DPadDown),
        ("dpad_left", Button::DPadLeft),
        ("dpad_right", Button::DPadRight),
        ("c", Button::C),
        ("z", Button::Z),
    ];

    /// The gilrs axis behind each stick axis, in `PAD_AXIS_NAMES` order.
    pub(super) const STICK_AXES: [Axis; STICKS] = [
        Axis::LeftStickX,
        Axis::LeftStickY,
        Axis::RightStickX,
        Axis::RightStickY,
    ];

    pub(super) struct GilrsBackend {
        gilrs: Gilrs,
        opened: bool,
        /// Each pad's power as last reported, so only a change is.
        power: Vec<(GamepadId, Power)>,
    }

    impl GilrsBackend {
        /// gilrs's own filters are off: the deadzone, the jitter cut and the
        /// trigger thresholds are the engine's. gilrs cannot open its platform
        /// backend everywhere (no udev in a bare container, no backend at all
        /// on mobile); the same game runs, with no pads.
        pub(super) fn open(mappings: &[String]) -> Option<Self> {
            let built = GilrsBuilder::new()
                .with_default_filters(false)
                .add_mappings(&mappings.join("\n"))
                .build();
            match built {
                Ok(gilrs) => Some(Self {
                    gilrs,
                    opened: false,
                    power: Vec::new(),
                }),
                // The dummy backend (iOS, Android): expected, not news.
                Err(gilrs::Error::NotImplemented(_)) => None,
                Err(err) => {
                    tracing::warn!("gamepads disabled: {err}");
                    None
                }
            }
        }

        fn connect(&mut self, id: GamepadId, batch: &mut Batch) {
            let key = key_of(id);
            let motors = if self.gilrs.gamepad(id).is_ff_supported() {
                GilrsMotors::build(&mut self.gilrs, id)
            } else {
                None
            };
            let gamepad = self.gilrs.gamepad(id);
            let mapping = match gamepad.mapping_source() {
                gilrs::MappingSource::SdlMappings => w::MAPPING_SDL,
                _ => w::MAPPING_DRIVER,
            };
            let info = PadInfo {
                name: gamepad.name().to_string(),
                os_name: gamepad.os_name().to_string(),
                guid: hex(&gamepad.uuid()),
                vendor: gamepad.vendor_id().unwrap_or_default(),
                product: gamepad.product_id().unwrap_or_default(),
                mapping: mapping.to_string(),
                rumble: motors.is_some(),
            };
            batch.events.push(PadEvent::Connected(key, Box::new(info)));
            if let Some(motors) = motors {
                batch.motors.push((key, Box::new(motors)));
            }
            for (i, (_, button)) in BUTTONS.iter().enumerate() {
                let value = gamepad
                    .button_data(*button)
                    .map_or(0.0, gilrs::ev::state::ButtonData::value);
                if value > 0.0 {
                    batch.events.push(PadEvent::Button(key, i, value));
                }
            }
            for (i, axis) in STICK_AXES.iter().enumerate() {
                let value = gamepad
                    .axis_data(*axis)
                    .map_or(0.0, gilrs::ev::state::AxisData::value);
                if value.abs() > 0.0 {
                    batch.events.push(PadEvent::Axis(key, i, value));
                }
            }
            for axis in [Axis::DPadX, Axis::DPadY] {
                if let Some(data) = gamepad.axis_data(axis) {
                    self.dpad(id, axis, data.value(), batch);
                }
            }
        }

        fn translate(&mut self, event: Event, batch: &mut Batch) {
            let key = key_of(event.id);
            match event.event {
                EventType::Connected => self.connect(event.id, batch),
                EventType::Disconnected => {
                    batch.events.push(PadEvent::Disconnected(key));
                    self.power.retain(|(id, _)| *id != event.id);
                }
                EventType::ButtonChanged(button, value, _) => {
                    if let Some(i) = BUTTONS.iter().position(|(_, known)| *known == button) {
                        batch.events.push(PadEvent::Button(key, i, value));
                    }
                }
                EventType::AxisChanged(axis, value, _) => {
                    match STICK_AXES.iter().position(|known| *known == axis) {
                        Some(i) => batch.events.push(PadEvent::Axis(key, i, value)),
                        None => self.dpad(event.id, axis, value, batch),
                    }
                }
                _ => {}
            }
        }

        /// A d-pad some pads report as two axes, read as its four buttons,
        /// unless the pad has d-pad buttons of its own: gilrs's own rule.
        fn dpad(&self, id: GamepadId, axis: Axis, value: f32, batch: &mut Batch) {
            if self
                .gilrs
                .gamepad(id)
                .button_code(Button::DPadRight)
                .is_some()
            {
                return;
            }
            let (negative, positive) = match axis {
                Axis::DPadX => ("dpad_left", "dpad_right"),
                Axis::DPadY => ("dpad_down", "dpad_up"),
                _ => return,
            };
            for (name, on) in [(negative, value <= -0.5), (positive, value >= 0.5)] {
                if let Some(i) = PAD_BUTTON_NAMES.iter().position(|known| *known == name) {
                    let pressure = f32::from(u8::from(on));
                    batch.events.push(PadEvent::Button(key_of(id), i, pressure));
                }
            }
        }

        /// Power is not an event in gilrs, so it is read whenever a pad sends
        /// anything.
        fn read_power(&mut self, batch: &mut Batch) {
            let now: Vec<(GamepadId, Power)> = self
                .gilrs
                .gamepads()
                .map(|(id, gamepad)| (id, power_of(gamepad.power_info())))
                .collect();
            for (id, power) in now {
                let last = self.power.iter().find(|(known, _)| *known == id);
                if last.map(|(_, last)| *last) != Some(power) {
                    self.power.retain(|(known, _)| *known != id);
                    self.power.push((id, power));
                    batch.events.push(PadEvent::Power(key_of(id), power));
                }
            }
        }
    }

    impl Backend for GilrsBackend {
        fn wait(&mut self) -> Batch {
            let mut batch = Batch::default();
            if !self.opened {
                self.opened = true;
                let ids: Vec<GamepadId> = self.gilrs.gamepads().map(|(id, _)| id).collect();
                for id in ids {
                    self.connect(id, &mut batch);
                }
                self.read_power(&mut batch);
                return batch;
            }
            let Some(first) = self.gilrs.next_event_blocking(None) else {
                return batch;
            };
            self.translate(first, &mut batch);
            while let Some(event) = self.gilrs.next_event() {
                self.translate(event, &mut batch);
            }
            self.read_power(&mut batch);
            batch
        }
    }

    /// SDL's spelling of a GUID: each byte as two hex digits, in order.
    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
    }

    fn key_of(id: GamepadId) -> i64 {
        i64::try_from(usize::from(id)).unwrap_or(i64::MAX)
    }

    fn power_of(info: gilrs::PowerInfo) -> Power {
        let (state, level) = match info {
            gilrs::PowerInfo::Unknown => (PowerState::Unknown, -1.0),
            gilrs::PowerInfo::Wired => (PowerState::NoBattery, -1.0),
            gilrs::PowerInfo::Discharging(percent) => {
                (PowerState::OnBattery, f32::from(percent) / 100.0)
            }
            gilrs::PowerInfo::Charging(percent) => {
                (PowerState::Charging, f32::from(percent) / 100.0)
            }
            gilrs::PowerInfo::Charged => (PowerState::Charged, 1.0),
        };
        Power { state, level }
    }

    #[cfg(test)]
    mod tests {
        use super::BUTTONS;
        use crate::gamepad::PAD_BUTTON_NAMES;

        #[test]
        fn the_pad_vocabulary_and_the_gilrs_mapping_agree() {
            let names: Vec<&str> = BUTTONS.iter().map(|(name, _)| *name).collect();
            assert_eq!(names, PAD_BUTTON_NAMES);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::{Backend, Batch, Hub, STOPPED_AFTER, run, start};
    use crate::GamepadState;
    use crate::gamepad::{PadEvent, PadInfo, button_index};
    use crate::haptics::Motors;
    use crate::settings::PadSettings;

    const DT: f32 = 1.0 / 60.0;

    fn connect(key: i64, guid: &str) -> PadEvent {
        let info = PadInfo {
            name: "Stand-in Pad".into(),
            guid: guid.into(),
            ..PadInfo::default()
        };
        PadEvent::Connected(key, Box::new(info))
    }

    fn batch(events: Vec<PadEvent>) -> Batch {
        Batch {
            events,
            motors: Vec::new(),
        }
    }

    fn south(key: i64, value: f32) -> PadEvent {
        PadEvent::Button(key, button_index("south").unwrap(), value)
    }

    /// Hands over whatever the test sends; with the sender gone it blocks for
    /// good, as a hung controller service does.
    struct Fed(Receiver<Batch>);

    impl Backend for Fed {
        fn wait(&mut self) -> Batch {
            if let Ok(batch) = self.0.recv() {
                return batch;
            }
            let (_held, never) = channel::<()>();
            let _ = never.recv();
            Batch::default()
        }
    }

    fn fed() -> (Sender<Batch>, Arc<Hub>) {
        let (feed, next) = channel();
        (feed, start(move || Some(Fed(next))))
    }

    /// Polls until `ready` holds, as a frame loop would.
    #[allow(clippy::disallowed_methods, reason = "a test's deadline")]
    fn poll_until(state: &mut GamepadState, ready: impl Fn(&GamepadState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            state.poll(DT, &PadSettings::default());
            if ready(state) {
                return;
            }
            assert!(Instant::now() < deadline, "the thread never published");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_tap_the_thread_reads_between_two_ticks_is_both_edges_next_tick() {
        let (feed, hub) = fed();
        let mut state = GamepadState::reading_from(hub);
        feed.send(batch(vec![connect(7, "a")])).unwrap();
        poll_until(&mut state, |s| s.pad(0).is_some());

        feed.send(batch(vec![south(7, 1.0), south(7, 0.0)]))
            .unwrap();
        poll_until(&mut state, |s| {
            s.pad(0).is_some_and(|p| p.just_released("south"))
        });
        let pad = state.pad(0).unwrap();
        assert!(pad.just_pressed("south") && !pad.is_down("south"));

        feed.send(batch(vec![PadEvent::Disconnected(7)])).unwrap();
        poll_until(&mut state, |s| s.pads().is_empty());
    }

    #[test]
    fn a_backend_that_hangs_leaves_the_last_pads_and_every_tick_running() {
        let (feed, hub) = fed();
        let mut state = GamepadState::reading_from(hub);
        feed.send(batch(vec![connect(0, "a"), south(0, 1.0)]))
            .unwrap();
        poll_until(&mut state, |s| s.pad(0).is_some_and(|p| p.is_down("south")));
        // With its sender gone, the stand-in blocks in its next wait for good.
        drop(feed);

        for _ in 0..3 {
            state.poll(DT, &PadSettings::default());
        }
        let pad = state.pad(0).expect("the last reading stays");
        assert!(pad.is_down("south") && !pad.just_pressed("south"));
    }

    #[test]
    fn a_backend_that_never_opens_leaves_no_pads_and_every_tick_running() {
        let hub = start(|| {
            let (_held, never) = channel::<()>();
            let _ = never.recv();
            None::<Fed>
        });
        let mut state = GamepadState::reading_from(hub);
        for _ in 0..3 {
            state.poll(DT, &PadSettings::default());
        }
        assert!(state.pads().is_empty());
    }

    /// Answers nothing, at once, the way a backend whose service went away does.
    struct Stopped(Arc<AtomicU32>);

    impl Backend for Stopped {
        fn wait(&mut self) -> Batch {
            self.0.fetch_add(1, Ordering::SeqCst);
            Batch::default()
        }
    }

    #[test]
    fn a_backend_that_stops_ends_its_thread_and_lets_go_of_its_pads() {
        let hub = Arc::new(Hub::default());
        hub.publish(batch(vec![connect(3, "a"), south(3, 1.0)]));
        let mut state = GamepadState::reading_from(Arc::clone(&hub));
        state.poll(DT, &PadSettings::default());
        assert!(state.pad(0).is_some_and(|p| p.is_down("south")));

        let waits = Arc::new(AtomicU32::new(0));
        run(Stopped(Arc::clone(&waits)), &hub);
        assert_eq!(waits.load(Ordering::SeqCst), STOPPED_AFTER, "no spin");
        state.poll(DT, &PadSettings::default());
        assert!(
            state.pads().is_empty(),
            "a held button is not held for good"
        );
    }

    fn slots(hub: &Hub) -> Vec<i64> {
        let mut pads: Vec<i64> = super::lock(&hub.inner)
            .pads
            .iter()
            .map(|p| p.slot)
            .collect();
        pads.sort_unstable();
        pads
    }

    #[test]
    fn a_pad_put_back_takes_its_slot_again_and_a_newcomer_the_lowest_free() {
        let hub = Hub::default();
        hub.publish(batch(vec![connect(10, "pad-a"), connect(11, "pad-b")]));
        assert_eq!(slots(&hub), [0, 1]);
        hub.publish(batch(vec![
            PadEvent::Disconnected(10),
            PadEvent::Disconnected(11),
        ]));
        hub.publish(batch(vec![connect(12, "pad-b")]));
        assert_eq!(slots(&hub), [1], "the same model takes back slot 1");
        hub.publish(batch(vec![connect(13, "pad-c")]));
        assert_eq!(slots(&hub), [0, 1], "another model takes the lowest free");
    }

    #[test]
    fn a_change_too_small_to_be_anything_but_noise_reaches_no_engine() {
        let hub = Hub::default();
        let queue = hub.subscribe();
        let stick = |value| PadEvent::Axis(0, 0, value);
        hub.publish(batch(vec![connect(0, "a"), stick(0.5)]));
        queue.try_take();
        assert!(
            !hub.publish(batch(vec![stick(0.505)])),
            "noise wakes nothing"
        );
        assert!(hub.publish(batch(vec![stick(0.52)])));
        assert!(hub.publish(batch(vec![stick(0.0)])), "rest always lands");
        assert_eq!(queue.try_take().unwrap(), [stick(0.52), stick(0.0)]);
    }

    #[test]
    fn an_engine_that_starts_late_hears_every_pad_as_it_is_now() {
        let hub = Arc::new(Hub::default());
        hub.publish(batch(vec![connect(5, "a"), south(5, 1.0)]));
        let mut state = GamepadState::reading_from(hub);
        state.poll(DT, &PadSettings::default());
        assert!(state.pad(0).is_some_and(|p| p.is_down("south")));
    }

    /// Records what it is told to hold.
    struct Recorded(Arc<Mutex<Vec<(f32, f32)>>>);

    impl Motors for Recorded {
        fn hold(&mut self, strong: f32, weak: f32, _seconds: f32) {
            self.0.lock().unwrap().push((strong, weak));
        }
    }

    #[test]
    fn a_pad_s_motors_answer_to_its_slot_and_leave_with_it() {
        let hub = Hub::default();
        let held = Arc::new(Mutex::new(Vec::new()));
        let motors: Box<dyn Motors> = Box::new(Recorded(Arc::clone(&held)));
        hub.publish(Batch {
            events: vec![connect(42, "a")],
            motors: vec![(42, motors)],
        });
        hub.hold(0, 0.5, 0.25, 1.0);
        hub.publish(batch(vec![PadEvent::Disconnected(42)]));
        hub.hold(0, 1.0, 1.0, 1.0);
        assert_eq!(*held.lock().unwrap(), [(0.5, 0.25)]);
    }
}
