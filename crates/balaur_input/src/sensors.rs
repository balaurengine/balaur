//! Motion and touchpad, read straight from a PlayStation pad's HID reports.
//!
//! gilrs reports buttons and axes and nothing else — it has no notion of a
//! gyroscope or a touchpad — so the sensors come from a second, narrower
//! reader that opens the same pad over raw HID and decodes the report gilrs
//! throws away. gilrs stays the source of truth for everything it does cover;
//! this only ever fills [`crate::gamepad::Motion`] and the touch list.
//!
//! Report layouts are the ones Linux's `hid-playstation.c` driver documents,
//! which is why the offsets below are stated as constants rather than derived:
//! they are a wire format, not a decision.
//!
//! Desktop only. Android and iOS have no hidraw to open and wasm has no
//! devices at all, so there the reader is a stub and every pad reads zero —
//! the same neutral answer an absent pad gives.

use crate::gamepad::{Motion, PadTouch};

/// Sony, and the pads of theirs that carry a gyroscope and a touchpad.
pub(crate) const SONY: u16 = 0x054C;
const DUALSHOCK4_V1: u16 = 0x05C4;
const DUALSHOCK4_V2: u16 = 0x09CC;
const DUALSENSE: u16 = 0x0CE6;
const DUALSENSE_EDGE: u16 = 0x0DF2;

/// Raw counts per g and per degree per second: the nominal figures, good to a
/// few percent. Each pad also ships a calibration report trimming them to the
/// unit, which is a refinement nobody can check without holding one.
const ACCEL_PER_G: f32 = 8192.0;
const GYRO_PER_DEG: f32 = 1024.0;

/// Where the sensors sit in one report kind, as absolute byte offsets into the
/// buffer the pad sends — report id included, so there is nothing to add.
pub(crate) struct Layout {
    report_id: u8,
    len: usize,
    gyro: usize,
    accel: usize,
    touch: usize,
    width: f32,
    height: f32,
}

/// DualSense over USB (report 1) and Bluetooth (report 0x31), which shifts
/// every field by the one padding byte in front of the payload.
const DUALSENSE_LAYOUTS: &[Layout] = &[
    Layout {
        report_id: 0x01,
        len: 64,
        gyro: 16,
        accel: 22,
        touch: 33,
        width: 1920.0,
        height: 1080.0,
    },
    Layout {
        report_id: 0x31,
        len: 78,
        gyro: 17,
        accel: 23,
        touch: 34,
        width: 1920.0,
        height: 1080.0,
    },
];

/// DualShock 4 over USB (report 1) and Bluetooth (report 0x11). Its touch
/// points sit behind a report count and a timestamp, not directly in the body.
const DUALSHOCK4_LAYOUTS: &[Layout] = &[
    Layout {
        report_id: 0x01,
        len: 64,
        gyro: 13,
        accel: 19,
        touch: 35,
        width: 1920.0,
        height: 942.0,
    },
    Layout {
        report_id: 0x11,
        len: 78,
        gyro: 15,
        accel: 21,
        touch: 37,
        width: 1920.0,
        height: 942.0,
    },
];

/// Whether this reader knows how to decode the pad's sensors at all.
pub(crate) fn layouts(vendor: u16, product: u16) -> Option<&'static [Layout]> {
    if vendor != SONY {
        return None;
    }
    match product {
        DUALSENSE | DUALSENSE_EDGE => Some(DUALSENSE_LAYOUTS),
        DUALSHOCK4_V1 | DUALSHOCK4_V2 => Some(DUALSHOCK4_LAYOUTS),
        _ => None,
    }
}

/// One report's worth of sensors.
#[derive(Clone)]
pub(crate) struct Reading {
    pub(crate) motion: Motion,
    pub(crate) touches: Vec<PadTouch>,
}

/// Decode a report against the first layout whose id and length it matches.
/// `None` for anything else: a pad sends other reports and a short read is not
/// a reading.
pub(crate) fn decode(report: &[u8], layouts: &'static [Layout]) -> Option<Reading> {
    let layout = layouts
        .iter()
        .find(|l| report.first() == Some(&l.report_id) && report.len() >= l.len)?;
    let axes = |at: usize, per_unit: f32| {
        [0, 1, 2].map(|i| f32::from(le16(report, at + i * 2)) / per_unit)
    };
    let gyro = axes(layout.gyro, GYRO_PER_DEG).map(f32::to_radians);
    Some(Reading {
        motion: Motion {
            gyro,
            acceleration: axes(layout.accel, ACCEL_PER_G),
        },
        touches: touches(report, layout),
    })
}

