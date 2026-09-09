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
  # Stated once; the plist below declares it too. StoreKit 2 is the floor:
  # the Swift shim in crates/balaur_apple is built for iOS 15 and a template
  # that claimed less would not load its own symbols.
  ios_min=15.0
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
  # Every ABI Play installs, each with the Rust target that builds it. The
  # template carries all four; `[android] abis` picks what an export keeps.
  abis="arm64-v8a:aarch64-linux-android armeabi-v7a:armv7-linux-androideabi \
x86:i686-linux-android x86_64:x86_64-linux-android"

  # Laid out the way an APK expects, so the remaining step is assembling and
  # signing one — which needs aapt2 and a keystore that belongs to whoever
  # ships the game, not to CI.
  skeleton="$dist/balaur-template-android"
  rm -rf "$skeleton"
  mkdir -p "$skeleton/assets"

  for pair in $abis; do
    abi=${pair%%:*}
    target=${pair#*:}
    step "build ($abi, $target)"
    rustup target add "$target"
    # The template is a NativeActivity library, not an executable — Android
    # dlopens libmain.so and calls android_main. See crates/balaur_android.
    cargo build --release -p balaur_android --target "$target"
    lib="target/$target/release/libmain.so"
    [ -f "$lib" ] || { printf '::error::no library at %s\n' "$lib"; exit 1; }

    # Android 15 maps a 64-bit library on 16 KB pages and Play rejects one
    # linked for less. .cargo/config.toml sets it; this reads it back.
    case "$abi" in
    arm64-v8a | x86_64)
      loads=$(readelf -lW "$lib" | awk '$1 == "LOAD" { print $NF }')
      [ -n "$loads" ] || { printf '::error::no LOAD segments in %s\n' "$lib"; exit 1; }
      for align in $loads; do
        [ "$((align))" -ge 16384 ] || {
          printf '::error::%s libmain.so is aligned to %s, under 16 KB\n' "$abi" "$align"
          exit 1
        }
      done
      ;;
    esac

    mkdir -p "$skeleton/lib/$abi"
    cp "$lib" "$skeleton/lib/$abi/libmain.so"
  done

  step "stage (apk layout)"
  # `android.app.lib_name` is how NativeActivity finds the library, so it has
  # to stay in step with [lib] name in crates/balaur_android/Cargo.toml.
  cat >"$skeleton/AndroidManifest.xml" <<'MANIFEST'
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="org.balaur.template"
    android:versionCode="1"
    android:versionName="1.0">
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
  target=wasm32-unknown-unknown
  # WEB_THREADS builds the shared-memory variant: threading is a property of
  # the module, not a runtime switch, and a browser refuses a shared one off a
  # page that is not cross-origin isolated. So it is a second template.
  threads=${WEB_THREADS:-}
  name=balaur-template-web${threads:+-threads}
  step "build ($target, windowed${threads:+, threads})"
  rustup target add "$target"
  # WEB_FEATURES builds a smaller template; docs/generated/features.md says
  # what each feature costs, and gen_docs.py reads the default off this line.
  features=${WEB_FEATURES:-audio,http,websocket,gamend,web,window}
  # The solver threads only where the module can: `parallel` pulls rayon in,
  # and rayon blocks on `Atomics.wait`, which a browser refuses off a page that
  # is not cross-origin isolated. The plain template must not have it.
  if [ -n "$threads" ]; then
    features="$features,parallel"
  fi
  # wasm-bindgen, not emscripten: kiss3d declares its web dependencies under
  # [target.wasm32-unknown-unknown] and wgpu reaches WebGPU only through web-sys.
  # webtransport is out: a browser backend exists, but no plugin registers it.
  if [ -n "$threads" ]; then
    # std itself has to be rebuilt with atomics, and `-Z build-std` is nightly
    # only. The pinned stable in rust-toolchain.toml stays the default; this
    # names its own toolchain so the two never fight.
    toolchain=${WEB_THREADS_TOOLCHAIN:-nightly}
    rustup component add rust-src --toolchain "$toolchain"
    # Shared, imported memory is what a worker attaches to: `+atomics` alone
    # links a private one, and posting it to a worker fails to clone. A shared
    # memory must declare a maximum, so the flags name one.
    flags="-C target-feature=+atomics,+bulk-memory,+mutable-globals"
    flags="$flags -C link-arg=--shared-memory -C link-arg=--import-memory"
    flags="$flags -C link-arg=--max-memory=4294967296"
    # wasm-bindgen's threading pass rewrites TLS per worker and looks these up
    # by name. wasm-ld keeps them internal unless asked, and the pass then
    # fails with `failed to find __wasm_init_tls`.
    for sym in __wasm_init_tls __tls_size __tls_align __tls_base; do
      flags="$flags -C link-arg=--export=$sym"
    done
    RUSTFLAGS="${RUSTFLAGS:-} $flags" \
      cargo "+$toolchain" build --profile web --target "$target" -p balaur_cli \
      --no-default-features --features "$features" \
      -Z build-std=std,panic_abort
  else
    cargo build --profile web --target "$target" -p balaur_cli \
      --no-default-features --features "$features"
  fi

  wasm="target/$target/web/balaur.wasm"
  [ -f "$wasm" ] || { printf '::error::no %s\n' "$wasm"; exit 1; }

  # The CLI must match the wasm-bindgen the build linked against; read it off
  # Cargo.lock rather than pinning a second copy of the number here.
  step "bindgen"
  want=$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/[" ]/,""); sub(/version=/,""); print; exit}' Cargo.lock)
  have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
  if [ "$want" != "$have" ]; then
    printf '::error::wasm-bindgen CLI is %s, Cargo.lock wants %s\n' "${have:-missing}" "$want"
    exit 1
  fi
  wasm-bindgen --target web --no-typescript --out-dir "$dist" --out-name balaur "$wasm"

  # -Oz after bindgen, never before: bindgen rewrites the module. Every
  # --enable is a feature rustc emits for this target by default, and wasm-opt
  # rejects input using one it was not told about, so the list tracks rustc.
  step "wasm-opt"
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    --enable-sign-ext --enable-mutable-globals --enable-reference-types \
    ${threads:+--enable-threads} \
    -o "$dist/balaur_bg.wasm" "$dist/balaur_bg.wasm"

  # The number that decides whether a browser can be asked to load this.
  # Brotli is what a static host actually serves.
  step "size"
  raw=$(wc -c <"$dist/balaur_bg.wasm")
  # A floor, not only a ceiling: bindgen drops everything its exports cannot
  # reach, so a template that lost its #[wasm_bindgen] entry point still builds
  # — to a module with no engine in it. That shipped once, at 73 KB.
  floor=$((5 * 1024 * 1024))
  if [ "$raw" -lt "$floor" ]; then
    printf '::error::web template is %d bytes, under the %d floor — the engine is not in it. Check that web.rs still exports a #[wasm_bindgen] entry point.\n' "$raw" "$floor"
    exit 1
  fi
  gz=$(gzip -9 -c "$dist/balaur_bg.wasm" | wc -c)
  br=$(command -v brotli >/dev/null && brotli -q 11 -c "$dist/balaur_bg.wasm" | wc -c || echo 0)
  printf 'wasm raw    %8.2f MB\n' "$(echo "$raw/1048576" | bc -l)"
  printf 'wasm gzip   %8.2f MB\n' "$(echo "$gz/1048576" | bc -l)"
  [ "$br" -gt 0 ] && printf 'wasm brotli %8.2f MB\n' "$(echo "$br/1048576" | bc -l)"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      printf '### Web template size (%s)\n\n' "$name"
      printf '| | MB |\n| --- | --- |\n'
      printf '| raw | %.2f |\n' "$(echo "$raw/1048576" | bc -l)"
      printf '| gzip | %.2f |\n' "$(echo "$gz/1048576" | bc -l)"
      [ "$br" -gt 0 ] && printf '| brotli | %.2f |\n' "$(echo "$br/1048576" | bc -l)"
    } >>"$GITHUB_STEP_SUMMARY"
  fi

  # The directory `balaur export --target web` copies: the glue and the wasm.
  # The exporter adds the page and the pack beside them.
  step "template dir"
  skeleton="$dist/$name"
  rm -rf "$skeleton"
  mkdir -p "$skeleton"
  cp "$dist/balaur.js" "$dist/balaur_bg.wasm" "$skeleton/"
  # The shared-memory build's worker helper: wasm-bindgen emits it beside the
  # glue, which imports it by relative path, so it travels with them.
  if [ -d "$dist/snippets" ]; then
    cp -R "$dist/snippets" "$skeleton/"
  fi
  (cd "$dist" && tar -czf "$name.tar.gz" "$name")
  # Staged, not shipped: dist is uploaded whole, and a directory is not an
  # asset a release can carry.
  rm -rf "$skeleton"
  ;;

*)
  printf '::error::unknown platform %s (ios, android, web)\n' "$platform"
  exit 1
  ;;
esac

step "done"
ls -l "$dist"
