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
  # Stated once; the plist below declares it too. Below 12.0 the linker emits
  # the pre-LC_BUILD_VERSION load command, which records no platform at all.
  ios_min=13.0
  step "build ($target, windowed, min iOS $ios_min)"
  rustup target add "$target"
  IPHONEOS_DEPLOYMENT_TARGET=$ios_min \
    cargo build --release -p balaur_cli --features "window,apple" --target "$target"

  # The smallest bundle iOS will launch: the executable and a plist naming it.
  # Unsigned on purpose — signing certificates must never live in this repo's CI.
  step "bundle (unsigned .app)"
  app="$dist/Balaur.app"
  rm -rf "$app"
  mkdir -p "$app"
  cp "target/$target/release/balaur" "$app/Balaur"
  chmod +x "$app/Balaur"
  cat >"$app/Info.plist" <<PLIST
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
  <key>MinimumOSVersion</key><string>$ios_min</string>
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

  step "assemble (debug apk)"
  # The skeleton assembled into an `adb install`-able APK. Hard-required: a
  # silent skip would read as "template works" while shipping uninstallable.
  ./scripts/assemble_apk.sh "$skeleton" "$dist/balaur-template-debug.apk"
  rm -rf "$skeleton"
  ;;

web)
  target=wasm32-unknown-emscripten
  step "build ($target)"
  rustup target add "$target"
  # The link tolerates undefined symbols (.cargo/config.toml) because egui's
  # web-sys dependency leaves wasm-bindgen intrinsics unresolved on emscripten.
  # So the link no longer catches a missing symbol of ours; this does.
  build_log=$(mktemp)
  cargo build --release -p balaur_cli --target "$target" 2>&1 | tee "$build_log"
  stray=$(grep -oE 'undefined symbol: [A-Za-z0-9_]+' "$build_log" |
    sed 's/undefined symbol: //' | grep -vE '^__wb(indgen|g_)' | sort -u || true)
  rm -f "$build_log"
  if [ -n "$stray" ]; then
    printf '::error::undefined symbols that are not wasm-bindgen intrinsics: %s\n' "$(tr '\n' ' ' <<<"$stray")"
    exit 1
  fi
  # emcc emits the .wasm beside the .js loader that instantiates it. Either one
  # alone is unusable, and `[ -f ] && cp` in a loop exits the script with no
  # message when the last file is missing, which is how this went unnoticed.
  for f in balaur.wasm balaur.js; do
    from="target/$target/release/$f"
    [ -f "$from" ] || { printf '::error::emscripten produced no %s\n' "$f"; exit 1; }
    cp "$from" "$dist/$f"
  done
  ;;

*)
  printf '::error::unknown platform %s (ios, android, web)\n' "$platform"
  exit 1
  ;;
esac

step "done"
ls -l "$dist"
