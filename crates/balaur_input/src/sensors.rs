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

/// Raw counts per g and per degree per second, from the same driver. A pad
/// also carries a calibration report trimming these per unit; without it a
/// resting gyro reads a small constant bias.
const ACCEL_PER_G: f32 = 8192.0;
const GYRO_PER_DEG: f32 = 1024.0;

/// Where the sensors sit in one report kind, as absolute byte offsets into the
/// buffer the pad sends — report id included, so there is nothing to add.
struct Layout {
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
    Layout { report_id: 0x01, len: 64, gyro: 16, accel: 22, touch: 33, width: 1920.0, height: 1080.0 },
    Layout { report_id: 0x31, len: 78, gyro: 17, accel: 23, touch: 34, width: 1920.0, height: 1080.0 },
];

/// DualShock 4 over USB (report 1) and Bluetooth (report 0x11). Its touch
/// points sit behind a report count and a timestamp, not directly in the body.
const DUALSHOCK4_LAYOUTS: &[Layout] = &[
    Layout { report_id: 0x01, len: 64, gyro: 13, accel: 19, touch: 35, width: 1920.0, height: 942.0 },
    Layout { report_id: 0x11, len: 78, gyro: 15, accel: 21, touch: 37, width: 1920.0, height: 942.0 },
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
