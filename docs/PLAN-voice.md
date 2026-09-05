> **Status:** not started. Written on 2026-09-05 from the Photon parity
> investigation, which found the roadmap's "what is missing is a codec"
> undersold it: capture, a jitter buffer, voice detection, echo cancellation
> and a browser path are missing too. Rides on `docs/PLAN-sessions.md` for
> the roster and the link, and on `docs/PLAN-gamend.md` step S5 for the
> browser fallback.

# Plan: voice

Players heard as well as seen: a microphone in, Opus over the session's
datagrams, each player's voice out on a bus, from where their node stands
if the game says so. Photon Voice is the product this mirrors; the engine's
shape decides the one rule Photon does not have to state, which is that
voice never enters the simulation.

## 0. Where the tree is today

| Have | Where |
| --- | --- |
| Output on every target through rodio 0.22 over cpal, WebAudio on wasm | `balaur_audio`, `rodio` with `wasm-bindgen` on wasm |
| Buses with volumes, and a `master` they mix into | `balaur_audio::bus`, `audio::buses`, `bus_volume` |
| Positional sound from a listener node: pan and distance gain per emitter, moved per frame | `balaur_audio::spatial`, `emitter_position`, `set_listener`, `pan`, `distance_gain` |
| Unreliable datagrams on the session link, with a size the link reports | `Transport::send_datagram`, `max_datagram` |
| The roster: who is in the session and which slot is local | `docs/PLAN-sessions.md` step 1 |
| A server-side WebRTC peer on Gamend that could carry an audio track | `GamendWeb.WebRTCPeer` over `ex_webrtc` |
| Signed macOS bundles with a plist the exporter writes | `bundle.rs::export_macos_app` |

Missing:

- **Capture.** No input stream is ever opened. cpal opens one on desktop,
  iOS and Android; its WebAudio host is output only, so the browser needs
  `getUserMedia` and an `AudioWorklet` behind a shim like the fetch shim.
- **A codec.** No Opus in `Cargo.lock`, and there is no pure-Rust Opus
  encoder: every crate binds libopus.
- **A jitter buffer and loss concealment.** A datagram arrives late, early or
  not at all; a voice frame has to play on time regardless.
- **Voice detection, push-to-talk, echo cancellation, noise suppression,
  gain control.** All of it is what makes voice bearable and none of it is
  in the tree.
- **A voice source on a bus.** rodio mixes `Source`s; nothing turns a stream
  of decoded frames into one that waits for the next frame without
  underrunning.
- **Permissions.** No `NSMicrophoneUsageDescription`, no
  `com.apple.security.device.audio-input` entitlement, no `RECORD_AUDIO`
  in the Android manifest.
- **A rule for recordings.** Nothing says whether a `.blr` carries voice.

## 1. Design

**Voice is an observer.** Like `engine.timings()` and the session's link
stats, voice is heard and never simulated: no tick reads it, no digest hashes
it, no snapshot restores it, and a `.blr` does not carry it. A script may ask
who is speaking for a UI indicator, which is a frame-scoped answer like
`input::just_pressed`. The pipeline runs on the audio thread and a worker,
and the only thing the tick does is hand the roster to it.

**The pipeline.** Mono at 48 kHz in 20 ms frames, which is 960 samples and
one Opus packet of 60 to 80 bytes at the 24 to 32 kbit/s a voice wants:

```
cpal input ─ APM (echo, noise, gain) ─ gate (push-to-talk / activity) ─ Opus encode
   ─ datagram on the session link, one message kind, never in the journal ─
receive ─ jitter buffer (60 to 100 ms target, adaptive) ─ Opus decode + PLC
   ─ a rodio Source per speaker ─ the `voice` bus, positional through `spatial` if attached ─ master
```

**Codec.** Opus, the same choice Photon, Discord and every browser make,
through `audiopus` (libopus built by `audiopus_sys`, which needs cmake or a
system libopus). Behind a `voice` feature, off by default like
`webtransport`, because a C build on six export targets is a cost a game
without voice should not pay.

**Transport.** Voice frames are datagrams on the session link
(`docs/PLAN-sessions.md`), tagged as their own message kind so the session
never journals them. Over QUIC a lost frame is a lost frame and concealment
covers it; over the websocket a datagram is reliable and late, which is
right for a turn-based game and wrong for a shooter, and the plan does not
pretend otherwise. Under `host` and `server` roles the ordering end forwards
voice like inputs, with a per-player mute list and a `voice/max_distance`
filter applied there, which is Photon's interest group for voice.

**The browser.** WebTransport in a tab is `docs/PLAN-networking.md` step 13
and web export is missing, so the first browser path is a WebRTC audio track
to Gamend's peer (`docs/PLAN-gamend.md` step S5), where the browser encodes
Opus itself and Gamend forwards RTP between the lobby's members. Once step 13
exists, the tab captures through `getUserMedia`, encodes with WebCodecs'
`AudioEncoder` where the browser has Opus in it, and speaks the same
datagrams as native.

**Audio processing.** Echo cancellation, noise suppression and gain control
through `webrtc-audio-processing`, WebRTC's own module bound from Rust, C++
under it, behind a second feature because it is the largest build in the
plan; `nnnoiseless` (RNNoise in pure Rust) is the noise gate and activity
detector where the C++ build is not wanted, and an energy threshold is the
floor that always works.