/// The two touch slots, keeping only the fingers actually down.
fn touches(report: &[u8], layout: &Layout) -> Vec<PadTouch> {
    (0..2)
        .filter_map(|slot| {
            let at = layout.touch + slot * 4;
            let point: [u8; 4] = report.get(at..at + 4)?.try_into().ok()?;
            // Bit 7 of the contact byte marks the slot *empty*; the rest is
            // the id the pad gives a finger for as long as it stays down.
            if point[0] & 0x80 != 0 {
                return None;
            }
            let x = u16::from(point[2] & 0x0F) << 8 | u16::from(point[1]);
            let y = u16::from(point[3]) << 4 | u16::from(point[2] >> 4);
            Some(PadTouch {
                id: i64::from(point[0] & 0x7F),
                x: f32::from(x) / layout.width,
                y: f32::from(y) / layout.height,
            })
        })
        .collect()
}

/// A signed little-endian pair, which is how every sensor axis is sent.
fn le16(report: &[u8], at: usize) -> i16 {
    let lo = report.get(at).copied().unwrap_or(0);
    let hi = report.get(at + 1).copied().unwrap_or(0);
    i16::from_le_bytes([lo, hi])
}

/// The desktop reader: hidraw on Linux, IOKit on macOS, hid.dll on Windows.
///
/// Nothing here runs on the tick. A manager thread sleeps until the set of
/// pads changes, then opens each pad's HID device; one reader thread per
/// device sleeps in a blocking read until the pad sends a report, and keeps
/// the newest decoded reading. The tick copies those readings and never
/// waits for them: a pad that stops answering stops its own reader, not the
/// game.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod reader {
    use std::ffi::{CStr, CString};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

    use super::{Layout, Reading, decode, layouts};

    /// The longest report any pad here sends (both Bluetooth layouts).
    const MAX_REPORT: usize = 78;

    /// Generic Desktop / Gamepad. Windows and macOS enumerate one entry per
    /// top-level collection, so without this a pad opens on the wrong one.
    /// Linux's hidraw reports no usage at all and leaves the pair zero.
    const GAMEPAD_USAGE: (u16, u16) = (0x01, 0x05);

    /// A pad by model and by its place among pads of that model, the order
    /// the snapshot lists them in: how twins are told apart.
    type Key = (u16, u16, usize);

    /// Where a manager finds and opens devices: hidapi in a build, a stand-in
    /// in a test.
    pub(crate) trait Hid {
        /// The paths of every gamepad of this model, in the OS's order.
        fn paths(&mut self, vendor: u16, product: u16) -> Vec<CString>;
        fn open(&mut self, path: &CStr) -> Option<Box<dyn Device>>;
    }

    /// One opened device, read on a thread of its own.
    pub(crate) trait Device: Send {
        /// Sleep until the pad sends a report and copy it in; `None` once the
        /// device is gone.
        fn read(&mut self, buf: &mut [u8]) -> Option<usize>;
    }

    /// Between the threads and the tick.
    #[derive(Default)]
    struct Shared {
        state: Mutex<State>,
        /// Cleared when the tick's side drops, so every reader ends at its
        /// next report rather than reading for an engine that is gone.
        alive: AtomicBool,
    }

    #[derive(Default)]
    struct State {
        /// Which open device answers for which pad.
        assigned: Vec<(Key, CString)>,
        /// Every running reader's device and its newest reading.
        readings: Vec<(CString, Option<Reading>)>,
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The tick's side.
    #[derive(Default)]
    pub(crate) struct Sensors {
        link: Option<Link>,
        /// The pads last handed to the manager, so it only hears of a change.
        wanted: Vec<(u16, u16)>,
        /// The readings as last copied, kept when the threads are mid-write.
        copied: Vec<(Key, Reading)>,
    }

    struct Link {
        shared: Arc<Shared>,
        wants: Sender<Vec<(u16, u16)>>,
    }

    impl Drop for Link {
        fn drop(&mut self) {
            self.shared.alive.store(false, Ordering::Release);
        }
    }

    impl Sensors {
        /// Tell the manager which pads are connected, in the order the
        /// snapshot lists them, and copy whatever they have sent.
        pub(crate) fn poll(&mut self, pads: &[(u16, u16)]) {
            self.poll_with(pads, HidApiSource::open);
        }

        fn poll_with<H: Hid + 'static>(&mut self, pads: &[(u16, u16)], open: fn() -> Option<H>) {
            let want: Vec<(u16, u16)> = pads
                .iter()
                .copied()
                .filter(|(vendor, product)| layouts(*vendor, *product).is_some())
                .collect();
            if want != self.wanted {
                self.wanted.clone_from(&want);
                if self.link.is_some() || !want.is_empty() {
                    let link = self.link.get_or_insert_with(|| Link::start(open));
                    let _ = link.wants.send(want);
                }
            }
            let Some(link) = &self.link else {
                return;
            };
            if let Ok(state) = link.shared.state.try_lock() {
                self.copied = state
                    .assigned
                    .iter()
                    .filter_map(|(key, path)| {
                        let (_, reading) = state.readings.iter().find(|(open, _)| open == path)?;
                        Some((*key, reading.clone()?))
                    })
                    .collect();
            }
        }

        /// The `nth` pad with this vendor and product, matching the snapshot's
        /// nth such pad. Two identical controllers stay told apart by order.
        pub(crate) fn reading(&self, vendor: u16, product: u16, nth: usize) -> Option<&Reading> {
            self.copied
                .iter()
                .find(|(key, _)| *key == (vendor, product, nth))
                .map(|(_, reading)| reading)
        }
    }

    impl Link {
        fn start<H: Hid + 'static>(open: fn() -> Option<H>) -> Self {
            let shared = Arc::new(Shared::default());
            shared.alive.store(true, Ordering::Release);
            // Only the pad set goes in, and the poll that sends it is skipped
            // while a recording plays, so no device is opened during a replay.
            let (wants, receiver) = channel();
            let theirs = Arc::clone(&shared);
            let spawned = std::thread::Builder::new()
                .name("balaur-pad-sensors".into())
                .spawn(move || {
                    if let Some(hid) = open() {
                        manage(hid, &receiver, &theirs);
                    }
                });
            if let Err(err) = spawned {
                tracing::warn!("pad motion and touchpad disabled: {err}");
            }
            Self { shared, wants }
        }
    }

    /// Sleep until the pads change, then open whatever is newly wanted. Ends
    /// when the tick's side drops its sender.
    fn manage(mut hid: impl Hid, wants: &Receiver<Vec<(u16, u16)>>, shared: &Arc<Shared>) {
        while let Ok(mut want) = wants.recv() {
            // Only the newest set matters when several queued up.
            while let Ok(newer) = wants.try_recv() {
                want = newer;
            }
            assign(&mut hid, &want, shared);
        }
    }

    fn assign(hid: &mut impl Hid, want: &[(u16, u16)], shared: &Arc<Shared>) {
        let mut assigned = Vec::new();
        for (i, (vendor, product)) in want.iter().enumerate() {
            let nth = want[..i].iter().filter(|pair| **pair == want[i]).count();
            let Some(layouts) = layouts(*vendor, *product) else {
                continue;
            };
            let paths = hid.paths(*vendor, *product);
            let Some(path) = paths.get(nth) else {
                continue;
            };
            let reading = lock(&shared.state)
                .readings
                .iter()
                .any(|(open, _)| open == path);
            if !reading {
                // A pad the OS will not hand over (no hidraw rule on Linux)
                // simply reports no motion, as an absent one does.
                let Some(device) = hid.open(path) else {
                    tracing::debug!(vendor, product, "pad sensors: could not open");
                    continue;
                };
                lock(&shared.state).readings.push((path.clone(), None));
                spawn_reader(device, path.clone(), layouts, shared);
            }
            assigned.push(((*vendor, *product, nth), path.clone()));
        }
        lock(&shared.state).assigned = assigned;
    }

    fn spawn_reader(
        device: Box<dyn Device>,
        path: CString,
        layouts: &'static [Layout],
        shared: &Arc<Shared>,
    ) {
        let theirs = Arc::clone(shared);
        let gone = path.clone();
        let spawned = std::thread::Builder::new()
            .name("balaur-pad-sensor".into())
            .spawn(move || read(device, &path, layouts, &theirs));
        if let Err(err) = spawned {
            tracing::debug!("pad sensors: {err}");
            lock(&shared.state)
                .readings
                .retain(|(open, _)| *open != gone);
        }
    }

    /// Keep the newest reading until the device goes away or the engine does.
    fn read(mut device: Box<dyn Device>, path: &CStr, layouts: &'static [Layout], shared: &Shared) {
        let mut buf = [0u8; MAX_REPORT];
        while shared.alive.load(Ordering::Acquire) {
            let Some(len) = device.read(&mut buf) else {
                break;
            };
            if let Some(reading) = decode(&buf[..len], layouts) {
                let mut state = lock(&shared.state);
                if let Some((_, newest)) =
                    state.readings.iter_mut().find(|(open, _)| **open == *path)
                {
                    *newest = Some(reading);
                }
            }
        }
        lock(&shared.state)
            .readings
            .retain(|(open, _)| **open != *path);
    }

    /// hidapi, opened on the manager thread: enumerating is a device call.
    struct HidApiSource(hidapi::HidApi);

    impl HidApiSource {
        fn open() -> Option<Self> {
            match hidapi::HidApi::new() {
                Ok(api) => Some(Self(api)),
                Err(err) => {
                    tracing::warn!("pad motion and touchpad disabled: {err}");
                    None
                }
            }
        }
    }

    impl Hid for HidApiSource {
        fn paths(&mut self, vendor: u16, product: u16) -> Vec<CString> {
            if let Err(err) = self.0.refresh_devices() {
                tracing::debug!("pad sensors: {err}");
                return Vec::new();
            }
            self.0
                .device_list()
                .filter(|dev| dev.vendor_id() == vendor && dev.product_id() == product)
                .filter(|dev| {
                    dev.usage_page() == 0 || (dev.usage_page(), dev.usage()) == GAMEPAD_USAGE
                })
                .map(|dev| dev.path().to_owned())
                .collect()
        }

        fn open(&mut self, path: &CStr) -> Option<Box<dyn Device>> {
            match self.0.open_path(path) {
                Ok(device) => Some(Box::new(HidApiDevice(device))),
                Err(err) => {
                    tracing::debug!("pad sensors: {err}");
                    None
                }
            }
        }
    }

    struct HidApiDevice(hidapi::HidDevice);

    impl Device for HidApiDevice {
        fn read(&mut self, buf: &mut [u8]) -> Option<usize> {
            self.0.read(buf).ok()
        }
    }

    #[cfg(test)]
    mod tests {
        use std::ffi::{CStr, CString};
        use std::sync::Mutex;
        use std::sync::mpsc::{Receiver, Sender, channel};
        use std::time::{Duration, Instant};

        use super::{Device, Hid, Sensors};
        use crate::sensors::SONY;

        const DUALSENSE: u16 = 0x0CE6;

        /// A DualSense USB report with the y acceleration at `g`.
        fn report(g: i16) -> Vec<u8> {
            let mut buf = vec![0u8; 64];
            buf[0] = 0x01;
            buf[24..26].copy_from_slice(&(g * 8192).to_le_bytes());
            buf
        }

        /// Reports the test sends; with its sender gone the device is unplugged.
        struct Fed(Receiver<Vec<u8>>);

        impl Device for Fed {
            fn read(&mut self, buf: &mut [u8]) -> Option<usize> {
                let report = self.0.recv().ok()?;
                buf[..report.len()].copy_from_slice(&report);
                Some(report.len())
            }
        }

        /// Hands out the devices queued for it, under paths `0`, `1`, ...
        #[derive(Default)]
        struct Desk {
            devices: Vec<Option<Fed>>,
        }

        static DESK: Mutex<Vec<Receiver<Vec<u8>>>> = Mutex::new(Vec::new());

        impl Hid for Desk {
            fn paths(&mut self, vendor: u16, product: u16) -> Vec<CString> {
                for receiver in DESK.lock().unwrap().drain(..) {
                    self.devices.push(Some(Fed(receiver)));
                }
                if (vendor, product) != (SONY, DUALSENSE) {
                    return Vec::new();
                }
                (0..self.devices.len())
                    .map(|i| CString::new(i.to_string()).unwrap())
                    .collect()
            }

            fn open(&mut self, path: &CStr) -> Option<Box<dyn Device>> {
                let i: usize = path.to_str().ok()?.parse().ok()?;
                let device = self.devices.get_mut(i)?.take()?;
                Some(Box::new(device))
            }
        }

        fn plug_in() -> Sender<Vec<u8>> {
            let (sender, receiver) = channel();
            DESK.lock().unwrap().push(receiver);
            sender
        }

        #[allow(clippy::disallowed_methods, reason = "a test's deadline")]
        fn poll_until(
            sensors: &mut Sensors,
            pads: &[(u16, u16)],
            ready: impl Fn(&Sensors) -> bool,
        ) {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                sensors.poll_with(pads, || Some(Desk::default()));
                if ready(sensors) {
                    return;
                }
                assert!(Instant::now() < deadline, "the reader never delivered");
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        /// The tests share `DESK`, so they run as one.
        #[test]
        fn a_reader_delivers_the_newest_report_and_ends_with_its_device() {
            let first = plug_in();
            let second = plug_in();
            let pads = [(SONY, DUALSENSE), (SONY, DUALSENSE)];
            let mut sensors = Sensors::default();
            let accel = |s: &Sensors, nth| {
                s.reading(SONY, DUALSENSE, nth)
                    .map(|r| r.motion.acceleration[1])
            };

            first.send(report(1)).unwrap();
            second.send(report(-1)).unwrap();
            poll_until(&mut sensors, &pads, |s| {
                accel(s, 0) == Some(1.0) && accel(s, 1) == Some(-1.0)
            });

            first.send(report(0)).unwrap();
            poll_until(&mut sensors, &pads, |s| accel(s, 0) == Some(0.0));
            assert_eq!(accel(&sensors, 1), Some(-1.0), "twins kept apart by order");

            drop(first);
            poll_until(&mut sensors, &pads, |s| {
                s.reading(SONY, DUALSENSE, 0).is_none()
            });
            assert_eq!(
                accel(&sensors, 1),
                Some(-1.0),
                "unplugging one leaves the other"
            );
        }
    }
}

