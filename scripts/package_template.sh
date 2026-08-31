#!/usr/bin/env bash
# Build a runtime template for a platform that is not a desktop.
#
# Desktop templates are plain executables — a game is the template with a pack
# appended, and the OS runs the file. None of these platforms work that way, so
# each gets the shape its OS actually launches, and what a game does with the
# template differs per platform. See docs/PLAN-mobile-export.md.
#
# Usage: package_template.sh <platform>     ios | android | web
set -euo pipefail
cd "$(dirname "$0")/.."

platform=${1:?usage: package_template.sh <platform>}
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

case "$platform" in
ios)
  target=aarch64-apple-ios
  step "build ($target, windowed)"
  rustup target add "$target"
  cargo build --release -p balaur_cli --features window --target "$target"

  # An iOS app is an executable inside a bundle, not a bare binary. This is the
  # smallest bundle the OS will launch: the executable, and a plist naming it.
  # Unsigned on purpose — signing needs a developer certificate, which belongs
  # to whoever ships the game and must never live in this repo's CI.
  step "bundle (unsigned .app)"
  app="$dist/Balaur.app"
  rm -rf "$app"
  mkdir -p "$app"
  cp "target/$target/release/balaur" "$app/Balaur"
  chmod +x "$app/Balaur"
  cat >"$app/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>Balaur</string>
  <key>CFBundleIdentifier</key><string>org.balaur.template</string>
  <key>CFBundleName</key><string>Balaur</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSRequiresIPhoneOS</key><true/>
  <key>UILaunchScreen</key><dict/>
  <key>MinimumOSVersion</key><string>13.0</string>
</dict>
</plist>
PLIST
  (cd "$dist" && tar -czf "balaur-template-ios.tar.gz" "Balaur.app")
  rm -rf "$app"
  ;;

android)
  target=aarch64-linux-android
  step "build ($target)"
  rustup target add "$target"
  # The template is a NativeActivity library, not an executable — Android
  # dlopens libmain.so and calls android_main. See crates/balaur_android.
  cargo build --release -p balaur_android --target "$target"
  lib="target/$target/release/libmain.so"
  [ -f "$lib" ] || { printf '::error::no library at %s\n' "$lib"; exit 1; }

  step "stage (apk layout)"
  # Laid out the way an APK expects, so the remaining step is assembling and
  # signing one — which needs aapt2 and a keystore that belongs to whoever
  # ships the game, not to CI.
  skeleton="$dist/balaur-template-android"
  rm -rf "$skeleton"
  mkdir -p "$skeleton/lib/arm64-v8a" "$skeleton/assets"
  cp "$lib" "$skeleton/lib/arm64-v8a/libmain.so"
  # `android.app.lib_name` is how NativeActivity finds the library, so it has
  # to stay in step with [lib] name in crates/balaur_android/Cargo.toml.
  cat >"$skeleton/AndroidManifest.xml" <<'MANIFEST'
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="org.balaur.template">
  <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="35" />
  <application android:label="Balaur" android:hasCode="false">
    <activity
        android:name="android.app.NativeActivity"
        android:exported="true"
        android:configChanges="orientation|keyboardHidden|screenSize">
      <meta-data android:name="android.app.lib_name" android:value="main" />
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
      </intent-filter>
    </activity>
  </application>
</manifest>
MANIFEST
  # An exported game drops its pack in here; the bare template ships it empty.
  printf 'A game exported for Android puts game.bpak in this directory.\n' \
    >"$skeleton/assets/README"
  (cd "$dist" && tar -czf balaur-template-android.tar.gz balaur-template-android)
  rm -rf "$skeleton"
  ;;

web)
  target=wasm32-unknown-emscripten
  step "build ($target)"
  rustup target add "$target"
  cargo build --release -p balaur_cli --target "$target"
  # emcc emits the .wasm beside the .js loader; ship both or neither.
  for f in "target/$target/release/balaur.wasm" "target/$target/release/balaur.js"; do
    [ -f "$f" ] && cp "$f" "$dist/"
  done
  ;;

*)
  printf '::error::unknown platform %s (ios, android, web)\n' "$platform"
  exit 1
  ;;
esac

step "done"
ls -l "$dist"
