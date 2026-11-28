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
2. **What a phone lends a game.** The `device` module
   (`crates/balaur_platform/src/device.rs`) holds the motor, the share sheet,
   the photo picker (`pick_photo`, with `image.encode` to shrink it) and the
   review prompt; `secrets.*` keeps a token in the Keychain or behind an
   Android Keystore key. The picker and the secrets are tested through
   stand-ins and the file store only. Still to add behind it: the camera,
   geolocation and biometrics, `navigator.share` on a page, and an exact
   iPhone buzz through Core Haptics. Done: a game on a phone reads its
   position, picks a photo and keeps a secret across a restart, and a page
   shares a file.
3. **An iOS build App Store Connect takes.** The export's `Info.plist`
   names `CFBundleSupportedPlatforms`, `UIDeviceFamily` from `[apple]
   devices`, `arm64` in `UIRequiredDeviceCapabilities` on a device, and every
   orientation or `UIRequiresFullScreen` on an iPad. It keeps the `DT*` keys
   and `BuildMachineOSBuild` that `scripts/package_runtime.sh` writes into
   the template from the SDK and the Xcode it built with. The bundle, its
   executable and its folder in the `.ipa` are named for `[application]
   name`, and a device bundle missing a key fails the export, naming it
   (`crates/balaur_export/src/apple/store.rs`). The native scripts
   framework carries the keys of the Xcode that built it, and a device's is
   built with Xcode alone. A signature against a provisioning profile names
   the app as the profile does: its identifier, its team, `get-task-allow`
   and `beta-reports-active`, read back with `codesign -d` and refused where
   they differ. The SDK is whichever Xcode the `macos-latest` runner has
   selected when `build-platforms` builds the templates. From April 2027 App
   Store Connect takes only builds made with the iOS 27 SDK, so that runner
   needs Xcode 27 selected by then. Done: an `.ipa` App Store Connect
   accepts, installed from TestFlight.