**The script surface.** A `voice` module.

| Call | Answers |
| --- | --- |
| `voice::start(options)` / `stop()` | Open the input and join the session's voice; `mode` is `MODE_PUSH_TO_TALK`, `MODE_ACTIVITY` or `MODE_OPEN` |
| `voice::set_transmit(bool)` | The push-to-talk key, from a script's own binding |
| `voice::level()` | The local input level this frame, for a meter |
| `voice::speaking(player)` | Whether that player's frames are playing this frame |
| `voice::set_muted(player, bool)`, `set_volume(player, gain)` | Per-player controls, local only |
| `voice::attach(player, node)` / `detach(player)` | Hear that player from a node's position through the existing `spatial` emitter |
| `voice::devices()`, `set_device(name)` | Input devices, and a change that survives a device unplugging |
| `on_voice(e)` | `started`, `stopped`, `device_lost`, `speaking`, `silent`, `denied` (the OS refused the microphone) |

```toml
[voice]
bus = "voice"
mode = "activity"            # push_to_talk | activity | open
activity_threshold = 0.02
jitter_ms = 80
bitrate = 32000
max_distance = 40.0           # positional voice beyond this is not sent
```

**Permissions.** `balaur export` writes `NSMicrophoneUsageDescription` and
the audio-input entitlement into the macOS and iOS bundles and
`RECORD_AUDIO` into the Android manifest when `[voice]` is present, and
nothing when it is not, so a game without voice never shows a microphone
prompt.

## 2. The surface

Everything Photon Voice ships, and the decision on each.

| Feature | Decision |
| --- | --- |
| Microphone capture, device list, device change | Step 1 |
| Opus encode and decode | Step 2 |
| Jitter buffer, packet loss concealment, playback delay | Step 2 |
| Push-to-talk | Step 2 |
| Voice activity detection | Step 2 with an energy threshold; step 4 with `nnnoiseless` or WebRTC's VAD |
| Echo cancellation, noise suppression, automatic gain | Step 4, `webrtc-audio-processing`, behind a feature |
| Spatial voice from a node | Step 3, through `spatial` |
| Per-player mute and volume | Step 3 |
| Interest: distance and mute lists applied at the ordering end | Step 3 |
| A speaker component on a node, so a scene declares who is heard from where | Step 3: a `voice` key on a node naming a player slot, which is `attach` in a scene file |
| Audio sources other than the microphone: a file, a script-supplied buffer | Step 5, as a `Capture` trait the file-backed test source already implements |
| Voice in a recording | Step 5: not in the `.blr`; an optional `.ogg` per player beside it for a replay with commentary |
| The browser through Gamend's WebRTC peer | Step 6, with `docs/PLAN-gamend.md` step S5 |
| The browser over WebTransport and WebCodecs | Step 6, after `docs/PLAN-networking.md` step 13 |
| Text chat | `docs/PLAN-gamend.md`: Gamend's chat is built, typed bindings are its step E1 |
| Speech to text, text to speech | Not planned; nothing in Photon Voice either, and a game that wants either wants a specific model |
| Video | Not planned. The roadmap's video row is playback onto a texture, which shares nothing with this pipeline |

## 3. Steps

1. **Capture and a meter.** cpal input on desktop, iOS and Android, the
   device list, `level()`, the permission strings in every export, and the
   `denied` event. Ends with: a bar in a UI that moves when you speak.
2. **Loopback voice.** `audiopus`, the jitter buffer, concealment,
   push-to-talk, the energy gate, the `voice` bus, frames on the session
   link, under `netcode/faults`. Ends with: two engines on one machine hear
   each other through five percent loss.
3. **Positional voice and per-player controls.** `attach`, mute, volume,
   `max_distance` at the ordering end, the scene-file key. Ends with: a
   player heard from their node, and not heard past the distance.
4. **Processing.** `webrtc-audio-processing` behind its feature,
   `nnnoiseless` as the pure-Rust gate. Ends with: a speaker playing into an
   open microphone does not echo.
5. **Sources and recordings.** The `Capture` trait, a file source, the
   `.ogg` beside a recording. Ends with: a replay plays back the commentary.
6. **The browser.** The WebRTC track through Gamend, then WebCodecs over
   WebTransport when step 13 lands.

## 4. What CI can prove

A runner has no microphone, so `Capture` is a trait and the tests feed a
file: the codec round-trips, the jitter buffer plays on time under
`transport::Faulty`, a muted player's frames never leave the ordering end,
the permission strings appear in exports exactly when `[voice]` does, and a
`.blr` recorded with voice on carries none of it. What it cannot: a real
device, real echo, the OS prompt.

## 5. Open questions

1. **libopus on every target.** cmake on the Windows cross build, the iOS
   and Android toolchains, and the wasm build, or a vendored build script.
   Decide at step 2; it is the main cost of the feature.
2. **The session link or a link of its own.** Datagrams already avoid
   head-of-line blocking behind a snapshot, so one link is the default; a
   second QUIC connection is the fallback if voice and inputs contend.
3. **Rates.** Whether the `voice` bus resamples to rodio's mixer rate or the
   mixer runs at 48 kHz. The second is simpler and is what every device does.