/// Everywhere else: phones have no hidraw to open and wasm has no devices, so
/// every pad reads zero rather than the build failing to compile.
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod reader {
    use super::Reading;

    #[derive(Default)]
    pub(crate) struct Sensors;

    impl Sensors {
        pub(crate) fn poll(&mut self, _pads: &[(u16, u16)]) {}

        pub(crate) const fn reading(&self, _v: u16, _p: u16, _nth: usize) -> Option<&Reading> {
            None
        }
    }
}

pub(crate) use reader::Sensors;

#[cfg(test)]
mod tests {
    use super::{SONY, decode, layouts};

    const DUALSENSE: u16 = 0x0CE6;
    const DUALSHOCK4: u16 = 0x09CC;

    /// A report with the sensor fields filled the way the pad fills them.
    /// The offsets are `hid-playstation.c`'s; this fixture asserts the
    /// arithmetic on top of them, not the offsets themselves.
    fn report(id: u8, len: usize, gyro_at: usize, accel_at: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        buf[0] = id;
        for (axis, raw) in [1024i16, -2048, 512].into_iter().enumerate() {
            buf[gyro_at + axis * 2..gyro_at + axis * 2 + 2].copy_from_slice(&raw.to_le_bytes());
        }
        for (axis, raw) in [0i16, 8192, -4096].into_iter().enumerate() {
            buf[accel_at + axis * 2..accel_at + axis * 2 + 2].copy_from_slice(&raw.to_le_bytes());
        }
        buf
    }

