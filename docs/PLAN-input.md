> **Status:** keyboard, mouse, touch, the action layer, gamepad buttons and
> axes, rumble on both motors, and gyro, accelerometer and touchpad on
> DualSense and DualShock 4 are built and recorded; ARCHITECTURE.md's Input
> section is the record. Written down after that rather than before, because
> the first sensor backend is what made the shape of the rest visible: one pad
> family is covered, and every other device — another pad, a phone, a Steam
> Deck — is the same backend problem behind the same snapshot.

# Plan: Input

Everything a player touches: keyboard, mouse, wheel, touch screen, gamepads,
the sensors inside them and the motors that push back. All of it arrives as
one snapshot per frame, and all of it is recorded, because input is the half
of a simulation the engine does not compute.

## 0. What is missing

Motion on any pad that is not Sony's, per-unit sensor calibration, anything a
pad does other than rumble, motion from a phone or tablet, a gamepad backend
on iOS or Android, and a way for a script to stand in for a pad the way it
already can for a keyboard.

Three things the first backend got wrong, found on 2026-09-04 by reading
`sensors.rs` and `actions.rs`: over Bluetooth a PlayStation pad keeps sending
the short report `0x01` until a calibration feature report is read, and
nothing reads one, so on macOS and Windows the full report never arrives
(step 2 fixes this as a side effect); two identical pads are matched by index
in gilrs order against hidapi's enumeration order, which nothing keeps in
step, so twins can swap sensors — match by HID path or serial; and
`input.reset_bindings` reloads the host's `project.toml`, discarding a table
`declare_actions` declared, which under the editor resets a played game's
actions to the editor's own.

## 1. Design

Five rules already hold, and the point of writing them down is that every
backend below has to keep holding them.

1. **One snapshot per frame.** A backend writes it, everything else reads it.
   No script reaches a device.
2. **Neutral answers, never failure.** An absent pad, a headless run, a pad
   with no gyroscope and a platform with no HID all read the same zero. This
   is what lets one game run in CI and in a window.
3. **What a script can read is recorded.** Otherwise a replay diverges. That
   is why `can_rumble` is in the snapshot rather than asked of the hardware:
   a script may branch on it, and the branch has to be the same on replay.
4. **Output is not recorded.** A rumble is re-asked by the script that ran, so
   the recording carries the input, not the effect.
5. **One reading of one concern.** gilrs owns buttons and axes; `sensors.rs`
   owns gyro and the touchpad. A backend that covers both replaces both,
   rather than joining them — two readings of one pad is a bug with a name.

## 2. The surface

Every device and API worth naming that is not built. A row saying "not
planned" is a decision, not an oversight.

### Pads and their sensors

| Thing | Verdict |
| --- | --- |
| Per-unit calibration report (DualSense feature `0x05`, DS4 `0x02` / `0x05`) | Step 2. Nominal scaling today, good to a few percent; the report trims it to the unit and corrects the accelerometer's bias |
| Switch Pro and Joy-Con: gyro and accelerometer | Step 3. Report `0x30` behind a USB handshake, and a calibration block in SPI flash. No touchpad on either |
| DualSense adaptive triggers, light bar, player and mute LEDs | Step 4. One output report carries all of them, so they arrive together or not at all |
| Core Haptics on Apple, and waveform haptics generally | Step 5, as a backend under the same verb as rumble rather than a second API |
| Steam Deck and Steam Controller: gyro, trackpads, back buttons | Not here — Steam Input, `docs/PLAN-steam.md` step 10, which replaces both readers at once |
| Xbox pads: motion | Not planned. No Xbox pad reports any |
| Pad speaker, microphone, headphone jack | Not planned here. They are audio devices; a stream belongs in `balaur_audio` |
| Trackballs, wheels, flight sticks, pedals | Not planned. gilrs presents them as axes already, which is the honest shape |

### Platforms

| Thing | Verdict |
| --- | --- |
| Linux: hidraw permission | Warns once and reports no sensors, per rule 2. Shipping a udev rule with the export is step 8 |
| iOS and Android: gamepad buttons and axes | Step 7. gilrs covers neither, so a pad on a phone reads nothing at all today |
| Phone and tablet device motion (CoreMotion, Android `SensorManager`) | Step 6. Not a pad — a sensor in the device — but it lands in the same snapshot and wants `balaur_apple` / `balaur_android` |
| wasm | Not planned for sensors. The Gamepad API may cover buttons and axes later; there is no HID in a tab |

### Standing in for a person

| Thing | Verdict |
| --- | --- |
| Feeding a pad: buttons, axes, motion, touch | Step 1. A showcase or a test can drive a keyboard but not a controller, which is the gap that makes every row above hard to demonstrate |
| Sensor decode asserted against captured reports | Step 1. Synthetic fixtures today; the ones worth having come off real hardware — see §4 |

## 3. Steps

1. Feeding a pad, and report fixtures captured from real hardware.
2. Calibration for the pads already decoded.
3. Switch Pro and Joy-Con.
4. What a DualSense does besides rumble: triggers, light bar, LEDs.
5. Core Haptics behind the rumble verb.
6. Device motion on phones and tablets.
7. Gamepad buttons and axes on iOS and Android.
8. A udev rule in the Linux export, so a player is not the one debugging it.

## 4. What CI can prove, and what it cannot

CI has no controller, and no runner will grow one. It can prove the decode
arithmetic against fixed bytes, and that the snapshot round-trips through a
recording. It cannot prove a byte offset is the one the hardware actually
sends. Those came from Linux's `hid-playstation.c` and both report sizes
reconstruct exactly, which is strong evidence and not a test. **No PlayStation
pad has been held against this code.** The first person with one should check
that a resting pad reads about 1 g on one axis and that a finger at the centre
of the touchpad reads about `(0.5, 0.5)`; step 1's fixtures should then be
captured from that pad so CI can hold the line afterwards.

## 5. Open questions

1. **Whether the sensor reader survives Steam Input.** Rule 5 says a backend
   covering both readings replaces both. Steam Input covers gyro and buttons,
   so on a Steam build `sensors.rs` should go quiet — but a player running the
   Steam build with a pad Steam does not recognise wants it back.
2. **Whether motion belongs in the action layer.** Actions map a name to a key
   or an axis. Gyro aiming is an axis with a filter in front of it, and the
   filter is the part an action table has no word for yet.
3. **How a pad is identified across a replay.** Vendor and product match a
   pad to its HID device today, and two identical pads are told apart by
   order. A recording made with two pads swapped replays with them swapped.
