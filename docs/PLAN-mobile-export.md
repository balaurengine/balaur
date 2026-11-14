# Exporting for mobile

What is left before an iOS or Android export is a game a player can hold.
`balaur export --target ios` and `--target android` are built, and
`scripts/export_check.sh` checks the bundles on every build.

## Steps

1. **A tick that pauses on suspend.** `on_suspended_changed` already tells
   every script when the app goes to the background, from kiss3d's `Iconify`
   and a page's visibility. Nothing else answers it: the fixed step keeps
   running and the audio device stays open. Pause the tick in
   `crates/balaur_core/src/app.rs` while the device fact `suspended` holds,
   and release the device in `crates/balaur_audio` and take it back on resume.
   Done: a test that suspends the device sees no tick advance and the audio
   device closed, and a recording replayed through a suspend keeps its digest.
2. **A frame and a touch on a real phone.** CI stops at "a device would
   install this" and checks the shaders as GLSL ES 3.10
   (`crates/balaur_render/src/shader_checks.rs`). The Android emulator's GLES
   backend draws an exported game. `touch_button`, `touch_stick`, the gestures
   and the keyboard height are tested headless only. Done: an exported game
   draws, plays sound and answers touch on one iPhone and one Android phone.
3. **What a phone lends a game.** The share sheet, the camera, geolocation,
   biometrics and the phone's own motor behind one `device` module.
   `input.vibrate` reaches only a page's `navigator.vibrate`
   (`crates/balaur_input/src/haptics.rs`). The clipboard (`ui.set_clipboard`)
   and keep-awake (`window.set_keep_awake`) already have a home, so the
   module does not spell them again. Done: a game on a phone shares a file
   and reads its position.