    /// One finger, packed the way the four-byte touch point packs it.
    fn touch(buf: &mut [u8], at: usize, id: u8, x: u16, y: u16) {
        buf[at] = id;
        buf[at + 1] = (x & 0xFF) as u8;
        buf[at + 2] = ((y & 0x0F) << 4) as u8 | (x >> 8) as u8;
        buf[at + 3] = (y >> 4) as u8;
    }

    fn close(got: f32, want: f32) -> bool {
        (got - want).abs() < 1e-4
    }

    #[test]
    fn a_dualsense_report_decodes_to_radians_and_g() {
        let buf = report(0x01, 64, 16, 22);
        let reading = decode(&buf, layouts(SONY, DUALSENSE).unwrap()).unwrap();
        // 1024 raw counts is one degree per second.
        assert!(close(reading.motion.gyro[0], 1.0_f32.to_radians()));
        assert!(close(reading.motion.gyro[1], -2.0_f32.to_radians()));
        assert!(close(reading.motion.gyro[2], 0.5_f32.to_radians()));
        // 8192 raw counts is one g, so a resting pad reads 1 on one axis.
        assert!(close(reading.motion.acceleration[0], 0.0));
        assert!(close(reading.motion.acceleration[1], 1.0));
        assert!(close(reading.motion.acceleration[2], -0.5));
    }

