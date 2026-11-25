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
   - `device.vibrate(seconds)`: Android's
     `Vibrator`, a page's `navigator.vibrate`, the iPhone's one system buzz.
     `device.haptic(HAPTIC_*)`: UIKit's feedback generators, Android's
     predefined effects (a short buzz below Android 10). Output, never recorded.
   - `device.share({ text, file, title })`: `ACTION_SEND` through the chooser,
     a file as a content URI from the shim's own read-only provider
     (`Files.java`); `UIActivityViewController`; `NSSharingServicePicker`.
     It answers `shared` with the `target` picked, or `cancelled`. On
     Android the pick can land after the chooser's result, so a shut chooser
     waits two seconds for it before answering `cancelled`.
   - `device.request_review()`: `SKStoreReviewController`, and on Android
     Play's review flow in a build with the `in-app-review` capability
     (`PLAN-google.md` step 9).
   - `platform.notify`, `request_notifications`, `cancel_notification`,
     `register_for_push` and `platform.is_available(SERVICE_*)` sit beside it
     (ARCHITECTURE.md). Push is APNs on Apple (`PLAN-apple.md` step 5) and FCM
     on Android (`PLAN-google.md` step 8), its token answered in the fields
     Gamend's `push_tokens` takes.
   - A link into the game is `url_opened` on `platform_event`, with its whole
     `url`, on Apple (`PLAN-apple.md` step 5) and on Android: a link on one
     of `[android] schemes` that ends no open sign-in page, the one that
     started the game too, each heard once (`Redirect` and `Links`).
   - A notification's small icon on Android is `[android] notification_icon`,
     a square picture with transparency the export writes white from its
     alpha at each density, named to `Alarm` and to FCM in the manifest; a
     game naming none shows the system's reminder icon.
   Tested headless against a canned OS, stand-ins and a replay; the Java
   compiles under `-Werror` and dexes. Not run on a device. Still to do: the
   camera, geolocation and biometrics; `navigator.share` on a page; an exact
   iPhone buzz through Core Haptics.
