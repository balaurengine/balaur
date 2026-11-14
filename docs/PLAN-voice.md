# Plan: voice

Players heard as well as seen: a microphone in, Opus over the match's
datagrams, and each player's voice out on a bus, from their node if the game
says so. Voice is an observer: no tick reads it, no digest hashes it, no
snapshot restores it, and a `.blr` does not carry it.

## Steps

1. **Capture and a meter.** cpal input on desktop, iOS and Android, and a
   `voice` module with `devices()`, `set_device`, `level()` and `on_voice`
   (`started`, `stopped`, `device_lost`, `denied`). `balaur export` writes
   `NSMicrophoneUsageDescription`, the audio-input entitlement and
   `RECORD_AUDIO` only when `[voice]` is present. Ends with: a bar in a UI
   that moves when you speak.
2. **Loopback voice.** Mono at 48 kHz in 20 ms frames, Opus through
   `audiopus` behind a `voice` feature that is off by default. Frames go as
   their own message kind on the match's link, never journaled. The receiver
   runs a jitter buffer and loss concealment into a rodio `Source` per
   speaker on the `voice` bus. `start`, `stop`, `set_transmit` for
   push-to-talk, an energy gate, and `speaking(player)`. Ends with: two
   engines on one machine hear each other through five percent loss under
   `balaur_net::Faulty`.
3. **Positional voice and per-player controls.** `attach(player, node)` and
   `detach` through `balaur_audio::spatial`, `set_muted` and `set_volume` per
   player, and a `voice` key on a node naming a player slot. Under the `host`
   and `server` topologies the ordering end applies mute lists and a
   `max_distance` filter. Ends with: a player heard from their node, and not
   heard past the distance.
4. **Processing.** Echo cancellation, noise suppression and gain control
   through `webrtc-audio-processing` behind its own feature, and
   `nnnoiseless` as the pure-Rust gate. Ends with: a speaker playing into an
   open microphone does not echo.
5. **Sources and recordings.** A `Capture` trait, a file source for tests,
   and an `.ogg` per player beside a recording. Ends with: a replay plays back
   the commentary.
6. **The browser.** First a WebRTC audio track to Gamend's peer
   (`docs/PLAN-gamend.md` step S5). Then, after `docs/PLAN-networking.md`
   step 13, `getUserMedia` capture and WebCodecs' `AudioEncoder` over the same
   datagrams as native. Ends with: a tab and a native engine hear each other.

Not planned: speech to text, text to speech, and video.

## Open questions

1. **libopus on every target.** Every Opus crate binds it. cmake on the
   Windows cross build, the iOS and Android toolchains and wasm, or a vendored
   build script. Decide at step 2.
2. **The match's link or a link of its own.** One link is the default; a
   second QUIC connection is the fallback if voice and inputs contend.
3. **Rates.** Whether the `voice` bus resamples to rodio's mixer rate or the
   mixer runs at 48 kHz.
