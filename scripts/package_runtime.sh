#!/usr/bin/env bash
# Build a runtime template for a platform that is not a desktop.
#
# Desktop templates are plain executables — a game is the template with a pack
# appended, and the OS runs the file. None of these platforms work that way, so
# each gets the shape its OS actually launches, and what a game does with the
# template differs per platform.
#
# Usage: [VARIANT=2d|3d|bare] package_runtime.sh <platform>   ios | ios-sim | android | web
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/features.sh

platform=${1:?usage: package_runtime.sh <platform>}
dist=$(mkdir -p "${DIST:-dist}" && cd "${DIST:-dist}" && pwd)
variant=${VARIANT:-}

step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

case "$platform" in
ios | ios-sim)
  # The simulator runs the host's own architecture, so its slice is a target
  # of its own. Same bundle, same plist: what a simulator launches is what a
  # phone launches, which is the point of checking there first.
  if [ "$platform" = ios-sim ]; then
    target=$(uname -m | sed 's/arm64/aarch64/')-apple-ios-sim
  else
    target=aarch64-apple-ios
  fi
  # Stated once; the plist below declares it too. StoreKit 2 is the floor:
  # the Swift shim in crates/balaur_apple is built for iOS 15 and a template
  # that claimed less would not load its own symbols.
  ios_min=15.0
  # A variant is a game template; the platform's own build is the whole CLI.
  if [ -n "$variant" ]; then
    flags=(--no-default-features --features "window,$(game_features "$variant")")
  else
    flags=(--features "apple")
  fi
  step "build ($target, windowed, min iOS $ios_min${variant:+, $variant})"
  rustup target add "$target"
  IPHONEOS_DEPLOYMENT_TARGET=$ios_min \
    cargo build --release -p balaur_cli "${flags[@]}" --target "$target" --config "$(short_paths)"

  # The smallest bundle iOS will launch: the executable and a plist naming it.
  # Unsigned on purpose — signing certificates must never live in this repo's CI.
  step "bundle (unsigned .app)"
  app="$dist/Balaur.app"
  rm -rf "$app"
  mkdir -p "$app"
  cp "target/$target/release/balaur" "$app/Balaur"
  chmod +x "$app/Balaur"
  code_by_crate native_crates.py "$app/Balaur" "balaur-runtime-$platform${variant:+-$variant}" "$dist"
  strip_template "$app/Balaur"
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
  # A variant's bundle sits in a directory named for it, so two unpack side
  # by side; the device's own stays at the top, where an export looks first.
  # The simulator's is never at the top: it would land on the device's.
  if [ -n "$variant" ] || [ "$platform" = ios-sim ]; then
    name=balaur-runtime-$platform${variant:+-$variant}
    rm -rf "${dist:?}/$name"
    mkdir -p "$dist/$name"
    mv "$app" "$dist/$name/"
    (cd "$dist" && tar -czf "$name.tar.gz" "$name")
    rm -rf "${dist:?}/$name"
  else
    (cd "$dist" && tar -czf "balaur-runtime-$platform.tar.gz" "Balaur.app")
    rm -rf "$app"
  fi
  ;;