    /// The Bluetooth report is the same body one byte further in, which is the
    /// single most likely thing to get wrong.
    #[test]
    fn the_bluetooth_layout_reads_the_same_values() {
        let usb = decode(&report(0x01, 64, 16, 22), layouts(SONY, DUALSENSE).unwrap()).unwrap();
        let bt = decode(&report(0x31, 78, 17, 23), layouts(SONY, DUALSENSE).unwrap()).unwrap();
        assert_eq!(usb.motion, bt.motion);
    }

    #[test]
    fn a_dualshock4_report_decodes_on_both_transports() {
        let usb = decode(
            &report(0x01, 64, 13, 19),
            layouts(SONY, DUALSHOCK4).unwrap(),
        )
        .unwrap();
        let bt = decode(
            &report(0x11, 78, 15, 21),
            layouts(SONY, DUALSHOCK4).unwrap(),
        )
        .unwrap();
        assert_eq!(usb.motion, bt.motion);
        assert!(close(usb.motion.acceleration[1], 1.0));
    }

    /// x is 12 bits split across two bytes and y is 12 bits split across the
    /// other two, sharing a byte in the middle.
    #[test]
    fn a_touch_point_unpacks_the_shared_middle_byte() {
        let mut buf = report(0x01, 64, 16, 22);
        touch(&mut buf, 33, 7, 960, 540);
        touch(&mut buf, 37, 8, 1919, 1079);
        let reading = decode(&buf, layouts(SONY, DUALSENSE).unwrap()).unwrap();

        assert_eq!(reading.touches.len(), 2);
        assert_eq!(reading.touches[0].id, 7);
        assert!(
            close(reading.touches[0].x, 0.5),
            "x was {}",
            reading.touches[0].x
        );
        assert!(
            close(reading.touches[0].y, 0.5),
            "y was {}",
            reading.touches[0].y
        );
        // The far corner normalises to just under 1, never past it.
        assert!(reading.touches[1].x < 1.0 && reading.touches[1].x > 0.999);
        assert!(reading.touches[1].y < 1.0 && reading.touches[1].y > 0.999);
    }

