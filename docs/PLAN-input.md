# Plan: input

What is left of input is motion and output on more devices, and pads on
phones. Every new backend lands in the one snapshot per frame and keeps the
rules and the contract in §1.

## 1. The rules and the gamepad contract

Six rules every backend holds:

1. One snapshot per frame: a backend writes it and everything else reads it.
   No script reaches a device.
2. Neutral answers, never failure: an absent pad, a headless run, a pad with
   no gyroscope and a platform with no HID all read zero.
3. What a script can read is recorded, `can_rumble` included, so a replay
   takes the same branches.
4. Output is not recorded. A rumble is asked again by the script on replay.
5. One reading of one concern: gilrs owns buttons and axes, `sensors.rs` gyro
   and the touchpad. A backend covering both replaces both rather than
   joining them.
6. No device call on the tick. A backend reads on its own thread and the tick
   takes its newest reading.

A finger, move, button or wheel turn a script feeds (`input.set_touch`,
`set_mouse_position`, `set_mouse_down`, `set_scroll_delta`) lands in the
snapshot on the next frame and reaches the widgets in that frame's UI pass, as
the window's own input does. A fed key reaches the snapshot only.

The gamepad contract is the engine's, so a second backend (SDL3, Steam Input,
a browser's Gamepad API) reads the same. A backend only reports changes and
holds two motor levels. `gamepad.rs`, `pad_thread.rs` and `rumble.rs` enforce
it, and `crates/balaur/tests/suite/gamepad.rs` checks it through
`input.set_gamepad`, never through gilrs.

- The pads open on the first read: `input.gamepads()`, any `input.gamepad_*`
  or rumble verb, or an action bound to `gamepad:` or `axis:`. Until then no
  thread enumerates them, which on Windows is gilrs reading every pad every
  8 ms. An `on_gamepad_connected` handler alone opens nothing, and a fed pad
  needs nothing opened.
- Ids are player slots. A pad takes the lowest free slot last held by its SDL
  GUID, else the lowest free slot, and keeps it while connected.
- Values are shaped once. Sticks read -1 to 1 with up positive, triggers and
  pressure 0 to 1. `[input] gamepad_deadzone` applies radially to a stick and
  along a trigger, rescaled so the first live reading is near zero.
- Down is a threshold on pressure: past `gamepad_press`, up below
  `gamepad_release`.
- Every change reaches the tick, in order. A press and a release between two
  frames are both edges in one frame.
- Repeat is counted in tick time: `gamepad_repeat_delay_seconds`, then every
  `gamepad_repeat_interval_seconds`.
- Rumble is the engine's. Delay, pulses, envelope and falloff are worked out
  each tick. The backend holds two motor levels only as long as the rumble
  has left, so a stalled game cannot leave a pad buzzing.
- Identity and power come from standards: SDL GUID, USB vendor and product,
  SDL mappings, and SDL3's power states.

## 2. Steps

1. Sensor fixtures from real hardware: reports captured from a DualSense and
   a DualShock 4, so CI holds the decode against real bytes. Ends with: a
   resting pad reads about 1 g on one axis, and a finger at the touchpad's
   centre about `(0.5, 0.5)`.
2. Calibration: DualSense feature report `0x05`, DS4 `0x02` and `0x05`, read
   per unit. Reading one also ends the Bluetooth stall, where a pad sends the
   short report `0x01` until a calibration report is read. Ends with: the
   full report arrives over Bluetooth on macOS and Windows.
3. Switch Pro and Joy-Con gyro and accelerometer: report `0x30` behind a USB
   handshake, with calibration from SPI flash.
4. What a DualSense does besides rumble: adaptive triggers, the light bar,
   and the player and mute LEDs, all in one output report.
5. Core Haptics on Apple, as a backend under the rumble verb rather than a
   second API.
6. Device motion on phones and tablets: CoreMotion and Android's
   `SensorManager`, in the same snapshot, through `balaur_apple` and
   `balaur_android`.
7. Gamepad buttons and axes on iOS and Android, which gilrs covers on
   neither.
8. A udev rule in the Linux export, since without one hidraw reports no
   sensors.
9. Key and mouse button tests: `KEY_CAPS_LOCK`, `KEY_INTERNATIONAL_RO`,
   `KEY_CONTEXT_MENU` and `KEY_NUMPAD_EQUAL` fire on desktop and on the web,
   and Back and Forward stay two mouse buttons. The kiss3d fork maps them,
   and no test holds it.

Not planned: motion on Xbox pads (none reports any), raw codes for wheels and
flight sticks (an SDL mapping names their controls), remapping a connected
pad through gilrs (`input.bind` remaps actions), and sensors on wasm. Steam
Deck and Steam Controller input is `docs/PLAN-steam.md`.

## Open questions

1. Whether the sensor reader survives Steam Input: on a Steam build
   `sensors.rs` should go quiet, but a pad Steam does not recognise still
   wants it.
2. Whether motion belongs in the action layer: gyro aiming is an axis with a
   filter in front of it, and an action table has no word for the filter.
3. Twin pads: two identical pads are matched to their HID devices by
   enumeration order, so they can swap sensors, and a recording replays them
   swapped. Matching by HID path or serial needs one of them from the pad
   backend, and gilrs gives neither.
