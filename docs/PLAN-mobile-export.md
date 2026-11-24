# Exporting for mobile

What is left before an iOS or Android export is a game a player can hold.
`balaur export --target ios` and `--target android` are built, and
`scripts/export_check.sh` checks the bundles on every build. An app sent to
the background runs no fixed step and holds its sounds until it comes back
(`App::run_fixed_steps`, `AudioState::set_away`).

## Steps

1. **A frame and a touch on a real phone.** CI stops at "a device would
   install this" and checks the shaders as GLSL ES 3.10
   (`crates/balaur_render/src/shader_checks.rs`). The Android emulator's GLES
   backend draws an exported game. `touch_button`, `touch_stick`, the gestures
   and the keyboard height are tested headless only. Done: an exported game
   draws, plays sound and answers touch on one iPhone and one Android phone.
2. **What a phone lends a game.** The share sheet, the camera, geolocation,
   biometrics and the phone's own motor behind one `device` module. The
   clipboard (`ui.set_clipboard`) and keep-awake (`window.set_keep_awake`)
   already have a home, so the module does not spell them again. Done: a game
   on a phone shares a file and reads its position.
   *First part built 2026-11-24*, behind `balaur_platform`'s device seam
   (`device.rs`), which Apple's plugin fills on iOS and macOS and the Java
   shims in `crates/balaur_platform/java` on Android:
   - `device.vibrate(seconds)`, which replaces `input.vibrate`: Android's
     `Vibrator`, a page's `navigator.vibrate`, the iPhone's one system buzz.
     `device.haptic(HAPTIC_*)`: UIKit's feedback generators, Android's
     predefined effects (a short buzz below Android 10). Output, never recorded.
   - `device.share({ text, file, title })`: `ACTION_SEND` through the chooser,
     a file as a content URI from the shim's own read-only provider
     (`Files.java`); `UIActivityViewController`; `NSSharingServicePicker`.
     It answers `shared` with the `target` picked, or `cancelled`.
   - `device.request_review()`: `SKStoreReviewController`. `unsupported` on
     Android until Play's in-app review library comes with Play Billing.
   - `platform.notify`, `request_notifications`, `cancel_notification` and
     `platform.is_available(SERVICE_*)` sit beside it (ARCHITECTURE.md).
   Tested headless against a canned OS, stand-ins and a replay; the Java
   compiles under `-Werror` and dexes. Not run on a device. Still to do: the
   camera, geolocation and biometrics; `navigator.share` on a page; an exact
   iPhone buzz through Core Haptics; a monochrome small icon for Android's
   notifications, which show the system's reminder icon.