    /// Bit 7 of the contact byte means the slot holds no finger, which is what
    /// a pad sends far more often than it sends a touch.
    #[test]
    fn an_empty_touch_slot_is_not_a_finger() {
        let mut buf = report(0x01, 64, 16, 22);
        touch(&mut buf, 33, 0x80, 100, 100);
        touch(&mut buf, 37, 3, 960, 540);
        let reading = decode(&buf, layouts(SONY, DUALSENSE).unwrap()).unwrap();

        assert_eq!(reading.touches.len(), 1, "the empty slot was counted");
        assert_eq!(reading.touches[0].id, 3);
    }

    #[test]
    fn a_report_of_another_kind_is_not_a_reading() {
        let ds = layouts(SONY, DUALSENSE).unwrap();
        assert!(
            decode(&report(0x02, 64, 16, 22), ds).is_none(),
            "wrong report id"
        );
        assert!(decode(&[0x01, 0x00], ds).is_none(), "truncated report");
        assert!(decode(&[], ds).is_none(), "empty read");
    }

    #[test]
    fn only_the_pads_with_sensors_are_claimed() {
        assert!(layouts(SONY, DUALSENSE).is_some());
        assert!(layouts(SONY, DUALSHOCK4).is_some());
        assert!(layouts(SONY, 0x0001).is_none(), "an unknown Sony device");
        assert!(layouts(0x045E, 0x02FD).is_none(), "an Xbox pad has neither");
    }
}