android)
  # Every ABI Play installs, each with the Rust target that builds it. The
  # template carries all four; `[android] abis` picks what an export keeps.
  abis="arm64-v8a:aarch64-linux-android armeabi-v7a:armv7-linux-androideabi \
x86:i686-linux-android x86_64:x86_64-linux-android"

  # Laid out the way an APK expects, so the remaining step is assembling and
  # signing one — which needs aapt2 and a keystore that belongs to whoever
  # ships the game, not to CI.
  name=balaur-runtime-android${variant:+-$variant}
  skeleton="$dist/$name"
  # The game set every template has, less the physics world a variant drops.
  features=$(game_features "$variant")
  rm -rf "$skeleton"
  mkdir -p "$skeleton/assets"

  for pair in $abis; do
    abi=${pair%%:*}
    target=${pair#*:}
    step "build ($abi, $target)"
    rustup target add "$target"
    # The template is a NativeActivity library, not an executable — Android
    # dlopens libmain.so and calls android_main. See crates/balaur_android.
    cargo build --release -p balaur_android --no-default-features \
      --features "$features" --target "$target" --config "$(short_paths)"
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
    # The host's strip reads its own ELF alone; the NDK's reads every ABI.
    ndk_strip=$(compgen -G "${ANDROID_NDK_ROOT:-}/toolchains/llvm/prebuilt/*/bin/llvm-strip" | head -1 || true)
    [ -n "$ndk_strip" ] || { printf '::error::no llvm-strip under ANDROID_NDK_ROOT\n'; exit 1; }
    strip_template "$skeleton/lib/$abi/libmain.so" "$ndk_strip"
    if [ "$abi" = arm64-v8a ]; then
      # The host's nm reads its own ELF alone; the NDK's reads every ABI.
      ndk_nm=$(compgen -G "${ANDROID_NDK_ROOT:-}/toolchains/llvm/prebuilt/*/bin/llvm-nm" | head -1 || true)
      NM=$ndk_nm code_by_crate native_crates.py "$lib" "$name-$abi" "$dist"
    fi
  done

  step "dex (the Java shims)"
  # crates/balaur_google/java, reached over JNI: compiled against the
  # platform's android.jar and dexed for the template's floor. Nothing in it
  # comes from Maven: Credential Manager is the system's own.
  sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}
  android_jar=$(compgen -G "$sdk/platforms/android-*/android.jar" | sort -V | tail -1 || true)
  d8=$(compgen -G "$sdk/build-tools/*/d8" | sort -V | tail -1 || true)
  [ -n "$android_jar" ] || { printf '::error::no platforms/android-*/android.jar under %s\n' "$sdk"; exit 1; }
  [ -n "$d8" ] || { printf '::error::no build-tools/*/d8 under %s\n' "$sdk"; exit 1; }
  classes=$(mktemp -d)
  # shellcheck disable=SC2046 # one path a source file, none with spaces
  javac --release 11 -Werror -Xlint:all -classpath "$android_jar" -d "$classes" \
    $(find crates/balaur_google/java -name '*.java' | sort)
  # shellcheck disable=SC2046 # one path a class file, none with spaces
  "$d8" --release --min-api 26 --lib "$android_jar" --output "$skeleton" \
    $(find "$classes" -name '*.class' | sort)
  rm -rf "$classes"
  [ -f "$skeleton/classes.dex" ] || { printf '::error::d8 wrote no classes.dex\n'; exit 1; }

  step "stage (apk layout)"
  # `android.app.lib_name` is how NativeActivity finds the library, so it has
  # to stay in step with [lib] name in crates/balaur_android/Cargo.toml.
  cat >"$skeleton/AndroidManifest.xml" <<'MANIFEST'
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="org.balaur.template"
    android:versionCode="1"
    android:versionName="1.0">
  <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="36" />
  <application android:label="Balaur" android:hasCode="true">
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
  (cd "$dist" && tar -czf "$name.tar.gz" "$name")
  rm -rf "$skeleton"
  ;;

