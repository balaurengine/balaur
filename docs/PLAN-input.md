> **Status:** keyboard, mouse, touch, the action layer, gamepad buttons and
> axes, rumble on both motors, and gyro, accelerometer and touchpad on
> DualSense and DualShock 4 are built and recorded; ARCHITECTURE.md's Input
> section is the record. The gamepad contract in §1 and everything gilrs
> offers in §2 are built. Written down after that rather than before, because
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
pad does other than rumble, motion from a phone or tablet, and a gamepad
backend on iOS or Android.

Two things the first sensor reader got wrong, found on 2026-09-04 by reading
`sensors.rs`: over Bluetooth a PlayStation pad keeps sending the short report
`0x01` until a calibration feature report is read, and nothing reads one, so
on macOS and Windows the full report never arrives (step 2 fixes this as a
side effect); and two identical pads are matched by index in gilrs order
against hidapi's enumeration order, which nothing keeps in step, so twins
can swap sensors. Matching by HID path or serial needs one of them from the
pad backend, and gilrs gives neither.

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

A sixth came with the gamepad thread:

6. **No device call on the tick.** A backend reads on a thread of its own and
   the tick takes its newest reading, so a controller service that hangs
   stops the pads, not the game. gilrs runs on `pad_thread.rs`'s thread;
   `sensors.rs` opens devices on a manager thread that sleeps until the pads
   change, and reads each on a thread that sleeps in a blocking read.

### The gamepad contract

