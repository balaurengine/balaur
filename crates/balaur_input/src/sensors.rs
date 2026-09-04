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
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod reader {
    use std::ffi::CString;

    use hidapi::{HidApi, HidDevice};

    use super::{decode, layouts, Layout, Reading};

    /// Reports one pad may have queued between two frames before we stop
    /// draining. A DualSense sends 1000 a second over USB, so a slow frame
    /// leaves a few dozen and only the newest is worth decoding.
    const MAX_DRAIN: usize = 64;

    /// The longest report any pad here sends (both Bluetooth layouts).
    const MAX_REPORT: usize = 78;

    /// Generic Desktop / Gamepad. Windows and macOS enumerate one entry per
    /// top-level collection, so without this a pad opens on the wrong one.
    /// Linux's hidraw reports no usage at all and leaves the pair zero.
    const GAMEPAD_USAGE: (u16, u16) = (0x01, 0x05);

    #[derive(Default)]
    pub(crate) struct Sensors {
        api: Option<HidApi>,
        /// The pads we last tried to open, in gilrs order, so the expensive
        /// enumeration only reruns when the set on the desk changes.
        attempted: Vec<(u16, u16)>,
        opened: Vec<Opened>,
        failed: bool,
    }

    struct Opened {
        vendor: u16,
        product: u16,
        device: HidDevice,
        layouts: &'static [Layout],
        reading: Option<Reading>,
    }

    impl Sensors {
        /// Take whatever each pad has sent since the last frame. `pads` is
        /// every connected pad's vendor and product, in the order the snapshot
        /// lists them.
        pub(crate) fn poll(&mut self, pads: &[(u16, u16)]) {
            self.reopen(pads);
            for open in &mut self.opened {
                let mut buf = [0u8; MAX_REPORT];
                let mut newest = None;
                for _ in 0..MAX_DRAIN {
                    match open.device.read_timeout(&mut buf, 0) {
                        // Nothing queued, or the pad went away mid-frame; the
                        // next `reopen` is what notices the second case.
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            if let Some(reading) = decode(&buf[..read], open.layouts) {
                                newest = Some(reading);
                            }
                        }
                    }
                }
                if newest.is_some() {
                    open.reading = newest;
                }
            }
        }

        /// The `nth` pad with this vendor and product, matching the snapshot's
        /// nth such pad. Two identical controllers stay told apart by order.
        pub(crate) fn reading(&self, vendor: u16, product: u16, nth: usize) -> Option<&Reading> {
            self.opened
                .iter()
                .filter(|open| open.vendor == vendor && open.product == product)
                .nth(nth)
                .and_then(|open| open.reading.as_ref())
        }

        fn reopen(&mut self, pads: &[(u16, u16)]) {
            let want: Vec<(u16, u16)> = pads
                .iter()
                .copied()
                .filter(|(vendor, product)| layouts(*vendor, *product).is_some())
                .collect();
            if want == self.attempted {
                return;
            }
            self.attempted.clone_from(&want);
            self.opened.clear();
            if want.is_empty() || self.failed {
                return;
            }
            // One context for the process, opened on the first pad that needs
            // it and never retried once it has failed.
            if self.api.is_none() {
                match HidApi::new() {
                    Ok(api) => self.api = Some(api),
                    Err(err) => {
                        tracing::warn!("pad motion and touchpad disabled: {err}");
                        self.failed = true;
                    }
                }
            }
            let Some(api) = self.api.as_mut() else {
                return;
            };
            if let Err(err) = api.refresh_devices() {
                tracing::debug!("pad sensors: {err}");
                return;
            }

            let mut opened = Vec::new();
            for (i, (vendor, product)) in want.iter().enumerate() {
                let nth = want[..i].iter().filter(|pair| **pair == want[i]).count();
                let Some(layouts) = layouts(*vendor, *product) else {
                    continue;
                };
                let paths: Vec<CString> = api
                    .device_list()
                    .filter(|dev| dev.vendor_id() == *vendor && dev.product_id() == *product)
                    .filter(|dev| {
                        dev.usage_page() == 0 || (dev.usage_page(), dev.usage()) == GAMEPAD_USAGE
                    })
                    .map(|dev| dev.path().to_owned())
                    .collect();
                let Some(path) = paths.get(nth) else {
                    continue;
                };
                match api.open_path(path) {
                    Ok(device) => {
                        let _ = device.set_blocking_mode(false);
                        opened.push(Opened {
                            vendor: *vendor,
                            product: *product,
                            device,
                            layouts,
                            reading: None,
                        });
                    }
                    // A pad the OS will not hand over (no hidraw rule on
                    // Linux) simply reports no motion, as an absent one does.
                    Err(err) => tracing::debug!(vendor, product, "pad sensors: {err}"),
                }
            }
            self.opened = opened;
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
    use super::{decode, layouts, SONY};

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