web)
  target=wasm32-unknown-unknown
  # WEB_THREADS builds the shared-memory variant: threading is a property of
  # the module, not a runtime switch, and a browser refuses a shared one off a
  # page that is not cross-origin isolated. So it is a second template.
  threads=${WEB_THREADS:-}
  # A variant ships beside the plain module rather than replacing it, so it
  # carries its own name: `-threads` for the shared-memory build, `-editor`
  # for the module the web editor runs.
  variant=${variant:-${threads:+threads}}
  name=balaur-runtime-web${variant:+-$variant}
  step "build ($target, windowed${threads:+, threads})"
  rustup target add "$target"
  # WEB_FEATURES builds a smaller template; docs/generated/features.md says
  # what each feature costs, and gen_docs.py reads the default off features.sh.
  # `.bs` scripts on the page's own engine, not on wasmi compiled to wasm.
  features=$(game_features "$variant")
  # A page has no environment for `RUST_LOG` to come from.
  features=${features/,env-filter/}
  features=${WEB_FEATURES:-${features/,bs/,bs-web},window}
  # The solver threads only where the module can: `parallel` pulls rayon in,
  # and rayon blocks on `Atomics.wait`, which a browser refuses off a page that
  # is not cross-origin isolated. The plain template must not have it.
  if [ -n "$threads" ]; then
    features="$features,parallel"
  fi
  # Built for speed, but for the bare template, whose size has a budget below:
  # `[profile.web-small]`, std and wasm-opt's passes for size with it.
  profile=web
  wasm_opt=-O3
  web_std=stock
  if [ "$variant" = bare ]; then
    profile=web-small
    wasm_opt=-Oz
    web_std=size
  fi
  # The symbols stay until the table below has charged the code to crates;
  # the shipped module is stripped after it.
  export CARGO_PROFILE_WEB_STRIP=false CARGO_PROFILE_WEB_SMALL_STRIP=false
  # std rebuilt for size: smaller sorting, formatting and hashing, and a slower
  # sort on a large array. WEB_STD=stock keeps the std rustup ships, and
  # WEB_STD=size rebuilds it for any template.
  size_std=()
  if [ "${WEB_STD:-$web_std}" = size ]; then
    size_std=(-Z build-std-features=optimize_for_size)
  fi
  # wasm-bindgen's target: kiss3d declares its web dependencies under
  # [target.wasm32-unknown-unknown] and wgpu reaches WebGPU only through web-sys.
  if [ -n "$threads" ]; then
    # std itself has to be rebuilt with atomics, and `-Z build-std` is nightly
    # only. The pinned stable in rust-toolchain.toml stays the default; this
    # names its own toolchain so the two never fight.
    toolchain=$WEB_THREADS_TOOLCHAIN
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
    RUSTFLAGS="${RUSTFLAGS:-} $flags $(remap_flags)" \
      cargo "+$toolchain" build --profile "$profile" --target "$target" -p balaur_cli \
      --no-default-features --features "$features" \
      -Z build-std=std,panic_abort ${size_std[@]+"${size_std[@]}"}
  elif [ ${#size_std[@]} -gt 0 ]; then
    # `-Z` on the pinned stable, so the template and the editor share one
    # compiler; std's sources are a component of it.
    rustup component add rust-src
    RUSTC_BOOTSTRAP=1 cargo build --profile "$profile" --target "$target" -p balaur_cli \
      --no-default-features --features "$features" --config "$(short_paths)" \
      -Z build-std=std,panic_abort "${size_std[@]}"
  else
    cargo build --profile "$profile" --target "$target" -p balaur_cli \
      --no-default-features --features "$features" --config "$(short_paths)"
  fi

  wasm="target/$target/$profile/balaur.wasm"
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
  wasm-bindgen --target web --no-typescript --keep-debug --out-dir "$dist" --out-name balaur "$wasm"

  # The profile's -O3 or -Oz after bindgen, never before: bindgen rewrites the
  # module. Every --enable is a feature rustc emits for this target, and
  # wasm-opt rejects input using one it was not told about.
  step "wasm-opt"
  enable=(--enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext
    --enable-mutable-globals --enable-reference-types ${threads:+--enable-threads})
  named="$dist/$name.named.wasm"
  # std's DWARF goes in a run of its own: present while wasm-opt plans, it
  # leaves the module 5% larger.
  wasm-opt --strip-dwarf -g "${enable[@]}" -o "$named" "$dist/balaur_bg.wasm"
  wasm-opt "$wasm_opt" -g "${enable[@]}" -o "$named" "$named"
  wasm-opt --strip-debug "${enable[@]}" -o "$dist/balaur_bg.wasm" "$named"

  # The number that decides whether a browser can be asked to load this.
  # Brotli is what a static host actually serves.
  step "size"
  raw=$(wc -c <"$dist/balaur_bg.wasm")
  # A floor, not only a ceiling: bindgen drops everything its exports cannot
  # reach, so a template that lost its #[wasm_bindgen] entry point still builds
  # — to a module with no engine in it. That shipped once, at 73 KB.
  floor=$((5 * 1024 * 1024))
  if [ "$raw" -lt "$floor" ]; then
    printf '::error::web runtime is %d bytes, under the %d floor — the engine is not in it. Check that web.rs still exports a #[wasm_bindgen] entry point.\n' "$raw" "$floor"
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
  step "code by crate"
  code_by_crate wasm_crates.py "$named" "$name" "$dist"
  rm "$named"
  # The bare template's ceiling: docs/PLAN-build-size.md's last measured
  # size rounded up to a tenth. A push past it moves both, and says why.
  budget_mb=10.6
  if [ "$variant" = bare ] && [ "$(echo "$raw > $budget_mb * 1048576" | bc)" = 1 ]; then
    printf '::error::the bare web template is %.2f MB, over its %s MB budget; the table above says where the bytes went\n' \
      "$(echo "$raw/1048576" | bc -l)" "$budget_mb"
    exit 1
  fi

  # The directory `balaur export --target web` copies: the glue and the wasm.
  # The exporter adds the page and the pack beside them.
  step "template dir"
  skeleton="$dist/$name"
  rm -rf "$skeleton"
  mkdir -p "$skeleton"
  cp "$dist/balaur.js" "$dist/balaur_bg.wasm" "$skeleton/"
  # wasm-bindgen emits `inline_js` and the shared-memory worker helper beside
  # the glue, which names them by relative path, so they travel with it.
  if [ -d "$dist/snippets" ]; then
    cp -R "$dist/snippets" "$skeleton/"
  fi
  ./scripts/check_web_module.sh "$skeleton"
  (cd "$dist" && tar -czf "$name.tar.gz" "$name")
  # Staged, not shipped: dist is uploaded whole, and a directory is not an
  # asset a release can carry.
  rm -rf "$skeleton"
  ;;

*)
  printf '::error::unknown platform %s (ios, ios-sim, android, web)\n' "$platform"
  exit 1
  ;;
esac

step "done"
ls -l "$dist"