What a script sees of a pad is the engine's to define, so a second backend
(SDL3, Steam Input, a browser's Gamepad API) reads the same. A backend only
reports changes and holds two motor levels. Every rule below is enforced in
`gamepad.rs`, `pad_thread.rs` and `rumble.rs`. The unit tests beside them and
`crates/balaur/tests/suite/gamepad.rs` check each one through
`input.feed_gamepad` or `GamepadState::feed`, the path a backend's changes
take, and never through gilrs.

- **Ids are player slots.** A pad takes the lowest free slot last held by the
  same model (its SDL GUID), else the lowest free slot, and keeps it while
  connected. A backend's own handles never reach a script.
- **Values arrive raw and are shaped once.** Sticks read -1 to 1 with up
  positive, triggers and button pressure 0 to 1. `[input] gamepad_deadzone`
  applies radially to a stick and along a trigger, rescaled so the first live
  reading is near zero. gilrs's own filters are off.
- **Down is a threshold on pressure.** A button goes down past
  `gamepad_press` and up below `gamepad_release`; a digital button reads 0 or 1.
- **Every change reaches the tick, in order.** A press and a release between
  two frames are both edges in one frame, the way a key's are, and an action
  bound to the button is pressed for that frame.
- **Noise does not wake the loop.** The thread drops an axis or pressure
  change under 0.01 of its travel unless it lands on rest or full.
- **Repeat is counted in tick time.** `gamepad_repeated` fires on the press,
  then every `gamepad_repeat_interval_seconds` once the button has been held
  for `gamepad_repeat_delay_seconds`.
- **Rumble is the engine's.** Delay, pulses, envelope and distance falloff
  are computed each tick in tick time; the backend holds two motor levels for
  as long as the rumble has left, so a stalled game cannot leave a pad
  buzzing. Strength is linear to the motor's 16-bit resolution.
  `gamepad_rumble`'s answer and `on_gamepad_rumble_finished` follow from the
  recorded snapshot, so a replay without the pad takes the same branches.
- **Identity comes from standards.** SDL GUID, USB vendor and product, the
  OS's name, and whether an SDL mapping matched. Mappings are SDL's format: the
  bundled database, `SDL_GAMECONTROLLERCONFIG`, and `[input] gamepad_mappings`.
- **Power uses SDL3's states:** `on_battery`, `no_battery`, `charging`,
  `charged` and `unknown`, with a level from 0 to 1, or -1 when not known.

A backend that stops answering at once (64 empty waits in a row) ends its
thread and unplugs its pads, so a button it last saw held is released. One
that hangs mid-call cannot be told from an idle one, so its pads stay as
they were until it returns.

These limits come from gilrs and are kept, by decision, rather than patched:

- **The gamepad thread cannot be woken.** It sleeps inside gilrs, so anything
  that needs gilrs itself happens on that thread: motors are built when a pad
  connects, and mappings load when the process first reads pads. The editor
  reads them with its own project, so a game's `gamepad_mappings` apply from
  its export. Opening a second gilrs for new mappings would leak a thread,
  because gilrs's force-feedback loop never ends.
- **Power is read whenever a pad sends anything**, since gilrs reports no
  power event.
- **gilrs silences an effect whose gain is under 0.05**, so each motor is
  three effects a factor of 20 apart in strength, and a level plays on the
  one whose gain clears that floor.
- **Motor levels go out every 50 ms**, so an envelope reaches the pad in
  50 ms steps and the shortest rumble a pad feels is one step. The same loop
  wakes 20 times a second for the life of the process, rumbling or not.
- **On Windows gilrs polls**: Windows Gaming Input every 8 ms and XInput
  every 10 ms, since neither API reports a change.

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
| Trackballs, wheels, flight sticks, pedals | Through a custom SDL mapping, which names their controls as a pad's. Their raw codes are not planned: each OS numbers them its own way, and gilrs lists a code only once it has moved |

### What gilrs offers

| Thing | Verdict |
| --- | --- |
| Buttons and axes by position, and analog pressure on any button | Have: `gamepad_down`, `gamepad_axis`, `gamepad_pressure` |
| `C` and `Z` on six-button pads | Have: `GAMEPAD_BUTTON_C`, `GAMEPAD_BUTTON_Z` |
| A d-pad reported as two axes | Have, read as the four d-pad buttons |
| `LeftZ` and `RightZ` | Not planned. They are an unmapped pad's triggers, and a mapping names them as triggers |
| Battery (`power_info`) | Have: `gamepad_power` |
| GUID, vendor, product, OS name, mapping source | Have: `gamepad_info` |
| Custom SDL mappings | Have, at start: `[input] gamepad_mappings` and `SDL_GAMECONTROLLERCONFIG` |
| Remapping a connected pad (`Gilrs::set_mapping`) | Not planned. It needs the gamepad thread, which sleeps in gilrs; `input.bind` remaps a game's actions |
| Deadzone, trigger thresholds, filters | Have, as `[input]` settings; gilrs's filters are off |
| `ButtonRepeated` | Have: `gamepad_repeated` |
| Event timestamps | Not planned. A script reads frames, and the order of changes inside one is kept |
| Strong and weak motors | Have |
| Envelope, start delay, repeated pulses | Have: `gamepad_rumble`'s `attack`, `attack_level`, `fade`, `fade_level`, `delay`, `pulse`, `gap` |
| Positional rumble and distance falloff | Have: `position` with `gamepad_set_listener`, falloff `inverse`, `linear` or `exponential` |
| One effect over several pads | Not planned as an option. A script rumbles each pad in the same tick |
| An effect finishing | Have: `on_gamepad_rumble_finished` |

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
| Feeding a pad: buttons, axes, power, identity, motion, touch | Have: `input.feed_gamepad`, through the same path a backend's changes take |
| A fed key, click or finger is the next frame's | Have. `input.feed_*` queues the event and the frame delivers it when it begins, headless or windowed, so a hook dispatched at the top of a tick sees it as it would an OS event |
| Sensor decode asserted against captured reports | Step 1. Synthetic fixtures today; the ones worth having come off real hardware — see §4 |

## 3. Steps

1. Report fixtures captured from real hardware. Feeding a pad is built.
2. Calibration for the pads already decoded.
3. Switch Pro and Joy-Con.
4. What a DualSense does besides rumble: triggers, light bar, LEDs.
5. Core Haptics behind the rumble verb.
6. Device motion on phones and tablets.
7. Gamepad buttons and axes on iOS and Android.
8. A udev rule in the Linux export, so a player is not the one debugging it.
9. The sensor reads on a thread of their own, built: rule 6 holds for every
   backend.
10. The gamepad contract, built: everything in §1's contract and §2's gilrs
    table marked have. Ends with: `crates/balaur/tests/suite/gamepad.rs`
    passes on fed pads alone, and a recorded pad session replays to the same
    digests.

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
